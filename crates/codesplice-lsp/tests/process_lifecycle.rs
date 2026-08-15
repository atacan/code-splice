//! Unix integration tests for bounded language-server process supervision.

#![cfg(unix)]

use std::thread;
use std::time::{Duration, Instant};

use codesplice_lsp::process::{
    ManagedProcess, ProcessError, ProcessEvent, ProcessLimits, ProcessSpec, ProcessWorker,
};

const SHORT: Duration = Duration::from_millis(250);
const CLEANUP: Duration = Duration::from_secs(3);

fn shell(script: &str, limits: ProcessLimits) -> ManagedProcess {
    ManagedProcess::spawn(&ProcessSpec::new("/bin/sh").args(["-c", script]), limits)
        .expect("fixture process should spawn")
}

fn deadline_after(duration: Duration) -> Instant {
    Instant::now() + duration
}

fn wait_for_pid_disappearance(pid: rustix::process::Pid) -> bool {
    use rustix::io::Errno;
    use rustix::process::test_kill_process;

    (0..100).any(|_| {
        if matches!(test_kill_process(pid), Err(Errno::SRCH)) {
            true
        } else {
            thread::sleep(Duration::from_millis(10));
            false
        }
    })
}

#[test]
fn completion_capacity_requires_one_slot_per_terminal_producer() {
    let below = ProcessLimits {
        completion_events: 3,
        ..ProcessLimits::default()
    };
    let result = ManagedProcess::spawn(&ProcessSpec::new("/bin/sh").args(["-c", "exit 0"]), below);
    let Err(error) = result else {
        panic!("capacity below four terminal producers must be rejected");
    };
    assert!(matches!(error, ProcessError::InvalidLimits));

    let at = ProcessLimits {
        completion_events: 4,
        ..ProcessLimits::default()
    };
    let mut process = shell("exit 0", at);
    process
        .finish(deadline_after(SHORT), deadline_after(CLEANUP))
        .expect("one completion slot per terminal producer should be accepted");
}

#[test]
fn every_process_capacity_rejects_zero() {
    let mut cases = Vec::new();
    let defaults = ProcessLimits::default();
    cases.push(ProcessLimits {
        inbound_events: 0,
        ..defaults
    });
    cases.push(ProcessLimits {
        inbound_bytes: 0,
        ..defaults
    });
    cases.push(ProcessLimits {
        outbound_frames: 0,
        ..defaults
    });
    cases.push(ProcessLimits {
        outbound_bytes: 0,
        ..defaults
    });
    cases.push(ProcessLimits {
        stderr_tail_bytes: 0,
        ..defaults
    });

    for limits in cases {
        let result = ManagedProcess::spawn(&ProcessSpec::new("/bin/true"), limits);
        assert!(matches!(result, Err(ProcessError::InvalidLimits)));
    }
}

#[test]
fn observes_early_exit_and_reaps_child() {
    let mut process = shell("exit 7", ProcessLimits::default());
    let deadline = deadline_after(CLEANUP);
    let status = loop {
        let event = process
            .next_event(deadline)
            .expect("exit event should arrive");
        if let ProcessEvent::Exited(status) = event {
            break status;
        }
    };
    assert_eq!(status.code(), Some(7));

    let status = process
        .finish(deadline_after(SHORT), deadline_after(CLEANUP))
        .expect("already exited process should finish");
    assert_eq!(status.code(), Some(7));
}

#[test]
fn event_wait_honors_fixed_deadline() {
    let mut process = shell("sleep 60", ProcessLimits::default());
    let started = Instant::now();
    let error = process
        .next_event(deadline_after(Duration::from_millis(40)))
        .expect_err("silent child should time out");
    assert!(matches!(error, ProcessError::DeadlineExceeded(_)));
    assert!(started.elapsed() < Duration::from_secs(1));
    process
        .abort(deadline_after(CLEANUP))
        .expect("timed-out process should be cleaned up");
}

#[test]
fn rejects_single_outbound_frame_over_cumulative_byte_limit() {
    let limits = ProcessLimits {
        outbound_bytes: 8,
        ..ProcessLimits::default()
    };
    let mut process = shell("sleep 60", limits);
    let error = process
        .send_frame(vec![0; 9], deadline_after(SHORT))
        .expect_err("oversized frame must be rejected before queueing");
    assert!(matches!(
        error,
        ProcessError::ByteCapacityExceeded {
            queue: "outbound",
            item_bytes: 9,
            capacity_bytes: 8
        }
    ));
    assert_eq!(process.queued_outbound_bytes(), 0);
    process
        .abort(deadline_after(CLEANUP))
        .expect("process should be cleaned up");
}

#[test]
fn outbound_byte_capacity_accepts_below_and_at_limit() {
    for frame_bytes in [7, 8] {
        let limits = ProcessLimits {
            outbound_bytes: 8,
            ..ProcessLimits::default()
        };
        let mut process = shell("sleep 60", limits);
        process
            .send_frame(vec![0; frame_bytes], deadline_after(SHORT))
            .expect("frame at or below byte capacity should be queued");
        process
            .abort(deadline_after(CLEANUP))
            .expect("process should be cleaned up");
    }
}

