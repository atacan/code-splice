//! Process re-entry and deterministic runner support for session integration tests.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use codesplice_lsp::process::ProcessSpec;
use codesplice_test_support::fake_lsp::{FakeLspScenario, run_from_process_args};

const FIXTURE_MODE: &str = "--codesplice-lsp-fixture";
const SLEEP_FOREVER_MODE: &str = "--sleep-forever";

/// One scenario reserved for a session integration case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlannedSessionCase {
    /// Stable case name used by the custom test runner.
    pub name: &'static str,
    /// Fake-server behavior exercised by the case.
    pub scenario: FakeLspScenario,
    /// Lifecycle result expected from the production session client.
    pub outcome: ExpectedSessionOutcome,
}

/// Coarse result class used while the production session API is under construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExpectedSessionOutcome {
    /// The full lifecycle returns document symbols and shuts down cleanly.
    Success,
    /// Capability negotiation rejects the initialize result.
    CapabilityError,
    /// The server returns an explicit JSON-RPC initialize failure.
    InitializeError,
    /// Framing or JSON-RPC envelope validation rejects server output.
    ProtocolError,
    /// The child exits before the expected lifecycle boundary.
    EarlyExit,
    /// A fixed lifecycle deadline expires and cleanup is forced.
    Timeout,
}

/// Lifecycle and capability cases waiting for the production session API.
pub(crate) const PLANNED_SESSION_CASES: &[PlannedSessionCase] = &[
    PlannedSessionCase {
        name: "successful_session",
        scenario: FakeLspScenario::Success,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "configuration_before_did_open",
        scenario: FakeLspScenario::SuccessWithConfiguration,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "server_requests",
        scenario: FakeLspScenario::ServerRequests,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "stderr_is_drained",
        scenario: FakeLspScenario::StderrFlood,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "notifications_are_bounded_and_consumed",
        scenario: FakeLspScenario::NotificationFlood,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "no_document_symbols",
        scenario: FakeLspScenario::NoDocumentSymbols,
        outcome: ExpectedSessionOutcome::CapabilityError,
    },
    PlannedSessionCase {
        name: "no_document_sync",
        scenario: FakeLspScenario::NoDocumentSync,
        outcome: ExpectedSessionOutcome::CapabilityError,
    },
    PlannedSessionCase {
        name: "incremental_sync_options",
        scenario: FakeLspScenario::IncrementalSync,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "legacy_full_sync",
        scenario: FakeLspScenario::LegacyFullSync,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "legacy_incremental_sync",
        scenario: FakeLspScenario::LegacyIncrementalSync,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "open_close_false",
        scenario: FakeLspScenario::OpenCloseFalse,
        outcome: ExpectedSessionOutcome::CapabilityError,
    },
    PlannedSessionCase {
        name: "utf8_encoding",
        scenario: FakeLspScenario::Utf8Encoding,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "utf32_encoding",
        scenario: FakeLspScenario::Utf32Encoding,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "default_utf16_encoding",
        scenario: FakeLspScenario::DefaultEncoding,
        outcome: ExpectedSessionOutcome::Success,
    },
    PlannedSessionCase {
        name: "unsupported_encoding",
        scenario: FakeLspScenario::UnsupportedEncoding,
        outcome: ExpectedSessionOutcome::CapabilityError,
    },
    PlannedSessionCase {
        name: "initialize_error_response",
        scenario: FakeLspScenario::InitializeError,
        outcome: ExpectedSessionOutcome::InitializeError,
    },
    PlannedSessionCase {
        name: "malformed_header",
        scenario: FakeLspScenario::MalformedHeader,
        outcome: ExpectedSessionOutcome::ProtocolError,
    },
    PlannedSessionCase {
        name: "invalid_json",
        scenario: FakeLspScenario::InvalidJson,
        outcome: ExpectedSessionOutcome::ProtocolError,
    },
    PlannedSessionCase {
        name: "unknown_response_id",
        scenario: FakeLspScenario::UnknownResponseId,
        outcome: ExpectedSessionOutcome::ProtocolError,
    },
    PlannedSessionCase {
        name: "response_with_result_and_error",
        scenario: FakeLspScenario::ResponseAndError,
        outcome: ExpectedSessionOutcome::ProtocolError,
    },
    PlannedSessionCase {
        name: "early_exit",
        scenario: FakeLspScenario::ExitAfterInitialize,
        outcome: ExpectedSessionOutcome::EarlyExit,
    },
    PlannedSessionCase {
        name: "initialize_timeout",
        scenario: FakeLspScenario::HangInitialize,
        outcome: ExpectedSessionOutcome::Timeout,
    },
    PlannedSessionCase {
        name: "document_symbol_timeout",
        scenario: FakeLspScenario::HangDocumentSymbols,
        outcome: ExpectedSessionOutcome::Timeout,
    },
    PlannedSessionCase {
        name: "shutdown_timeout",
        scenario: FakeLspScenario::IgnoreShutdown,
        outcome: ExpectedSessionOutcome::Timeout,
    },
    PlannedSessionCase {
        name: "shutdown_descendant_cleanup",
        scenario: FakeLspScenario::IgnoreShutdownWithChild,
        outcome: ExpectedSessionOutcome::Timeout,
    },
];

