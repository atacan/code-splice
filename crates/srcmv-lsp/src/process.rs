//! Bounded process supervision for language servers using standard I/O.
//!
//! This module intentionally transports opaque byte chunks. JSON-RPC framing
//! belongs to the protocol layer: it supplies complete outbound frames and
//! incrementally consumes [`crate::process::ProcessEvent::Stdout`] chunks. Keeping this seam
//! narrow lets lifecycle cleanup remain effective even after framing fails.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded};

/// Frozen default maximum number of queued stdout chunks.
pub const DEFAULT_INBOUND_EVENT_CAPACITY: usize = 32;
/// Frozen default maximum cumulative bytes queued from stdout.
pub const DEFAULT_INBOUND_BYTE_CAPACITY: usize = 32 * 1024 * 1024;
/// Frozen default maximum number of queued stdin frames.
pub const DEFAULT_OUTBOUND_FRAME_CAPACITY: usize = 16;
/// Frozen default maximum cumulative bytes queued for stdin.
pub const DEFAULT_OUTBOUND_BYTE_CAPACITY: usize = 64 * 1024 * 1024;
/// Frozen default maximum number of lifecycle completion events.
pub const DEFAULT_COMPLETION_EVENT_CAPACITY: usize = 16;
/// Frozen default number of recent stderr bytes retained for diagnostics.
pub const DEFAULT_STDERR_TAIL_CAPACITY: usize = 64 * 1024;

const READ_CHUNK_BYTES: usize = 8 * 1024;
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(5);
const DESCENDANT_TERM_GRACE: Duration = Duration::from_millis(100);
const DROP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

/// Capacity limits for a supervised language-server process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessLimits {
    /// Maximum number of stdout chunks waiting for the orchestrator.
    pub inbound_events: usize,
    /// Maximum cumulative bytes in queued stdout chunks.
    pub inbound_bytes: usize,
    /// Maximum number of complete frames waiting for the stdin worker.
    pub outbound_frames: usize,
    /// Maximum cumulative bytes in queued stdin frames.
    pub outbound_bytes: usize,
    /// Maximum number of queued status and worker-failure events.
    pub completion_events: usize,
    /// Maximum number of recent stderr bytes retained.
    pub stderr_tail_bytes: usize,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            inbound_events: DEFAULT_INBOUND_EVENT_CAPACITY,
            inbound_bytes: DEFAULT_INBOUND_BYTE_CAPACITY,
            outbound_frames: DEFAULT_OUTBOUND_FRAME_CAPACITY,
            outbound_bytes: DEFAULT_OUTBOUND_BYTE_CAPACITY,
            completion_events: DEFAULT_COMPLETION_EVENT_CAPACITY,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_CAPACITY,
        }
    }
}

impl ProcessLimits {
    fn validate(self) -> Result<Self, ProcessError> {
        if self.inbound_events == 0
            || self.inbound_bytes == 0
            || self.outbound_frames == 0
            || self.outbound_bytes == 0
            || self.completion_events == 0
            || self.stderr_tail_bytes == 0
            || self.completion_events < 4
        {
            return Err(ProcessError::InvalidLimits);
        }
        Ok(self)
    }
}

/// Explicit executable configuration for a language server.
///
/// No shell is involved. Arguments and environment values are passed directly
/// to [`Command`]. The inherited environment is retained unless
/// [`Self::clear_environment`] is selected.
#[derive(Clone, Eq, PartialEq)]
pub struct ProcessSpec {
    program: OsString,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    current_directory: Option<PathBuf>,
    clear_environment: bool,
}

impl fmt::Debug for ProcessSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessSpec")
            .field("program", &"<redacted>")
            .field("argument_count", &self.arguments.len())
            .field("environment_entry_count", &self.environment.len())
            .field("has_current_directory", &self.current_directory.is_some())
            .field("clear_environment", &self.clear_environment)
            .finish()
    }
}

impl ProcessSpec {
    /// Creates a specification for an explicit executable path or program name.
    #[must_use]
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            environment: Vec::new(),
            current_directory: None,
            clear_environment: false,
        }
    }

    /// Appends one literal process argument.
    #[must_use]
    pub fn arg(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    /// Appends several literal process arguments.
    #[must_use]
    pub fn args<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.arguments.extend(arguments.into_iter().map(Into::into));
        self
    }

    /// Sets one environment variable in the child.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }

    /// Sets the child's current directory.
    #[must_use]
    pub fn current_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.current_directory = Some(directory.into());
        self
    }

    /// Prevents the child from inheriting the current process environment.
    #[must_use]
    pub fn clear_environment(mut self) -> Self {
        self.clear_environment = true;
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.arguments);
        if self.clear_environment {
            command.env_clear();
        }
        command.envs(self.environment.iter().map(|(key, value)| (key, value)));
        if let Some(directory) = &self.current_directory {
            command.current_dir(directory);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        configure_process_group(&mut command);
        command
    }

    /// Returns the configured executable without exposing environment values.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }
}

/// Identifies the worker that reported a lifecycle fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessWorker {
    /// The child stdin writer.
    Stdin,
    /// The child stdout reader.
    Stdout,
    /// The child stderr drainer.
    Stderr,
    /// The child status/reaping worker.
    Status,
}

