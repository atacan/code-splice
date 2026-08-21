//! Phase 7 subprocess crash, completion, rollback, and identity-conflict recovery tests.

use std::fs;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn request() -> Value {
    json!({
        "protocol_version":1,
        "operations":[{
            "kind":"copy",
            "source":{"path":"source","selector":{"kind":"bytes","start":0,"end":7},"precondition":{"kind":"sha256","value":digest(b"payload")}},
            "destination":{"path":"target","anchor":{"kind":"file_end"},"precondition":{"kind":"sha256","value":digest(b"before-")}}
        }]
    })
}

fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"payload").expect("source should be written");
    fs::write(workspace.path().join("target"), b"before-").expect("target should be written");
    workspace
}

fn crash_commit(workspace: &TempDir, failpoint: &str) -> Output {
    crash_commit_request(workspace, failpoint, &request())
}

fn crash_commit_request(workspace: &TempDir, failpoint: &str, request: &Value) -> Output {
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
        .env("CODESPLICE_TEST_FAILPOINT", failpoint)
        .env("CODESPLICE_TEST_FAILPOINT_ACTION", "exit")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("crashing commit process should start");
    serde_json::to_writer(
        child.stdin.as_mut().expect("stdin should be piped"),
        request,
    )
    .expect("request should serialize");
    child.stdin.take();
    child
        .wait_with_output()
        .expect("commit process should exit")
}

fn recover(workspace: &TempDir, id: &str, action: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args(["recover", id, action, "--json"])
        .output()
        .expect("recovery process should run")
}

fn crash_recovery(workspace: &TempDir, id: &str, action: &str, failpoint: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args(["recover", id, action, "--json"])
        .env("CODESPLICE_TEST_FAILPOINT", failpoint)
        .env("CODESPLICE_TEST_FAILPOINT_ACTION", "exit")
        .output()
        .expect("crashing recovery process should run")
}

fn transaction_id(workspace: &TempDir) -> String {
    let mut entries = fs::read_dir(workspace.path().join(".codesplice/transactions"))
        .expect("active transaction directory should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("transaction entries should read");
    assert_eq!(entries.len(), 1);
    entries
        .pop()
        .expect("one entry should exist")
        .file_name()
        .into_string()
        .expect("transaction ID should be UTF-8")
}

fn any_transaction_id(workspace: &TempDir) -> Option<String> {
    let active = workspace.path().join(".codesplice/transactions");
    if let Ok(mut entries) = fs::read_dir(active)
        && let Some(Ok(entry)) = entries.next()
    {
        return entry.file_name().into_string().ok();
    }
    let completed = workspace.path().join(".codesplice/completed");
    if let Ok(mut entries) = fs::read_dir(completed)
        && let Some(Ok(entry)) = entries.next()
    {
        let name = entry.file_name().into_string().ok()?;
        return name
            .strip_suffix("-committed")
            .or_else(|| name.strip_suffix("-rolledback"))
            .map(str::to_owned);
    }
    None
}

fn report(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.ends_with('\n'));
    serde_json::from_str(stdout).expect("stdout should be one JSON value")
}

#[test]
fn single_target_crash_recovery_should_complete_after_backup_rename() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_backup_rename");
    assert_eq!(crashed.status.code(), Some(86));
    assert!(!workspace.path().join("target").exists());
    let id = transaction_id(&workspace);

    let status = recover(&workspace, &id, "--status");
    assert_eq!(status.status.code(), Some(0));
    assert_eq!(report(&status)["transaction"]["classification"], "active");
    let completed = recover(&workspace, &id, "--complete");

    assert_eq!(completed.status.code(), Some(0));
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-payload"
    );
    assert!(
        !workspace
            .path()
            .join(".codesplice/transactions")
            .join(&id)
            .exists()
    );
}

#[test]
fn single_target_crash_recovery_should_rollback_after_install_rename() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_install_rename");
    assert_eq!(crashed.status.code(), Some(86));
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-payload"
    );
    let id = transaction_id(&workspace);

    let rolled_back = recover(&workspace, &id, "--rollback");

    assert_eq!(rolled_back.status.code(), Some(0));
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-"
    );
    assert!(
        !workspace
            .path()
            .join(".codesplice/transactions")
            .join(&id)
            .exists()
    );
}

#[test]
fn single_target_crash_recovery_should_reject_equal_byte_candidate_replacement() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_state-00000001.rec_publication");
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);
    let candidate = workspace
        .path()
        .join(".codesplice/transactions")
        .join(&id)
        .join("candidate-00000000");
    let bytes = fs::read(&candidate).expect("candidate should read");
    let replacement = candidate.with_file_name("replacement");
    fs::write(&replacement, bytes).expect("equal-byte replacement should be written");
    fs::remove_file(&candidate).expect("recorded candidate should be replaced");
    fs::rename(replacement, &candidate).expect("replacement should take candidate name");

    let recovery = recover(&workspace, &id, "--rollback");
    let error = report(&recovery);

    assert_eq!(recovery.status.code(), Some(3));
    assert_eq!(error["code"], "RECOVERY_CONFLICT");
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-"
    );
    assert!(
        workspace
            .path()
            .join(".codesplice/transactions")
            .join(&id)
            .exists()
    );
}

