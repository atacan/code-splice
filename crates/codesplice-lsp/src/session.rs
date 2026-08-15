//! Stateful, read-only LSP document-symbol sessions.

use std::fmt;
use std::time::{Duration, Instant};

use gen_lsp_types::{InitializeResult, WorkspaceFolder};
use serde_json::{Value, json};

use crate::capabilities::{
    CapabilityError, NegotiatedCapabilities, initialize_params, validate_initialize_result,
};
use crate::jsonrpc::{
    ClientRequestId, IncomingMessage, ResponsePayload, ServerRequest, encode_message,
};
use crate::process::ProcessSpec;
use crate::transport::{Transport, TransportError, TransportLimits};

/// Default maximum server-initiated requests in one selection session.
pub const DEFAULT_MAX_SERVER_REQUESTS: usize = 64;
/// Default maximum server notifications in one selection session.
pub const DEFAULT_MAX_NOTIFICATIONS: usize = 1024;
/// Default maximum immutable source bytes sent in `didOpen`.
pub const DEFAULT_MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Default maximum serialized initialization-options bytes.
pub const DEFAULT_MAX_INITIALIZATION_OPTIONS_BYTES: usize = 1024 * 1024;
/// Default maximum serialized settings bytes.
pub const DEFAULT_MAX_SETTINGS_BYTES: usize = 1024 * 1024;
/// Default maximum items in one `workspace/configuration` request.
pub const DEFAULT_MAX_CONFIGURATION_ITEMS: usize = 256;
/// Default maximum serialized `workspace/configuration` result bytes.
pub const DEFAULT_MAX_CONFIGURATION_RESPONSE_BYTES: usize = 1024 * 1024;

/// Immutable source document opened in the language server.
#[derive(Clone, Eq, PartialEq)]
pub struct ImmutableDocument {
    /// Canonical file URI.
    pub uri: gen_lsp_types::Uri,
    /// Language identifier supplied to the server.
    pub language_id: String,
    /// Exact UTF-8 snapshot text.
    pub text: String,
}

impl fmt::Debug for ImmutableDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmutableDocument")
            .field("uri", &"<redacted>")
            .field("language_id", &self.language_id)
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

/// Deadline durations for one LSP session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionDeadlines {
    /// Maximum initialization time.
    pub initialize: Duration,
    /// Maximum document-symbol request time.
    pub document_symbols: Duration,
    /// Maximum graceful shutdown time.
    pub shutdown: Duration,
    /// Maximum forced-cleanup time after a failure.
    pub cleanup: Duration,
    /// Maximum total session wall-clock time.
    pub total: Duration,
}

impl Default for SessionDeadlines {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(10),
            document_symbols: Duration::from_secs(30),
            shutdown: Duration::from_secs(5),
            cleanup: Duration::from_secs(5),
            total: Duration::from_secs(60),
        }
    }
}

/// Per-session inbound message limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLimits {
    /// Maximum server requests handled while awaiting client responses.
    pub server_requests: usize,
    /// Maximum notifications ignored while awaiting client responses.
    pub notifications: usize,
    /// Maximum exact UTF-8 source bytes sent in `didOpen`.
    pub source_bytes: usize,
    /// Maximum serialized initialization-options bytes.
    pub initialization_options_bytes: usize,
    /// Maximum serialized settings bytes.
    pub settings_bytes: usize,
    /// Maximum items accepted in one `workspace/configuration` request.
    pub configuration_items: usize,
    /// Maximum serialized result bytes for one `workspace/configuration` request.
    pub configuration_response_bytes: usize,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            server_requests: DEFAULT_MAX_SERVER_REQUESTS,
            notifications: DEFAULT_MAX_NOTIFICATIONS,
            source_bytes: DEFAULT_MAX_SOURCE_BYTES,
            initialization_options_bytes: DEFAULT_MAX_INITIALIZATION_OPTIONS_BYTES,
            settings_bytes: DEFAULT_MAX_SETTINGS_BYTES,
            configuration_items: DEFAULT_MAX_CONFIGURATION_ITEMS,
            configuration_response_bytes: DEFAULT_MAX_CONFIGURATION_RESPONSE_BYTES,
        }
    }
}

