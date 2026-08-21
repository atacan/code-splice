//! Compile-tested compatibility probes for the selected third-party crates.

use std::fs;
use std::path::{Path, PathBuf};

use crossbeam_channel::TrySendError;
use gen_lsp_types::{
    DocumentSymbolParams, DocumentSymbolRequest, DocumentSymbolResponse, InitializeParams,
    InitializeResult, PositionEncodingKind, Request, SymbolKind, TextDocumentIdentifier,
    TextDocumentSync, TextDocumentSyncKind, WorkspaceFolder, WorkspaceFoldersRequest,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use srcmv_lsp::{configuration_path_in, default_configuration_path};
use url::Url;

#[derive(Debug, Deserialize)]
struct TranscriptRecord {
    sequence: u64,
    direction: String,
    message: Value,
}

fn transcript_records(name: &str) -> Vec<TranscriptRecord> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/lsp/transcripts")
        .join(name);
    let contents = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read frozen transcript {}: {error}",
            path.display()
        )
    });

    contents
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("invalid JSONL record in {}: {error}", path.display())
            })
        })
        .collect()
}

fn message_with_method<'a>(records: &'a [TranscriptRecord], method: &str) -> &'a Value {
    &records
        .iter()
        .find(|record| record.message.get("method").and_then(Value::as_str) == Some(method))
        .unwrap_or_else(|| panic!("transcript should contain {method}"))
        .message
}

fn response_with_id(records: &[TranscriptRecord], id: u64) -> &Value {
    &records
        .iter()
        .find(|record| {
            record.message.get("id").and_then(Value::as_u64) == Some(id)
                && record.message.get("result").is_some()
        })
        .unwrap_or_else(|| panic!("transcript should contain response {id}"))
        .message
}

fn typed_round_trip<T>(input: &Value) -> T
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = serde_json::from_value(input.clone()).expect("frozen shape should deserialize");
    let serialized = serde_json::to_value(&typed).expect("frozen shape should serialize");
    assert_eq!(&serialized, input);
    typed
}

#[test]
fn generated_lsp_types_round_trip_frozen_successful_session_shapes() {
    let records = transcript_records("successful-session.jsonl");
    assert!(
        records
            .windows(2)
            .all(|pair| pair[0].sequence + 1 == pair[1].sequence)
    );
    assert!(records.iter().all(|record| {
        matches!(
            record.direction.as_str(),
            "client_to_server" | "server_to_client"
        )
    }));

    let initialize = message_with_method(&records, "initialize");
    let _: InitializeParams = typed_round_trip(&initialize["params"]);

    let initialize_response = response_with_id(&records, 1);
    let result: InitializeResult = typed_round_trip(&initialize_response["result"]);
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF16)
    );
    assert!(matches!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSync::Options(options))
            if options.open_close == Some(true)
                && options.change == Some(TextDocumentSyncKind::Full)
    ));

    let document_symbols = message_with_method(&records, "textDocument/documentSymbol");
    let _: DocumentSymbolParams = typed_round_trip(&document_symbols["params"]);

    let document_symbol_response = response_with_id(&records, 2);
    let response: <DocumentSymbolRequest as Request>::Result =
        typed_round_trip(&document_symbol_response["result"]);
    assert!(matches!(
        response,
        Some(DocumentSymbolResponse::DocumentSymbolList(symbols))
            if symbols.len() == 1
                && symbols[0].children.as_ref().is_some_and(|children| children.len() == 1)
    ));
}

#[test]
fn generated_workspace_folder_result_round_trips_frozen_server_request_transcript() {
    let records = transcript_records("server-requests.jsonl");
    let folders_response = records
        .iter()
        .find(|record| record.message.get("id").and_then(Value::as_str) == Some("folders-request"))
        .expect("server-request transcript should contain the workspace folders response");

    let _: <WorkspaceFoldersRequest as Request>::Result =
        typed_round_trip(&folders_response.message["result"]);
}

#[test]
fn initialize_params_round_trip_with_url_backed_uris_and_position_encodings() {
    let input = json!({
        "processId": 314,
        "rootUri": "file:///workspace/Code%20Splice",
        "capabilities": {
            "general": {
                "positionEncodings": ["utf-8", "utf-16", "utf-32"]
            }
        },
        "workspaceFolders": [{
            "uri": "file:///workspace/Code%20Splice",
            "name": "Code Splice"
        }]
    });

    let params: InitializeParams =
        serde_json::from_value(input.clone()).expect("initialize params should deserialize");
    let serialized = serde_json::to_value(params).expect("initialize params should serialize");
    assert_eq!(serialized, input);
}

#[test]
fn workspace_folder_accepts_url_from_directory_path_without_conversion() {
    let uri = Url::from_directory_path("/workspace/Code Splice")
        .expect("an absolute Unix path should convert to a directory URI");
    let folder = WorkspaceFolder::new(uri.clone(), "Code Splice".to_owned());

    assert_eq!(folder.uri, uri);
}

#[test]
fn initialize_result_deserializes_position_encoding_and_sync_options() {
    let input = json!({
        "capabilities": {
            "positionEncoding": "utf-8",
            "textDocumentSync": {
                "openClose": true,
                "change": 2
            },
            "documentSymbolProvider": true
        },
        "serverInfo": {
            "name": "fixture-ls",
            "version": "1.0.0"
        }
    });

    let result: InitializeResult =
        serde_json::from_value(input.clone()).expect("initialize result should deserialize");
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
    assert!(matches!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSync::Options(options))
            if options.open_close == Some(true)
                && options.change == Some(TextDocumentSyncKind::Incremental)
    ));

    let serialized = serde_json::to_value(result).expect("initialize result should serialize");
    assert_eq!(serialized, input);
}

