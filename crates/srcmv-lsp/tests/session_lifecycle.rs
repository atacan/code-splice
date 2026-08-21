//! Custom-harness session integration tests with fake-server process re-entry.

mod support;

use std::ffi::OsStr;
use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use codesplice_lsp::capabilities::CapabilityError;
use codesplice_lsp::session::{
    ImmutableDocument, SessionDeadlines, SessionError, SessionInput, SessionLimits, run_session,
};
use codesplice_lsp::transport::{TransportError, TransportLimits};
use codesplice_test_support::fake_lsp::FakeLspScenario;
use gen_lsp_types::{Uri, WorkspaceFolder};
use serde_json::json;
use support::{
    ExpectedSessionOutcome, FakeLspCommand, PLANNED_SESSION_CASES, TestCase, repository_fixture,
    run_fixture_if_requested, run_tests,
};

const DOCUMENT_URI: &str = "file:///fixture/workspace/source.rs";
const LANGUAGE_ID: &str = "fixture-rust";

fn main() -> ExitCode {
    if let Some(exit_code) = run_fixture_if_requested() {
        return exit_code;
    }

    run_tests(&[
        TestCase::new("fixture_arguments_are_exact", fixture_arguments_are_exact),
        TestCase::new(
            "fixture_reentry_launches_without_cargo",
            fixture_reentry_launches_without_cargo,
        ),
        TestCase::new(
            "planned_session_matrix_is_complete",
            planned_session_matrix_is_complete,
        ),
        TestCase::new(
            "production_session_executes_planned_matrix",
            production_session_executes_planned_matrix,
        ),
        TestCase::new(
            "early_exit_transport_signals_stay_terminal",
            early_exit_transport_signals_stay_terminal,
        ),
        TestCase::new(
            "session_rate_limits_are_enforced",
            session_rate_limits_are_enforced,
        ),
        TestCase::new(
            "session_payload_limits_are_checked_before_spawn",
            session_payload_limits_are_checked_before_spawn,
        ),
    ])
}

fn early_exit_transport_signals_stay_terminal() -> Result<(), String> {
    for iteration in 0..256 {
        let result = run_case(FakeLspScenario::ExitAfterInitialize);
        if !matches!(
            result,
            Err(SessionError::Transport(
                TransportError::Exited(_)
                    | TransportError::StdoutClosed
                    | TransportError::StdinClosed
            ))
        ) {
            return Err(format!(
                "early-exit iteration {iteration} produced nonterminal classification: {result:?}"
            ));
        }
    }
    Ok(())
}

fn production_session_executes_planned_matrix() -> Result<(), String> {
    for case in PLANNED_SESSION_CASES {
        let result = run_case(case.scenario);
        let matches = match (case.outcome, &result) {
            (ExpectedSessionOutcome::Success, Ok(output)) => {
                output.symbols.is_array()
                    && output.timings.initialize <= SessionDeadlines::default().initialize
                    && output.timings.document_symbols
                        <= SessionDeadlines::default().document_symbols
                    && output.timings.shutdown <= SessionDeadlines::default().shutdown
            }
            (
                ExpectedSessionOutcome::CapabilityError,
                Err(SessionError::Capability(
                    CapabilityError::DocumentSymbolsUnavailable
                    | CapabilityError::DocumentSyncUnavailable
                    | CapabilityError::UnsupportedPositionEncoding,
                )),
            ) => true,
            (
                ExpectedSessionOutcome::InitializeError,
                Err(SessionError::RequestFailed {
                    method: "initialize",
                    ..
                }),
            ) => true,
            (
                ExpectedSessionOutcome::ProtocolError,
                Err(SessionError::Transport(TransportError::Protocol(_))),
            ) => true,
            (
                ExpectedSessionOutcome::EarlyExit,
                Err(SessionError::Transport(
                    TransportError::Exited(_)
                    | TransportError::StdoutClosed
                    | TransportError::StdinClosed,
                )),
            ) => true,
            (ExpectedSessionOutcome::Timeout, Err(SessionError::Timeout(_))) => true,
            _ => false,
        };
        if !matches {
            return Err(format!(
                "scenario `{}` produced unexpected result: {result:?}",
                case.name
            ));
        }
    }
    Ok(())
}

fn run_case(
    scenario: FakeLspScenario,
) -> Result<codesplice_lsp::session::SessionOutput, SessionError> {
    run_case_with_limits(scenario, SessionLimits::default())
}