/// A failure reported asynchronously by one process worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessFault {
    /// Worker that failed.
    pub worker: ProcessWorker,
    /// Typed failure category and bounded diagnostic detail.
    pub kind: ProcessFaultKind,
}

/// A typed asynchronous process-worker failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessFaultKind {
    /// A standard-I/O or child-status operation failed.
    Io {
        /// Standard-library category retained without parsing diagnostic text.
        error_kind: io::ErrorKind,
        /// Stable, user-readable I/O failure detail.
        message: String,
    },
    /// A cumulative queue-byte limit was exceeded.
    ResourceLimit {
        /// Queue whose byte capacity was exceeded.
        queue: &'static str,
        /// Attempted item size.
        item_bytes: usize,
        /// Configured cumulative byte capacity.
        capacity_bytes: usize,
    },
}

impl fmt::Display for ProcessFaultKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message, .. } => formatter.write_str(message),
            Self::ResourceLimit {
                queue,
                item_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "{queue} item has {item_bytes} bytes but cumulative capacity is {capacity_bytes} bytes"
            ),
        }
    }
}

/// A deterministic event delivered at the lifecycle orchestration boundary.
#[derive(Debug)]
pub enum ProcessEvent {
    /// The child has exited and was reaped.
    Exited(ExitStatus),
    /// A worker failed while transporting bytes or observing child status.
    Fault(ProcessFault),
    /// One opaque, non-empty chunk read from child stdout.
    Stdout(Vec<u8>),
    /// Child stdout reached end-of-file.
    StdoutClosed,
    /// The child closed or rejected further stdin writes.
    StdinClosed,
}

