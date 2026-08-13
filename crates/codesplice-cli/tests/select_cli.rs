//! End-to-end semantic-selection CLI tests with fake-server process re-entry.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use codesplice_fs::Workspace;
use codesplice_test_support::fake_lsp::run_from_process_args;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const HOLD_MODE: &str = "--hold-before-success";
type TestFunction = fn() -> Result<(), String>;

fn main() -> ExitCode {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|value| value == HOLD_MODE) {
        return run_holding_fixture(&arguments[1..]);
    }
    if arguments.first().is_some_and(|value| value == "--scenario") {
        return fake_server(arguments);
    }

    let tests: &[(&str, TestFunction)] = &[
        (
            "fake_selection_composes_into_preview",
            fake_selection_composes_into_preview,
        ),
        (
            "slow_server_does_not_hold_diagnostic_lock",
            slow_server_does_not_hold_diagnostic_lock,
        ),
        (
            "flat_symbols_return_typed_error",
            flat_symbols_return_typed_error,
        ),
        (
            "non_utf8_source_fails_before_spawn",
            non_utf8_source_fails_before_spawn,
        ),
        (
            "human_output_reports_auditable_ranges",
            human_output_reports_auditable_ranges,
        ),
    ];
    let mut failures = 0;
    for (name, test) in tests {
        match test() {
            Ok(()) => println!("test {name} ... ok"),
            Err(error) => {
                failures += 1;
                eprintln!("test {name} ... FAILED\n{error}");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failures} failed",
        if failures == 0 { "ok" } else { "FAILED" },
        tests.len() - failures
    );
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fake_selection_composes_into_preview() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let selection = selection_command(workspace.path(), &[], "success", true)
        .output()
        .map_err(display)?;
    ensure_success(&selection, "selection")?;
    let response: Value = serde_json::from_slice(&selection.stdout).map_err(display)?;
    let request_source = response
        .pointer("/matches/0/request_source")
        .cloned()
        .ok_or_else(|| "selection response omitted request_source".to_owned())?;
    let request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": request_source,
            "destination": {
                "path": "source.rs",
                "anchor": {"kind": "file_end"},
                "precondition": {
                    "kind": "sha256",
                    "value": response["source"]["sha256"]
                }
            }
        }]
    });
    let request_path = workspace.path().join("request.json");
    fs::write(
        &request_path,
        serde_json::to_vec(&request).map_err(display)?,
    )
    .map_err(display)?;
    let preview = Command::new(codesplice_binary())
        .args(["--workspace"])
        .arg(workspace.path())
        .args(["apply", "--request"])
        .arg(&request_path)
        .args(["--preview", "--json"])
        .output()
        .map_err(display)?;
    ensure_success(&preview, "preview")?;
    let preview_json: Value = serde_json::from_slice(&preview.stdout).map_err(display)?;
    let preview_operation = &preview_json["resolved_operations"][0];
    let selector = &response["matches"][0]["selector"];
    if preview_operation["source_start"] != selector["start"]
        || preview_operation["source_end"] != selector["end"]
    {
        return Err("preview did not preserve the selected byte selector".to_owned());
    }
    Ok(())
}

fn slow_server_does_not_hold_diagnostic_lock() -> Result<(), String> {
    let workspace_directory = fixture_workspace()?;
    let sentinel = workspace_directory.path().join("server-started");
    let expected_source = workspace_directory.path().join("captured-source.rs");
    fs::copy(
        workspace_directory.path().join("source.rs"),
        &expected_source,
    )
    .map_err(display)?;
    fs::write(workspace_directory.path().join("commit.txt"), b"commit\n").map_err(display)?;
    let mut command = selection_command_with_expected(
        workspace_directory.path(),
        &[
            HOLD_MODE,
            sentinel.to_str().ok_or("sentinel path is not UTF-8")?,
        ],
        "success",
        true,
        &expected_source,
    );
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(display)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !sentinel.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !sentinel.exists() {
        return Err("fake server did not start".to_owned());
    }
    fs::write(
        workspace_directory.path().join("source.rs"),
        b"changed after immutable capture\n",
    )
    .map_err(display)?;
    let workspace = Workspace::open(workspace_directory.path()).map_err(display)?;
    let lock = workspace.mutation_lock().map_err(display)?;
    drop(lock);
    run_unrelated_commit(workspace_directory.path())?;
    let output = child.wait_with_output().map_err(display)?;
    ensure_success(&output, "slow selection")?;
    if fs::read(workspace_directory.path().join("committed.txt")).map_err(display)? != b"c" {
        return Err("the unrelated CodeSplice commit did not complete".to_owned());
    }
    Ok(())
}