#[test]
fn inbound_byte_capacity_accepts_at_limit_and_reports_above() {
    let at_limits = ProcessLimits {
        inbound_bytes: 8,
        ..ProcessLimits::default()
    };
    let mut at = shell("printf 12345678; sleep 60", at_limits);
    let event = at
        .next_event(deadline_after(CLEANUP))
        .expect("exactly bounded stdout should arrive");
    assert!(matches!(event, ProcessEvent::Stdout(bytes) if bytes == b"12345678"));
    at.abort(deadline_after(CLEANUP))
        .expect("at-limit process should be cleaned up");

    let above_limits = ProcessLimits {
        inbound_bytes: 7,
        ..ProcessLimits::default()
    };
    let mut above = shell("printf 12345678; sleep 60", above_limits);
    let error = loop {
        match above.next_event(deadline_after(CLEANUP)) {
            Ok(ProcessEvent::Fault(fault)) => break fault,
            Ok(ProcessEvent::Stdout(_)) => continue,
            Ok(event) => panic!("unexpected event before resource fault: {event:?}"),
            Err(error) => panic!("resource fault should be delivered: {error:?}"),
        }
    };
    assert!(matches!(
        error.kind,
        codesplice_lsp::process::ProcessFaultKind::ResourceLimit {
            queue: "inbound",
            capacity_bytes: 7,
            ..
        }
    ));
    above
        .abort(deadline_after(CLEANUP))
        .expect("above-limit process should be cleaned up");
}

#[test]
fn blocked_stdin_queue_honors_deadline_and_abort_does_not_hang() {
    let limits = ProcessLimits {
        outbound_frames: 1,
        outbound_bytes: 24 * 1024 * 1024,
        ..ProcessLimits::default()
    };
    let mut process = shell("sleep 60", limits);
    let large_frame = vec![b'x'; 8 * 1024 * 1024];
    process
        .send_frame(large_frame.clone(), deadline_after(SHORT))
        .expect("first frame should be accepted");
    process
        .send_frame(large_frame.clone(), deadline_after(SHORT))
        .expect("writer should take the first frame, leaving one queue slot");
    let error = process
        .send_frame(large_frame, deadline_after(Duration::from_millis(40)))
        .expect_err("full outbound channel should honor deadline");
    assert!(matches!(error, ProcessError::DeadlineExceeded(_)));

    process
        .abort(deadline_after(CLEANUP))
        .expect("blocked stdin writer should unblock after process-group kill");
}

#[test]
fn stdout_saturation_is_bounded_and_abort_does_not_hang() {
    let limits = ProcessLimits {
        inbound_events: 1,
        inbound_bytes: 16 * 1024,
        ..ProcessLimits::default()
    };
    let mut process = shell("while :; do printf '0123456789abcdef'; done", limits);
    thread::sleep(Duration::from_millis(80));
    assert!(process.queued_inbound_bytes() <= limits.inbound_bytes);

    let started = Instant::now();
    process
        .abort(deadline_after(CLEANUP))
        .expect("full stdout queue must not wedge cleanup");
    assert!(started.elapsed() < CLEANUP);
}

#[test]
fn consuming_stdout_releases_cumulative_byte_budget() {
    let limits = ProcessLimits {
        inbound_events: 2,
        inbound_bytes: 32,
        ..ProcessLimits::default()
    };
    let mut process = shell("printf 'hello'; sleep 60", limits);
    let event = process
        .next_event(deadline_after(CLEANUP))
        .expect("stdout should arrive");
    assert!(matches!(event, ProcessEvent::Stdout(bytes) if bytes == b"hello"));
    assert_eq!(process.queued_inbound_bytes(), 0);
    process
        .abort(deadline_after(CLEANUP))
        .expect("process should be cleaned up");
}

#[test]
fn stderr_is_drained_without_blocking_and_only_bounded_tail_is_retained() {
    let limits = ProcessLimits {
        stderr_tail_bytes: 64,
        ..ProcessLimits::default()
    };
    let mut process = shell(
        "i=0; while [ $i -lt 4096 ]; do printf x >&2; i=$((i+1)); done; exit 0",
        limits,
    );
    let status = process
        .finish(deadline_after(CLEANUP), deadline_after(CLEANUP + CLEANUP))
        .expect("stderr flood should not block child exit");
    assert!(status.success());
    assert_eq!(process.stderr_tail(), vec![b'x'; 64]);
}

#[test]
fn process_spec_debug_redacts_program_paths_arguments_and_environment_values() {
    let specification = ProcessSpec::new("/secret/path/server")
        .arg("--token=secret")
        .env("SECRET", "do-not-print")
        .current_directory("/secret/workspace");
    let rendered = format!("{specification:?}");
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains("do-not-print"));
    assert!(rendered.contains("argument_count: 1"));
    assert!(rendered.contains("environment_entry_count: 1"));
}

