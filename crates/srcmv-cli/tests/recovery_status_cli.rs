//! End-to-end Phase 5 recovery list, status, contention, and control-only rollback tests.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use srcmv_fs::{
    Manifest, ManifestTarget, MetadataPolicy, PersistedIdentity, TransactionJournal, Workspace,
};
use tempfile::TempDir;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct TestWorkspace {
    root: TempDir,
}

impl TestWorkspace {
    fn new() -> Self {
        Self {
            root: TempDir::new().expect("temporary workspace should be created"),
        }
    }

    fn open(&self) -> Workspace {
        Workspace::open(self.root.path()).expect("workspace should open")
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_srcmv"))
            .arg("--workspace")
            .arg(self.root.path())
            .args(arguments)
            .output()
            .expect("srcmv should run")
    }
}

fn json_stdout(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout).expect("stdout should contain one JSON value")
}

fn assert_transaction_busy_response(response: &Value) {
    assert_eq!(response["code"], "TRANSACTION_BUSY");
    assert_eq!(response["category"], "transaction");
    assert_eq!(response["retryable"], true);
    assert_eq!(
        response["context"],
        serde_json::json!({
            "lock_state": "contended",
            "recovery_required": "unknown",
            "safe_next_action": "wait_then_retry"
        })
    );
}

fn empty_manifest(transaction_id: &str) -> Manifest {
    Manifest {
        transaction_version: 1,
        transaction_id: transaction_id.to_owned(),
        workspace_identity: PersistedIdentity {
            device: 1,
            inode: 2,
        },
        plan_sha256: DIGEST.to_owned(),
        inputs: Vec::new(),
        targets: vec![ManifestTarget {
            target_index: 0,
            path: "future-target".to_owned(),
            parent_identity: PersistedIdentity {
                device: 1,
                inode: 2,
            },
            original_existed: false,
            original_identity: None,
            original_sha256: None,
            original_length: None,
            candidate_name: "candidate-00000000".to_owned(),
            backup_name: "backup-00000000".to_owned(),
            candidate_sha256: DIGEST.to_owned(),
            candidate_length: 0,
            metadata_policy: MetadataPolicy::NewFileMode,
            new_file_mode: Some(0o644),
            segments: Vec::new(),
        }],
        metadata_limitations: Vec::new(),
    }
}

#[test]
fn recovery_list_should_create_nothing_when_control_tree_is_absent() {
    let workspace = TestWorkspace::new();

    let output = workspace.invoke(&["recover", "--list", "--json"]);
    let response = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(response["protocol_version"], 1);
    assert_eq!(response["transactions"], serde_json::json!([]));
    assert!(!workspace.root.path().join(".codesplice").exists());
}

#[test]
fn recovery_list_and_status_should_report_orphan_manifest_and_active_entries() {
    let workspace = TestWorkspace::new();
    let fs_workspace = workspace.open();
    let lock = fs_workspace.mutation_lock().expect("lock should succeed");
    let orphan = lock
        .create_transaction_directory()
        .expect("orphan should allocate");
    let orphan_id = orphan.transaction_id().to_owned();
    drop(orphan);
    let manifest_only = lock
        .create_transaction_directory()
        .expect("manifest-only should allocate");
    let manifest_id = manifest_only.transaction_id().to_owned();
    TransactionJournal::create(manifest_only, &empty_manifest(&manifest_id))
        .expect("manifest should publish");
    drop(lock);

    let output = workspace.invoke(&["recover", "--list", "--json"]);
    let response = json_stdout(&output);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(response["transactions"].as_array().map(Vec::len), Some(2));

    for (id, classification) in [
        (orphan_id.as_str(), "orphan_record"),
        (manifest_id.as_str(), "manifest_only"),
    ] {
        let output = workspace.invoke(&["recover", id, "--status", "--json"]);
        let response = json_stdout(&output);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(response["transaction"]["transaction_id"], id);
        assert_eq!(response["transaction"]["classification"], classification);
        assert_eq!(
            response["transaction"]["actions"],
            serde_json::json!(["status", "rollback"])
        );
    }
}

