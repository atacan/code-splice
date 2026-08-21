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
        let mut child = Command::new(env!("CARGO_BIN_EXE_srcmv"))
            .arg("--workspace")
            .arg(self.0.path())
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
    assert!(!workspace.0.path().join(".srcmv").exists());
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
fn summary_modes_should_preserve_plan_and_add_complete_nonduplicative_review_metadata() {
    let workspace = TestWorkspace::new();
    let source = b"one\nmove\ncopy\nend\n";
    let destination = b"dest\n";
    workspace.write("a.txt", source);
    workspace.write("b.txt", destination);
    let request = demonstration_request(source, destination);

    let default = json_stdout(&workspace.invoke(
        &["apply", "--request", "-", "--preview", "--json"],
        &request,
    ));
    let no_diff = json_stdout(&workspace.invoke(
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
    let summary = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--json",
        ],
        &request,
    ));
    let concise = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--no-diff",
            "--json",
        ],
        &request,
    ));

    assert_eq!(summary["plan_sha256"], default["plan_sha256"]);
    assert_eq!(concise["plan_sha256"], default["plan_sha256"]);
    assert_eq!(no_diff["plan_sha256"], default["plan_sha256"]);
    assert_eq!(summary["plan_hash_version"], default["plan_hash_version"]);
    assert_eq!(concise["plan_hash_version"], default["plan_hash_version"]);
    assert_eq!(summary["diff"]["text"], default["diff"]["text"]);
    assert_eq!(
        summary["resolved_operations"],
        default["resolved_operations"]
    );
    assert_eq!(summary["outputs"], default["outputs"]);
    assert_eq!(summary["warnings"], default["warnings"]);
    assert_eq!(concise["warnings"], no_diff["warnings"]);
    assert_eq!(concise["diff"]["kind"], "omitted");
    assert_eq!(concise["diff"]["text"], Value::Null);

    let review = &summary["diff"]["summary"]["review"];
    assert!(summary.get("summary").is_none());
    assert_eq!(concise["diff"]["summary"]["review"], *review);
    assert_eq!(review["version"], 1);
    assert_eq!(review["plan_hash_version"], 1);
    assert_eq!(review["plan_sha256"], default["plan_sha256"]);
    assert_eq!(
        review["operations"],
        json!([
            {"operation_index":0,"selected_byte_length":5,"selected_logical_line_count":1},
            {"operation_index":1,"selected_byte_length":5,"selected_logical_line_count":1},
            {"operation_index":2,"selected_byte_length":4,"selected_logical_line_count":1}
        ])
    );
    assert_eq!(
        review["outputs"],
        json!([
            {
                "output_index":0,
                "before_logical_line_count":4,
                "after_logical_line_count":3,
                "insertion_groups_in_output_order":[]
            },
            {
                "output_index":1,
                "before_logical_line_count":1,
                "after_logical_line_count":2,
                "insertion_groups_in_output_order":[
                    {"destination_offset":5,"operation_indices":[0]}
                ]
            },
            {
                "output_index":2,
                "before_logical_line_count":null,
                "after_logical_line_count":1,
                "insertion_groups_in_output_order":[
                    {"destination_offset":0,"operation_indices":[1]}
                ]
            }
        ])
    );
}

