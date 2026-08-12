#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Command grammar, protocol orchestration, and output discipline for CodeSplice.
//!
//! Phase 2 fully implements target-independent capability queries and validates
//! every other command before returning an explicit development-only error. No
//! command in this phase inspects or mutates a workspace.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{ArgAction, Args, Parser, Subcommand};
use codesplice_protocol::{
    CapabilitiesResponse, ErrorCode, ErrorDto, MAX_OPERATION_PATHS, MAX_PATH_BYTES,
    MAX_REQUEST_BYTES, ProtocolVersionResponse, escape_terminal_text, parse_request, parse_sha256,
    redact_path, to_json_line,
};
use serde_json::json;

/// Parses process arguments, runs the selected command, and returns its exit status.
///
/// Output is written according to the command's JSON or human-mode contract. This
/// Phase 2 entry point reads only an explicitly supplied request file or standard
/// input; it does not inspect or mutate the selected workspace.
#[must_use]
pub fn run() -> ExitCode {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let status = run_with_io(env::args_os(), &mut stdin, &mut stdout, &mut stderr);
    ExitCode::from(status)
}

#[derive(Debug, Parser)]
#[command(
    name = "codesplice",
    version,
    about = "Move or copy exact bytes already present in workspace files",
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect(InspectArgs),
    Apply(ApplyArgs),
    Recover(RecoverArgs),
    Capabilities(JsonOnlyArgs),
    ProtocolVersion(JsonOnlyArgs),
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(long = "path", required = true, value_name = "RELATIVE")]
    paths: Vec<String>,
    #[arg(long, required = true, action = ArgAction::SetTrue)]
    json: bool,
}

#[derive(Debug, Args)]
struct ApplyArgs {
    #[arg(long, value_name = "FILE|-", required = true)]
    request: String,
    #[command(flatten)]
    mode: ApplyMode,
    #[arg(long, value_name = "sha256:DIGEST", requires = "commit")]
    expect_plan: Option<String>,
    #[arg(long, conflicts_with = "expect_plan", requires = "commit")]
    accept_current_plan: bool,
    #[arg(long)]
    json: bool,
    #[arg(long, requires = "preview")]
    no_diff: bool,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct ApplyMode {
    #[arg(long)]
    preview: bool,
    #[arg(long)]
    commit: bool,
}

#[derive(Debug, Args)]
struct RecoverArgs {
    #[arg(
        value_name = "ID",
        required_unless_present = "list",
        conflicts_with = "list"
    )]
    id: Option<String>,
    #[command(flatten)]
    action: RecoveryAction,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct RecoveryAction {
    #[arg(long, conflicts_with = "id")]
    list: bool,
    #[arg(long, requires = "id")]
    status: bool,
    #[arg(long, requires = "id")]
    complete: bool,
    #[arg(long, requires = "id")]
    rollback: bool,
}

#[derive(Debug, Args)]
struct JsonOnlyArgs {
    #[arg(long, required = true, action = ArgAction::SetTrue)]
    json: bool,
}

fn run_with_io<I, T>(
    arguments: I,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments = arguments
        .into_iter()
        .map(Into::into)
        .collect::<Vec<OsString>>();
    let json_requested = arguments.iter().any(|argument| argument == "--json");

    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return if stdout.write_all(error.to_string().as_bytes()).is_ok() {
                0
            } else {
                8
            };
        }
        Err(error) => {
            let report = ErrorDto::new(
                ErrorCode::InvalidCli,
                "the command line does not match the CodeSplice grammar",
                BTreeMap::from([("reason".to_string(), json!(error.to_string()))]),
            );
            return render_error(&report, json_requested, stdout, stderr);
        }
    };

    match execute(cli, stdin) {
        Ok(response) => render_success(&response, stdout, stderr),
        Err((report, json)) => render_error(&report, json, stdout, stderr),
    }
}