fn run_case_with_limits(
    scenario: FakeLspScenario,
    limits: SessionLimits,
) -> Result<codesplice_lsp::session::SessionOutput, SessionError> {
    let document_path = repository_fixture("tests/fixtures/lsp/documents/source.rs");
    let text = std::fs::read_to_string(&document_path)
        .map_err(|_| SessionError::InvalidLspPayload("fixture document"))?;
    let process = FakeLspCommand::new(scenario)
        .expect_document(DOCUMENT_URI, LANGUAGE_ID, &document_path)
        .process_spec()
        .map_err(|error| {
            SessionError::Transport(TransportError::Process(
                codesplice_lsp::process::ProcessError::Spawn(error),
            ))
        })?;
    let short = Duration::from_millis(120);
    let deadlines = SessionDeadlines {
        initialize: if scenario == FakeLspScenario::HangInitialize {
            short
        } else {
            Duration::from_secs(2)
        },
        document_symbols: if scenario == FakeLspScenario::HangDocumentSymbols {
            short
        } else {
            Duration::from_secs(2)
        },
        shutdown: if matches!(
            scenario,
            FakeLspScenario::IgnoreShutdown | FakeLspScenario::IgnoreShutdownWithChild
        ) {
            short
        } else {
            Duration::from_secs(2)
        },
        cleanup: Duration::from_secs(2),
        total: Duration::from_secs(6),
    };
    let settings = matches!(
        scenario,
        FakeLspScenario::SuccessWithConfiguration | FakeLspScenario::ServerRequests
    )
    .then(|| json!({"fixture": {"enabled": true}}));
    let workspace_uri: Uri = "file:///fixture/workspace/"
        .parse()
        .map_err(|_| SessionError::InvalidLspPayload("workspace URI"))?;
    let document_uri: Uri = DOCUMENT_URI
        .parse()
        .map_err(|_| SessionError::InvalidLspPayload("document URI"))?;
    run_session(
        SessionInput {
            process,
            workspace: WorkspaceFolder::new(workspace_uri, "workspace".to_owned()),
            document: ImmutableDocument {
                uri: document_uri,
                language_id: LANGUAGE_ID.to_owned(),
                text,
            },
            initialization_options: None,
            settings,
            deadlines,
            limits,
        },
        TransportLimits::default(),
    )
}

fn session_rate_limits_are_enforced() -> Result<(), String> {
    run_case_with_limits(
        FakeLspScenario::NotificationFlood,
        SessionLimits {
            notifications: 64,
            ..SessionLimits::default()
        },
    )
    .map_err(|error| format!("the exact notification limit must succeed: {error:?}"))?;

    let notification_error = run_case_with_limits(
        FakeLspScenario::NotificationFlood,
        SessionLimits {
            notifications: 63,
            ..SessionLimits::default()
        },
    )
    .expect_err("the 64th notification must exceed the configured limit");
    if !matches!(
        notification_error,
        SessionError::ResourceLimit {
            resource: "notification",
            limit: 63
        }
    ) {
        return Err(format!(
            "notification limit produced unexpected error: {notification_error:?}"
        ));
    }

    run_case_with_limits(
        FakeLspScenario::ServerRequests,
        SessionLimits {
            server_requests: 5,
            ..SessionLimits::default()
        },
    )
    .map_err(|error| format!("the exact server-request limit must succeed: {error:?}"))?;

    let request_error = run_case_with_limits(
        FakeLspScenario::ServerRequests,
        SessionLimits {
            server_requests: 4,
            ..SessionLimits::default()
        },
    )
    .expect_err("the fifth server request must exceed the configured limit");
    if !matches!(
        request_error,
        SessionError::ResourceLimit {
            resource: "server request",
            limit: 4
        }
    ) {
        return Err(format!(
            "server-request limit produced unexpected error: {request_error:?}"
        ));
    }
    Ok(())
}

fn session_payload_limits_are_checked_before_spawn() -> Result<(), String> {
    let base = SessionLimits::default();
    let source_at_limit = run_case_with_limits(
        FakeLspScenario::Success,
        SessionLimits {
            source_bytes: std::fs::read(repository_fixture(
                "tests/fixtures/lsp/documents/source.rs",
            ))
            .map_err(|error| error.to_string())?
            .len(),
            ..base
        },
    );
    if source_at_limit.is_err() {
        return Err(format!(
            "source at its exact byte limit failed: {source_at_limit:?}"
        ));
    }

    let error = run_case_with_limits(
        FakeLspScenario::Success,
        SessionLimits {
            source_bytes: 0,
            ..base
        },
    )
    .expect_err("source above a zero-byte limit must fail");
    if !matches!(
        error,
        SessionError::ResourceLimit {
            resource: "source bytes",
            limit: 0
        }
    ) {
        return Err(format!("source limit produced unexpected error: {error:?}"));
    }

    let settings_error = run_case_with_limits(
        FakeLspScenario::SuccessWithConfiguration,
        SessionLimits {
            settings_bytes: 1,
            ..base
        },
    )
    .expect_err("settings above their serialized limit must fail");
    if !matches!(
        settings_error,
        SessionError::ResourceLimit {
            resource: "settings bytes",
            limit: 1
        }
    ) {
        return Err(format!(
            "settings limit produced unexpected error: {settings_error:?}"
        ));
    }
    Ok(())
}

