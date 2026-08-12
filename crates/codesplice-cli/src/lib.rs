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
use codesplice_fs::{FsError, InspectedState, SnapshotLimits, Workspace};
use codesplice_protocol::{
    CapabilitiesResponse, ErrorCode, ErrorDto, InspectPathResponse, InspectResponse,
    MAX_OPERATION_PATHS, MAX_PATH_BYTES, MAX_REQUEST_BYTES, ProtocolVersionResponse,
    RecoveryEntryResponse, RecoveryListResponse, RecoveryStatusResponse, WarningCode, WarningDto,
    escape_terminal_text, parse_request, parse_sha256, redact_path, to_json_line,
};
use serde_json::json;

/// Parses process arguments, runs the selected command, and returns its exit status.
///
/// Output is written according to the command's JSON or human-mode contract. This
/// Inspection uses read-only Phase 3 workspace acquisition. Other execution
/// routes read only an explicitly supplied request file or standard input and
/// remain development-only stubs.
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
            serialize_success(&CapabilitiesResponse::phase_five(), arguments.json)
        }
        Command::ProtocolVersion(arguments) => {
            reject_workspace_for_target_independent(has_workspace, arguments.json)?;
            serialize_success(&ProtocolVersionResponse::current(), arguments.json)
        }
        Command::Inspect(arguments) => {
            validate_inspect_paths(&arguments.paths).map_err(|report| (report, arguments.json))?;
            execute_inspect(cli.workspace.as_deref(), arguments)
        }
        Command::Apply(arguments) => execute_apply(arguments, stdin),
        Command::Recover(arguments) => execute_recovery(cli.workspace.as_deref(), arguments),
    }
}

fn execute_inspect(
    workspace_path: Option<&std::path::Path>,
    arguments: InspectArgs,
) -> Result<String, (ErrorDto, bool)> {
    let workspace = Workspace::open(workspace_path.unwrap_or_else(|| std::path::Path::new(".")))
        .map_err(|error| (filesystem_error(error), arguments.json))?;
    let inspections = workspace
        .inspect(&arguments.paths, SnapshotLimits::default())
        .map_err(|error| (filesystem_error(error), arguments.json))?;
    let paths = inspections
        .into_iter()
        .map(|inspection| match inspection.state {
            InspectedState::Existing {
                digest,
                byte_length,
                line_count,
                identity_hash,
            } => InspectPathResponse::existing(
                inspection.path.value,
                digest,
                byte_length,
                line_count,
                identity_hash,
            ),
            InspectedState::Absent => InspectPathResponse::absent(inspection.path.value),
        })
        .collect();
    let warning = WarningDto::new(
        WarningCode::ObservationMayBeStale,
        "inspection is read-only and is not coordinated by a Phase 5 workspace lock",
        BTreeMap::new(),
    );
    serialize_success(&InspectResponse::new(paths, vec![warning]), arguments.json)
}