/// Error returned by process setup, bounded transport, or cleanup.
#[derive(Debug)]
pub enum ProcessError {
    /// One or more capacities were zero, or completion capacity was below four.
    InvalidLimits,
    /// The child process could not be spawned.
    Spawn(io::Error),
    /// A required standard-I/O pipe was unexpectedly unavailable.
    MissingPipe(&'static str),
    /// A worker thread could not be spawned.
    SpawnWorker(io::Error),
    /// A queue deadline expired before an item could be delivered.
    DeadlineExceeded(&'static str),
    /// A cumulative byte limit rejected a frame or chunk.
    ByteCapacityExceeded {
        /// Queue whose byte capacity was exceeded.
        queue: &'static str,
        /// Attempted item size.
        item_bytes: usize,
        /// Configured cumulative byte capacity.
        capacity_bytes: usize,
    },
    /// A lifecycle channel disconnected unexpectedly.
    Disconnected(&'static str),
    /// The child exited before another stdin command could be queued.
    Exited(ExitStatus),
    /// The child closed or rejected stdin before another command could be queued.
    StdinClosed,
    /// Cleanup did not observe process exit by the supplied deadline.
    CleanupDeadlineExceeded,
    /// A worker panicked during final joining.
    WorkerPanicked(ProcessWorker),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => formatter.write_str(
                "process capacities must be nonzero and completion capacity must be at least four",
            ),
            Self::Spawn(error) => write!(formatter, "failed to spawn language server: {error}"),
            Self::MissingPipe(pipe) => {
                write!(formatter, "language server {pipe} pipe is unavailable")
            }
            Self::SpawnWorker(error) => {
                write!(formatter, "failed to spawn process worker: {error}")
            }
            Self::DeadlineExceeded(operation) => {
                write!(formatter, "deadline exceeded while {operation}")
            }
            Self::ByteCapacityExceeded {
                queue,
                item_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "{queue} item has {item_bytes} bytes but cumulative capacity is {capacity_bytes} bytes"
            ),
            Self::Disconnected(channel) => write!(formatter, "{channel} channel disconnected"),
            Self::Exited(status) => {
                write!(
                    formatter,
                    "language server exited before stdin queueing: {status}"
                )
            }
            Self::StdinClosed => formatter.write_str("language-server stdin closed unexpectedly"),
            Self::CleanupDeadlineExceeded => {
                formatter.write_str("language-server cleanup deadline exceeded")
            }
            Self::WorkerPanicked(worker) => write!(formatter, "{worker:?} worker panicked"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::SpawnWorker(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum InputCommand {
    Bytes(Vec<u8>),
    Close,
}

#[derive(Debug)]
enum Completion {
    Exited(ExitStatus),
    Fault(ProcessFault),
    StdinClosed,
}

#[derive(Debug)]
enum StdoutItem {
    Bytes(Vec<u8>),
    Closed,
}

#[derive(Debug)]
struct ByteBudget {
    used: AtomicUsize,
    capacity: usize,
}

impl ByteBudget {
    fn new(capacity: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            capacity,
        }
    }

    fn reserve(&self, bytes: usize, queue: &'static str) -> Result<(), ProcessError> {
        let result = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= self.capacity)
            });
        result
            .map(|_| ())
            .map_err(|_| ProcessError::ByteCapacityExceeded {
                queue,
                item_bytes: bytes,
                capacity_bytes: self.capacity,
            })
    }

    fn release(&self, bytes: usize) {
        let previous = self.used.fetch_sub(bytes, Ordering::AcqRel);
        debug_assert!(previous >= bytes, "queued-byte accounting underflow");
    }

    fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
struct StderrTail {
    bytes: VecDeque<u8>,
    capacity: usize,
}

impl StderrTail {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= self.capacity {
            self.bytes.clear();
            self.bytes
                .extend(chunk[chunk.len() - self.capacity..].iter().copied());
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.capacity);
        self.bytes.drain(..overflow);
        self.bytes.extend(chunk.iter().copied());
    }

    fn snapshot(&self) -> Vec<u8> {
        self.bytes.iter().copied().collect()
    }
}

/// A running language server with bounded standard-I/O transport and cleanup.
///
/// Event precedence is deterministic whenever [`Self::next_event`] crosses an
/// orchestration boundary: resource faults precede confirmed child exit, which
/// precedes terminal pipe closure and ordinary worker faults. Ready completion
/// events are delivered before ready stdout chunks.
pub struct ManagedProcess {
    child: Arc<Mutex<Child>>,
    process_id: u32,
    input_sender: Option<Sender<InputCommand>>,
    stdout_receiver: Receiver<StdoutItem>,
    completion_receiver: Receiver<Completion>,
    wake_receiver: Receiver<()>,
    outbound_budget: Arc<ByteBudget>,
    inbound_budget: Arc<ByteBudget>,
    stderr_tail: Arc<Mutex<StderrTail>>,
    observed_status: Arc<Mutex<Option<ExitStatus>>>,
    exit_event_delivered: AtomicBool,
    pending_stdin_closed: AtomicBool,
    stdin_closed: Arc<AtomicBool>,
    pending_faults: Mutex<VecDeque<ProcessFault>>,
    cancelling_transport: Arc<AtomicBool>,
    workers: Vec<(ProcessWorker, JoinHandle<()>)>,
}

impl ManagedProcess {
    /// Spawns a process and starts its dedicated stdin, stdout, stderr, and
    /// status/reaping workers.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, process/thread spawn failures, or
    /// missing standard-I/O pipes. A partially initialized child is terminated
    /// and reaped before this function returns an error.
    pub fn spawn(specification: &ProcessSpec, limits: ProcessLimits) -> Result<Self, ProcessError> {
        let limits = limits.validate()?;
        let mut child = specification
            .command()
            .spawn()
            .map_err(ProcessError::Spawn)?;
        let process_id = child.id();
        let missing_pipe = if child.stdin.is_none() {
            Some("stdin")
        } else if child.stdout.is_none() {
            Some("stdout")
        } else if child.stderr.is_none() {
            Some("stderr")
        } else {
            None
        };
        if let Some(pipe) = missing_pipe {
            terminate_and_reap_unmanaged(&mut child, process_id);
            return Err(ProcessError::MissingPipe(pipe));
        }
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(mut stdin), Some(mut stdout), Some(mut stderr)) = pipes else {
            terminate_and_reap_unmanaged(&mut child, process_id);
            return Err(ProcessError::MissingPipe("standard I/O"));
        };
        if let Err(error) = configure_nonblocking_pipes(&mut stdin, &mut stdout, &mut stderr) {
            terminate_and_reap_unmanaged(&mut child, process_id);
            return Err(ProcessError::SpawnWorker(error));
        }
        let child = Arc::new(Mutex::new(child));

        let (input_sender, input_receiver) = bounded(limits.outbound_frames);
        let (stdout_sender, stdout_receiver) = bounded(limits.inbound_events);
        let (completion_sender, completion_receiver) = bounded(limits.completion_events);
        let (wake_sender, wake_receiver) = bounded(1);
        let outbound_budget = Arc::new(ByteBudget::new(limits.outbound_bytes));
        let inbound_budget = Arc::new(ByteBudget::new(limits.inbound_bytes));
        let stderr_tail = Arc::new(Mutex::new(StderrTail::new(limits.stderr_tail_bytes)));
        let observed_status = Arc::new(Mutex::new(None));
        let stdin_closed = Arc::new(AtomicBool::new(false));
        let cancelling_transport = Arc::new(AtomicBool::new(false));

        let mut workers = Vec::with_capacity(4);
        let spawn_result = (|| {
            workers.push((
                ProcessWorker::Stdin,
                spawn_input_worker(
                    stdin,
                    input_receiver,
                    Arc::clone(&outbound_budget),
                    Arc::clone(&cancelling_transport),
                    Arc::clone(&stdin_closed),
                    completion_sender.clone(),
                    wake_sender.clone(),
                )?,
            ));
            workers.push((
                ProcessWorker::Stdout,
                spawn_stdout_worker(
                    stdout,
                    stdout_sender,
                    Arc::clone(&inbound_budget),
                    Arc::clone(&cancelling_transport),
                    completion_sender.clone(),
                    wake_sender.clone(),
                )?,
            ));
            workers.push((
                ProcessWorker::Stderr,
                spawn_stderr_worker(
                    stderr,
                    Arc::clone(&stderr_tail),
                    Arc::clone(&cancelling_transport),
                    completion_sender.clone(),
                    wake_sender.clone(),
                )?,
            ));
            workers.push((
                ProcessWorker::Status,
                spawn_status_worker(
                    Arc::clone(&child),
                    Arc::clone(&observed_status),
                    completion_sender,
                    wake_sender,
                )?,
            ));
            Ok::<(), ProcessError>(())
        })();

        if let Err(error) = spawn_result {
            cancelling_transport.store(true, Ordering::Release);
            drop(input_sender);
            terminate_child(&child, process_id, true);
            for (_, worker) in workers {
                let _ = worker.join();
            }
            // If failure occurred before the status worker was installed, no
            // background owner exists to reap the direct child.
            if lock_or_recover(&observed_status).is_none() {
                let mut child = lock_or_recover(&child);
                let _ = child.wait();
            }
            return Err(error);
        }

        Ok(Self {
            child,
            process_id,
            input_sender: Some(input_sender),
            stdout_receiver,
            completion_receiver,
            wake_receiver,
            outbound_budget,
            inbound_budget,
            stderr_tail,
            observed_status,
            exit_event_delivered: AtomicBool::new(false),
            pending_stdin_closed: AtomicBool::new(false),
            stdin_closed,
            pending_faults: Mutex::new(VecDeque::new()),
            cancelling_transport,
            workers,
        })
    }

    /// Returns the operating-system process identifier.
    #[must_use]
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Queues one complete outbound JSON-RPC frame by a fixed deadline.
    ///
    /// # Errors
    ///
    /// Returns an error when the cumulative byte bound or deadline is reached,
    /// or when the stdin worker is no longer available.
    pub fn send_frame(&self, frame: Vec<u8>, deadline: Instant) -> Result<(), ProcessError> {
        self.outbound_budget.reserve(frame.len(), "outbound")?;
        let frame_bytes = frame.len();
        let result = self.send_input(InputCommand::Bytes(frame), deadline, "queueing stdin frame");
        if result.is_err() {
            self.outbound_budget.release(frame_bytes);
        }
        result
    }

    /// Closes child stdin after all already queued frames have been written.
    ///
    /// # Errors
    ///
    /// Returns an error if the deadline is reached or the writer disconnected.
    pub fn close_stdin(&mut self, deadline: Instant) -> Result<(), ProcessError> {
        let result = self.send_input(InputCommand::Close, deadline, "closing child stdin");
        self.input_sender.take();
        result
    }

    fn send_input(
        &self,
        command: InputCommand,
        deadline: Instant,
        operation: &'static str,
    ) -> Result<(), ProcessError> {
        let sender = self
            .input_sender
            .as_ref()
            .ok_or(ProcessError::Disconnected("stdin"))?;
        let timeout = remaining(deadline).ok_or(ProcessError::DeadlineExceeded(operation))?;
        sender.send_timeout(command, timeout).map_err(|error| {
            if error.is_timeout() {
                return ProcessError::DeadlineExceeded(operation);
            }
            if let Some(status) = self.refresh_child_status() {
                return ProcessError::Exited(status);
            }
            if self.stdin_closed.load(Ordering::Acquire) {
                return ProcessError::StdinClosed;
            }
            ProcessError::Disconnected("stdin")
        })
    }

    /// Waits for the next event until a fixed deadline.
    ///
    /// Ready completions always take precedence over stdout. The wake channel
    /// carries no data and is coalesced, so worker scheduling cannot reorder
    /// ready event classes at the orchestration boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the deadline expires or all event producers have
    /// disconnected.
    pub fn next_event(&self, deadline: Instant) -> Result<ProcessEvent, ProcessError> {
        loop {
            if let Some(event) = self.try_next_event()? {
                return Ok(event);
            }
            let timeout = remaining(deadline)
                .ok_or(ProcessError::DeadlineExceeded("waiting for process event"))?;
            match self.wake_receiver.recv_timeout(timeout) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return Err(ProcessError::DeadlineExceeded("waiting for process event"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(event) = self.try_next_event()? {
                        return Ok(event);
                    }
                    return Err(ProcessError::Disconnected("process event"));
                }
            }
        }
    }

    /// Returns the highest-precedence process event that is already ready
    /// without waiting.
    ///
    /// Ready resource faults precede confirmed child exit, followed by terminal
    /// pipe closure and ordinary worker faults. Completion events precede ready
    /// stdout chunks. Callers that arbitrate across protocol and process event
    /// classes should call this repeatedly until it returns `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns an error if internal process event state cannot be consumed.
    pub fn try_next_event(&self) -> Result<Option<ProcessEvent>, ProcessError> {
        let mut ready_exit = None;
        loop {
            match self.completion_receiver.try_recv() {
                Ok(Completion::Exited(status)) => ready_exit = Some(status),
                Ok(Completion::StdinClosed) => {
                    self.pending_stdin_closed.store(true, Ordering::Release);
                }
                Ok(Completion::Fault(fault)) => match fault.kind {
                    ProcessFaultKind::ResourceLimit { .. } => {
                        lock_or_recover(&self.pending_faults).push_front(fault);
                    }
                    ProcessFaultKind::Io { .. } => {
                        lock_or_recover(&self.pending_faults).push_back(fault);
                    }
                },
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        let resource_fault = {
            let mut faults = lock_or_recover(&self.pending_faults);
            if faults
                .front()
                .is_some_and(|fault| matches!(fault.kind, ProcessFaultKind::ResourceLimit { .. }))
            {
                faults.pop_front()
            } else {
                None
            }
        };
        if let Some(fault) = resource_fault {
            return Ok(Some(ProcessEvent::Fault(fault)));
        }

        if ready_exit.is_none() {
            ready_exit = *lock_or_recover(&self.observed_status);
        }
        if let Some(status) = ready_exit
            && let Some(event) = self.take_exit_event(status)
        {
            return Ok(Some(event));
        }

        if self.pending_stdin_closed.load(Ordering::Acquire) {
            if let Some(status) = self.refresh_child_status()
                && let Some(event) = self.take_exit_event(status)
            {
                return Ok(Some(event));
            }
            self.pending_stdin_closed.store(false, Ordering::Release);
            return Ok(Some(ProcessEvent::StdinClosed));
        }

        if !lock_or_recover(&self.pending_faults).is_empty()
            && let Some(status) = self.refresh_child_status()
            && let Some(event) = self.take_exit_event(status)
        {
            return Ok(Some(event));
        }
        if let Some(fault) = lock_or_recover(&self.pending_faults).pop_front() {
            return Ok(Some(ProcessEvent::Fault(fault)));
        }
        match self.stdout_receiver.try_recv() {
            Ok(StdoutItem::Bytes(bytes)) => {
                self.inbound_budget.release(bytes.len());
                Ok(Some(ProcessEvent::Stdout(bytes)))
            }
            Ok(StdoutItem::Closed) => {
                if let Some(status) = self.refresh_child_status()
                    && let Some(event) = self.take_exit_event(status)
                {
                    return Ok(Some(event));
                }
                Ok(Some(ProcessEvent::StdoutClosed))
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn take_exit_event(&self, status: ExitStatus) -> Option<ProcessEvent> {
        (!self.exit_event_delivered.swap(true, Ordering::AcqRel))
            .then_some(ProcessEvent::Exited(status))
    }

    fn refresh_child_status(&self) -> Option<ExitStatus> {
        if let Some(status) = *lock_or_recover(&self.observed_status) {
            return Some(status);
        }
        // The status worker owns reporting `try_wait` failures. This bounded
        // probe only upgrades an already-ready lower-priority terminal event.
        let status = lock_or_recover(&self.child).try_wait().ok().flatten()?;
        *lock_or_recover(&self.observed_status) = Some(status);
        Some(status)
    }

    /// Returns the number of bytes currently reserved in the outbound queue.
    #[must_use]
    pub fn queued_outbound_bytes(&self) -> usize {
        self.outbound_budget.used()
    }

    /// Returns the number of bytes currently reserved in the stdout queue.
    #[must_use]
    pub fn queued_inbound_bytes(&self) -> usize {
        self.inbound_budget.used()
    }

    /// Returns a snapshot of the bounded recent stderr tail.
    #[must_use]
    pub fn stderr_tail(&self) -> Vec<u8> {
        lock_or_recover(&self.stderr_tail).snapshot()
    }

    /// Waits for natural exit until `graceful_deadline`, then terminates the
    /// dedicated process group and waits until `cleanup_deadline`.
    ///
    /// Callers perform the protocol-specific graceful hook by sending `shutdown`
    /// and `exit` frames before invoking this method. Child stdin is closed here.
    /// All children in the dedicated Unix process group receive termination;
    /// other platforms fall back to terminating the direct child.
    ///
    /// # Errors
    ///
    /// Returns an error if cleanup cannot observe and reap the direct child by
    /// the cleanup deadline.
    pub fn finish(
        &mut self,
        graceful_deadline: Instant,
        cleanup_deadline: Instant,
    ) -> Result<ExitStatus, ProcessError> {
        let _ = self.close_stdin(graceful_deadline);
        if let Some(status) = self.wait_for_status(graceful_deadline) {
            return self.finalize_after_exit(status, cleanup_deadline);
        }

        terminate_child(&self.child, self.process_id, false);
        let term_deadline = midpoint_deadline(cleanup_deadline);
        if let Some(status) = self.wait_for_status(term_deadline) {
            return self.finalize_after_exit(status, cleanup_deadline);
        }

        self.cancelling_transport.store(true, Ordering::Release);
        terminate_child(&self.child, self.process_id, true);
        if let Some(status) = self.reap_after_termination(midpoint_deadline(cleanup_deadline)) {
            self.wait_for_cleanup(cleanup_deadline)?;
            return Ok(status);
        }
        Err(ProcessError::CleanupDeadlineExceeded)
    }

    /// Immediately terminates the dedicated process group, reaps the direct
    /// child, and joins every worker by `cleanup_deadline`.
    ///
    /// This is the failure-path counterpart to [`Self::finish`]; it does not
    /// wait for a protocol-level graceful shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if cleanup cannot observe and reap the direct child by
    /// the deadline or if a worker panicked.
    pub fn abort(&mut self, cleanup_deadline: Instant) -> Result<ExitStatus, ProcessError> {
        self.input_sender.take();
        self.cancelling_transport.store(true, Ordering::Release);
        terminate_child(&self.child, self.process_id, false);
        let term_deadline = bounded_grace_deadline(cleanup_deadline, DESCENDANT_TERM_GRACE);
        if !wait_for_process_group_exit(self.process_id, term_deadline) {
            terminate_child(&self.child, self.process_id, true);
        }
        let reap_deadline = midpoint_deadline(cleanup_deadline);
        let status = self
            .reap_after_termination(reap_deadline)
            .ok_or(ProcessError::CleanupDeadlineExceeded)?;
        self.wait_for_cleanup(cleanup_deadline)?;
        Ok(status)
    }

    fn finalize_after_exit(
        &mut self,
        status: ExitStatus,
        cleanup_deadline: Instant,
    ) -> Result<ExitStatus, ProcessError> {
        self.input_sender.take();
        self.cancelling_transport.store(true, Ordering::Release);

        // A reaped direct child can leave descendants in its dedicated process
        // group. Worker completion is not proof that the group is empty: a
        // descendant may close inherited pipes and still ignore TERM.
        terminate_child(&self.child, self.process_id, false);
        let term_deadline = bounded_grace_deadline(cleanup_deadline, DESCENDANT_TERM_GRACE);
        if !wait_for_process_group_exit(self.process_id, term_deadline) {
            terminate_child(&self.child, self.process_id, true);
        }
        self.wait_for_cleanup(cleanup_deadline)?;
        Ok(status)
    }

    fn reap_after_termination(&self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            if let Some(status) = *lock_or_recover(&self.observed_status) {
                return Some(status);
            }
            let result = lock_or_recover(&self.child).try_wait();
            if let Ok(Some(status)) = result {
                *lock_or_recover(&self.observed_status) = Some(status);
                return Some(status);
            }
            let wait = remaining(deadline)?;
            if wait.is_zero() {
                return None;
            }
            thread::sleep(wait.min(STATUS_POLL_INTERVAL));
        }
    }

    fn wait_for_status(&self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            if let Some(status) = *lock_or_recover(&self.observed_status) {
                return Some(status);
            }
            let wait = remaining(deadline)?;
            if wait.is_zero() {
                return None;
            }
            thread::sleep(wait.min(STATUS_POLL_INTERVAL));
        }
    }

    fn join_workers_until(&mut self, deadline: Instant) -> Result<(), ProcessError> {
        self.cancelling_transport.store(true, Ordering::Release);
        self.input_sender.take();
        join_worker_handles_until(&mut self.workers, deadline)
    }

    fn wait_for_cleanup(&mut self, deadline: Instant) -> Result<(), ProcessError> {
        loop {
            let workers_finished = self.workers.iter().all(|(_, worker)| worker.is_finished());
            let process_group_exited = process_group_has_exited(self.process_id);
            if workers_finished && process_group_exited {
                return self.join_workers_until(deadline);
            }
            let Some(wait) = remaining(deadline) else {
                return Err(ProcessError::CleanupDeadlineExceeded);
            };
            if wait.is_zero() {
                return Err(ProcessError::CleanupDeadlineExceeded);
            }
            thread::sleep(wait.min(STATUS_POLL_INTERVAL));
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.workers.is_empty() {
            return;
        }
        self.input_sender.take();
        self.cancelling_transport.store(true, Ordering::Release);
        terminate_child(&self.child, self.process_id, true);
        let deadline = Instant::now() + DROP_CLEANUP_TIMEOUT;
        let _ = self.reap_after_termination(midpoint_deadline(deadline));
        let _ = self.wait_for_cleanup(deadline);
    }
}

fn join_worker_handles_until(
    workers: &mut Vec<(ProcessWorker, JoinHandle<()>)>,
    deadline: Instant,
) -> Result<(), ProcessError> {
    loop {
        if workers.iter().all(|(_, worker)| worker.is_finished()) {
            let workers = std::mem::take(workers);
            for (kind, worker) in workers {
                if worker.join().is_err() {
                    return Err(ProcessError::WorkerPanicked(kind));
                }
            }
            return Ok(());
        }
        let Some(wait) = remaining(deadline) else {
            break;
        };
        if wait.is_zero() {
            break;
        }
        thread::sleep(wait.min(STATUS_POLL_INTERVAL));
    }

    Err(ProcessError::CleanupDeadlineExceeded)
}

fn spawn_input_worker(
    mut stdin: impl Write + Send + 'static,
    receiver: Receiver<InputCommand>,
    budget: Arc<ByteBudget>,
    cancelling: Arc<AtomicBool>,
    stdin_closed: Arc<AtomicBool>,
    completions: Sender<Completion>,
    wake: Sender<()>,
) -> Result<JoinHandle<()>, ProcessError> {
    thread::Builder::new()
        .name("codesplice-lsp-stdin".to_owned())
        .spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    InputCommand::Bytes(bytes) => {
                        let result = write_all_cancellable(&mut stdin, &bytes, &cancelling);
                        budget.release(bytes.len());
                        if let Err(error) = result {
                            if is_terminal_stdin_error(error.kind()) {
                                stdin_closed.store(true, Ordering::Release);
                            }
                            report_fault(&completions, &wake, ProcessWorker::Stdin, error);
                            break;
                        }
                    }
                    InputCommand::Close => break,
                }
            }
        })
        .map_err(ProcessError::SpawnWorker)
}

fn spawn_stdout_worker(
    mut stdout: impl Read + Send + 'static,
    sender: Sender<StdoutItem>,
    budget: Arc<ByteBudget>,
    cancelling: Arc<AtomicBool>,
    completions: Sender<Completion>,
    wake: Sender<()>,
) -> Result<JoinHandle<()>, ProcessError> {
    thread::Builder::new()
        .name("codesplice-lsp-stdout".to_owned())
        .spawn(move || {
            let mut buffer = vec![0_u8; READ_CHUNK_BYTES];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => {
                        let mut item = StdoutItem::Closed;
                        loop {
                            match sender.send_timeout(item, STATUS_POLL_INTERVAL) {
                                Ok(()) => {
                                    notify(&wake);
                                    break;
                                }
                                Err(error) if error.is_timeout() => {
                                    item = error.into_inner();
                                    if cancelling.load(Ordering::Acquire) {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        break;
                    }
                    Ok(count) => {
                        let bytes = buffer[..count].to_vec();
                        if let Err(ProcessError::ByteCapacityExceeded {
                            queue,
                            item_bytes,
                            capacity_bytes,
                        }) = budget.reserve(bytes.len(), "inbound")
                        {
                            report_resource_fault(
                                &completions,
                                &wake,
                                ProcessWorker::Stdout,
                                queue,
                                item_bytes,
                                capacity_bytes,
                            );
                            break;
                        }
                        let mut item = StdoutItem::Bytes(bytes);
                        loop {
                            match sender.send_timeout(item, STATUS_POLL_INTERVAL) {
                                Ok(()) => break,
                                Err(error) if error.is_timeout() => {
                                    item = error.into_inner();
                                    if cancelling.load(Ordering::Acquire) {
                                        budget.release(count);
                                        return;
                                    }
                                }
                                Err(_) => {
                                    budget.release(count);
                                    return;
                                }
                            }
                        }
                        notify(&wake);
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if cancelling.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(STATUS_POLL_INTERVAL);
                    }
                    Err(error) => {
                        report_fault(&completions, &wake, ProcessWorker::Stdout, error);
                        break;
                    }
                }
            }
        })
        .map_err(ProcessError::SpawnWorker)
}

fn spawn_stderr_worker(
    mut stderr: impl Read + Send + 'static,
    tail: Arc<Mutex<StderrTail>>,
    cancelling: Arc<AtomicBool>,
    completions: Sender<Completion>,
    wake: Sender<()>,
) -> Result<JoinHandle<()>, ProcessError> {
    thread::Builder::new()
        .name("codesplice-lsp-stderr".to_owned())
        .spawn(move || {
            let mut buffer = vec![0_u8; READ_CHUNK_BYTES];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => lock_or_recover(&tail).append(&buffer[..count]),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if cancelling.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(STATUS_POLL_INTERVAL);
                    }
                    Err(error) => {
                        report_fault(&completions, &wake, ProcessWorker::Stderr, error);
                        break;
                    }
                }
            }
        })
        .map_err(ProcessError::SpawnWorker)
}

