//! End-to-end semantic-selection composition contract test.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_composition_workspace(destination: &Path) {
    let fixture = repository_root().join("tests/golden/selection-v1/composition-workspace/src");
    let destination = destination.join("src");
    fs::create_dir_all(&destination).expect("fixture source directory should be created");
    for name in ["input.rs", "output.rs"] {
        fs::copy(fixture.join(name), destination.join(name))
            .unwrap_or_else(|error| panic!("{name} fixture should be copied: {error}"));
    }
}

fn preview(workspace: &Path, request: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .arg("--workspace")
        .arg(workspace)
        .args(["apply", "--request", "-", "--preview", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codesplice should start");
    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(request)
        .expect("unchanged golden request should be written");
    drop(child.stdin.take());
    child.wait_with_output().expect("codesplice should exit")
}

#[test]
fn copied_request_source_should_preview_unchanged_against_fixture_workspace() {
    let workspace = TempDir::new().expect("temporary workspace should be created");
    copy_composition_workspace(workspace.path());
    let request =
        fs::read(repository_root().join("tests/golden/selection-v1/composition-edit-request.json"))
            .expect("composition edit request should be readable");
    let source_before =
        fs::read(workspace.path().join("src/input.rs")).expect("fixture source should be readable");
    let destination_before = fs::read(workspace.path().join("src/output.rs"))
        .expect("fixture destination should be readable");

    let output = preview(workspace.path(), &request);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("preview stdout should be JSON");
    assert_eq!(report["protocol_version"], 1);
    assert_eq!(report["resolved_operations"][0]["source_start"], 0);
    assert_eq!(report["resolved_operations"][0]["source_end"], 42);
    assert_eq!(
        report["resolved_operations"][0]["selected_payload_sha256"],
        "sha256:be453f70d1e77e49cac7efacebf2d46d789fbc3629aef4337e25bf566c3f780b"
    );
    assert_eq!(
        fs::read(workspace.path().join("src/input.rs")).expect("source should remain readable"),
        source_before
    );
    assert_eq!(
        fs::read(workspace.path().join("src/output.rs"))
            .expect("destination should remain readable"),
        destination_before
    );
    assert!(!workspace.path().join(".codesplice").exists());
}