fn filesystem_error(error: FsError) -> ErrorDto {
    match error {
        FsError::UnsupportedPlatform => ErrorDto::new(
            ErrorCode::UnsupportedPlatform,
            "workspace inspection requires Linux or macOS",
            BTreeMap::new(),
        ),
        FsError::WorkspaceRootNotDirectory => ErrorDto::new(
            ErrorCode::InvalidRequest,
            "the selected workspace root is not a directory",
            BTreeMap::from([("reason".to_string(), json!("workspace_not_directory"))]),
        ),
        FsError::InvalidPath { path, reason } => ErrorDto::new(
            ErrorCode::InvalidRequest,
            "an inspection path is invalid",
            BTreeMap::from([
                ("path".to_string(), json!(redact_path(&path))),
                ("reason".to_string(), json!(reason)),
            ]),
        ),
        FsError::SymlinkNotAllowed { path } => ErrorDto::new(
            ErrorCode::SymlinkNotAllowed,
            "an inspection path traverses or names a symbolic link",
            BTreeMap::from([("path".to_string(), json!(path))]),
        ),
        FsError::UnsupportedFileType { path } => ErrorDto::new(
            ErrorCode::UnsupportedFileType,
            "an inspection path is not a regular file",
            BTreeMap::from([("path".to_string(), json!(path))]),
        ),
        FsError::PreconditionFailed {
            path,
            expected,
            actual,
        } => {
            let mut context = BTreeMap::from([("path".to_string(), json!(path))]);
            context.insert(
                "expected".to_string(),
                json!(expected.map(codesplice_core::Sha256Digest::to_prefixed_hex)),
            );
            context.insert(
                "actual".to_string(),
                json!(actual.map(codesplice_core::Sha256Digest::to_prefixed_hex)),
            );
            ErrorDto::new(
                ErrorCode::PreconditionFailed,
                "a path precondition does not match the stable workspace state",
                context,
            )
        }
        FsError::IncompatiblePrecondition { path } => ErrorDto::new(
            ErrorCode::EditConflict,
            "one path has incompatible preconditions",
            BTreeMap::from([
                ("path".to_string(), json!(path)),
                ("reason".to_string(), json!("incompatible_preconditions")),
            ]),
        ),
        FsError::FileAlias {
            first_path,
            second_path,
        } => ErrorDto::new(
            ErrorCode::FileAlias,
            "distinct paths identify the same existing file",
            BTreeMap::from([
                ("first_path".to_string(), json!(first_path)),
                ("second_path".to_string(), json!(second_path)),
            ]),
        ),
        FsError::FileChanged { path, attempts } => ErrorDto::new(
            ErrorCode::FileChanged,
            "a file remained unstable during bounded snapshot acquisition",
            BTreeMap::from([
                ("attempts".to_string(), json!(attempts)),
                ("path".to_string(), json!(path)),
            ]),
        ),
        FsError::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        }
        | FsError::Core(codesplice_core::CoreError::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        }) => limit_error(resource, actual, limit),
        FsError::TransactionBusy => ErrorDto::new(
            ErrorCode::TransactionBusy,
            "another CodeSplice process holds the workspace lock",
            BTreeMap::new(),
        ),
        FsError::TransactionRecoveryRequired { transaction_ids } => ErrorDto::new(
            ErrorCode::TransactionRecoveryRequired,
            "unfinished transactions require explicit recovery",
            BTreeMap::from([("transaction_ids".to_string(), json!(transaction_ids))]),
        ),
        FsError::TransactionNotFound { transaction_id } => ErrorDto::new(
            ErrorCode::TransactionNotFound,
            "the requested transaction does not exist",
            BTreeMap::from([("transaction_id".to_string(), json!(transaction_id))]),
        ),
        FsError::RecoveryActionNotAllowed {
            transaction_id,
            reason,
        } => ErrorDto::new(
            ErrorCode::RecoveryActionNotAllowed,
            "the requested recovery action is not safe in the current state",
            BTreeMap::from([
                ("reason".to_string(), json!(reason)),
                ("transaction_id".to_string(), json!(transaction_id)),
            ]),
        ),
        FsError::ControlDirectoryInvalid { reason } => ErrorDto::new(
            ErrorCode::ControlDirectoryInvalid,
            "the workspace control tree is invalid",
            BTreeMap::from([("reason".to_string(), json!(reason))]),
        ),
        FsError::TransactionRecordCorrupt {
            transaction_id,
            reason,
        } => {
            let mut context = BTreeMap::from([("reason".to_string(), json!(reason))]);
            if let Some(transaction_id) = transaction_id {
                context.insert("transaction_id".to_string(), json!(transaction_id));
            }
            ErrorDto::new(
                ErrorCode::TransactionRecordCorrupt,
                "a transaction record is corrupt",
                context,
            )
        }
        FsError::RecoveryConflict { reason } => ErrorDto::new(
            ErrorCode::RecoveryConflict,
            "filesystem observations conflict with the transaction journal",
            BTreeMap::from([("reason".to_string(), json!(reason))]),
        ),
        FsError::Io {
            operation,
            path,
            kind,
        } => {
            let mut context = BTreeMap::from([
                ("io_kind".to_string(), json!(format!("{kind:?}"))),
                ("operation".to_string(), json!(operation)),
            ]);
            if let Some(path) = path {
                context.insert("path".to_string(), json!(path));
            }
            ErrorDto::new(ErrorCode::IoError, "workspace inspection failed", context)
        }
        FsError::Core(error) => ErrorDto::new(
            ErrorCode::InternalError,
            "the core snapshot model rejected acquired data",
            BTreeMap::from([("reason".to_string(), json!(error.to_string()))]),
        ),
        FsError::InternalInvariant { invariant } => ErrorDto::new(
            ErrorCode::InternalError,
            "an internal workspace inspection invariant failed",
            BTreeMap::from([("invariant".to_string(), json!(invariant))]),
        ),
        _ => ErrorDto::new(
            ErrorCode::InternalError,
            "an unrecognized filesystem error occurred",
            BTreeMap::new(),
        ),
    }
}