/// Complete explicit input for one read-only document-symbol session.
#[derive(Clone)]
pub struct SessionInput {
    /// Language-server executable and literal arguments.
    pub process: ProcessSpec,
    /// The single canonical workspace folder.
    pub workspace: WorkspaceFolder,
    /// Immutable source document.
    pub document: ImmutableDocument,
    /// Optional server-specific initialization options.
    pub initialization_options: Option<Value>,
    /// Optional settings sent after initialization and served to configuration requests.
    pub settings: Option<Value>,
    /// Phase and total deadlines.
    pub deadlines: SessionDeadlines,
    /// Server request and notification limits.
    pub limits: SessionLimits,
}

impl fmt::Debug for SessionInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionInput")
            .field("process", &self.process)
            .field("workspace", &"<redacted>")
            .field("document", &self.document)
            .field(
                "initialization_options",
                &self.initialization_options.as_ref().map(|_| "<redacted>"),
            )
            .field("settings", &self.settings.as_ref().map(|_| "<redacted>"))
            .field("deadlines", &self.deadlines)
            .field("limits", &self.limits)
            .finish()
    }
}

/// Successful session result for later symbol validation and resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionOutput {
    /// Validated initialization capabilities.
    pub capabilities: NegotiatedCapabilities,
    /// Raw document-symbol result retained for structurally unambiguous normalization.
    ///
    /// The generated LSP response union is intentionally not used here because
    /// its untagged empty array is ambiguous between hierarchical and flat
    /// results. The symbol layer validates this value before producing matches.
    pub symbols: Value,
    /// Wall-clock lifecycle timings measured inside the client.
    pub timings: SessionTimings,
}

/// Wall-clock measurements for one completed LSP selection lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionTimings {
    /// Initialization through capability validation and configured notification.
    pub initialize: Duration,
    /// `didOpen` through document-symbol response and `didClose`.
    pub document_symbols: Duration,
    /// Shutdown request through process cleanup.
    pub shutdown: Duration,
}

/// Lifecycle phase associated with a session failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPhase {
    /// Process creation and initialization.
    Initialize,
    /// Document opening and symbol request.
    DocumentSymbols,
    /// Graceful protocol shutdown.
    Shutdown,
}

/// A typed, non-sensitive session failure.
#[derive(Debug)]
pub enum SessionError {
    /// The bounded transport failed.
    Transport(TransportError),
    /// Required server capabilities were absent or unsupported.
    Capability(CapabilityError),
    /// A generated LSP value could not be encoded or decoded.
    InvalidLspPayload(&'static str),
    /// The server returned a JSON-RPC error for a client request.
    RequestFailed {
        /// Method whose request failed.
        method: &'static str,
        /// JSON-RPC error code.
        code: i64,
    },
    /// A response arrived for a different currently awaited request.
    UnexpectedResponse,
    /// A phase deadline expired.
    Timeout(SessionPhase),
    /// A per-session request or notification limit was exceeded.
    ResourceLimit {
        /// Resource whose limit was exceeded.
        resource: &'static str,
        /// Configured maximum.
        limit: usize,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => {
                write!(formatter, "language-server transport failed: {error}")
            }
            Self::Capability(error) => error.fmt(formatter),
            Self::InvalidLspPayload(payload) => write!(formatter, "invalid {payload} payload"),
            Self::RequestFailed { method, code } => {
                write!(
                    formatter,
                    "language-server request `{method}` failed with code {code}"
                )
            }
            Self::UnexpectedResponse => formatter.write_str("unexpected language-server response"),
            Self::Timeout(phase) => write!(formatter, "language-server {phase:?} deadline expired"),
            Self::ResourceLimit { resource, limit } => {
                write!(
                    formatter,
                    "language-server {resource} limit of {limit} exceeded"
                )
            }
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Capability(error) => Some(error),
            _ => None,
        }
    }
}

