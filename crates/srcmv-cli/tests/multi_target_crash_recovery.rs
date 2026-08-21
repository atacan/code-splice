//! Phase 8 subprocess crash, indexed failpoint, and multi-target recovery tests.

use std::fs;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use srcmv_fs::decode_manifest_record;
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
        "protocol_version":1,
        "operations":[
            {
                "kind":"move",
                "source":{"path":"source","selector":{"kind":"bytes","start":0,"end":1},"precondition":{"kind":"sha256","value":digest(b"abc")}},
                "destination":{"path":"target","anchor":{"kind":"file_end"},"precondition":{"kind":"sha256","value":digest(b"XYZ")}}
            },
            {
                "kind":"copy",
                "source":{"path":"source","selector":{"kind":"bytes","start":1,"end":3},"precondition":{"kind":"sha256","value":digest(b"abc")}},
                "destination":{"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            }
        ]
    })
}

fn crash_commit(workspace: &TempDir, failpoint: &str) -> Output {
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
        .expect("srcmv should start");
    serde_json::to_writer(
        child.stdin.as_mut().expect("stdin should be piped"),
        &request(),
    )
    .expect("request should serialize");
    child.stdin.take();
    child.wait_with_output().expect("srcmv should exit")
}

fn recover(workspace: &TempDir, transaction_id: &str, action: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args(["recover", transaction_id, action, "--json"])
        .output()
        .expect("recovery should run")
}

fn crash_recovery(
    workspace: &TempDir,
    transaction_id: &str,
    action: &str,
    failpoint: &str,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace.path())
        .args(["recover", transaction_id, action, "--json"])
        .env("CODESPLICE_TEST_FAILPOINT", failpoint)
        .env("CODESPLICE_TEST_FAILPOINT_ACTION", "exit")
        .output()
        .expect("crashing recovery should run")
}

fn transaction_id(workspace: &TempDir) -> String {
    fs::read_dir(workspace.path().join(".codesplice/transactions"))
        .expect("transactions should exist")
        .next()
        .expect("active transaction should exist")
        .expect("transaction entry should read")
        .file_name()
        .into_string()
        .expect("transaction ID should be UTF-8")
}

fn any_transaction_id(workspace: &TempDir) -> Option<String> {
    let active = workspace.path().join(".codesplice/transactions");
    if let Some(entry) = fs::read_dir(active).ok()?.next() {
        return Some(
            entry
                .ok()?
                .file_name()
                .into_string()
                .ok()?
                .trim_end_matches("-committed")
                .trim_end_matches("-rolledback")
                .to_owned(),
        );
    }
    let completed = workspace.path().join(".codesplice/completed");
    let name = fs::read_dir(completed).ok()?.next()?.ok()?.file_name();
    let name = name.into_string().ok()?;
    Some(
        name.trim_end_matches("-committed")
            .trim_end_matches("-rolledback")
            .to_owned(),
    )
}

fn status(workspace: &TempDir, transaction_id: &str) -> Value {
    let output = recover(workspace, transaction_id, "--status");
    assert_eq!(output.status.code(), Some(0));
    serde_json::from_slice(&output.stdout).expect("status should be JSON")
}

fn manifest(workspace: &TempDir, transaction_id: &str) -> srcmv_fs::Manifest {
    let bytes = fs::read(
        workspace
            .path()
            .join(".codesplice/transactions")
            .join(transaction_id)
            .join("manifest.rec"),
    )
    .expect("manifest should read");
    decode_manifest_record(&bytes).expect("manifest should decode")
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

fn resolve_to_uniform_state(workspace: &TempDir) {
    let Some(id) = any_transaction_id(workspace) else {
        return;
    };
    let completion = recover(workspace, &id, "--complete");
    if completion.status.code() != Some(0) {
        let rollback = recover(workspace, &id, "--rollback");
        assert_eq!(rollback.status.code(), Some(0));
    }
    let source = fs::read(workspace.path().join("source")).expect("source should read");
    if source == b"abc" {
        assert_original(workspace);
    } else {
        assert_planned(workspace);
    }
}

#[test]
fn multi_target_crash_after_first_install_should_report_mixed_visibility_and_roll_back() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_install_rename_target-00000000");
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);

    let report = status(&workspace, &id);
    let manifest = manifest(&workspace, &id);
    assert_eq!(
        report["transaction"]["visibility"],
        "mixed_old_new_possible"
    );
    assert_eq!(
        manifest
            .targets
            .iter()
            .map(|target| (target.target_index, target.path.as_str()))
            .collect::<Vec<_>>(),
        [(0, "new"), (1, "source"), (2, "target")]
    );
    let rollback = recover(&workspace, &id, "--rollback");

    assert_eq!(rollback.status.code(), Some(0));
    assert_original(&workspace);
}