fn run_unrelated_commit(workspace: &Path) -> Result<(), String> {
    let source = b"commit\n";
    let digest = format!("sha256:{:x}", Sha256::digest(source));
    let request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {
                "path": "commit.txt",
                "selector": {"kind": "bytes", "start": 0, "end": 1},
                "precondition": {"kind": "sha256", "value": digest}
            },
            "destination": {
                "path": "committed.txt",
                "anchor": {"kind": "file_start"},
                "precondition": {"kind": "must_not_exist"}
            }
        }]
    });
    let request_path = workspace.join("commit-request.json");
    fs::write(
        &request_path,
        serde_json::to_vec(&request).map_err(display)?,
    )
    .map_err(display)?;
    let output = Command::new(codesplice_binary())
        .args(["--workspace"])
        .arg(workspace)
        .args(["apply", "--request"])
        .arg(request_path)
        .args(["--commit", "--accept-current-plan", "--json"])
        .output()
        .map_err(display)?;
    ensure_success(&output, "concurrent commit")
}

fn flat_symbols_return_typed_error() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], "flat-symbols", true)
        .output()
        .map_err(display)?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if output.status.code() != Some(4) || response["code"] != "LSP_FLAT_SYMBOLS_UNSUPPORTED" {
        return Err(format!("unexpected flat-symbol response: {response}"));
    }
    Ok(())
}

fn non_utf8_source_fails_before_spawn() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    fs::write(workspace.path().join("source.rs"), [0xff, 0xfe]).map_err(display)?;
    let output = selection_command(workspace.path(), &[], "success", true)
        .output()
        .map_err(display)?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if output.status.code() != Some(4) || response["code"] != "UNSUPPORTED_TEXT_ENCODING" {
        return Err(format!("unexpected non-UTF-8 response: {response}"));
    }
    Ok(())
}

fn human_output_reports_auditable_ranges() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], "success", false)
        .output()
        .map_err(display)?;
    ensure_success(&output, "human selection")?;
    let stdout = String::from_utf8(output.stdout).map_err(display)?;
    if !stdout.contains("source.rs:32..76 function alpha")
        || !stdout.contains("lsp=3:4..5:5")
        || !stdout.contains("selection=3:11..3:16")
    {
        return Err(format!("human output omitted audit ranges: {stdout}"));
    }
    Ok(())
}

fn run_holding_fixture(arguments: &[std::ffi::OsString]) -> ExitCode {
    let Some(sentinel) = arguments.first() else {
        return ExitCode::FAILURE;
    };
    if fs::File::create(sentinel)
        .and_then(|mut file| file.write_all(b"started"))
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    thread::sleep(Duration::from_secs(2));
    fake_server(arguments[1..].to_vec())
}

fn fake_server(arguments: Vec<std::ffi::OsString>) -> ExitCode {
    match run_from_process_args(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fake LSP failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn fixture_workspace() -> Result<TempDir, String> {
    let directory = TempDir::new().map_err(display)?;
    fs::write(
        directory.path().join("source.rs"),
        b"pub struct Outer;\n\nimpl Outer {\n    pub fn alpha() -> u32 {\n        1\n    }\n}\n",
    )
    .map_err(display)?;
    Ok(directory)
}

fn selection_command(
    workspace: &Path,
    server_prefix_arguments: &[&str],
    scenario: &str,
    json_output: bool,
) -> Command {
    let source = workspace.join("source.rs");
    selection_command_with_expected(
        workspace,
        server_prefix_arguments,
        scenario,
        json_output,
        &source,
    )
}

fn selection_command_with_expected(
    workspace: &Path,
    server_prefix_arguments: &[&str],
    scenario: &str,
    json_output: bool,
    expected_source: &Path,
) -> Command {
    let source = workspace.join("source.rs");
    let canonical_source = source.canonicalize().expect("canonical fixture source");
    let uri = url::Url::from_file_path(&canonical_source).expect("absolute fixture URI");
    let mut command = Command::new(codesplice_binary());
    command
        .args(["--workspace"])
        .arg(workspace)
        .args([
            "select",
            "--path",
            "source.rs",
            "--name",
            "alpha",
            "--kind",
            "function",
            "--server-program",
        ])
        .arg(env::current_exe().expect("current test executable"))
        .args(["--language-id", "fixture-rust"]);
    if json_output {
        command.arg("--json");
    }
    for argument in server_prefix_arguments {
        command.arg(format!("--server-arg={argument}"));
    }
    command
        .arg("--server-arg=--scenario")
        .arg(format!("--server-arg={scenario}"))
        .arg("--server-arg=--expected-document-uri")
        .arg(format!("--server-arg={uri}"))
        .arg("--server-arg=--expected-language-id")
        .arg("--server-arg=fixture-rust")
        .arg("--server-arg=--expected-document-text-file")
        .arg(format!("--server-arg={}", expected_source.display()));
    command
}

fn codesplice_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codesplice"))
}

fn ensure_success(output: &Output, label: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} failed with {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}