/// Runs one complete read-only LSP session and always cleans up its process.
///
/// # Errors
///
/// Returns a typed error for transport, protocol, capability, deadline, or
/// resource-limit failures. Every failure path attempts forced process-group
/// cleanup before returning.
pub fn run_session(
    input: SessionInput,
    transport_limits: TransportLimits,
) -> Result<SessionOutput, SessionError> {
    validate_input_limits(&input)?;
    preflight_did_open(&input, transport_limits)?;
    let initialize_started = Instant::now();
    let total_deadline = deadline_from_now(input.deadlines.total, "total deadline")?;
    let _cleanup_deadline = deadline_from_now(input.deadlines.cleanup, "cleanup deadline")?;
    let mut transport =
        Transport::spawn(&input.process, transport_limits).map_err(SessionError::Transport)?;
    let result = run_active_session(&mut transport, &input, total_deadline, initialize_started);
    if result.is_err() {
        let cleanup_deadline = deadline_from_now(input.deadlines.cleanup, "cleanup deadline")
            .unwrap_or_else(|_| Instant::now());
        let _ = transport.abort(cleanup_deadline);
    }
    result
}

fn validate_input_limits(input: &SessionInput) -> Result<(), SessionError> {
    ensure_within_limit(
        input.document.text.len(),
        input.limits.source_bytes,
        "source bytes",
    )?;
    if let Some(options) = &input.initialization_options {
        ensure_within_limit(
            serialized_len(options, "initialization options")?,
            input.limits.initialization_options_bytes,
            "initialization-options bytes",
        )?;
    }
    if let Some(settings) = &input.settings {
        ensure_within_limit(
            serialized_len(settings, "settings")?,
            input.limits.settings_bytes,
            "settings bytes",
        )?;
    }
    Ok(())
}

fn did_open_message(input: &SessionInput) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {
            "uri": input.document.uri,
            "languageId": input.document.language_id,
            "version": 1,
            "text": input.document.text,
        }}
    })
}

fn preflight_did_open(
    input: &SessionInput,
    transport_limits: TransportLimits,
) -> Result<(), SessionError> {
    encode_message(&did_open_message(input), transport_limits.framing)
        .map(|_| ())
        .map_err(|error| SessionError::Transport(TransportError::Protocol(error)))
}

fn serialized_len(value: &Value, name: &'static str) -> Result<usize, SessionError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| SessionError::InvalidLspPayload(name))
}

fn ensure_within_limit(
    actual: usize,
    limit: usize,
    resource: &'static str,
) -> Result<(), SessionError> {
    if actual > limit {
        return Err(SessionError::ResourceLimit { resource, limit });
    }
    Ok(())
}