#[test]
fn multi_target_crash_after_first_install_should_complete_all_outputs() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_install_rename_target-00000000");
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);

    let completion = recover(&workspace, &id, "--complete");

    assert_eq!(completion.status.code(), Some(0));
    assert_planned(&workspace);
}

#[test]
fn multi_target_recovery_should_mutate_nothing_when_any_target_is_unexpected() {
    let workspace = workspace();
    let crashed = crash_commit(&workspace, "after_install_rename_target-00000000");
    assert_eq!(crashed.status.code(), Some(86));
    let id = transaction_id(&workspace);
    fs::write(workspace.path().join("target"), b"third-party")
        .expect("third party should modify a later target");

    let completion = recover(&workspace, &id, "--complete");
    let error: Value = serde_json::from_slice(&completion.stdout).expect("error should be JSON");

    assert_eq!(completion.status.code(), Some(3));
    assert_eq!(error["code"], "RECOVERY_CONFLICT");
    assert_eq!(
        fs::read(workspace.path().join("new")).expect("installed target should remain"),
        b"bc"
    );
    assert_eq!(
        fs::read(workspace.path().join("source")).expect("untouched source should remain"),
        b"abc"
    );
    assert_eq!(
        fs::read(workspace.path().join("target")).expect("third-party bytes should remain"),
        b"third-party"
    );
}

#[test]
fn multi_target_crash_recovery_should_cover_every_target_index() {
    let failpoints = [
        "after_candidate_verification_target-00000000",
        "after_candidate_verification_target-00000001",
        "after_candidate_verification_target-00000002",
        "after_install_rename_target-00000000",
        "after_backup_rename_target-00000001",
        "after_install_rename_target-00000001",
        "after_backup_rename_target-00000002",
        "after_install_rename_target-00000002",
    ];
    for failpoint in failpoints {
        let workspace = workspace();
        let crashed = crash_commit(&workspace, failpoint);
        assert_eq!(crashed.status.code(), Some(86), "failpoint={failpoint}");
        resolve_to_uniform_state(&workspace);
    }
}

#[test]
fn multi_target_crash_recovery_should_cover_every_state_record_boundary() {
    for sequence in 0..=10 {
        for boundary in ["before", "after"] {
            let workspace = workspace();
            let failpoint = format!("{boundary}_state-{sequence:08}.rec_publication");
            let crashed = crash_commit(&workspace, &failpoint);
            assert_eq!(crashed.status.code(), Some(86), "failpoint={failpoint}");
            resolve_to_uniform_state(&workspace);
        }
    }
}

#[test]
fn multi_target_rollback_should_resume_at_every_reverse_target_index() {
    for target_index in 0..=2 {
        let workspace = workspace();
        let crashed = crash_commit(&workspace, "after_install_rename_target-00000000");
        assert_eq!(crashed.status.code(), Some(86));
        let id = transaction_id(&workspace);
        let failpoint = format!("after_rollback_target_step_target-{target_index:08}");

        let rollback = crash_recovery(&workspace, &id, "--rollback", &failpoint);
        assert_eq!(rollback.status.code(), Some(86), "failpoint={failpoint}");
        let resumed = recover(&workspace, &id, "--rollback");

        assert_eq!(resumed.status.code(), Some(0), "failpoint={failpoint}");
        assert_original(&workspace);
    }
}