#[test]
fn recovery_status_should_return_busy_deterministically_under_exclusive_lock() {
    let workspace = TestWorkspace::new();
    let fs_workspace = workspace.open();
    let lock = fs_workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction should allocate");
    let id = directory.transaction_id().to_owned();
    drop(directory);

    let output = workspace.invoke(&["recover", &id, "--status", "--json"]);
    let response = json_stdout(&output);

    assert_eq!(output.status.code(), Some(5));
    assert_transaction_busy_response(&response);

    drop(lock);
    let list = workspace.invoke(&["recover", "--list", "--json"]);
    let response = json_stdout(&list);
    assert_eq!(list.status.code(), Some(0));
    assert_eq!(response["transactions"][0]["transaction_id"], id);
    assert_eq!(
        response["transactions"][0]["classification"],
        "orphan_record"
    );

    let inspect = workspace.invoke(&["inspect", "--path", "future-target", "--json"]);
    let response = json_stdout(&inspect);
    assert_eq!(inspect.status.code(), Some(5));
    assert_eq!(response["code"], "TRANSACTION_RECOVERY_REQUIRED");
    assert_eq!(
        response["context"]["transaction_ids"],
        serde_json::json!([id])
    );
}

#[test]
fn recovery_control_only_rollback_should_remove_manifest_only_without_touching_user_target() {
    let workspace = TestWorkspace::new();
    let target = workspace.root.path().join("user-target");
    fs::write(&target, b"exact user bytes").expect("target fixture should be written");
    let before = tree_without_control(workspace.root.path());
    let fs_workspace = workspace.open();
    let lock = fs_workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction should allocate");
    let id = directory.transaction_id().to_owned();
    TransactionJournal::create(directory, &empty_manifest(&id)).expect("manifest should publish");
    drop(lock);

    let output = workspace.invoke(&["recover", &id, "--rollback", "--json"]);
    let response = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(response["transaction"]["transaction_id"], id);
    assert_eq!(response["transaction"]["classification"], "cleanup_only");
    assert_eq!(tree_without_control(workspace.root.path()), before);
    assert_eq!(
        fs::read(target).expect("target should remain"),
        b"exact user bytes"
    );
}

#[test]
fn recovery_commands_should_reject_bad_ids_unknown_entries_and_missing_transactions() {
    let workspace = TestWorkspace::new();
    let invalid = workspace.invoke(&["recover", "ABC", "--status", "--json"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(json_stdout(&invalid)["code"], "INVALID_REQUEST");

    let missing_id = "0123456789abcdef0123456789abcdef";
    let missing = workspace.invoke(&["recover", missing_id, "--status", "--json"]);
    assert_eq!(missing.status.code(), Some(5));
    assert_eq!(json_stdout(&missing)["code"], "TRANSACTION_NOT_FOUND");

    let fs_workspace = workspace.open();
    let lock = fs_workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction should allocate");
    let id = directory.transaction_id().to_owned();
    fs::write(directory.path().join("unknown"), b"preserve")
        .expect("unknown entry should be created");
    drop(lock);
    let corrupt = workspace.invoke(&["recover", "--list", "--json"]);
    assert_eq!(corrupt.status.code(), Some(6));
    assert_eq!(json_stdout(&corrupt)["code"], "TRANSACTION_RECORD_CORRUPT");
    assert_eq!(
        fs::read(
            workspace
                .root
                .path()
                .join(".codesplice/transactions")
                .join(id)
                .join("unknown")
        )
        .expect("unknown entry should remain"),
        b"preserve"
    );
}

fn tree_without_control(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = fs::read_dir(root)
        .expect("workspace should read")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() != ".codesplice")
        .map(|entry| {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("entry should be beneath root")
                .to_path_buf();
            (
                relative,
                fs::read(path).expect("fixture entries should be files"),
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