/// Builder for the fake-server process used by session integration tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FakeLspCommand {
    scenario: FakeLspScenario,
    expected_document_uri: Option<String>,
    expected_language_id: Option<String>,
    expected_document_text_file: Option<PathBuf>,
}

impl FakeLspCommand {
    /// Creates a command for one named fake-server scenario.
    pub(crate) fn new(scenario: FakeLspScenario) -> Self {
        Self {
            scenario,
            expected_document_uri: None,
            expected_language_id: None,
            expected_document_text_file: None,
        }
    }

    /// Adds all exact `didOpen` expectations to the process arguments.
    pub(crate) fn expect_document(
        mut self,
        uri: impl Into<String>,
        language_id: impl Into<String>,
        text_file: impl Into<PathBuf>,
    ) -> Self {
        self.expected_document_uri = Some(uri.into());
        self.expected_language_id = Some(language_id.into());
        self.expected_document_text_file = Some(text_file.into());
        self
    }

    /// Builds the production process specification for a session under test.
    pub(crate) fn process_spec(&self) -> io::Result<ProcessSpec> {
        Ok(ProcessSpec::new(std::env::current_exe()?).args(self.arguments()))
    }

    /// Builds a standard command for testing the launch seam itself.
    pub(crate) fn command(&self) -> io::Result<Command> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(self.arguments())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(command)
    }

    fn arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            OsString::from(FIXTURE_MODE),
            OsString::from("--scenario"),
            OsString::from(self.scenario.as_str()),
        ];
        if let Some(uri) = &self.expected_document_uri {
            push_option(&mut arguments, "--expected-document-uri", uri);
        }
        if let Some(language_id) = &self.expected_language_id {
            push_option(&mut arguments, "--expected-language-id", language_id);
        }
        if let Some(text_file) = &self.expected_document_text_file {
            push_option(
                &mut arguments,
                "--expected-document-text-file",
                text_file.as_os_str(),
            );
        }
        arguments
    }
}

fn push_option(arguments: &mut Vec<OsString>, option: &str, value: impl AsRef<OsStr>) {
    arguments.push(OsString::from(option));
    arguments.push(value.as_ref().to_os_string());
}

/// Runs fixture mode when the executable was re-entered as a fake server.
pub(crate) fn run_fixture_if_requested() -> Option<ExitCode> {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next()?;
    let fixture_arguments: Box<dyn Iterator<Item = OsString>> =
        if first.as_os_str() == OsStr::new(FIXTURE_MODE) {
            Box::new(arguments)
        } else if first.as_os_str() == OsStr::new(SLEEP_FOREVER_MODE) {
            // The fake server uses current_exe for its descendant-cleanup fixture.
            Box::new(std::iter::once(first).chain(arguments))
        } else {
            return None;
        };

    Some(match run_from_process_args(fixture_arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("codesplice-lsp fixture: {error}");
            ExitCode::from(2)
        }
    })
}

/// A named fallible function run by the custom integration-test executable.
pub(crate) struct TestCase {
    name: &'static str,
    run: fn() -> Result<(), String>,
}

impl TestCase {
    /// Creates a named custom-harness test case.
    pub(crate) const fn new(name: &'static str, run: fn() -> Result<(), String>) -> Self {
        Self { name, run }
    }
}

/// Runs every case, reports every failure, and returns Cargo's conventional status.
pub(crate) fn run_tests(cases: &[TestCase]) -> ExitCode {
    println!("running {} tests", cases.len());
    let mut failures = Vec::new();
    for case in cases {
        match (case.run)() {
            Ok(()) => println!("test {} ... ok", case.name),
            Err(error) => {
                println!("test {} ... FAILED", case.name);
                failures.push((case.name, error));
            }
        }
    }

    if failures.is_empty() {
        println!("\ntest result: ok. {} passed; 0 failed", cases.len());
        ExitCode::SUCCESS
    } else {
        eprintln!("\nfailures:");
        for (name, error) in &failures {
            eprintln!("\n---- {name} ----\n{error}");
        }
        eprintln!(
            "\ntest result: FAILED. {} passed; {} failed",
            cases.len() - failures.len(),
            failures.len()
        );
        ExitCode::from(101)
    }
}

/// Resolves a repository fixture from this crate's manifest directory.
pub(crate) fn repository_fixture(relative: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