fn spawn_status_worker(
    child: Arc<Mutex<Child>>,
    observed_status: Arc<Mutex<Option<ExitStatus>>>,
    completions: Sender<Completion>,
    wake: Sender<()>,
) -> Result<JoinHandle<()>, ProcessError> {
    thread::Builder::new()
        .name("codesplice-lsp-status".to_owned())
        .spawn(move || {
            loop {
                let status_result = lock_or_recover(&child).try_wait();
                match status_result {
                    Ok(Some(status)) => {
                        *lock_or_recover(&observed_status) = Some(status);
                        let _ = completions.try_send(Completion::Exited(status));
                        notify(&wake);
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        report_fault(&completions, &wake, ProcessWorker::Status, error);
                        break;
                    }
                }
                thread::sleep(STATUS_POLL_INTERVAL);
            }
        })
        .map_err(ProcessError::SpawnWorker)
}

fn report_fault(
    completions: &Sender<Completion>,
    wake: &Sender<()>,
    worker: ProcessWorker,
    error: io::Error,
) {
    let error_kind = error.kind();
    let completion = if worker == ProcessWorker::Stdin && is_terminal_stdin_error(error_kind) {
        Completion::StdinClosed
    } else {
        Completion::Fault(ProcessFault {
            worker,
            kind: ProcessFaultKind::Io {
                error_kind,
                message: error.to_string(),
            },
        })
    };
    let _ = completions.try_send(completion);
    notify(wake);
}