fn run_active_session(
    transport: &mut Transport,
    input: &SessionInput,
    total_deadline: Instant,
    initialize_started: Instant,
) -> Result<SessionOutput, SessionError> {
    let mut counters = MessageCounters::default();
    let initialize_deadline = phase_deadline(total_deadline, input.deadlines.initialize);
    let mut params = serde_json::to_value(initialize_params(
        i32::try_from(std::process::id()).ok(),
        Some(env!("CARGO_PKG_VERSION").to_owned()),
        input.workspace.uri.clone(),
        input.workspace.name.clone(),
    ))
    .map_err(|_| SessionError::InvalidLspPayload("initialize parameters"))?;
    if let Some(options) = &input.initialization_options {
        params["initializationOptions"] = options.clone();
    }
    let initialize_id = transport
        .send_request("initialize", Some(params), initialize_deadline)
        .map_err(map_transport(SessionPhase::Initialize))?;
    let initialize_value = await_response(
        transport,
        initialize_id,
        "initialize",
        initialize_deadline,
        SessionPhase::Initialize,
        input,
        &mut counters,
    )?;
    let initialize_result: InitializeResult = serde_json::from_value(initialize_value)
        .map_err(|_| SessionError::InvalidLspPayload("initialize result"))?;
    let capabilities =
        validate_initialize_result(&initialize_result).map_err(SessionError::Capability)?;

    send_notification(
        transport,
        "initialized",
        json!({}),
        initialize_deadline,
        SessionPhase::Initialize,
    )?;
    if let Some(settings) = &input.settings {
        send_notification(
            transport,
            "workspace/didChangeConfiguration",
            json!({"settings": settings}),
            initialize_deadline,
            SessionPhase::Initialize,
        )?;
    }
    let initialize_elapsed = initialize_started.elapsed();

    let document_symbols_started = Instant::now();
    let document_deadline = phase_deadline(total_deadline, input.deadlines.document_symbols);
    transport
        .send_value(&did_open_message(input), document_deadline)
        .map_err(map_transport(SessionPhase::DocumentSymbols))?;
    let symbol_id = transport
        .send_request(
            "textDocument/documentSymbol",
            Some(json!({"textDocument": {"uri": input.document.uri}})),
            document_deadline,
        )
        .map_err(map_transport(SessionPhase::DocumentSymbols))?;
    let symbol_value = await_response(
        transport,
        symbol_id,
        "textDocument/documentSymbol",
        document_deadline,
        SessionPhase::DocumentSymbols,
        input,
        &mut counters,
    )?;
    let symbols = symbol_value;
    send_notification(
        transport,
        "textDocument/didClose",
        json!({"textDocument": {"uri": input.document.uri}}),
        document_deadline,
        SessionPhase::DocumentSymbols,
    )?;
    let document_symbols_elapsed = document_symbols_started.elapsed();

    let shutdown_started = Instant::now();
    let shutdown_deadline = phase_deadline(total_deadline, input.deadlines.shutdown);
    let shutdown_id = transport
        .send_request("shutdown", Some(Value::Null), shutdown_deadline)
        .map_err(map_transport(SessionPhase::Shutdown))?;
    let shutdown = await_response(
        transport,
        shutdown_id,
        "shutdown",
        shutdown_deadline,
        SessionPhase::Shutdown,
        input,
        &mut counters,
    )?;
    if !shutdown.is_null() {
        return Err(SessionError::InvalidLspPayload("shutdown result"));
    }
    send_notification(
        transport,
        "exit",
        Value::Null,
        shutdown_deadline,
        SessionPhase::Shutdown,
    )?;
    transport
        .finish(
            shutdown_deadline,
            deadline_from_now(input.deadlines.cleanup, "cleanup deadline")?,
        )
        .map_err(SessionError::Transport)?;
    let shutdown_elapsed = shutdown_started.elapsed();

    Ok(SessionOutput {
        capabilities,
        symbols,
        timings: SessionTimings {
            initialize: initialize_elapsed,
            document_symbols: document_symbols_elapsed,
            shutdown: shutdown_elapsed,
        },
    })
}

#[derive(Default)]
struct MessageCounters {
    requests: usize,
    notifications: usize,
}