#[test]
fn single_target_crash_recovery_should_revalidate_source_only_inputs_before_completion() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_state-00000001.rec_publication");
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);
    fs::write(workspace.path().join("source"), b"changed").expect("source should change");

    let completion = recover(&workspace, &id, "--complete");
    let error = report(&completion);

    assert_eq!(completion.status.code(), Some(3));
    assert_eq!(error["code"], "PRECONDITION_FAILED");
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("target should read"),
        b"before-"
    );
    assert_eq!(
        recover(&workspace, &id, "--rollback").status.code(),
        Some(0)
    );
}

#[test]
fn single_target_crash_recovery_should_never_overwrite_external_install_collision() {
    let workspace = TempDir::new().expect("workspace should be created");
    fs::write(workspace.path().join("source"), b"payload").expect("source should be written");
    let request = json!({
        "protocol_version":1,
        "operations":[{
            "kind":"copy",
            "source":{"path":"source","selector":{"kind":"bytes","start":0,"end":7},"precondition":{"kind":"sha256","value":digest(b"payload")}},
            "destination":{"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
        }]
    });
    let crashed =
        crash_commit_request(&workspace, "after_state-00000001.rec_publication", &request);
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);
    fs::write(workspace.path().join("new"), b"external").expect("collision should be created");

    let completion = recover(&workspace, &id, "--complete");
    let error = report(&completion);

    assert_eq!(completion.status.code(), Some(3));
    assert_eq!(error["code"], "PRECONDITION_FAILED");
    assert_eq!(
        fs::read(workspace.path().join("new")).expect("collision should read"),
        b"external"
    );
}

#[test]
fn single_target_crash_recovery_should_resume_interrupted_rollback_steps() {
    let failpoints = [
        "before_state-00000004.rec_publication",
        "after_state-00000004.rec_publication",
        "before_rollback_target_step",
        "after_rollback_target_step",
        "before_rollback_restore_step",
        "after_rollback_restore_step",
        "before_rollback_candidate_cleanup",
        "after_rollback_candidate_cleanup",
        "before_rollback_verification",
        "after_rollback_verification",
        "before_state-00000005.rec_publication",
        "after_state-00000005.rec_publication",
        "before_state-00000006.rec_publication",
        "after_state-00000006.rec_publication",
        "before_terminal_directory_rename",
        "after_terminal_directory_rename",
        "before_terminal_cleanup",
        "after_terminal_cleanup",
    ];
    for failpoint in failpoints {
        let workspace = workspace();
        let crashed = crash_commit(&workspace, "after_install_rename");
        assert_eq!(crashed.status.code(), Some(86));
        let id = transaction_id(&workspace);
        let rollback = crash_recovery(&workspace, &id, "--rollback", failpoint);
        assert_eq!(rollback.status.code(), Some(86), "failpoint={failpoint}");

        if let Some(recovery_id) = any_transaction_id(&workspace) {
            let resumed = recover(&workspace, &recovery_id, "--rollback");
            assert_eq!(resumed.status.code(), Some(0), "failpoint={failpoint}");
        }
        assert_eq!(
            fs::read(workspace.path().join("target")).expect("target should read"),
            b"before-",
            "failpoint={failpoint}"
        );
    }
}

#[test]
fn single_target_crash_recovery_should_resolve_every_commit_failpoint_to_old_or_new() {
    let failpoints = [
        "before_manifest.rec_publication",
        "after_manifest.rec_publication",
        "before_state-00000000.rec_publication",
        "after_state-00000000.rec_publication",
        "before_candidate_create",
        "after_candidate_create",
        "before_candidate_write",
        "after_candidate_write",
        "before_candidate_sync",
        "after_candidate_sync",
        "before_candidate_verification",
        "after_candidate_verification",
        "before_state-00000001.rec_publication",
        "after_state-00000001.rec_publication",
        "before_state-00000002.rec_publication",
        "after_state-00000002.rec_publication",
        "before_backup_rename",
        "after_backup_rename",
        "before_state-00000003.rec_publication",
        "after_state-00000003.rec_publication",
        "before_install_rename",
        "after_install_rename",
        "before_state-00000004.rec_publication",
        "after_state-00000004.rec_publication",
        "before_final_verification",
        "after_final_verification",
        "before_state-00000005.rec_publication",
        "after_state-00000005.rec_publication",
        "before_terminal_directory_rename",
        "after_terminal_directory_rename",
        "before_terminal_cleanup",
        "after_terminal_cleanup",
    ];
    for failpoint in failpoints {
        let workspace = workspace();
        let crashed = crash_commit(&workspace, failpoint);
        assert_eq!(crashed.status.code(), Some(86), "failpoint={failpoint}");
        if let Some(id) = any_transaction_id(&workspace) {
            let completion = recover(&workspace, &id, "--complete");
            if completion.status.code() != Some(0) {
                let rollback = recover(&workspace, &id, "--rollback");
                assert_eq!(rollback.status.code(), Some(0), "failpoint={failpoint}");
            }
        }
        let target = fs::read(workspace.path().join("target")).expect("target should read");
        assert!(
            target == b"before-" || target == b"before-payload",
            "failpoint={failpoint} target={target:?}"
        );
    }
}
