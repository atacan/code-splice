//! Phase 6 integration checks for diagnostic locking and read-only behavior.

use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use srcmv_fs::Workspace;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

struct TestWorkspace(TempDir);

impl TestWorkspace {
    fn new() -> Self {
        let root = TempDir::new().expect("temporary workspace should be created");
        fs::write(root.path().join("source"), b"source\n").expect("fixture should be written");
        Self(root)
    }

    fn open(&self) -> Workspace {
        Workspace::open(self.0.path()).expect("workspace should open")
    }

    fn request(&self) -> Value {
        let digest = format!("sha256:{:x}", Sha256::digest(b"source\n"));
        json!({
            "protocol_version": 1,
            "operations": [{
                "kind": "copy",
                "source": {"path":"source","selector":{"kind":"bytes","start":0,"end":7},"precondition":{"kind":"sha256","value":digest}},
                "destination": {"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            }]
        })
    }

    fn invoke(&self, arguments: &[&str], request: Option<&Value>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_srcmv"))
            .arg("--workspace")
            .arg(self.0.path())
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("codesplice should start");
        if let Some(request) = request {
            serde_json::to_writer(
                child.stdin.as_mut().expect("stdin should be piped"),
                request,
            )
            .expect("request should serialize");
        }
        child.stdin.take();
        child.wait_with_output().expect("codesplice should exit")
    }
}

fn json_stdout(output: &Output) -> Value {
    assert!(output.stderr.is_empty());
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout).expect("stdout should contain one JSON value")
}

fn assert_transaction_busy_response(report: &Value) {
    assert_eq!(report["code"], "TRANSACTION_BUSY");
    assert_eq!(report["category"], "transaction");
    assert_eq!(report["retryable"], true);
    assert_eq!(
        report["context"],
        json!({
            "lock_state": "contended",
            "recovery_required": "unknown",
            "safe_next_action": "wait_then_retry"
        })
    );
}

#[test]
fn preview_should_leave_an_absent_control_tree_and_workspace_unchanged() {
    let workspace = TestWorkspace::new();
    let before = tree_observation(workspace.0.path());

    let output = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        Some(&workspace.request()),
    );
    let after = tree_observation(workspace.0.path());

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(before, after);
    assert!(!workspace.0.path().join(".codesplice").exists());
    assert_eq!(
        json_stdout(&output)["warnings"][0]["code"],
        "OBSERVATION_MAY_BE_STALE"
    );
}

#[test]
fn preview_should_hold_a_valid_shared_lock_without_changing_the_tree() {
    let workspace = TestWorkspace::new();
    drop(
        workspace
            .open()
            .mutation_lock()
            .expect("control tree should be created"),
    );
    let before = tree_observation(workspace.0.path());

    let output = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        Some(&workspace.request()),
    );
    let report = json_stdout(&output);
    let after = tree_observation(workspace.0.path());

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(before, after);
    assert_eq!(report["warnings"], json!([]));
}

#[test]
fn preview_should_report_safe_contention_before_a_transaction_exists() {
    let workspace = TestWorkspace::new();
    let lock = workspace
        .open()
        .mutation_lock()
        .expect("exclusive lock should succeed");

    let output = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        Some(&workspace.request()),
    );
    let report = json_stdout(&output);

    assert_eq!(output.status.code(), Some(5));
    assert_transaction_busy_response(&report);

    let human = workspace.invoke(
        &["apply", "--request", "-", "--preview"],
        Some(&workspace.request()),
    );
    assert_eq!(human.status.code(), Some(5));
    assert!(human.stdout.is_empty());
    assert_eq!(
        human.stderr,
        b"codesplice: TRANSACTION_BUSY: an incompatible workspace lock is held; wait and retry; never bypass or remove the lock\n"
    );

    drop(lock);

    let recovered = workspace.invoke(&["recover", "--list", "--json"], None);
    assert_eq!(recovered.status.code(), Some(0));
    assert_eq!(json_stdout(&recovered)["transactions"], json!([]));
}

#[test]
fn changing_commit_should_report_contention_while_a_shared_reader_holds_the_lock() {
    let workspace = TestWorkspace::new();
    drop(
        workspace
            .open()
            .mutation_lock()
            .expect("control tree should be created"),
    );
    let request = workspace.request();
    let preview = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        Some(&request),
    );
    let preview_report = json_stdout(&preview);
    assert_eq!(preview.status.code(), Some(0));
    let plan = preview_report["plan_sha256"]
        .as_str()
        .expect("preview should report a plan digest")
        .to_owned();
    let shared = workspace
        .open()
        .diagnostic_lock()
        .expect("diagnostic lock should succeed")
        .expect("control tree should exist");

    let commit = workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--expect-plan",
            &plan,
            "--json",
        ],
        Some(&request),
    );
    let report = json_stdout(&commit);

    assert_eq!(commit.status.code(), Some(5));
    assert_transaction_busy_response(&report);
    assert!(!workspace.0.path().join("new").exists());

    drop(shared);
    let recovered = workspace.invoke(&["recover", "--list", "--json"], None);
    assert_eq!(recovered.status.code(), Some(0));
    assert_eq!(json_stdout(&recovered)["transactions"], json!([]));
}

#[test]
fn preview_and_inspect_should_require_recovery_for_a_quiescent_active_transaction() {
    let workspace = TestWorkspace::new();
    let lock = workspace
        .open()
        .mutation_lock()
        .expect("exclusive lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("active transaction should be created");
    let transaction_id = directory.transaction_id().to_owned();
    drop(directory);
    drop(lock);

    let preview = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        Some(&workspace.request()),
    );
    let inspect = workspace.invoke(&["inspect", "--path", "source", "--json"], None);

    for output in [preview, inspect] {
        let report = json_stdout(&output);
        assert_eq!(output.status.code(), Some(5));
        assert_eq!(report["code"], "TRANSACTION_RECOVERY_REQUIRED");
        assert_eq!(report["context"]["transaction_ids"][0], transaction_id);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntryObservation {
    path: PathBuf,
    file_type: &'static str,
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

fn tree_observation(root: &Path) -> Vec<EntryObservation> {
    let mut pending = vec![root.to_path_buf()];
    let mut observations = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("workspace tree should be readable") {
            let entry = entry.expect("workspace entry should be readable");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("metadata should be readable");
            if metadata.is_dir() {
                pending.push(path.clone());
            }
            observations.push(observe(root, path, &metadata));
        }
    }
    observations.sort_by(|left, right| left.path.cmp(&right.path));
    observations
}

fn observe(root: &Path, path: PathBuf, metadata: &Metadata) -> EntryObservation {
    EntryObservation {
        path: path
            .strip_prefix(root)
            .expect("entry should be beneath root")
            .to_path_buf(),
        file_type: if metadata.is_dir() {
            "directory"
        } else {
            "file"
        },
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        length: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
    }
}
