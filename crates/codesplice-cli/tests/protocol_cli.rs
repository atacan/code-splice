//! End-to-end tests for the Phase 2 command grammar and output discipline.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> String {
    fs::read_to_string(
        repository_root()
            .join("tests/golden/protocol-v1")
            .join(name),
    )
    .expect("golden file must be readable")
}

fn valid_request() -> Vec<u8> {
    golden("request-all-variants.json").into_bytes()
}

fn invoke(arguments: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_codesplice"))
        .args(arguments)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codesplice must start");
    child
        .stdin
        .take()
        .expect("stdin must be piped")
        .write_all(stdin)
        .expect("request must be written");
    child.wait_with_output().expect("codesplice must exit")
}

fn json_stdout(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(stdout.ends_with('\n'), "JSON stdout must end in LF");
    assert_eq!(
        stdout.lines().count(),
        1,
        "JSON stdout must contain one value"
    );
    serde_json::from_str(stdout).expect("stdout must be one JSON value")
}

#[test]
fn capabilities_command_should_match_golden_output() {
    let output = invoke(&["capabilities", "--json"], b"");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, golden("capabilities.json").as_bytes());
    assert!(output.stderr.is_empty());
}

#[test]
fn protocol_version_command_should_match_golden_output() {
    let output = invoke(&["protocol-version", "--json"], b"");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, golden("protocol-version.json").as_bytes());
    assert!(output.stderr.is_empty());
}

#[test]
fn every_execution_command_should_validate_then_match_its_golden_stub() {
    let expected: Value = serde_json::from_str(&golden("cli-command-results.json"))
        .expect("CLI command golden must be JSON");
    let transaction_id = "0123456789abcdef0123456789abcdef";
    let cases: Vec<(&str, Vec<&str>, Vec<u8>)> = vec![
        (
            "inspect",
            vec!["inspect", "--path", "src/a.rs", "--json"],
            Vec::new(),
        ),
        (
            "preview",
            vec!["apply", "--request", "-", "--preview", "--json"],
            valid_request(),
        ),
        (
            "preview_without_diff",
            vec![
                "apply",
                "--request",
                "-",
                "--preview",
                "--no-diff",
                "--json",
            ],
            valid_request(),
        ),
        (
            "commit",
            vec![
                "apply",
                "--request",
                "-",
                "--commit",
                "--expect-plan",
                DIGEST,
                "--json",
            ],
            valid_request(),
        ),
        (
            "commit",
            vec![
                "apply",
                "--request",
                "-",
                "--commit",
                "--accept-current-plan",
                "--json",
            ],
            valid_request(),
        ),
        (
            "recovery_list",
            vec!["recover", "--list", "--json"],
            Vec::new(),
        ),
        (
            "recovery_status",
            vec!["recover", transaction_id, "--status", "--json"],
            Vec::new(),
        ),
        (
            "recovery_complete",
            vec!["recover", transaction_id, "--complete", "--json"],
            Vec::new(),
        ),
        (
            "recovery_rollback",
            vec!["recover", transaction_id, "--rollback", "--json"],
            Vec::new(),
        ),
    ];

    for (name, arguments, stdin) in cases {
        let output = invoke(&arguments, &stdin);
        assert_eq!(output.status.code(), Some(8), "command: {name}");
        assert_eq!(json_stdout(&output), expected[name], "command: {name}");
    }
}

#[test]
fn apply_should_read_and_validate_a_request_file() {
    let request_path = repository_root().join("tests/golden/protocol-v1/request-all-variants.json");
    let output = invoke(
        &[
            "apply",
            "--request",
            request_path
                .to_str()
                .expect("repository path must be UTF-8"),
            "--preview",
            "--json",
        ],
        b"",
    );
    let expected: Value = serde_json::from_str(&golden("cli-command-results.json"))
        .expect("CLI command golden must be JSON");

    assert_eq!(output.status.code(), Some(8));
    assert_eq!(json_stdout(&output), expected["preview"]);
}

#[test]
fn commit_without_an_expected_plan_policy_should_fail_before_execution() {
    let output = invoke(
        &["apply", "--request", "-", "--commit", "--json"],
        &valid_request(),
    );
    let error = json_stdout(&output);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(error["code"], "EXPECTED_PLAN_REQUIRED");
}

#[test]
fn commit_with_both_expected_plan_policies_should_be_invalid_cli() {
    let output = invoke(
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--expect-plan",
            DIGEST,
            "--accept-current-plan",
            "--json",
        ],
        &valid_request(),
    );
    let error = json_stdout(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error["code"], "INVALID_CLI");
}

#[test]
fn malformed_json_should_fail_without_reaching_the_execution_stub() {
    let output = invoke(&["apply", "--request", "-", "--preview", "--json"], b"{");
    let error = json_stdout(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error["code"], "INVALID_JSON");
}

#[test]
fn invalid_cli_in_json_mode_should_emit_no_prose() {
    let output = invoke(&["apply", "--preview", "--json"], b"");
    let error = json_stdout(&output);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error["code"], "INVALID_CLI");
}

#[test]
fn invalid_cli_in_human_mode_should_use_stderr_only() {
    let output = invoke(&["capabilities"], b"");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("codesplice: INVALID_CLI:"));
}