#[allow(clippy::too_many_arguments)]
fn await_response(
    transport: &mut Transport,
    expected_id: ClientRequestId,
    method: &'static str,
    deadline: Instant,
    phase: SessionPhase,
    input: &SessionInput,
    counters: &mut MessageCounters,
) -> Result<Value, SessionError> {
    loop {
        let message = transport
            .next_incoming(deadline)
            .map_err(map_transport(phase))?;
        match message {
            IncomingMessage::Response(response) if response.id == expected_id => {
                return match response.payload {
                    ResponsePayload::Result(value) => Ok(value),
                    ResponsePayload::Error(error) => Err(SessionError::RequestFailed {
                        method,
                        code: error.code,
                    }),
                };
            }
            IncomingMessage::Response(_) => return Err(SessionError::UnexpectedResponse),
            IncomingMessage::Notification(_) => {
                counters.notifications = counters.notifications.saturating_add(1);
                if counters.notifications > input.limits.notifications {
                    return Err(SessionError::ResourceLimit {
                        resource: "notification",
                        limit: input.limits.notifications,
                    });
                }
            }
            IncomingMessage::Request(request) => {
                counters.requests = counters.requests.saturating_add(1);
                if counters.requests > input.limits.server_requests {
                    return Err(SessionError::ResourceLimit {
                        resource: "server request",
                        limit: input.limits.server_requests,
                    });
                }
                let response = dispatch_server_request(request, input)?;
                transport
                    .send_value(&response, deadline)
                    .map_err(map_transport(phase))?;
            }
        }
    }
}

fn dispatch_server_request(
    request: ServerRequest,
    input: &SessionInput,
) -> Result<Value, SessionError> {
    let id = request.id.to_json();
    let response = match request.method.as_str() {
        "workspace/workspaceFolders" => json!({
            "jsonrpc": "2.0", "id": id, "result": [input.workspace]
        }),
        "workspace/configuration" => {
            let items = request
                .params
                .as_ref()
                .and_then(|params| params.get("items"))
                .and_then(Value::as_array)
                .ok_or(SessionError::InvalidLspPayload("configuration request"))?;
            ensure_within_limit(
                items.len(),
                input.limits.configuration_items,
                "configuration items",
            )?;
            let mut encoded_bytes = 2_usize;
            ensure_within_limit(
                encoded_bytes,
                input.limits.configuration_response_bytes,
                "configuration response bytes",
            )?;
            let mut result = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let configured = configured_section(input.settings.as_ref(), item)?;
                let item_bytes = match configured {
                    Some(value) => serialized_len(value, "configuration result")?,
                    None => 4,
                };
                encoded_bytes = encoded_bytes
                    .checked_add(usize::from(index != 0))
                    .and_then(|bytes| bytes.checked_add(item_bytes))
                    .ok_or(SessionError::ResourceLimit {
                        resource: "configuration response bytes",
                        limit: input.limits.configuration_response_bytes,
                    })?;
                ensure_within_limit(
                    encoded_bytes,
                    input.limits.configuration_response_bytes,
                    "configuration response bytes",
                )?;
                result.push(configured.cloned().unwrap_or(Value::Null));
            }
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        "window/showMessageRequest" => {
            json!({"jsonrpc": "2.0", "id": id, "result": null})
        }
        "workspace/applyEdit" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"applied": false, "failureReason": "CodeSplice selection is read-only"}
        }),
        _ => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": "Method not found"}
        }),
    };
    Ok(response)
}

fn configured_section<'a>(
    settings: Option<&'a Value>,
    item: &Value,
) -> Result<Option<&'a Value>, SessionError> {
    let item = item
        .as_object()
        .ok_or(SessionError::InvalidLspPayload("configuration item"))?;
    let Some(section) = item.get("section") else {
        return Ok(settings);
    };
    let section = section
        .as_str()
        .ok_or(SessionError::InvalidLspPayload("configuration section"))?;
    Ok(settings.and_then(|settings| {
        section
            .split('.')
            .try_fold(settings, |value, key| value.get(key))
    }))
}

fn send_notification(
    transport: &Transport,
    method: &'static str,
    params: Value,
    deadline: Instant,
    phase: SessionPhase,
) -> Result<(), SessionError> {
    transport
        .send_value(
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
            deadline,
        )
        .map_err(map_transport(phase))
}

fn phase_deadline(total: Instant, duration: Duration) -> Instant {
    Instant::now()
        .checked_add(duration)
        .map_or(total, |phase| phase.min(total))
}