#[test]
fn initialize_result_deserializes_legacy_text_document_sync_kind() {
    let result: InitializeResult = serde_json::from_value(json!({
        "capabilities": {
            "textDocumentSync": 1
        }
    }))
    .expect("legacy synchronization kind should deserialize");

    assert_eq!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSync::Kind(TextDocumentSyncKind::Full))
    );
}

#[test]
fn document_symbol_params_accept_url_from_file_path_without_conversion() {
    let uri = Url::from_file_path("/workspace/src/lib.rs")
        .expect("an absolute Unix path should convert to a file URI");
    let params: DocumentSymbolParams = serde_json::from_value(json!({
        "textDocument": { "uri": uri.as_str() }
    }))
    .expect("document symbol params should deserialize");

    let identifier: TextDocumentIdentifier = params.text_document;
    assert_eq!(identifier.uri, uri);
}

#[test]
fn document_symbol_response_round_trips_hierarchy_and_unknown_symbol_kind() {
    let input = json!([{
        "name": "outer",
        "kind": 5,
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 4, "character": 1 }
        },
        "selectionRange": {
            "start": { "line": 0, "character": 6 },
            "end": { "line": 0, "character": 11 }
        },
        "children": [{
            "name": "future-symbol",
            "kind": 99,
            "range": {
                "start": { "line": 1, "character": 4 },
                "end": { "line": 3, "character": 5 }
            },
            "selectionRange": {
                "start": { "line": 1, "character": 7 },
                "end": { "line": 1, "character": 20 }
            }
        }]
    }]);

    let response: DocumentSymbolResponse =
        serde_json::from_value(input.clone()).expect("document symbols should deserialize");
    let DocumentSymbolResponse::DocumentSymbolList(symbols) = &response else {
        panic!("hierarchical symbols should use DocumentSymbolList");
    };
    assert!(matches!(
        symbols[0].children.as_deref(),
        Some([child]) if child.kind == SymbolKind::Custom(99)
    ));

    let serialized = serde_json::to_value(response).expect("document symbols should serialize");
    assert_eq!(serialized, input);
}

#[test]
fn bounded_crossbeam_channel_reports_backpressure_without_blocking() {
    let (sender, _receiver) = crossbeam_channel::bounded(1);
    sender
        .try_send("first")
        .expect("the first slot should be free");

    let result = sender.try_send("second");
    assert!(matches!(result, Err(TrySendError::Full("second"))));
}

#[derive(Debug, Deserialize, PartialEq)]
struct ProbeConfiguration {
    version: u64,
    server: ProbeServer,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ProbeServer {
    program: String,
    args: Vec<String>,
}

#[test]
fn toml_deserializes_versioned_server_configuration_with_serde() {
    let input = r#"
        version = 1

        [server]
        program = "/usr/bin/rust-analyzer"
        args = ["--stdio"]
    "#;
    let expected = ProbeConfiguration {
        version: 1,
        server: ProbeServer {
            program: "/usr/bin/rust-analyzer".to_owned(),
            args: vec!["--stdio".to_owned()],
        },
    };

    let parsed: ProbeConfiguration =
        toml::from_str(input).expect("the versioned configuration should deserialize");
    assert_eq!(parsed, expected);
}

#[test]
fn configuration_path_join_is_environment_independent() {
    let base = Path::new("/configuration-root");

    assert_eq!(
        configuration_path_in(base),
        PathBuf::from("/configuration-root/srcmv/config.toml")
    );
}

#[test]
fn default_configuration_path_uses_base_dirs_config_directory() {
    let base_directories = directories::BaseDirs::new()
        .expect("qualified desktop platforms should resolve base directories");
    let expected = base_directories
        .config_dir()
        .join("srcmv")
        .join("config.toml");

    assert_eq!(default_configuration_path(), Some(expected));
}

#[cfg(target_os = "macos")]
#[test]
fn base_dirs_uses_macos_application_support_contract() {
    let base_directories =
        directories::BaseDirs::new().expect("qualified macOS should resolve base directories");
    let expected = base_directories
        .home_dir()
        .join("Library/Application Support/srcmv/config.toml");

    assert_eq!(default_configuration_path(), Some(expected));
}

#[cfg(target_os = "linux")]
#[test]
fn base_dirs_uses_linux_xdg_configuration_contract() {
    let base_directories =
        directories::BaseDirs::new().expect("qualified Linux should resolve base directories");
    let xdg_configuration = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let expected_base =
        xdg_configuration.unwrap_or_else(|| base_directories.home_dir().join(".config"));

    assert_eq!(
        default_configuration_path(),
        Some(expected_base.join("srcmv/config.toml"))
    );
}

#[test]
fn core_byte_range_and_rustix_process_id_types_are_compatible() {
    let range = srcmv_core::ByteRange { start: 3, end: 9 };
    let pid = rustix::process::Pid::from_raw(1).expect("one is a valid nonzero process ID");

    assert_eq!(
        (range.start, range.end, pid.as_raw_nonzero().get()),
        (3, 9, 1)
    );
}

#[test]
fn serde_json_value_remains_the_generated_lsp_any_representation() {
    let value: gen_lsp_types::LspAny = json!({ "custom": true });

    assert_eq!(
        value,
        Value::Object(serde_json::Map::from_iter([(
            "custom".to_owned(),
            Value::Bool(true),
        )]))
    );
}