fn execute_recovery(
    workspace_path: Option<&std::path::Path>,
    arguments: RecoverArgs,
) -> Result<String, (ErrorDto, bool)> {
    if let Some(transaction_id) = arguments.id.as_deref() {
        validate_transaction_id_argument(transaction_id)
            .map_err(|report| (report, arguments.json))?;
    }
    let workspace = Workspace::open(workspace_path.unwrap_or_else(|| std::path::Path::new(".")))
        .map_err(|error| (filesystem_error(error), arguments.json))?;
    if arguments.action.list {
        let observation = workspace
            .recovery_list()
            .map_err(|error| (filesystem_error(error), arguments.json))?;
        if arguments.json {
            let entries = observation
                .entries()
                .iter()
                .map(recovery_entry_response)
                .collect();
            return serialize_success(&RecoveryListResponse::new(entries), true);
        }
        let mut output = String::new();
        for entry in observation.entries() {
            output.push_str(&format!(
                "{} {} [{}]\n",
                entry.transaction_id(),
                entry.kind().as_str(),
                entry.actions().join(",")
            ));
        }
        return Ok(output);
    }

    let transaction_id = arguments.id.as_deref().ok_or_else(|| {
        (
            ErrorDto::new(
                ErrorCode::InvalidCli,
                "recovery requires a transaction ID",
                BTreeMap::new(),
            ),
            arguments.json,
        )
    })?;
    if arguments.action.status {
        let entry = workspace
            .recovery_status(transaction_id)
            .map_err(|error| (filesystem_error(error), arguments.json))?;
        if arguments.json {
            return serialize_success(
                &RecoveryStatusResponse::new(recovery_entry_response(&entry)),
                true,
            );
        }
        return Ok(format!(
            "{} {} [{}]\n",
            entry.transaction_id(),
            entry.kind().as_str(),
            entry.actions().join(",")
        ));
    }
    if arguments.action.rollback {
        workspace
            .recovery_rollback_control_only(transaction_id)
            .map_err(|error| (filesystem_error(error), arguments.json))?;
        let completed =
            RecoveryEntryResponse::new(transaction_id, "cleanup_only", std::iter::empty::<&str>());
        if arguments.json {
            return serialize_success(&RecoveryStatusResponse::new(completed), true);
        }
        return Ok(format!("{transaction_id} rolled_back_control_only\n"));
    }
    Err((
        development_unimplemented("recovery_complete"),
        arguments.json,
    ))
}

fn recovery_entry_response(entry: &codesplice_fs::RecoveryEntry) -> RecoveryEntryResponse {
    RecoveryEntryResponse::new(
        entry.transaction_id(),
        entry.kind().as_str(),
        entry.actions().iter().copied(),
    )
}

fn validate_transaction_id_argument(transaction_id: &str) -> Result<(), ErrorDto> {
    if transaction_id.len() == 32
        && transaction_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ErrorDto::new(
            ErrorCode::InvalidRequest,
            "the transaction ID must be exactly 32 lowercase hexadecimal characters",
            BTreeMap::from([("reason".to_string(), json!("invalid_transaction_id"))]),
        ))
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
