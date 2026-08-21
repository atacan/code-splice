//! Phase 7 end-to-end single-target commit, intent, permission, and guard tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn invoke(workspace: &TempDir, arguments: &[&str], request: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("srcmv should start");
    serde_json::to_writer(
        child.stdin.as_mut().expect("stdin should be piped"),
        request,
    )
    .expect("request should serialize");
    child.stdin.take();
    child.wait_with_output().expect("srcmv should exit")
}

fn invoke_with_umask(
    workspace: &TempDir,
    arguments: &[&str],
    request: &Value,
    umask: &str,
) -> Output {
    let binary = env!("CARGO_BIN_EXE_srcmv");
    let mut child = Command::new("sh")
        .args(["-c", &format!("umask {umask}; exec \"$0\" \"$@\"")])
        .arg(binary)
        .arg("--workspace")
        .arg(workspace.path())
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("srcmv should start beneath selected umask");
    serde_json::to_writer(
        child.stdin.as_mut().expect("stdin should be piped"),
        request,
    )
    .expect("request should serialize");
    child.stdin.take();
    child.wait_with_output().expect("srcmv should exit")
}

fn report(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout).expect("stdout should be one JSON value")
}

fn copy_request(source: &[u8], destination: &str, destination_state: Value) -> Value {
    json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {
                "path": "source",
                "selector": {"kind":"bytes","start":0,"end":source.len()},
                "precondition": {"kind":"sha256","value":digest(source)}
            },
            "destination": {
                "path": destination,
                "anchor": {"kind":"file_end"},
                "precondition": destination_state
            }
        }]
    })
}

#[test]
fn single_target_commit_should_copy_into_existing_and_preserve_post_preview_mode() {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"payload").expect("source should be written");
    fs::write(workspace.path().join("target"), b"before-").expect("target should be written");
    fs::set_permissions(
        workspace.path().join("target"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("initial mode should be set");
    let request = copy_request(
        b"payload",
        "target",
        json!({"kind":"sha256","value":digest(b"before-")}),
    );
    let preview = invoke(
        &workspace,
        &["apply", "--request", "-", "--preview", "--json"],
        &request,
    );
    let preview_report = report(&preview);
    fs::set_permissions(
        workspace.path().join("target"),
        fs::Permissions::from_mode(0o751),
    )
    .expect("post-preview mode should be set");

    let commit = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--expect-plan",
            preview_report["plan_sha256"]
                .as_str()
                .expect("preview should report plan"),
            "--json",
        ],
        &request,
    );
    let commit_report = report(&commit);

    assert_eq!(commit.status.code(), Some(0));
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-payload"
    );
    assert_eq!(
        fs::metadata(workspace.path().join("target"))
            .expect("target metadata should read")
            .permissions()
            .mode()
            & 0o7777,
        0o751
    );
    assert_eq!(commit_report["transaction_state"], "committed");
    assert_eq!(commit_report["preserved_permission_modes"]["target"], 0o751);
    assert_eq!(
        commit_report["resolved_operations"][0]["selected_payload_sha256"],
        commit_report["resolved_operations"][0]["inserted_payload_sha256"]
    );
    assert!(
        !workspace
            .path()
            .join(".codesplice/transactions")
            .read_dir()
            .expect("transactions should exist")
            .next()
            .is_some()
    );
}

#[test]
fn single_target_commit_should_create_new_file_with_startup_umask_mode() {
    for (umask, expected_mode) in [
        ("000", 0o666),
        ("022", 0o644),
        ("027", 0o640),
        ("077", 0o600),
    ] {
        let workspace = TempDir::new().expect("workspace should be created");
        fs::write(workspace.path().join("source"), b"payload").expect("source should be written");
        let request = copy_request(b"payload", "new", json!({"kind":"must_not_exist"}));

        let commit = invoke_with_umask(
            &workspace,
            &[
                "apply",
                "--request",
                "-",
                "--commit",
                "--accept-current-plan",
                "--json",
            ],
            &request,
            umask,
        );
        let commit_report = report(&commit);

        assert_eq!(commit.status.code(), Some(0), "umask={umask}");
        assert_eq!(
            fs::read(workspace.path().join("new")).expect("new file should read"),
            b"payload",
            "umask={umask}"
        );
        assert_eq!(
            fs::metadata(workspace.path().join("new"))
                .expect("new metadata should read")
                .permissions()
                .mode()
                & 0o777,
            expected_mode,
            "umask={umask}"
        );
        assert_eq!(commit_report["files_changed"], json!(["new"]));
        assert_eq!(commit_report["preserved_permission_modes"], json!({}));
    }
}

