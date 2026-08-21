//! Phase 8 end-to-end multi-target commit and automatic rollback tests.

use std::fs;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"abc").expect("source should be written");
    fs::write(workspace.path().join("target"), b"XYZ").expect("target should be written");
    workspace
}

fn request() -> Value {
    json!({
        "protocol_version": 1,
        "operations": [
            {
                "kind": "move",
                "source": {
                    "path": "source",
                    "selector": {"kind":"bytes","start":0,"end":1},
                    "precondition": {"kind":"sha256","value":digest(b"abc")}
                },
                "destination": {
                    "path": "target",
                    "anchor": {"kind":"file_end"},
                    "precondition": {"kind":"sha256","value":digest(b"XYZ")}
                }
            },
            {
                "kind": "copy",
                "source": {
                    "path": "source",
                    "selector": {"kind":"bytes","start":1,"end":3},
                    "precondition": {"kind":"sha256","value":digest(b"abc")}
                },
                "destination": {
                    "path": "new",
                    "anchor": {"kind":"file_start"},
                    "precondition": {"kind":"must_not_exist"}
                }
            }
        ]
    })
}

fn invoke(workspace: &TempDir, arguments: &[&str], failpoints: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_srcmv"));
    command
        .arg("--workspace")
        .arg(workspace.path())
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(failpoints) = failpoints {
        command
            .env("CODESPLICE_TEST_FAILPOINT", failpoints)
            .env("CODESPLICE_TEST_FAILPOINT_ACTION", "error");
    }
    let mut child = command.spawn().expect("srcmv should start");
    if arguments.first() == Some(&"apply") {
        serde_json::to_writer(
            child.stdin.as_mut().expect("stdin should be piped"),
            &request(),
        )
        .expect("request should serialize");
    }
    child.stdin.take();
    child.wait_with_output().expect("srcmv should exit")
}

fn report(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be one JSON value")
}

fn assert_original(workspace: &TempDir) {
    assert_eq!(
        fs::read(workspace.path().join("source")).expect("source should read"),
        b"abc"
    );
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"XYZ"
    );
    assert!(!workspace.path().join("new").exists());
}

fn assert_planned(workspace: &TempDir) {
    assert_eq!(
        fs::read(workspace.path().join("source")).expect("source should read"),
        b"bc"
    );
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"XYZa"
    );
    assert_eq!(
        fs::read(workspace.path().join("new")).expect("new target should read"),
        b"bc"
    );
}

fn active_transaction_id(workspace: &TempDir) -> String {
    fs::read_dir(workspace.path().join(".codesplice/transactions"))
        .expect("transactions should exist")
        .next()
        .expect("one active transaction should remain")
        .expect("transaction entry should read")
        .file_name()
        .into_string()
        .expect("transaction ID should be UTF-8")
}

#[test]
fn multi_target_commit_should_report_and_install_every_planned_output() {
    let workspace = workspace();

    let output = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        None,
    );
    let report = report(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["files_changed"], json!(["new", "source", "target"]));
    assert_eq!(report["visibility"], "recoverable_not_atomic");
    assert_eq!(
        report["resolved_operations"][0]["inserted_payload_sha256"],
        digest(b"a")
    );
    assert_eq!(
        report["resolved_operations"][1]["inserted_payload_sha256"],
        digest(b"bc")
    );
    assert_planned(&workspace);
}

#[test]
fn multi_target_commit_should_mutate_nothing_when_candidate_preparation_fails() {
    let workspace = workspace();

    let output = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        Some("after_candidate_verification_target-00000002"),
    );

    assert_eq!(output.status.code(), Some(8));
    assert_original(&workspace);
}

#[test]
fn multi_target_commit_should_roll_back_every_earlier_target_after_late_failure() {
    let workspace = workspace();

    let output = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        Some("before_install_rename_target-00000002"),
    );

    assert_eq!(output.status.code(), Some(8));
    assert_original(&workspace);
}

#[test]
fn multi_target_commit_should_require_recovery_when_automatic_rollback_is_incomplete() {
    let workspace = workspace();

    let output = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        Some("before_install_rename_target-00000002,before_rollback_target_step_target-00000000"),
    );
    let error = report(&output);

    assert_eq!(output.status.code(), Some(5));
    assert_eq!(error["code"], "TRANSACTION_RECOVERY_REQUIRED");
    let transaction_id = active_transaction_id(&workspace);
    let recovery = invoke(
        &workspace,
        &["recover", &transaction_id, "--rollback", "--json"],
        None,
    );
    assert_eq!(recovery.status.code(), Some(0));
    assert_original(&workspace);
}
