//! End-to-end semantic-selection CLI tests with fake-server process re-entry.

use std::collections::BTreeMap;
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
const MAXIMUM_SOURCE_BYTES: usize = 8 * 1024 * 1024;
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
            "server_requests_remain_read_only",
            server_requests_remain_read_only,
        ),
        (
            "server_request_flood_hits_rate_limit",
            server_request_flood_hits_rate_limit,
        ),
        (
            "bounded_notification_flood_succeeds",
            bounded_notification_flood_succeeds,
        ),
        (
            "notification_flood_hits_rate_limit",
            notification_flood_hits_rate_limit,
        ),
        (
            "deep_symbol_tree_hits_a_bounded_wire_failure",
            deep_symbol_tree_hits_a_bounded_wire_failure,
        ),
        (
            "identical_duplicate_symbols_are_deduplicated",
            identical_duplicate_symbols_are_deduplicated,
        ),
        (
            "distinct_path_symbols_are_ambiguous",
            distinct_path_symbols_are_ambiguous,
        ),
        ("malformed_range_is_rejected", malformed_range_is_rejected),
        (
            "invalid_selection_range_is_rejected",
            invalid_selection_range_is_rejected,
        ),
        (
            "non_utf8_source_fails_before_spawn",
            non_utf8_source_fails_before_spawn,
        ),
        (
            "source_just_below_limit_succeeds",
            source_just_below_limit_succeeds,
        ),
        ("source_at_limit_succeeds", source_at_limit_succeeds),
        (
            "source_above_limit_is_rejected",
            source_above_limit_is_rejected,
        ),
        (
            "initialize_request_failure_is_typed",
            initialize_request_failure_is_typed,
        ),
        ("early_server_exit_is_typed", early_server_exit_is_typed),
        ("malformed_header_is_typed", malformed_header_is_typed),
        (
            "initialize_timeout_uses_trusted_deadline",
            initialize_timeout_uses_trusted_deadline,
        ),
        (
            "document_symbol_timeout_uses_trusted_deadline",
            document_symbol_timeout_uses_trusted_deadline,
        ),
        (
            "human_output_reports_auditable_ranges",
            human_output_reports_auditable_ranges,
        ),
        (
            "byte_position_selects_function",
            byte_position_selects_function,
        ),
        (
            "line_scalar_position_selects_function",
            line_scalar_position_selects_function,
        ),
        (
            "line_position_defaults_to_column_one",
            line_position_defaults_to_column_one,
        ),
        ("all_allows_zero_matches", all_allows_zero_matches),
        (
            "all_returns_distinct_path_matches",
            all_returns_distinct_path_matches,
        ),
        (
            "extent_controls_final_byte_selector",
            extent_controls_final_byte_selector,
        ),
        (
            "help_documents_declaration_lines_extent",
            help_documents_declaration_lines_extent,
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

fn server_requests_remain_read_only() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let config = write_trusted_config(workspace.path(), "server-requests", 1_000)?;
    let before = snapshot_workspace(workspace.path())?;
    let output = trusted_selection_command(workspace.path(), &config, true)
        .output()
        .map_err(display)?;
    ensure_success(&output, "read-only server requests")?;
    let after = snapshot_workspace(workspace.path())?;
    if before != after {
        return Err("selection changed workspace source or control state".to_owned());
    }
    Ok(())
}

fn server_request_flood_hits_rate_limit() -> Result<(), String> {
    assert_scenario_error("server-request-flood", "LSP_RESOURCE_LIMIT_EXCEEDED")
}

fn bounded_notification_flood_succeeds() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], "notification-flood", true)
        .output()
        .map_err(display)?;
    ensure_success(&output, "bounded notification flood")
}

fn notification_flood_hits_rate_limit() -> Result<(), String> {
    assert_scenario_error("notification-limit-exceeded", "LSP_RESOURCE_LIMIT_EXCEEDED")
}

fn deep_symbol_tree_hits_a_bounded_wire_failure() -> Result<(), String> {
    assert_scenario_error("deep-symbols", "LSP_PROTOCOL_ERROR")
}

fn identical_duplicate_symbols_are_deduplicated() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], "duplicate-symbols", true)
        .output()
        .map_err(display)?;
    ensure_success(&output, "identical duplicate symbols")?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if response["matches"].as_array().map(Vec::len) != Some(1) {
        return Err(format!(
            "duplicate symbols were not deduplicated: {response}"
        ));
    }
    Ok(())
}

fn distinct_path_symbols_are_ambiguous() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], "ambiguous-symbols", true)
        .output()
        .map_err(display)?;
    assert_error_with_status(&output, "SELECTION_AMBIGUOUS", 3)
}

fn malformed_range_is_rejected() -> Result<(), String> {
    assert_scenario_error("malformed-range", "LSP_PROTOCOL_ERROR")
}