fn execute(cli: Cli, stdin: &mut dyn Read) -> Result<String, (ErrorDto, bool)> {
    let has_workspace = cli.workspace.is_some();
    match cli.command {
        Command::Capabilities(arguments) => {
            reject_workspace_for_target_independent(has_workspace, arguments.json)?;
            serialize_success(&CapabilitiesResponse::phase_two(), arguments.json)
        }
        Command::ProtocolVersion(arguments) => {
            reject_workspace_for_target_independent(has_workspace, arguments.json)?;
            serialize_success(&ProtocolVersionResponse::current(), arguments.json)
        }
        Command::Inspect(arguments) => {
            validate_inspect_paths(&arguments.paths).map_err(|report| (report, arguments.json))?;
            Err((
                development_unimplemented("workspace_inspection"),
                arguments.json,
            ))
        }
        Command::Apply(arguments) => execute_apply(arguments, stdin),
        Command::Recover(arguments) => {
            let route = if arguments.action.list {
                "recovery_list"
            } else if arguments.action.status {
                "recovery_status"
            } else if arguments.action.complete {
                "recovery_complete"
            } else if arguments.action.rollback {
                "recovery_rollback"
            } else {
                "recovery"
            };
            let _transaction_id = arguments.id;
            Err((development_unimplemented(route), arguments.json))
        }
    }
}

fn execute_apply(arguments: ApplyArgs, stdin: &mut dyn Read) -> Result<String, (ErrorDto, bool)> {
    if arguments.mode.commit && arguments.expect_plan.is_none() && !arguments.accept_current_plan {
        return Err((
            ErrorDto::new(
                ErrorCode::ExpectedPlanRequired,
                "commit requires exactly one expected-plan policy",
                BTreeMap::new(),
            ),
            arguments.json,
        ));
    }

    if let Some(expected) = &arguments.expect_plan {
        parse_sha256(expected, "--expect-plan")
            .map_err(|error| (error.into_report(), arguments.json))?;
    }

    let request =
        read_request(&arguments.request, stdin).map_err(|report| (report, arguments.json))?;
    parse_request(&request).map_err(|error| (error.into_report(), arguments.json))?;

    let route = if arguments.mode.preview {
        if arguments.no_diff {
            "preview_without_diff"
        } else {
            "preview"
        }
    } else {
        "commit"
    };
    Err((development_unimplemented(route), arguments.json))
}

fn read_request(path: &str, stdin: &mut dyn Read) -> Result<Vec<u8>, ErrorDto> {
    if path == "-" {
        return read_bounded(stdin, "standard input");
    }

    let mut file = File::open(path).map_err(|error| {
        ErrorDto::new(
            ErrorCode::IoError,
            "failed to open the request file",
            BTreeMap::from([
                ("io_kind".to_string(), json!(format!("{:?}", error.kind()))),
                ("path".to_string(), json!(redact_path(path))),
            ]),
        )
    })?;
    read_bounded(&mut file, "request file")
}

fn read_bounded(reader: &mut dyn Read, source: &'static str) -> Result<Vec<u8>, ErrorDto> {
    let take_limit = MAX_REQUEST_BYTES.saturating_add(1);
    let mut bounded = reader.take(take_limit);
    let mut bytes = Vec::new();
    bounded.read_to_end(&mut bytes).map_err(|error| {
        ErrorDto::new(
            ErrorCode::IoError,
            "failed to read the JSON request",
            BTreeMap::from([
                ("io_kind".to_string(), json!(format!("{:?}", error.kind()))),
                ("source".to_string(), json!(source)),
            ]),
        )
    })?;
    Ok(bytes)
}

