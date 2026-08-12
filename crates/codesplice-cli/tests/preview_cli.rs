//! End-to-end Phase 6 preview report and diff tests.

use std::fs;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

struct TestWorkspace(TempDir);

impl TestWorkspace {
    fn new() -> Self {
        Self(TempDir::new().expect("temporary workspace should be created"))
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.0.path().join(path), bytes).expect("fixture should be written");
    }

    fn invoke(&self, arguments: &[&str], request: &Value) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_codesplice"))
            .arg("--workspace")
            .arg(self.0.path())
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("codesplice should start");
        serde_json::to_writer(
            child.stdin.as_mut().expect("stdin should be piped"),
            request,
        )
        .expect("request should serialize");
        child.stdin.take();
        child.wait_with_output().expect("codesplice should exit")
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn existing(bytes: &[u8]) -> Value {
    json!({"kind": "sha256", "value": digest(bytes)})
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

fn demonstration_request(source: &[u8], destination: &[u8]) -> Value {
    json!({
        "protocol_version": 1,
        "operations": [
            {
                "kind": "move",
                "source": {
                    "path": "a.txt",
                    "selector": {"kind": "bytes", "start": 4, "end": 9},
                    "precondition": existing(source)
                },
                "destination": {
                    "path": "b.txt",
                    "anchor": {"kind": "file_end"},
                    "precondition": existing(destination)
                }
            },
            {
                "kind": "copy",
                "source": {
                    "path": "a.txt",
                    "selector": {"kind": "bytes", "start": 9, "end": 14},
                    "precondition": existing(source)
                },
                "destination": {
                    "path": "new.txt",
                    "anchor": {"kind": "file_start"},
                    "precondition": {"kind": "must_not_exist"}
                }
            },
            {
                "kind": "move",
                "source": {
                    "path": "a.txt",
                    "selector": {"kind": "bytes", "start": 0, "end": 4},
                    "precondition": existing(source)
                },
                "destination": {
                    "path": "a.txt",
                    "anchor": {"kind": "byte_offset", "offset": 0},
                    "precondition": existing(source)
                }
            }
        ]
    })
}

#[test]
fn preview_json_should_report_real_move_copy_no_op_and_new_destination() {
    let workspace = TestWorkspace::new();
    let source = b"one\nmove\ncopy\nend\n";
    let destination = b"dest\n";
    workspace.write("a.txt", source);
    workspace.write("b.txt", destination);
    let request = demonstration_request(source, destination);

    let output = workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &request,
    );
    let report = json_stdout(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report["protocol_version"], 1);
    assert_eq!(report["plan_hash_version"], 1);
    assert_eq!(report["plan_sha256"].as_str().map(str::len), Some(71));
    assert_eq!(
        report["workspace_identity_hash"].as_str().map(str::len),
        Some(71)
    );
    assert_eq!(
        report["resolved_operations"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(report["resolved_operations"][0]["source_start"], 4);
    assert_eq!(report["resolved_operations"][0]["source_end"], 9);
    assert_eq!(report["resolved_operations"][2]["effect"], "no_op");
    assert_eq!(
        report["resolved_operations"][0]["selected_payload_sha256"],
        digest(b"move\n")
    );
    assert_eq!(report["outputs"].as_array().map(Vec::len), Some(3));
    assert_eq!(report["outputs"][0]["path"], "a.txt");
    assert_eq!(
        report["outputs"][0]["after_sha256"],
        digest(b"one\ncopy\nend\n")
    );
    assert_eq!(
        report["outputs"][1]["after_sha256"],
        digest(b"dest\nmove\n")
    );
    assert_eq!(report["outputs"][2]["change_kind"], "created_new");
    assert_eq!(report["outputs"][2]["before_length"], Value::Null);
    assert_eq!(report["outputs"][2]["after_sha256"], digest(b"copy\n"));
    assert_eq!(report["diff"]["kind"], "text");
    assert_eq!(report["warnings"][0]["code"], "OBSERVATION_MAY_BE_STALE");
    assert_eq!(fs::read(workspace.0.path().join("a.txt")).unwrap(), source);
    assert_eq!(
        fs::read(workspace.0.path().join("b.txt")).unwrap(),
        destination
    );
    assert!(!workspace.0.path().join("new.txt").exists());
    assert!(!workspace.0.path().join(".codesplice").exists());
}

#[test]
fn no_diff_should_preserve_the_plan_and_omit_only_diff_detail() {
    let workspace = TestWorkspace::new();
    let source = b"one\nmove\ncopy\nend\n";
    let destination = b"dest\n";
    workspace.write("a.txt", source);
    workspace.write("b.txt", destination);
    let request = demonstration_request(source, destination);

    let detailed = json_stdout(&workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &request,
    ));
    let omitted = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--no-diff",
            "--json",
        ],
        &request,
    ));

    assert_eq!(omitted["plan_sha256"], detailed["plan_sha256"]);
    assert_eq!(
        omitted["resolved_operations"],
        detailed["resolved_operations"]
    );
    assert_eq!(omitted["outputs"], detailed["outputs"]);
    assert_eq!(
        omitted["diff"],
        json!({"kind":"omitted","text":null,"summary":null})
    );
}