fn invalid_selection_range_is_rejected() -> Result<(), String> {
    assert_scenario_error("invalid-selection-range", "LSP_PROTOCOL_ERROR")
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

fn source_just_below_limit_succeeds() -> Result<(), String> {
    assert_source_size_succeeds(MAXIMUM_SOURCE_BYTES - 1)
}

fn source_at_limit_succeeds() -> Result<(), String> {
    assert_source_size_succeeds(MAXIMUM_SOURCE_BYTES)
}

fn source_above_limit_is_rejected() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    resize_source(workspace.path(), MAXIMUM_SOURCE_BYTES + 1)?;
    let output = selection_command(workspace.path(), &[], "success", true)
        .output()
        .map_err(display)?;
    assert_error(&output, "LSP_RESOURCE_LIMIT_EXCEEDED")
}

fn initialize_request_failure_is_typed() -> Result<(), String> {
    assert_scenario_error("initialize-error", "LSP_REQUEST_FAILED")
}

fn early_server_exit_is_typed() -> Result<(), String> {
    for iteration in 0..32 {
        assert_scenario_error("exit-after-initialize", "LSP_EXITED").map_err(|error| {
            format!("early-exit iteration {iteration} did not remain stable: {error}")
        })?;
    }
    Ok(())
}

fn malformed_header_is_typed() -> Result<(), String> {
    assert_scenario_error("malformed-header", "LSP_PROTOCOL_ERROR")
}

fn initialize_timeout_uses_trusted_deadline() -> Result<(), String> {
    assert_timeout_scenario("hang-initialize", "initialize")
}

fn document_symbol_timeout_uses_trusted_deadline() -> Result<(), String> {
    assert_timeout_scenario("hang-document-symbols", "document_symbol")
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

fn byte_position_selects_function() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command_for_query(
        workspace.path(),
        &["--at-byte", "50", "--kind", "function"],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&output, "byte-position selection")?;
    assert_selected_name(&output, "alpha")
}

fn line_scalar_position_selects_function() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command_for_query(
        workspace.path(),
        &["--at-line", "4", "--at-column", "12", "--kind", "function"],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&output, "line-scalar selection")?;
    assert_selected_name(&output, "alpha")
}

fn line_position_defaults_to_column_one() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command_for_query(
        workspace.path(),
        &["--at-line", "5", "--kind", "function"],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&output, "default-column selection")?;
    assert_selected_name(&output, "alpha")
}

fn all_allows_zero_matches() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command_for_query(
        workspace.path(),
        &["--name", "missing", "--all"],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&output, "zero-match all selection")?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if response["matches"]
        .as_array()
        .is_none_or(|matches| !matches.is_empty())
    {
        return Err(format!(
            "--all did not return an empty match list: {response}"
        ));
    }
    Ok(())
}

fn all_returns_distinct_path_matches() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command_for_query(
        workspace.path(),
        &["--name", "alpha", "--kind", "function", "--all"],
        "ambiguous-symbols",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&output, "multiple-match all selection")?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if response["matches"].as_array().map(Vec::len) != Some(2) {
        return Err(format!("--all did not return both matches: {response}"));
    }
    Ok(())
}

fn extent_controls_final_byte_selector() -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let symbol = selection_command_for_query(
        workspace.path(),
        &[
            "--name", "alpha", "--kind", "function", "--extent", "symbol",
        ],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&symbol, "symbol extent")?;
    let declaration = selection_command_for_query(
        workspace.path(),
        &[
            "--name",
            "alpha",
            "--kind",
            "function",
            "--extent",
            "declaration_lines",
        ],
        "success",
        true,
    )
    .output()
    .map_err(display)?;
    ensure_success(&declaration, "declaration-lines extent")?;
    let symbol: Value = serde_json::from_slice(&symbol.stdout).map_err(display)?;
    let declaration: Value = serde_json::from_slice(&declaration.stdout).map_err(display)?;
    let actual = (
        symbol.pointer("/matches/0/selector/start"),
        symbol.pointer("/matches/0/selector/end"),
        declaration.pointer("/matches/0/selector/start"),
        declaration.pointer("/matches/0/selector/end"),
    );
    if actual
        != (
            Some(&json!(36)),
            Some(&json!(75)),
            Some(&json!(32)),
            Some(&json!(76)),
        )
    {
        return Err(format!("unexpected extent selectors: {actual:?}"));
    }
    Ok(())
}

fn help_documents_declaration_lines_extent() -> Result<(), String> {
    let output = Command::new(codesplice_binary())
        .args(["select", "--help"])
        .output()
        .map_err(display)?;
    ensure_success(&output, "select help")?;
    let help = String::from_utf8(output.stdout).map_err(display)?;
    if !help.contains("declaration_lines") {
        return Err(format!("select help omitted declaration_lines: {help}"));
    }
    Ok(())
}