fn validate_inspect_paths(paths: &[String]) -> Result<(), ErrorDto> {
    let actual = u64::try_from(paths.len()).unwrap_or(u64::MAX);
    if actual > MAX_OPERATION_PATHS {
        return Err(limit_error("operation_paths", actual, MAX_OPERATION_PATHS));
    }
    for path in paths {
        let length = u64::try_from(path.len()).unwrap_or(u64::MAX);
        if length > MAX_PATH_BYTES {
            return Err(limit_error("path_bytes", length, MAX_PATH_BYTES));
        }
        if path.contains('\0') {
            return Err(ErrorDto::new(
                ErrorCode::InvalidRequest,
                "an inspection path contains an invalid value",
                BTreeMap::from([("reason".to_string(), json!("path_contains_nul"))]),
            ));
        }
    }
    Ok(())
}

fn limit_error(resource: &'static str, actual: u64, limit: u64) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::ResourceLimitExceeded,
        "a command resource limit was exceeded",
        BTreeMap::from([
            ("actual".to_string(), json!(actual)),
            ("limit".to_string(), json!(limit)),
            ("resource".to_string(), json!(resource)),
        ]),
    )
}

fn reject_workspace_for_target_independent(
    has_workspace: bool,
    json: bool,
) -> Result<(), (ErrorDto, bool)> {
    if has_workspace {
        return Err((
            ErrorDto::new(
                ErrorCode::InvalidCli,
                "--workspace is not accepted by target-independent commands",
                BTreeMap::new(),
            ),
            json,
        ));
    }
    Ok(())
}

fn development_unimplemented(capability: &'static str) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::InternalError,
        "the command is validated but execution is not implemented in Phase 2",
        BTreeMap::from([
            ("capability".to_string(), json!(capability)),
            ("development_only".to_string(), json!(true)),
            ("implementation_phase".to_string(), json!(2)),
        ]),
    )
}

fn serialize_success<T: serde::Serialize>(
    response: &T,
    json: bool,
) -> Result<String, (ErrorDto, bool)> {
    to_json_line(response).map_err(|error| (error.into_report(), json))
}

fn render_success(response: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    if stdout.write_all(response.as_bytes()).is_ok() {
        0
    } else {
        let _ = stderr.write_all(b"codesplice: INTERNAL_ERROR: failed to write stdout\n");
        8
    }
}

fn render_error(
    report: &ErrorDto,
    json: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let result = if json {
        to_json_line(report)
            .and_then(|line| {
                stdout.write_all(line.as_bytes()).map_err(|_| {
                    codesplice_protocol::ProtocolError::new(ErrorDto::new(
                        ErrorCode::InternalError,
                        "failed to write stdout",
                        BTreeMap::new(),
                    ))
                })
            })
            .is_ok()
    } else {
        let line = format!(
            "codesplice: {}: {}\n",
            report.code().as_str(),
            escape_terminal_text(report.message())
        );
        stderr.write_all(line.as_bytes()).is_ok()
    };

    if result { report.exit_code() } else { 8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(arguments: &[&str], stdin: &[u8]) -> (u8, String, String) {
        let mut input = stdin;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = run_with_io(arguments, &mut input, &mut stdout, &mut stderr);
        (
            status,
            String::from_utf8(stdout).expect("stdout must be UTF-8"),
            String::from_utf8(stderr).expect("stderr must be UTF-8"),
        )
    }

    #[test]
    fn commit_should_require_one_expected_plan_policy() {
        let request = br#"{"protocol_version":1,"operations":[]}"#;

        let (status, stdout, stderr) = invoke(
            &[
                "codesplice",
                "apply",
                "--request",
                "-",
                "--commit",
                "--json",
            ],
            request,
        );

        assert_eq!(status, 3);
        assert!(stdout.contains("EXPECTED_PLAN_REQUIRED"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn human_errors_should_escape_terminal_control_characters() {
        let report = ErrorDto::new(
            ErrorCode::InvalidCli,
            "unsafe\u{202e}\nmessage",
            BTreeMap::new(),
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let status = render_error(&report, false, &mut stdout, &mut stderr);

        assert_eq!(status, 2);
        assert_eq!(
            stderr,
            b"codesplice: INVALID_CLI: unsafe\\u{202e}\\u{a}message\n"
        );
    }
}