#[test]
fn single_target_commit_should_execute_effectful_same_file_move() {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"abc").expect("source should be written");
    let request = json!({
        "protocol_version":1,
        "operations":[{
            "kind":"move",
            "source":{"path":"source","selector":{"kind":"bytes","start":1,"end":2},"precondition":{"kind":"sha256","value":digest(b"abc")}},
            "destination":{"path":"source","anchor":{"kind":"file_end"},"precondition":{"kind":"sha256","value":digest(b"abc")}}
        }]
    });

    let commit = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        &request,
    );

    assert_eq!(commit.status.code(), Some(0));
    assert_eq!(
        fs::read(workspace.path().join("source")).expect("source should read"),
        b"acb"
    );
}

#[test]
fn single_target_commit_should_keep_mismatch_and_noop_noncreating() {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"abc").expect("source should be written");
    let noop = json!({
        "protocol_version":1,
        "operations":[{
            "kind":"move",
            "source":{"path":"source","selector":{"kind":"bytes","start":0,"end":1},"precondition":{"kind":"sha256","value":digest(b"abc")}},
            "destination":{"path":"source","anchor":{"kind":"byte_offset","offset":0},"precondition":{"kind":"sha256","value":digest(b"abc")}}
        }]
    });
    let mismatch = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--expect-plan",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "--json",
        ],
        &noop,
    );
    assert_eq!(mismatch.status.code(), Some(3));
    assert_eq!(report(&mismatch)["code"], "EXPECTED_PLAN_MISMATCH");
    assert!(!workspace.path().join(".codesplice").exists());

    let success = invoke(
        &workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ],
        &noop,
    );
    let success_report = report(&success);
    assert_eq!(success.status.code(), Some(0));
    assert_eq!(success_report["transaction_id"], Value::Null);
    assert_eq!(success_report["transaction_state"], "no_op");
    assert_eq!(
        success_report["resolved_operations"][0]["inserted_payload_sha256"],
        Value::Null
    );
    assert!(!workspace.path().join(".codesplice").exists());
}

#[test]
fn single_target_commit_should_reject_a_prelock_plan_identity_change_without_transaction() {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"payload").expect("source should be written");
    fs::write(workspace.path().join("target"), b"before-").expect("target should be written");
    let request = copy_request(
        b"payload",
        "target",
        json!({"kind":"sha256","value":digest(b"before-")}),
    );
    let ready = workspace.path().join("hook-ready");
    let resume = workspace.path().join("hook-continue");
    let mut child = Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args([
            "apply",
            "--request",
            "-",
            "--commit",
            "--accept-current-plan",
            "--json",
        ])
        .env("CODESPLICE_TEST_HOOK", "after_prelock_plan")
        .env("CODESPLICE_TEST_READY", &ready)
        .env("CODESPLICE_TEST_CONTINUE", &resume)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("srcmv should start");
    serde_json::to_writer(
        child.stdin.as_mut().expect("stdin should be piped"),
        &request,
    )
    .expect("request should serialize");
    child.stdin.take();
    let started = Instant::now();
    while !ready.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "hook should become ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let replacement = workspace.path().join("source-replacement");
    fs::write(&replacement, b"payload").expect("replacement should be written");
    fs::rename(replacement, workspace.path().join("source"))
        .expect("source identity should be replaced with equal bytes");
    fs::write(&resume, b"continue").expect("hook should resume");
    let output = child.wait_with_output().expect("srcmv should exit");
    let error = report(&output);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(error["code"], "PLAN_CHANGED_DURING_COMMIT");
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-"
    );
    assert!(
        workspace
            .path()
            .join(".codesplice/transactions")
            .read_dir()
            .expect("transactions should exist")
            .next()
            .is_none()
    );
}
