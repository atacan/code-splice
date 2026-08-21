//! Concrete semantic-selection protocol-v1 DTO and registry tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use srcmv_protocol::{
    MAX_RESPONSE_BYTES, SelectionByteSelectorDto, SelectionErrorCode, SelectionErrorDto,
    SelectionExtentDto, SelectionKnownSymbolKindDto, SelectionLspPositionDto, SelectionLspRangeDto,
    SelectionMatchDto, SelectionPositionEncodingDto, SelectionQueryDto, SelectionResponse,
    SelectionServerDto, SelectionSourceDto, SelectionSymbolKindDto, WarningCode, WarningDto,
    parse_sha256, to_selection_json_line,
};

const COMPOSITION_SOURCE_DIGEST: &str =
    "sha256:7e05110a7dcdd32e6048b54848c84deb34c920d17678394730035e93fbd4e5be";
const COMPOSITION_PAYLOAD_DIGEST: &str =
    "sha256:be453f70d1e77e49cac7efacebf2d46d789fbc3629aef4337e25bf566c3f780b";
const COMPOSITION_WORKSPACE_DIGEST: &str =
    "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> Value {
    let path = repository_root()
        .join("tests/golden/selection-v1")
        .join(name);
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("{} must contain JSON: {error}", path.display()))
}

fn digest(value: &str) -> srcmv_core::Sha256Digest {
    parse_sha256(value, "test digest").expect("golden digest must parse")
}

fn composition_response() -> SelectionResponse {
    let source_digest = digest(COMPOSITION_SOURCE_DIGEST);
    let selector = SelectionByteSelectorDto::new(0, 42);
    let selected_match = SelectionMatchDto::new(
        "first",
        SelectionKnownSymbolKindDto::Function.into(),
        vec!["first".to_string()],
        Some("pub fn first()".to_string()),
        SelectionLspRangeDto::new(
            SelectionLspPositionDto::new(0, 0),
            SelectionLspPositionDto::new(2, 1),
        ),
        SelectionLspRangeDto::new(
            SelectionLspPositionDto::new(0, 7),
            SelectionLspPositionDto::new(0, 12),
        ),
        SelectionExtentDto::DeclarationLines,
        selector,
        digest(COMPOSITION_PAYLOAD_DIGEST),
        "src/input.rs",
        source_digest,
    );
    SelectionResponse::new(
        digest(COMPOSITION_WORKSPACE_DIGEST),
        SelectionSourceDto::new("src/input.rs", source_digest, 87),
        SelectionQueryDto::name("first", Some(SelectionKnownSymbolKindDto::Function)),
        SelectionServerDto::new(
            Some("rust".to_string()),
            Some("rust-analyzer".to_string()),
            Some("golden-1.0".to_string()),
            SelectionPositionEncodingDto::Utf8,
        ),
        vec![selected_match],
        Vec::new(),
    )
}

