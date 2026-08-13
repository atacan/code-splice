//! Validates the frozen fake-language-server transcript corpus.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("test-support crate must be nested under the workspace crates directory")
        .to_path_buf()
}

fn transcript(name: &str) -> Vec<Value> {
    let path = workspace_root()
        .join("tests/fixtures/lsp/transcripts")
        .join(name);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    contents
        .lines()
        .enumerate()
        .map(|(line, record)| {
            serde_json::from_str(record).unwrap_or_else(|error| {
                panic!(
                    "failed to parse {} line {}: {error}",
                    path.display(),
                    line + 1
                )
            })
        })
        .collect()
}

#[test]
fn successful_transcript_should_have_contiguous_sequence_numbers() {
    let records = transcript("successful-session.jsonl");
    let actual = records
        .iter()
        .map(|record| record["sequence"].as_u64())
        .collect::<Vec<_>>();
    let expected = (1..=records.len() as u64).map(Some).collect::<Vec<_>>();

    assert_eq!(actual, expected);
}

#[test]
fn successful_transcript_should_freeze_the_complete_method_order() {
    let methods = transcript("successful-session.jsonl")
        .iter()
        .filter_map(|record| record.pointer("/message/method").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();

    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "workspace/didChangeConfiguration",
            "textDocument/didOpen",
            "textDocument/documentSymbol",
            "textDocument/didClose",
            "shutdown",
            "exit",
        ]
    );
}

#[test]
fn successful_transcript_should_send_the_exact_fixture_document_text() {
    let records = transcript("successful-session.jsonl");
    let did_open_text = records
        .iter()
        .find_map(|record| {
            (record.pointer("/message/method").and_then(Value::as_str)
                == Some("textDocument/didOpen"))
            .then(|| {
                record
                    .pointer("/message/params/textDocument/text")
                    .and_then(Value::as_str)
            })
            .flatten()
        })
        .expect("successful transcript must contain didOpen text");
    let source_path = workspace_root().join("tests/fixtures/lsp/documents/source.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()));

    assert_eq!(did_open_text, source);
}

#[test]
fn successful_transcript_should_freeze_hierarchical_document_symbols() {
    let records = transcript("successful-session.jsonl");
    let symbol_response = records
        .iter()
        .find_map(|record| record.pointer("/message/result/0/children/0/name"))
        .and_then(Value::as_str);

    assert_eq!(symbol_response, Some("alpha"));
}

#[test]
fn server_request_transcript_should_pair_every_request_and_response_id() {
    let records = transcript("server-requests.jsonl");
    let ids = records
        .chunks_exact(2)
        .map(|pair| {
            (
                pair[0].pointer("/message/id").cloned(),
                pair[1].pointer("/message/id").cloned(),
            )
        })
        .collect::<Vec<_>>();

    assert!(
        ids.iter().all(|(request, response)| request == response),
        "request/response ID pairs differed: {ids:?}"
    );
}

#[test]
fn every_transcript_record_should_declare_json_rpc_2() {
    for name in ["successful-session.jsonl", "server-requests.jsonl"] {
        for record in transcript(name) {
            assert_eq!(
                record.pointer("/message/jsonrpc").and_then(Value::as_str),
                Some("2.0"),
                "invalid JSON-RPC version in {name}: {record}"
            );
        }
    }
}