#[test]
fn summary_should_report_final_same_offset_order_and_compose_cross_segment_crlf() {
    let workspace = TestWorkspace::new();
    let carriage_return = b"\r";
    let line_feed = b"\n";
    let mixed = b"a\nb\r\nc\rd";
    let destination = b"xy";
    workspace.write("a-cr", carriage_return);
    workspace.write("b-lf", line_feed);
    workspace.write("mixed", mixed);
    workspace.write("z-destination", destination);
    let request = json!({
        "protocol_version":1,
        "operations":[
            {
                "kind":"copy",
                "source":{"path":"a-cr","selector":{"kind":"bytes","start":0,"end":1},"precondition":existing(carriage_return)},
                "destination":{"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            },
            {
                "kind":"copy",
                "source":{"path":"b-lf","selector":{"kind":"bytes","start":0,"end":1},"precondition":existing(line_feed)},
                "destination":{"path":"new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            },
            {
                "kind":"copy",
                "source":{"path":"mixed","selector":{"kind":"bytes","start":3,"end":4},"precondition":existing(mixed)},
                "destination":{"path":"z-destination","anchor":{"kind":"byte_offset","offset":1},"precondition":existing(destination)}
            },
            {
                "kind":"copy",
                "source":{"path":"mixed","selector":{"kind":"bytes","start":4,"end":5},"precondition":existing(mixed)},
                "destination":{"path":"z-destination","anchor":{"kind":"byte_offset","offset":1},"precondition":existing(destination)}
            },
            {
                "kind":"copy",
                "source":{"path":"mixed","selector":{"kind":"bytes","start":5,"end":7},"precondition":existing(mixed)},
                "destination":{"path":"z-destination","anchor":{"kind":"file_end"},"precondition":existing(destination)}
            },
            {
                "kind":"copy",
                "source":{"path":"mixed","selector":{"kind":"bytes","start":0,"end":8},"precondition":existing(mixed)},
                "destination":{"path":"zz-mixed-new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            }
        ]
    });

    let report = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--no-diff",
            "--json",
        ],
        &request,
    ));
    let review = &report["diff"]["summary"]["review"];

    assert_eq!(
        review["operations"],
        json!([
            {"operation_index":0,"selected_byte_length":1,"selected_logical_line_count":1},
            {"operation_index":1,"selected_byte_length":1,"selected_logical_line_count":1},
            {"operation_index":2,"selected_byte_length":1,"selected_logical_line_count":1},
            {"operation_index":3,"selected_byte_length":1,"selected_logical_line_count":1},
            {"operation_index":4,"selected_byte_length":2,"selected_logical_line_count":1},
            {"operation_index":5,"selected_byte_length":8,"selected_logical_line_count":4}
        ])
    );
    assert_eq!(report["outputs"][0]["path"], "new");
    assert_eq!(report["outputs"][1]["path"], "z-destination");
    assert_eq!(review["outputs"][0]["after_logical_line_count"], 1);
    assert_eq!(
        review["outputs"][0]["insertion_groups_in_output_order"],
        json!([{"destination_offset":0,"operation_indices":[0,1]}])
    );
    assert_eq!(review["outputs"][1]["before_logical_line_count"], 1);
    assert_eq!(review["outputs"][1]["after_logical_line_count"], 2);
    assert_eq!(
        review["outputs"][1]["insertion_groups_in_output_order"],
        json!([
            {"destination_offset":1,"operation_indices":[2,3]},
            {"destination_offset":2,"operation_indices":[4]}
        ])
    );
    assert_eq!(report["outputs"][2]["path"], "zz-mixed-new");
    assert_eq!(
        review["outputs"][2]["before_logical_line_count"],
        Value::Null
    );
    assert_eq!(review["outputs"][2]["after_logical_line_count"], 4);
}

#[test]
fn summary_should_distinguish_output_change_kinds_and_exclude_no_op_insertions() {
    let workspace = TestWorkspace::new();
    let emptied = b"gone";
    let modified = b"z";
    let payload = b"P";
    let same = b"aa";
    workspace.write("empty-me", emptied);
    workspace.write("modified", modified);
    workspace.write("payload", payload);
    workspace.write("same", same);
    let request = json!({
        "protocol_version":1,
        "operations":[
            {
                "kind":"move",
                "source":{"path":"empty-me","selector":{"kind":"bytes","start":0,"end":4},"precondition":existing(emptied)},
                "destination":{"path":"moved-new","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}
            },
            {
                "kind":"move",
                "source":{"path":"same","selector":{"kind":"bytes","start":0,"end":1},"precondition":existing(same)},
                "destination":{"path":"same","anchor":{"kind":"file_end"},"precondition":existing(same)}
            },
            {
                "kind":"move",
                "source":{"path":"same","selector":{"kind":"bytes","start":0,"end":1},"precondition":existing(same)},
                "destination":{"path":"same","anchor":{"kind":"file_start"},"precondition":existing(same)}
            },
            {
                "kind":"copy",
                "source":{"path":"payload","selector":{"kind":"bytes","start":0,"end":1},"precondition":existing(payload)},
                "destination":{"path":"modified","anchor":{"kind":"file_end"},"precondition":existing(modified)}
            }
        ]
    });

    let report = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--no-diff",
            "--json",
        ],
        &request,
    ));
    let review_outputs = &report["diff"]["summary"]["review"]["outputs"];

    assert_eq!(report["resolved_operations"][2]["effect"], "no_op");
    let output_kinds = report["outputs"]
        .as_array()
        .expect("outputs should be an array")
        .iter()
        .map(|output| {
            (
                output["path"].as_str().expect("path should be text"),
                output["change_kind"]
                    .as_str()
                    .expect("change kind should be text"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        output_kinds,
        vec![
            ("empty-me", "emptied_existing"),
            ("modified", "modified_existing"),
            ("moved-new", "created_new"),
            ("same", "unchanged")
        ]
    );
    assert_eq!(review_outputs[0]["after_logical_line_count"], 0);
    assert_eq!(
        review_outputs[3]["insertion_groups_in_output_order"],
        json!([{"destination_offset":2,"operation_indices":[1]}])
    );
    assert!(
        review_outputs
            .as_array()
            .expect("review outputs should be an array")
            .iter()
            .flat_map(|output| output["insertion_groups_in_output_order"]
                .as_array()
                .unwrap())
            .flat_map(|group| group["operation_indices"].as_array().unwrap())
            .all(|operation_index| operation_index != 2)
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
    let binary_summary_report = json_stdout(&binary_workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--json",
        ],
        &binary_request,
    ));
    assert_eq!(binary_summary_report["diff"]["kind"], "binary");
    assert_eq!(
        binary_summary_report["diff"]["summary"]["outputs"],
        binary_report["diff"]["summary"]["outputs"]
    );
    assert_eq!(
        binary_summary_report["diff"]["summary"]["reason"],
        "binary_content"
    );
    assert_eq!(
        binary_summary_report["diff"]["summary"]["review"]["operations"][0]["selected_logical_line_count"],
        1
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

    let summary_report = json_stdout(&workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--json",
        ],
        &request,
    ));
    assert_eq!(summary_report["diff"]["summary"]["reason"], "diff_budget");
    assert_eq!(
        summary_report["diff"]["summary"]["maximum_diff_bytes"],
        4 * 1024 * 1024
    );
    assert_eq!(
        summary_report["diff"]["summary"]["maximum_work_units"],
        10_000_000
    );
    assert!(summary_report["diff"]["summary"]["review"].is_object());
    assert_eq!(
        summary_report["warnings"]
            .as_array()
            .expect("warnings should be an array")
            .iter()
            .filter(|warning| warning["code"] == "DIFF_TRUNCATED")
            .count(),
        1
    );
}

#[test]
fn summary_should_preserve_detailed_input_limit_fallback_keys() {
    let workspace = TestWorkspace::new();
    let large = vec![b'x'; 8 * 1024 * 1024 + 1];
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
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--json",
        ],
        &request,
    ));

    assert_eq!(report["diff"]["kind"], "text");
    assert_eq!(report["diff"]["text"], Value::Null);
    assert_eq!(report["diff"]["summary"]["reason"], "detailed_input_limit");
    assert!(report["diff"]["summary"]["outputs"].is_array());
    assert!(report["diff"]["summary"]["review"].is_object());
    assert_eq!(
        report["warnings"]
            .as_array()
            .expect("warnings should be an array")
            .iter()
            .filter(|warning| warning["code"] == "DIFF_TRUNCATED")
            .count(),
        1
    );
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
    assert!(stdout.contains("\n- new change=created_new"));
    assert!(!stdout.contains("\n- [0] new change=created_new"));

    let concise = workspace.invoke(
        &[
            "apply",
            "--request",
            "-",
            "--preview",
            "--summary",
            "--no-diff",
        ],
        &request,
    );
    let concise_stdout =
        String::from_utf8(concise.stdout).expect("concise human output should be UTF-8");
    assert_eq!(concise.status.code(), Some(0));
    assert!(concise.stderr.is_empty());
    assert!(concise_stdout.contains("bad\\u{a}\\u{202e}.txt"));
    assert!(!concise_stdout.contains('\u{202e}'));
    assert!(concise_stdout.contains("selected_byte_length=5 selected_logical_line_count=1"));
    assert!(concise_stdout.contains(
        "[0] new change=created_new before_length=null before_sha256=null after_length=5"
    ));
    assert!(concise_stdout.contains("before_logical_line_count=null after_logical_line_count=1"));
    assert!(concise_stdout.contains("insertion_group destination_offset=0 operation_indices=[0]"));
    assert!(concise_stdout.contains("diff:\nomitted (--no-diff)\n"));
    assert_eq!(
        concise_stdout.matches("OBSERVATION_MAY_BE_STALE:").count(),
        1
    );
}

#[test]
fn apply_help_should_document_summary_as_preview_only() {
    let output = Command::new(env!("CARGO_BIN_EXE_srcmv"))
        .args(["apply", "--help"])
        .output()
        .expect("srcmv help should run");
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("--summary"));
    assert!(stdout.contains("diff.summary.review"));
}