fn position_response() -> SelectionResponse {
    let source_digest =
        digest("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let selected_match = SelectionMatchDto::new(
        "calculate_rocket_🚀",
        SelectionKnownSymbolKindDto::Method.into(),
        vec!["Engine".to_string(), "calculate_rocket_🚀".to_string()],
        None,
        SelectionLspRangeDto::new(
            SelectionLspPositionDto::new(2, 4),
            SelectionLspPositionDto::new(5, 5),
        ),
        SelectionLspRangeDto::new(
            SelectionLspPositionDto::new(2, 7),
            SelectionLspPositionDto::new(2, 26),
        ),
        SelectionExtentDto::Symbol,
        SelectionByteSelectorDto::new(12, 64),
        digest("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        "src/unicode.rs",
        source_digest,
    );
    SelectionResponse::new(
        digest("sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"),
        SelectionSourceDto::new("src/unicode.rs", source_digest, 128),
        SelectionQueryDto::position(24, None),
        SelectionServerDto::new(None, None, None, SelectionPositionEncodingDto::Utf16),
        vec![selected_match],
        vec![WarningDto::new(
            WarningCode::ObservationMayBeStale,
            "no existing CodeSplice lock coordinated this read-only observation",
            BTreeMap::new(),
        )],
    )
}

#[test]
fn concrete_success_dtos_should_match_both_frozen_goldens() {
    assert_eq!(
        serde_json::to_value(composition_response()).expect("response must serialize"),
        golden("composition-selection.json")
    );
    assert_eq!(
        serde_json::to_value(position_response()).expect("response must serialize"),
        golden("success-position.json")
    );
}

#[test]
fn request_source_should_be_derived_as_the_copy_ready_golden_fragment() {
    let response = serde_json::to_value(composition_response()).expect("response must serialize");

    assert_eq!(
        response["matches"][0]["request_source"],
        golden("composition-request-source.json")
    );
}

#[test]
fn query_serialization_should_have_exact_fields_and_one_trailing_lf() {
    let line = to_selection_json_line(&SelectionQueryDto::name(
        "first",
        Some(SelectionKnownSymbolKindDto::Function),
    ))
    .expect("query must serialize");

    assert_eq!(
        line,
        "{\"kind\":\"name\",\"name\":\"first\",\"symbol_kind\":\"function\"}\n"
    );
}

#[test]
fn every_symbol_kind_should_use_the_frozen_wire_spelling() {
    let actual = SelectionKnownSymbolKindDto::ALL
        .into_iter()
        .map(|kind| serde_json::to_value(kind).expect("kind must serialize"))
        .collect::<Vec<_>>();
    let expected = vec![
        "file",
        "module",
        "namespace",
        "package",
        "class",
        "method",
        "property",
        "field",
        "constructor",
        "enum",
        "interface",
        "function",
        "variable",
        "constant",
        "string",
        "number",
        "boolean",
        "array",
        "object",
        "key",
        "null",
        "enum_member",
        "struct",
        "event",
        "operator",
        "type_parameter",
    ];

    assert_eq!(actual, expected);
    assert_eq!(
        serde_json::to_value(SelectionSymbolKindDto::Unknown).expect("unknown kind must serialize"),
        "unknown"
    );
}

#[test]
fn selection_error_registry_should_match_the_frozen_golden() {
    let actual = SelectionErrorCode::ALL
        .into_iter()
        .map(|code| {
            json!({
                "code": code,
                "category": code.category(),
                "exit": code.category().exit_code(),
                "retryable": code.retryable(),
            })
        })
        .collect::<Vec<_>>();

    assert_eq!(json!(actual), golden("error-registry.json"));
}

#[test]
fn concrete_error_dtos_should_match_the_frozen_goldens() {
    let not_found = SelectionErrorDto::new(
        SelectionErrorCode::SelectionNotFound,
        "no document symbol matched the selection query",
        BTreeMap::from([
            ("query_kind".to_string(), json!("name")),
            ("name".to_string(), json!("missing_function")),
            ("symbol_kind".to_string(), json!("function")),
        ]),
    );
    let ambiguous = SelectionErrorDto::new(
        SelectionErrorCode::SelectionAmbiguous,
        "the selection query matched more than one document symbol",
        BTreeMap::from([
            ("candidate_count".to_string(), json!(2)),
            (
                "candidates".to_string(),
                json!([
                    {
                        "name": "run",
                        "symbol_kind": "method",
                        "symbol_path": ["FirstRunner", "run"],
                        "selector": {"kind": "bytes", "start": 40, "end": 88}
                    },
                    {
                        "name": "run",
                        "symbol_kind": "method",
                        "symbol_path": ["SecondRunner", "run"],
                        "selector": {"kind": "bytes", "start": 120, "end": 171}
                    }
                ]),
            ),
        ]),
    );
    let timeout = SelectionErrorDto::new(
        SelectionErrorCode::LspTimeout,
        "the language server did not answer before the document-symbol deadline",
        BTreeMap::from([
            ("server_configuration_id".to_string(), json!("rust")),
            ("phase".to_string(), json!("document_symbol")),
            ("elapsed_ms".to_string(), json!(30_000)),
            ("limit_ms".to_string(), json!(30_000)),
        ]),
    );

    assert_eq!(
        serde_json::to_value(not_found).expect("error must serialize"),
        golden("error-not-found.json")
    );
    assert_eq!(
        serde_json::to_value(ambiguous).expect("error must serialize"),
        golden("error-ambiguous.json")
    );
    assert_eq!(
        serde_json::to_value(timeout).expect("error must serialize"),
        golden("error-timeout.json")
    );
}

#[test]
fn selection_serialization_preflight_should_enforce_exact_response_boundary() {
    let maximum = usize::try_from(MAX_RESPONSE_BYTES).expect("response limit must fit usize");
    let below_limit = "x".repeat(maximum - 4);
    let at_limit = "x".repeat(maximum - 3);
    let above_limit = "x".repeat(maximum - 2);

    assert_eq!(
        to_selection_json_line(&below_limit)
            .expect("response below the limit must serialize")
            .len(),
        maximum - 1
    );
    assert_eq!(
        to_selection_json_line(&at_limit)
            .expect("exact response limit must serialize")
            .len(),
        maximum
    );
    let error = to_selection_json_line(&above_limit).expect_err("above limit must fail");
    assert_eq!(
        error.report().code(),
        SelectionErrorCode::LspResourceLimitExceeded
    );
    assert_eq!(error.report().context()["actual"], MAX_RESPONSE_BYTES + 1);
}

#[test]
fn selection_error_dto_should_derive_registry_owned_exit_and_retryability() {
    for code in SelectionErrorCode::ALL {
        let report = SelectionErrorDto::new(code, "test", BTreeMap::new());
        assert_eq!(report.exit_code(), code.category().exit_code(), "{code:?}");
        assert_eq!(report.retryable(), code.retryable(), "{code:?}");
    }
}