fn fixture_arguments_are_exact() -> Result<(), String> {
    let document = repository_fixture("tests/fixtures/lsp/documents/source.rs");
    let command = FakeLspCommand::new(FakeLspScenario::Success)
        .expect_document(DOCUMENT_URI, LANGUAGE_ID, &document)
        .command()
        .map_err(|error| error.to_string())?;
    let arguments: Vec<_> = command.get_args().collect();
    let expected = [
        OsStr::new("--codesplice-lsp-fixture"),
        OsStr::new("--scenario"),
        OsStr::new("success"),
        OsStr::new("--expected-document-uri"),
        OsStr::new(DOCUMENT_URI),
        OsStr::new("--expected-language-id"),
        OsStr::new(LANGUAGE_ID),
        OsStr::new("--expected-document-text-file"),
        document.as_os_str(),
    ];
    if arguments != expected {
        return Err(format!(
            "fixture arguments differed: actual={arguments:?}, expected={expected:?}"
        ));
    }

    let process_spec = FakeLspCommand::new(FakeLspScenario::Success)
        .process_spec()
        .map_err(|error| error.to_string())?;
    let current_executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if process_spec.program() != current_executable.as_os_str() {
        return Err("process specification did not use the current test executable".to_owned());
    }
    Ok(())
}

fn fixture_reentry_launches_without_cargo() -> Result<(), String> {
    let mut child = FakeLspCommand::new(FakeLspScenario::ExitAfterInitialize)
        .command()
        .map_err(|error| error.to_string())?
        .spawn()
        .map_err(|error| error.to_string())?;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": 4242,
            "clientInfo": {"name": "codesplice", "version": "0.2.1"},
            "rootUri": "file:///fixture/workspace/",
            "workspaceFolders": [{"uri": "file:///fixture/workspace/", "name": "workspace"}],
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]},
                "textDocument": {"documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true
                }},
                "window": {"workDoneProgress": false},
                "workspace": {"applyEdit": false}
            }
        }
    });
    let body = serde_json::to_vec(&initialize).map_err(|error| error.to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "fixture stdin was not piped".to_owned())?;
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len())
        .and_then(|()| stdin.write_all(&body))
        .map_err(|error| error.to_string())?;
    drop(stdin);

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().map_err(|error| error.to_string())?;
            let _status = child.wait().map_err(|error| error.to_string())?;
            return Err("fixture re-entry did not exit before its deadline".to_owned());
        }
        thread::sleep(Duration::from_millis(5));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "fixture process failed: status={}, stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stdout.starts_with(b"Content-Length: ")
        || !output.stdout.windows(4).any(|bytes| bytes == b"\r\n\r\n")
    {
        return Err(format!(
            "fixture response differed: {:?}",
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    Ok(())
}

fn planned_session_matrix_is_complete() -> Result<(), String> {
    let required = [
        FakeLspScenario::Success,
        FakeLspScenario::SuccessWithConfiguration,
        FakeLspScenario::ServerRequests,
        FakeLspScenario::StderrFlood,
        FakeLspScenario::NotificationFlood,
        FakeLspScenario::NoDocumentSymbols,
        FakeLspScenario::NoDocumentSync,
        FakeLspScenario::IncrementalSync,
        FakeLspScenario::LegacyFullSync,
        FakeLspScenario::LegacyIncrementalSync,
        FakeLspScenario::OpenCloseFalse,
        FakeLspScenario::Utf8Encoding,
        FakeLspScenario::Utf32Encoding,
        FakeLspScenario::DefaultEncoding,
        FakeLspScenario::UnsupportedEncoding,
        FakeLspScenario::InitializeError,
        FakeLspScenario::MalformedHeader,
        FakeLspScenario::InvalidJson,
        FakeLspScenario::UnknownResponseId,
        FakeLspScenario::ResponseAndError,
        FakeLspScenario::ExitAfterInitialize,
        FakeLspScenario::HangInitialize,
        FakeLspScenario::HangDocumentSymbols,
        FakeLspScenario::IgnoreShutdown,
        FakeLspScenario::IgnoreShutdownWithChild,
    ];
    for scenario in required {
        let occurrences = PLANNED_SESSION_CASES
            .iter()
            .filter(|case| case.scenario == scenario)
            .count();
        if occurrences != 1 {
            return Err(format!(
                "scenario `{scenario}` has {occurrences} planned cases instead of one"
            ));
        }
    }
    if PLANNED_SESSION_CASES
        .iter()
        .any(|case| case.name.is_empty())
    {
        return Err("planned case names must not be empty".to_owned());
    }
    let success_cases = PLANNED_SESSION_CASES
        .iter()
        .filter(|case| case.outcome == support::ExpectedSessionOutcome::Success)
        .count();
    if success_cases != 11 {
        return Err(format!(
            "expected 11 successful lifecycle cases, found {success_cases}"
        ));
    }
    Ok(())
}