fn deadline_from_now(duration: Duration, name: &'static str) -> Result<Instant, SessionError> {
    Instant::now()
        .checked_add(duration)
        .ok_or(SessionError::InvalidLspPayload(name))
}

fn map_transport(phase: SessionPhase) -> impl FnOnce(TransportError) -> SessionError {
    move |error| match error {
        TransportError::DeadlineExceeded => SessionError::Timeout(phase),
        TransportError::Process(crate::process::ProcessError::DeadlineExceeded(_)) => {
            SessionError::Timeout(phase)
        }
        other => SessionError::Transport(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::ServerRequestId;

    fn input(settings: Option<Value>, limits: SessionLimits) -> SessionInput {
        SessionInput {
            process: ProcessSpec::new("unused"),
            workspace: WorkspaceFolder::new(
                "file:///workspace/".parse().expect("valid workspace URI"),
                "workspace".to_owned(),
            ),
            document: ImmutableDocument {
                uri: "file:///workspace/source.rs"
                    .parse()
                    .expect("valid document URI"),
                language_id: "rust".to_owned(),
                text: String::new(),
            },
            initialization_options: None,
            settings,
            deadlines: SessionDeadlines::default(),
            limits,
        }
    }

    fn configuration_request(items: Value) -> ServerRequest {
        ServerRequest {
            id: ServerRequestId::Integer(1),
            method: "workspace/configuration".to_owned(),
            params: Some(json!({"items": items})),
        }
    }

    #[test]
    fn configuration_item_and_response_bounds_accept_at_and_reject_above() {
        let empty_response = dispatch_server_request(
            configuration_request(json!([])),
            &input(
                None,
                SessionLimits {
                    configuration_response_bytes: 2,
                    ..SessionLimits::default()
                },
            ),
        )
        .expect("an empty result is exactly two bytes");
        assert_eq!(empty_response["result"], json!([]));

        let empty_error = dispatch_server_request(
            configuration_request(json!([])),
            &input(
                None,
                SessionLimits {
                    configuration_response_bytes: 1,
                    ..SessionLimits::default()
                },
            ),
        )
        .expect_err("an empty result is one byte over the configured limit");
        assert!(matches!(
            empty_error,
            SessionError::ResourceLimit {
                resource: "configuration response bytes",
                limit: 1
            }
        ));

        let at_limit = SessionLimits {
            configuration_items: 1,
            configuration_response_bytes: 5,
            ..SessionLimits::default()
        };
        let response = dispatch_server_request(
            configuration_request(json!([{}])),
            &input(Some(json!("x")), at_limit),
        )
        .expect("one three-byte value plus brackets is exactly five bytes");
        assert_eq!(response["result"], json!(["x"]));

        let item_error = dispatch_server_request(
            configuration_request(json!([{}, {}])),
            &input(Some(json!("x")), at_limit),
        )
        .expect_err("two items exceed the one-item limit");
        assert!(matches!(
            item_error,
            SessionError::ResourceLimit {
                resource: "configuration items",
                limit: 1
            }
        ));

        let response_error = dispatch_server_request(
            configuration_request(json!([{}])),
            &input(
                Some(json!("x")),
                SessionLimits {
                    configuration_response_bytes: 4,
                    ..SessionLimits::default()
                },
            ),
        )
        .expect_err("the exact response is one byte over the configured limit");
        assert!(matches!(
            response_error,
            SessionError::ResourceLimit {
                resource: "configuration response bytes",
                limit: 4
            }
        ));
    }

    #[test]
    fn configuration_items_and_sections_are_strictly_typed() {
        for items in [json!([null]), json!([1]), json!([{"section": 1}])] {
            let error = dispatch_server_request(
                configuration_request(items),
                &input(Some(json!({"fixture": true})), SessionLimits::default()),
            )
            .expect_err("malformed configuration item must fail");
            assert!(matches!(error, SessionError::InvalidLspPayload(_)));
        }
    }
}