fn is_terminal_stdin_error(error_kind: io::ErrorKind) -> bool {
    matches!(
        error_kind,
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::WriteZero
    )
}

fn report_resource_fault(
    completions: &Sender<Completion>,
    wake: &Sender<()>,
    worker: ProcessWorker,
    queue: &'static str,
    item_bytes: usize,
    capacity_bytes: usize,
) {
    let kind = ProcessFaultKind::ResourceLimit {
        queue,
        item_bytes,
        capacity_bytes,
    };
    let _ = completions.try_send(Completion::Fault(ProcessFault { worker, kind }));
    notify(wake);
}

fn write_all_cancellable(
    writer: &mut impl Write,
    bytes: &[u8],
    cancelling: &AtomicBool,
) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        match writer.write(&bytes[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write complete language-server frame",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if cancelling.load(Ordering::Acquire) {
                    return Ok(());
                }
                thread::sleep(STATUS_POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if cancelling.load(Ordering::Acquire) {
                    return Ok(());
                }
                thread::sleep(STATUS_POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn notify(wake: &Sender<()>) {
    let _ = wake.try_send(());
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline.checked_duration_since(Instant::now())
}

fn midpoint_deadline(deadline: Instant) -> Instant {
    let now = Instant::now();
    let remaining = deadline.saturating_duration_since(now);
    now + (remaining / 2)
}

fn bounded_grace_deadline(deadline: Instant, maximum_grace: Duration) -> Instant {
    let now = Instant::now();
    now + deadline.saturating_duration_since(now).min(maximum_grace)
}

#[cfg(unix)]
fn wait_for_process_group_exit(process_id: u32, deadline: Instant) -> bool {
    loop {
        if process_group_has_exited(process_id) {
            return true;
        }
        let Some(wait) = remaining(deadline) else {
            return false;
        };
        if wait.is_zero() {
            return false;
        }
        thread::sleep(wait.min(STATUS_POLL_INTERVAL));
    }
}

#[cfg(unix)]
fn process_group_has_exited(process_id: u32) -> bool {
    use rustix::io::Errno;
    use rustix::process::{Pid, test_kill_process_group};

    let Some(process_group) = Pid::from_raw(process_id.cast_signed()) else {
        return true;
    };
    matches!(test_kill_process_group(process_group), Err(Errno::SRCH))
}

#[cfg(not(unix))]
fn wait_for_process_group_exit(_process_id: u32, _deadline: Instant) -> bool {
    true
}

#[cfg(not(unix))]
fn process_group_has_exited(_process_id: u32) -> bool {
    true
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(unix)]
fn configure_nonblocking_pipes(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut std::process::ChildStdout,
    stderr: &mut std::process::ChildStderr,
) -> io::Result<()> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

    for pipe in [
        stdin as &dyn std::os::fd::AsFd,
        stdout as &dyn std::os::fd::AsFd,
        stderr as &dyn std::os::fd::AsFd,
    ] {
        let flags = fcntl_getfl(pipe)?;
        fcntl_setfl(pipe, flags | OFlags::NONBLOCK)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_nonblocking_pipes(
    _stdin: &mut std::process::ChildStdin,
    _stdout: &mut std::process::ChildStdout,
    _stderr: &mut std::process::ChildStderr,
) -> io::Result<()> {
    Ok(())
}

fn terminate_and_reap_unmanaged(child: &mut Child, process_id: u32) {
    terminate_unmanaged(child, process_id);
    let _ = child.wait();
}

#[cfg(unix)]
fn terminate_unmanaged(child: &mut Child, process_id: u32) {
    use rustix::process::{Pid, Signal, kill_process_group};

    if let Some(process_group) = Pid::from_raw(process_id.cast_signed()) {
        let _ = kill_process_group(process_group, Signal::KILL);
    }
    // Also target the direct child so a process-group setup/signal race cannot
    // leave synchronous construction cleanup blocked in `wait`.
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_unmanaged(child: &mut Child, _process_id: u32) {
    let _ = child.kill();
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_child(child: &Arc<Mutex<Child>>, process_id: u32, force: bool) {
    use rustix::process::{Pid, Signal, kill_process_group};

    if let Some(process_group) = Pid::from_raw(process_id.cast_signed()) {
        let signal = if force { Signal::KILL } else { Signal::TERM };
        let _ = kill_process_group(process_group, signal);
    }
    if force {
        // Also target the direct child. This is harmless after a successful
        // group signal and covers rare setup/teardown races where the group is
        // no longer addressable but the direct child still owns its pipes.
        let _ = lock_or_recover(child).kill();
    }
}

#[cfg(not(unix))]
fn terminate_child(child: &Arc<Mutex<Child>>, _process_id: u32, _force: bool) {
    let _ = lock_or_recover(child).kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn confirmed_exit_precedes_pending_io_fault() {
        let specification = ProcessSpec::new("/bin/sh").args(["-c", "exit 7"]);
        let mut process = ManagedProcess::spawn(&specification, ProcessLimits::default())
            .expect("fixture process should spawn");
        let status = process
            .finish(
                Instant::now() + Duration::from_secs(1),
                Instant::now() + Duration::from_secs(3),
            )
            .expect("fixture process should exit and be reaped");
        assert_eq!(status.code(), Some(7));

        lock_or_recover(&process.pending_faults).push_back(ProcessFault {
            worker: ProcessWorker::Stderr,
            kind: ProcessFaultKind::Io {
                error_kind: io::ErrorKind::Other,
                message: "synthetic worker failure".to_owned(),
            },
        });
        let event = process
            .try_next_event()
            .expect("ready process state should remain consumable")
            .expect("confirmed exit should remain ready");
        assert!(matches!(event, ProcessEvent::Exited(status) if status.code() == Some(7)));
    }

    #[test]
    fn stdin_closure_categories_are_structural_and_narrow() {
        assert!(is_terminal_stdin_error(io::ErrorKind::BrokenPipe));
        assert!(is_terminal_stdin_error(io::ErrorKind::WriteZero));
        assert!(!is_terminal_stdin_error(io::ErrorKind::PermissionDenied));
    }

    #[test]
    fn joining_workers_retains_unfinished_handle_at_deadline() {
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = release_receiver.recv();
        });
        let mut workers = vec![(ProcessWorker::Stdout, worker)];
        let started = Instant::now();
        let error =
            join_worker_handles_until(&mut workers, Instant::now() + Duration::from_millis(20))
                .expect_err("unfinished worker should remain owned at the deadline");
        assert!(matches!(error, ProcessError::CleanupDeadlineExceeded));
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(workers.len(), 1);
        release_sender
            .send(())
            .expect("retained test worker should still be releasable");
        join_worker_handles_until(&mut workers, Instant::now() + Duration::from_secs(1))
            .expect("released worker should be joined");
        assert!(workers.is_empty());
    }
}
