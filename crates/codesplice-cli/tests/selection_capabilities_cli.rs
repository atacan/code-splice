//! End-to-end tests for target-independent semantic-selection discovery.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codesplice"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("codesplice must start")
}

#[test]
fn selection_capabilities_command_should_match_exact_golden_output() {
    let output = invoke(&["selection-capabilities", "--json"]);
    let expected = fs::read(
        repository_root().join("tests/golden/selection-capabilities-v1/capabilities.json"),
    )
    .expect("selection-capabilities golden must be readable");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty());
}

#[test]
fn selection_capabilities_should_require_json_output() {
    let output = invoke(&["selection-capabilities"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("codesplice: INVALID_CLI:"));
}

#[test]
fn selection_capabilities_should_reject_a_workspace() {
    let output = invoke(&["--workspace", ".", "selection-capabilities", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .expect("stdout must contain JSON")["code"],
        "INVALID_CLI"
    );
}
