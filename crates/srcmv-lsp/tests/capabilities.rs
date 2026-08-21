//! Contract tests for LSP initialization capability negotiation.

use std::fs;
use std::path::Path;

use codesplice_lsp::capabilities::{
    CapabilityError, NegotiatedCapabilities, ServerIdentity, SupportedPositionEncoding,
    client_capabilities, initialize_params, validate_initialize_result,
};
use gen_lsp_types::InitializeResult;
use serde_json::{Value, json};
use url::Url;

fn initialize_result(capabilities: Value, server_info: Option<Value>) -> InitializeResult {
    let mut result = json!({"capabilities": capabilities});
    if let Some(server_info) = server_info {
        result["serverInfo"] = server_info;
    }
    serde_json::from_value(result).expect("fixture initialize result should be typed LSP JSON")
}

fn validate_capabilities(capabilities: Value) -> Result<NegotiatedCapabilities, CapabilityError> {
    validate_initialize_result(&initialize_result(
        capabilities,
        Some(json!({"name": "fixture", "version": "1"})),
    ))
}

fn full_capabilities() -> Value {
    json!({
        "positionEncoding": "utf-16",
        "textDocumentSync": {"openClose": true, "change": 1},
        "documentSymbolProvider": true
    })
}

#[test]
fn client_capabilities_have_the_frozen_minimal_shape() {
    let actual =
        serde_json::to_value(client_capabilities()).expect("capabilities should serialize");
    assert_eq!(
        actual,
        json!({
            "workspace": {
                "applyEdit": false,
                "workspaceFolders": true,
                "configuration": true
            },
            "textDocument": {
                "synchronization": {"dynamicRegistration": false},
                "documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true
                }
            },
            "window": {"workDoneProgress": false},
            "general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]}
        })
    );
}

#[test]
fn initialize_params_match_the_frozen_success_transcript() {
    let transcript_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/lsp/transcripts/successful-session.jsonl");
    let first_line = fs::read_to_string(&transcript_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", transcript_path.display()))
        .lines()
        .next()
        .expect("successful transcript should have an initialize record")
        .to_owned();
    let record: Value = serde_json::from_str(&first_line).expect("transcript should contain JSON");
    let expected = &record["message"]["params"];

    let actual = serde_json::to_value(initialize_params(
        Some(4242),
        Some("0.2.1".to_owned()),
        Url::parse("file:///fixture/workspace/").expect("fixture URI should parse"),
        "workspace".to_owned(),
    ))
    .expect("initialize params should serialize");

    assert_eq!(&actual, expected);
    assert_eq!(actual["rootUri"], actual["workspaceFolders"][0]["uri"]);
    assert_eq!(
        actual["workspaceFolders"]
            .as_array()
            .expect("workspace folders should be an array")
            .len(),
        1
    );
}

#[test]
fn accepts_success_and_reports_nullable_server_identity() {
    let negotiated = validate_initialize_result(&initialize_result(
        full_capabilities(),
        Some(json!({"name": "codesplice-fake-lsp"})),
    ))
    .expect("full capabilities should be accepted");
    assert_eq!(
        negotiated.position_encoding,
        SupportedPositionEncoding::Utf16
    );
    assert_eq!(
        negotiated.server,
        ServerIdentity {
            name: Some("codesplice-fake-lsp".to_owned()),
            version: None,
        }
    );

    let without_server_info =
        validate_initialize_result(&initialize_result(full_capabilities(), None))
            .expect("serverInfo is optional");
    assert_eq!(without_server_info.server, ServerIdentity::default());
}

#[test]
fn accepts_all_supported_position_encoding_variants_and_utf16_default() {
    let cases = [
        (Some("utf-8"), SupportedPositionEncoding::Utf8),
        (Some("utf-16"), SupportedPositionEncoding::Utf16),
        (Some("utf-32"), SupportedPositionEncoding::Utf32),
        (None, SupportedPositionEncoding::Utf16),
    ];

    for (wire_encoding, expected) in cases {
        let mut capabilities = full_capabilities();
        if let Some(wire_encoding) = wire_encoding {
            capabilities["positionEncoding"] = json!(wire_encoding);
        } else {
            capabilities
                .as_object_mut()
                .expect("fixture capabilities should be an object")
                .remove("positionEncoding");
        }
        let actual = validate_capabilities(capabilities)
            .unwrap_or_else(|error| panic!("{wire_encoding:?} should be accepted: {error}"));
        assert_eq!(actual.position_encoding, expected);
    }
}

#[test]
fn rejects_the_fake_servers_unsupported_future_encoding() {
    let mut capabilities = full_capabilities();
    capabilities["positionEncoding"] = json!("fixture-encoding");

    assert_eq!(
        validate_capabilities(capabilities),
        Err(CapabilityError::UnsupportedPositionEncoding)
    );
}

#[test]
fn accepts_full_and_incremental_sync_in_legacy_and_options_forms() {
    let cases = [
        ("legacy-full", json!(1)),
        ("legacy-incremental", json!(2)),
        ("options-full", json!({"openClose": true, "change": 1})),
        (
            "options-incremental",
            json!({"openClose": true, "change": 2}),
        ),
    ];

    for (scenario, sync) in cases {
        let capabilities = json!({
            "positionEncoding": "utf-16",
            "textDocumentSync": sync,
            "documentSymbolProvider": {}
        });
        assert!(
            validate_capabilities(capabilities).is_ok(),
            "scenario {scenario}"
        );
    }
}

#[test]
fn rejects_every_fake_server_document_symbol_failure_variant() {
    let mut missing = full_capabilities();
    missing
        .as_object_mut()
        .expect("fixture capabilities should be an object")
        .remove("documentSymbolProvider");
    assert_eq!(
        validate_capabilities(missing),
        Err(CapabilityError::DocumentSymbolsUnavailable)
    );

    let mut explicitly_false = full_capabilities();
    explicitly_false["documentSymbolProvider"] = json!(false);
    assert_eq!(
        validate_capabilities(explicitly_false),
        Err(CapabilityError::DocumentSymbolsUnavailable)
    );
}

#[test]
fn rejects_every_fake_server_document_sync_failure_variant() {
    let cases = [
        ("missing", None),
        (
            "open-close-false",
            Some(json!({"openClose": false, "change": 1})),
        ),
        ("open-close-omitted", Some(json!({"change": 1}))),
        ("change-omitted", Some(json!({"openClose": true}))),
        ("legacy-none", Some(json!(0))),
        ("future-kind", Some(json!(99))),
    ];

    for (scenario, sync) in cases {
        let mut capabilities = full_capabilities();
        if let Some(sync) = sync {
            capabilities["textDocumentSync"] = sync;
        } else {
            capabilities
                .as_object_mut()
                .expect("fixture capabilities should be an object")
                .remove("textDocumentSync");
        }
        assert_eq!(
            validate_capabilities(capabilities),
            Err(CapabilityError::DocumentSyncUnavailable),
            "scenario {scenario}"
        );
    }
}

#[test]
fn capability_errors_do_not_echo_untrusted_server_values() {
    let mut capabilities = full_capabilities();
    capabilities["positionEncoding"] = json!("secret-server-value");
    let error = validate_capabilities(capabilities).expect_err("encoding should be rejected");

    assert_eq!(error, CapabilityError::UnsupportedPositionEncoding);
    assert!(!error.to_string().contains("secret-server-value"));
}