fn assert_selected_name(output: &Output, expected: &str) -> Result<(), String> {
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if response["matches"][0]["name"] != expected {
        return Err(format!("unexpected selection response: {response}"));
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

fn assert_scenario_error(scenario: &str, expected_code: &str) -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let output = selection_command(workspace.path(), &[], scenario, true)
        .output()
        .map_err(display)?;
    assert_error(&output, expected_code)
}

fn assert_error(output: &Output, expected_code: &str) -> Result<(), String> {
    assert_error_with_status(output, expected_code, 4)
}

fn assert_error_with_status(
    output: &Output,
    expected_code: &str,
    expected_status: i32,
) -> Result<(), String> {
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if output.status.code() != Some(expected_status) || response["code"] != expected_code {
        return Err(format!(
            "expected {expected_code}, got status {} and response {response}",
            output.status
        ));
    }
    Ok(())
}

fn assert_source_size_succeeds(size: usize) -> Result<(), String> {
    let workspace = fixture_workspace()?;
    resize_source(workspace.path(), size)?;
    let output = selection_command(workspace.path(), &[], "success", true)
        .output()
        .map_err(display)?;
    ensure_success(&output, &format!("selection of {size} source bytes"))
}

fn resize_source(workspace: &Path, size: usize) -> Result<(), String> {
    let source_path = workspace.join("source.rs");
    let mut source = fs::read(&source_path).map_err(display)?;
    if size < source.len() {
        return Err("requested source size is smaller than the fixture".to_owned());
    }
    source.resize(size, b' ');
    fs::write(source_path, source).map_err(display)
}

fn assert_timeout_scenario(scenario: &str, expected_phase: &str) -> Result<(), String> {
    let workspace = fixture_workspace()?;
    let config = write_trusted_config(workspace.path(), scenario, 100)?;
    let started = Instant::now();
    let output = trusted_selection_command(workspace.path(), &config, true)
        .output()
        .map_err(display)?;
    let elapsed = started.elapsed();
    assert_error(&output, "LSP_TIMEOUT")?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(display)?;
    if response["context"]["phase"] != expected_phase {
        return Err(format!("unexpected timeout response: {response}"));
    }
    if elapsed >= Duration::from_secs(5) {
        return Err(format!("timeout scenario took too long: {elapsed:?}"));
    }
    Ok(())
}

fn write_trusted_config(
    workspace: &Path,
    scenario: &str,
    timeout_ms: u64,
) -> Result<PathBuf, String> {
    let source = workspace.join("source.rs");
    let canonical_source = source.canonicalize().map_err(display)?;
    let uri = url::Url::from_file_path(&canonical_source)
        .map_err(|()| "failed to build fixture source URI".to_owned())?;
    let arguments = vec![
        "--scenario".to_owned(),
        scenario.to_owned(),
        "--expected-document-uri".to_owned(),
        uri.to_string(),
        "--expected-language-id".to_owned(),
        "fixture-rust".to_owned(),
        "--expected-document-text-file".to_owned(),
        canonical_source.display().to_string(),
    ];
    let program = env::current_exe().map_err(display)?.display().to_string();
    let settings = if scenario == "server-requests" {
        "settings = { fixture = { enabled = true } }\n"
    } else {
        ""
    };
    let document = format!(
        "version = 1\n\n[[servers]]\nid = \"fixture\"\nextensions = [\"rs\"]\nlanguage_id = \"fixture-rust\"\nprogram = {}\nargs = {}\n{settings}startup_timeout_ms = {timeout_ms}\nrequest_timeout_ms = {timeout_ms}\n",
        serde_json::to_string(&program).map_err(display)?,
        serde_json::to_string(&arguments).map_err(display)?,
    );
    let path = workspace.join("lsp-config.toml");
    fs::write(&path, document).map_err(display)?;
    Ok(path)
}

fn trusted_selection_command(workspace: &Path, config: &Path, json_output: bool) -> Command {
    let mut command = Command::new(codesplice_binary());
    command
        .env("CODESPLICE_CONFIG", config)
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
            "--server-id",
            "fixture",
        ]);
    if json_output {
        command.arg("--json");
    }
    command
}

fn snapshot_workspace(root: &Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>, String> {
    let mut snapshot = BTreeMap::new();
    snapshot_directory(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn snapshot_directory(
    root: &Path,
    directory: &Path,
    snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(display)? {
        let entry = entry.map_err(display)?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(display)?.to_path_buf();
        let file_type = entry.file_type().map_err(display)?;
        if file_type.is_dir() {
            snapshot.insert(relative, None);
            snapshot_directory(root, &path, snapshot)?;
        } else if file_type.is_file() {
            snapshot.insert(relative, Some(fs::read(path).map_err(display)?));
        }
    }
    Ok(())
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

fn selection_command_for_query(
    workspace: &Path,
    query_arguments: &[&str],
    scenario: &str,
    json_output: bool,
) -> Command {
    let source = workspace.join("source.rs");
    selection_command_with_query_and_expected(
        workspace,
        query_arguments,
        &[],
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
    selection_command_with_query_and_expected(
        workspace,
        &["--name", "alpha", "--kind", "function"],
        server_prefix_arguments,
        scenario,
        json_output,
        expected_source,
    )
}

fn selection_command_with_query_and_expected(
    workspace: &Path,
    query_arguments: &[&str],
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
        .args(["select", "--path", "source.rs"])
        .args(query_arguments)
        .arg("--server-program")
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