#[test]
fn preview_should_emit_binary_samples_and_mixed_terminator_labels() {
    let binary_workspace = TestWorkspace::new();
    let binary = [0xff, 0x00, b'A'];
    binary_workspace.write("binary", binary);
    let binary_request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {"path":"binary","selector":{"kind":"bytes","start":0,"end":3},"precondition":existing(&binary)},
            "destination": {"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
        }]
    });
    let binary_report = json_stdout(&binary_workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &binary_request,
    ));
    assert_eq!(binary_report["diff"]["kind"], "binary");
    assert_eq!(
        binary_report["diff"]["summary"]["outputs"][0]["after_samples"]["head_base64"],
        "/wBB"
    );

    let text_workspace = TestWorkspace::new();
    let mixed = b"a\r\nb\rc\n";
    text_workspace.write("mixed", mixed);
    let text_request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {"path":"mixed","selector":{"kind":"bytes","start":0,"end":7},"precondition":existing(mixed)},
            "destination": {"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
        }]
    });
    let text_report = json_stdout(&text_workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &text_request,
    ));
    let diff = text_report["diff"]["text"].as_str().expect("text diff");
    assert!(diff.contains("[CRLF]"));
    assert!(diff.contains("[CR]"));
    assert!(diff.contains("[LF]"));
}

#[test]
fn preview_should_bound_large_text_diffs_and_warn() {
    let workspace = TestWorkspace::new();
    let large = vec![b'x'; 4 * 1024 * 1024 + 1];
    workspace.write("large", &large);
    let request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {"path":"large","selector":{"kind":"bytes","start":0,"end":large.len()},"precondition":existing(&large)},
            "destination": {"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
        }]
    });

    let report = json_stdout(&workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &request,
    ));

    assert_eq!(report["diff"]["kind"], "text");
    assert_eq!(report["diff"]["summary"]["reason"], "diff_budget");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "DIFF_TRUNCATED")
    );
    assert!(report["diff"]["text"].as_str().unwrap().len() <= 4 * 1024 * 1024);
}

#[test]
fn human_preview_should_escape_path_controls_and_report_resolved_values() {
    let workspace = TestWorkspace::new();
    let unsafe_path = "bad\n\u{202e}.txt";
    let source = b"safe\n";
    workspace.write(unsafe_path, source);
    let request = json!({
        "protocol_version": 1,
        "operations": [{
            "kind": "copy",
            "source": {"path":unsafe_path,"selector":{"kind":"bytes","start":0,"end":5},"precondition":existing(source)},
            "destination": {"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
        }]
    });

    let output = workspace.invoke(&["apply", "--request", "-", "--preview"], &request);
    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("bad\\u{a}\\u{202e}.txt"));
    assert!(!stdout.contains('\u{202e}'));
    assert!(stdout.contains("selected_payload_sha256=sha256:"));
    assert!(stdout.contains("after_sha256=sha256:"));
}
