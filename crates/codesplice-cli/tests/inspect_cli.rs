//! End-to-end tests for read-only Phase 3 workspace inspection.

use std::fs::{self, Metadata};
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestWorkspace(PathBuf);

impl TestWorkspace {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codesplice-inspect-cli-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test workspace should be created");
        Self(path)
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codesplice"))
            .arg("--workspace")
            .arg(&self.0)
            .args(arguments)
            .output()
            .expect("codesplice must run")
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("test workspace should be removed");
    }
}

fn json_stdout(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout).expect("stdout must be one JSON value")
}

#[test]
fn inspect_cli_should_report_existing_mixed_lines_and_valid_absence() {
    let workspace = TestWorkspace::new();
    fs::create_dir(workspace.0.join("src")).expect("parent should be created");
    fs::write(workspace.0.join("src/mixed"), b"a\r\nb\rc\n").expect("fixture should be written");

    let output = workspace.invoke(&[
        "inspect",
        "--path",
        "src/mixed",
        "--path",
        "src/new",
        "--json",
    ]);
    let report = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["protocol_version"], 1);
    assert_eq!(report["paths"][0]["path"], "src/mixed");
    assert_eq!(report["paths"][0]["exists"], true);
    assert_eq!(report["paths"][0]["file_type"], "regular");
    assert_eq!(report["paths"][0]["byte_length"], 7);
    assert_eq!(report["paths"][0]["line_count"], 3);
    assert_eq!(
        report["paths"][0]["sha256"],
        "sha256:7a481f8dd64383e5c6d7c7dd12a88d3594eed36da128736ebf4ffecb48f06cac"
    );
    let identity = report["paths"][0]["identity_hash"]
        .as_str()
        .expect("identity hash should be a string");
    assert!(identity.starts_with("sha256:"));
    assert_eq!(identity.len(), 71);
    assert_eq!(report["paths"][1]["path"], "src/new");
    assert_eq!(report["paths"][1]["exists"], false);
    assert_eq!(report["paths"][1]["file_type"], "absent");
    assert!(report["paths"][1]["sha256"].is_null());
    assert!(report["paths"][1]["byte_length"].is_null());
    assert!(report["paths"][1]["line_count"].is_null());
    assert!(report["paths"][1]["identity_hash"].is_null());
    assert_eq!(report["warnings"][0]["code"], "OBSERVATION_MAY_BE_STALE");
}

#[test]
fn inspect_cli_should_handle_empty_unterminated_and_non_utf8_files() {
    let workspace = TestWorkspace::new();
    fs::write(workspace.0.join("empty"), b"").expect("empty fixture should be written");
    fs::write(workspace.0.join("unterminated"), b"last")
        .expect("unterminated fixture should be written");
    fs::write(workspace.0.join("binary"), [0xff, 0x00, b'\r'])
        .expect("binary fixture should be written");

    let output = workspace.invoke(&[
        "inspect",
        "--path",
        "empty",
        "--path",
        "unterminated",
        "--path",
        "binary",
        "--json",
    ]);
    let report = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["paths"][0]["line_count"], 0);
    assert_eq!(report["paths"][1]["line_count"], 1);
    assert_eq!(report["paths"][2]["line_count"], 1);
    assert_eq!(report["paths"][2]["byte_length"], 3);
}

#[test]
fn inspect_cli_should_preserve_request_order_and_reuse_exact_duplicate_paths() {
    let workspace = TestWorkspace::new();
    fs::write(workspace.0.join("a"), b"a").expect("fixture should be written");

    let output = workspace.invoke(&[
        "inspect", "--path", "missing", "--path", "a", "--path", "missing", "--json",
    ]);
    let report = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["paths"][0]["path"], "missing");
    assert_eq!(report["paths"][1]["path"], "a");
    assert_eq!(report["paths"][2]["path"], "missing");
    assert_eq!(report["paths"][0], report["paths"][2]);
}

#[test]
fn inspect_cli_should_reject_symlinks_aliases_special_files_and_missing_parents() {
    let workspace = TestWorkspace::new();
    fs::write(workspace.0.join("real"), b"x").expect("fixture should be written");
    symlink("real", workspace.0.join("link")).expect("symlink should be created");
    fs::hard_link(workspace.0.join("real"), workspace.0.join("alias"))
        .expect("hard link should be created");
    fs::create_dir(workspace.0.join("directory")).expect("directory should be created");

    for (arguments, expected) in [
        (
            vec!["inspect", "--path", "link", "--json"],
            "SYMLINK_NOT_ALLOWED",
        ),
        (
            vec!["inspect", "--path", "directory", "--json"],
            "UNSUPPORTED_FILE_TYPE",
        ),
        (
            vec!["inspect", "--path", "missing/child", "--json"],
            "INVALID_REQUEST",
        ),
        (
            vec!["inspect", "--path", "real", "--path", "alias", "--json"],
            "FILE_ALIAS",
        ),
    ] {
        let output = workspace.invoke(&arguments);
        let report = json_stdout(&output);

        assert_ne!(output.status.code(), Some(0));
        assert_eq!(report["code"], expected);
    }
}

#[test]
fn inspect_cli_should_reject_every_invalid_path_form_and_redact_absolute_spelling() {
    let workspace = TestWorkspace::new();

    for path in [
        "/absolute",
        "a//b",
        "a/./b",
        "a/../b",
        ".codesplice/lock",
        ".CoDeSpLiCe/lock",
    ] {
        let output = workspace.invoke(&["inspect", "--path", path, "--json"]);
        let report = json_stdout(&output);

        assert_eq!(output.status.code(), Some(2), "path: {path}");
        assert_eq!(report["code"], "INVALID_REQUEST", "path: {path}");
        if path.starts_with('/') {
            assert_eq!(report["context"]["path"], "<redacted-absolute-path>");
        }
    }
}

#[test]
fn inspect_cli_should_not_create_or_modify_workspace_entries() {
    let workspace = TestWorkspace::new();
    fs::create_dir(workspace.0.join("src")).expect("parent should be created");
    fs::write(workspace.0.join("src/file"), b"read only\n").expect("fixture should be written");
    let before = tree_observation(&workspace.0);

    let output = workspace.invoke(&[
        "inspect",
        "--path",
        "src/file",
        "--path",
        "src/absent",
        "--json",
    ]);
    let after = tree_observation(&workspace.0);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(before, after);
    assert!(!workspace.0.join(".codesplice").exists());
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