#[test]
fn completion_fault_does_not_prevent_forced_cleanup() {
    let mut process = shell("exec 1>&-; sleep 60", ProcessLimits::default());
    let event = process
        .next_event(deadline_after(CLEANUP))
        .expect("stdout closure should be observable");
    assert!(matches!(event, ProcessEvent::StdoutClosed));
    process
        .abort(deadline_after(CLEANUP))
        .expect("closed stdout should not prevent cleanup");
}

#[test]
fn stdin_command_channel_disconnect_retains_terminal_cause() {
    let mut process = shell(
        "exec 0<&-; printf ready; sleep 60",
        ProcessLimits::default(),
    );
    let ready = process
        .next_event(deadline_after(CLEANUP))
        .expect("child should announce that stdin is closed");
    assert!(matches!(ready, ProcessEvent::Stdout(bytes) if bytes == b"ready"));

    process
        .send_frame(vec![b'x'; 1024 * 1024 + 1], deadline_after(SHORT))
        .expect("large frame should reach the asynchronous writer");
    let event = process
        .next_event(deadline_after(CLEANUP))
        .expect("closed stdin should be observable");
    assert!(matches!(event, ProcessEvent::StdinClosed));

    let error = process
        .send_frame(vec![b'x'], deadline_after(SHORT))
        .expect_err("closed stdin worker should disconnect its command channel");
    assert!(matches!(error, ProcessError::StdinClosed));

    process
        .abort(deadline_after(CLEANUP))
        .expect("live child with disconnected stdin should be cleaned up");
}

#[test]
fn exit_is_delivered_before_ready_stdout() {
    let mut process = shell("printf data", ProcessLimits::default());
    thread::sleep(Duration::from_millis(80));
    let first = process
        .next_event(deadline_after(CLEANUP))
        .expect("ready event should arrive");
    assert!(matches!(first, ProcessEvent::Exited(_)));
    let second = process
        .next_event(deadline_after(CLEANUP))
        .expect("stdout should remain queued");
    assert!(matches!(second, ProcessEvent::Stdout(bytes) if bytes == b"data"));
    process
        .finish(deadline_after(SHORT), deadline_after(CLEANUP))
        .expect("process should already be reaped");
}

#[test]
fn natural_parent_exit_terminates_pipe_holding_process_group_descendant() {
    use rustix::process::{Pid, test_kill_process};

    let mut process = shell(
        "trap '' TERM; sleep 60 & printf '%s\\n' \"$!\"; exit 0",
        ProcessLimits::default(),
    );
    let event_deadline = deadline_after(CLEANUP);
    let descendant_pid = loop {
        let event = process
            .next_event(event_deadline)
            .expect("descendant pid should be written before cleanup");
        if let ProcessEvent::Stdout(bytes) = event {
            let pid_text = std::str::from_utf8(&bytes)
                .expect("descendant pid should be ASCII")
                .trim();
            let raw_pid: i32 = pid_text.parse().expect("descendant pid should be numeric");
            break Pid::from_raw(raw_pid).expect("descendant pid should be positive");
        }
    };
    assert!(
        test_kill_process(descendant_pid).is_ok(),
        "TERM-ignoring descendant should be alive before finish"
    );

    let started = Instant::now();
    let status = process
        .finish(deadline_after(SHORT), deadline_after(CLEANUP))
        .expect("natural parent exit should still clean up its process group");
    assert!(status.success());
    assert!(started.elapsed() < CLEANUP);
    assert!(
        wait_for_pid_disappearance(descendant_pid),
        "finish must not return while the TERM-ignoring descendant survives"
    );
}

#[test]
fn process_worker_enum_remains_exhaustive_for_diagnostics() {
    assert_eq!(format!("{:?}", ProcessWorker::Stdin), "Stdin");
    assert_eq!(format!("{:?}", ProcessWorker::Stdout), "Stdout");
    assert_eq!(format!("{:?}", ProcessWorker::Stderr), "Stderr");
    assert_eq!(format!("{:?}", ProcessWorker::Status), "Status");
}

#[test]
fn forced_cleanup_terminates_descendants_in_dedicated_process_group() {
    use rustix::process::Pid;

    let mut process = shell("sleep 60 & printf '%s' $!; wait", ProcessLimits::default());
    let event = process
        .next_event(deadline_after(CLEANUP))
        .expect("descendant pid should be written");
    let ProcessEvent::Stdout(bytes) = event else {
        panic!("expected descendant pid on stdout: {event:?}");
    };
    let pid_text = std::str::from_utf8(&bytes).expect("pid should be ASCII");
    let raw_pid: i32 = pid_text.parse().expect("pid should be numeric");
    let pid = Pid::from_raw(raw_pid).expect("descendant pid should be positive");

    process
        .abort(deadline_after(CLEANUP))
        .expect("process group should be terminated and direct child reaped");
    let disappeared = wait_for_pid_disappearance(pid);
    assert!(
        disappeared,
        "descendant should not survive process-group cleanup"
    );
}
