//! Protocol-v1 boundary, registry, schema, and golden-vector tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use codesplice_core::{Anchor, Operation, Precondition, Selector};
use codesplice_protocol::{
    CapabilitiesResponse, ErrorCode, MAX_REQUEST_BYTES, ProtocolVersionResponse, RequestLimits,
    WarningCode, WarningDto, parse_request, parse_request_with_limits, to_json_line,
};
use serde_json::{Value, json};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> String {
    fs::read_to_string(
        repository_root()
            .join("tests/golden/protocol-v1")
            .join(name),
    )
    .expect("golden file must be readable")
}

fn operation(kind: &str, source: &str, destination: &str) -> String {
    format!(
        r#"{{"kind":"{kind}","source":{{"path":"{source}","selector":{{"kind":"bytes","start":0,"end":1}},"precondition":{{"kind":"sha256","value":"{DIGEST}"}}}},"destination":{{"path":"{destination}","anchor":{{"kind":"file_start"}},"precondition":{{"kind":"must_not_exist"}}}}}}"#
    )
}

fn request(operations: &[String]) -> String {
    format!(
        r#"{{"protocol_version":1,"operations":[{}]}}"#,
        operations.join(",")
    )
}

#[test]
fn all_request_variants_should_match_the_golden_domain_values() {
    let batch = parse_request(golden("request-all-variants.json").as_bytes())
        .expect("all request variants must parse");

    assert_eq!(batch.operations.len(), 5);
    assert!(matches!(batch.operations[0], Operation::Move(_)));
    assert!(matches!(batch.operations[1], Operation::Copy(_)));
    let anchors = batch
        .operations
        .iter()
        .map(|operation| match operation {
            Operation::Move(specification) | Operation::Copy(specification) => {
                specification.destination.anchor
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        anchors,
        vec![
            Anchor::BeforeLine(3),
            Anchor::AfterLine(4),
            Anchor::FileStart,
            Anchor::FileEnd,
            Anchor::ByteOffset(u64::MAX),
        ]
    );
    assert!(matches!(
        &batch.operations[0],
        Operation::Move(specification)
            if matches!(specification.source.selector, Selector::Lines { .. })
                && matches!(specification.destination.precondition, Precondition::Sha256(_))
    ));
    assert!(matches!(
        &batch.operations[1],
        Operation::Copy(specification)
            if matches!(specification.source.selector, Selector::Bytes { .. })
                && specification.destination.precondition == Precondition::MustNotExist
    ));
}

#[test]
fn invalid_protocol_values_should_return_stable_codes() {
    let cases = [
        (
            r#"{"protocol_version":1,"protocol_version":1,"operations":[]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[],"unknown":0}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":2,"operations":[]}"#,
            ErrorCode::UnsupportedProtocolVersion,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"delete"}]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":-1,"end":1},"precondition":{"kind":"sha256","value":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":0.5,"end":1},"precondition":{"kind":"sha256","value":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":0,"end":18446744073709551616},"precondition":{"kind":"sha256","value":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":0,"end":1}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}]}"#,
            ErrorCode::InvalidRequest,
        ),
        (
            r#"{"protocol_version":1,"operations":[{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":0,"end":1},"precondition":{"kind":"sha256","value":"SHA256:BAD"}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}]}"#,
            ErrorCode::InvalidDigest,
        ),
        ("{", ErrorCode::InvalidJson),
    ];

    for (input, expected) in cases {
        let error = parse_request(input.as_bytes()).expect_err("case must be rejected");
        assert_eq!(error.report().code(), expected, "input: {input}");
    }
}

#[test]
fn release_request_size_and_depth_limits_should_reject_above_boundary() {
    let oversized = vec![b' '; usize::try_from(MAX_REQUEST_BYTES + 1).expect("limit fits usize")];
    let size_error = parse_request(&oversized).expect_err("oversized request must fail");
    assert_eq!(size_error.report().code(), ErrorCode::ResourceLimitExceeded);

    let too_deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let depth_error = parse_request(too_deep.as_bytes()).expect_err("deep request must fail");
    assert_eq!(
        depth_error.report().code(),
        ErrorCode::ResourceLimitExceeded
    );
}

#[test]
fn request_resource_boundaries_should_fail_at_the_first_exceeded_limit() {
    let two_operations = request(&[operation("copy", "a", "b"), operation("move", "c", "d")]);
    let exact_limits = RequestLimits::new(two_operations.len() as u64, 8, 2, 4, 1);
    parse_request_with_limits(two_operations.as_bytes(), exact_limits)
        .expect("at-limit request must parse");

    let limit_cases = [
        RequestLimits::new(two_operations.len() as u64 - 1, 8, 2, 4, 1),
        RequestLimits::new(two_operations.len() as u64, 8, 1, 4, 1),
        RequestLimits::new(two_operations.len() as u64, 8, 2, 3, 1),
        RequestLimits::new(two_operations.len() as u64, 8, 2, 4, 0),
    ];
    for limits in limit_cases {
        let error = parse_request_with_limits(two_operations.as_bytes(), limits)
            .expect_err("above-limit request must fail");
        assert_eq!(error.report().code(), ErrorCode::ResourceLimitExceeded);
    }

    let depth_error = parse_request_with_limits(b"[[[]]]", RequestLimits::new(100, 2, 10, 10, 10))
        .expect_err("excess depth must fail before conversion");
    assert_eq!(
        depth_error.report().code(),
        ErrorCode::ResourceLimitExceeded
    );
}

#[test]
fn all_registered_errors_should_match_the_golden_registry() {
    let actual = ErrorCode::ALL
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
    let expected: Value = serde_json::from_str(&golden("error-registry.json"))
        .expect("error registry golden must be JSON");

    assert_eq!(json!(actual), expected);
}

#[test]
fn all_registered_warnings_should_match_the_golden_registry() {
    let actual = WarningCode::ALL
        .into_iter()
        .map(|code| WarningDto::new(code, format!("golden {}", code.as_str()), BTreeMap::new()))
        .collect::<Vec<_>>();
    let expected: Value = serde_json::from_str(&golden("warning-registry.json"))
        .expect("warning registry golden must be JSON");

    assert_eq!(json!(actual), expected);
}

#[test]
fn implemented_query_responses_should_match_golden_json_lines() {
    assert_eq!(
        to_json_line(&CapabilitiesResponse::phase_five()).expect("capabilities must serialize"),
        golden("capabilities.json")
    );
    assert_eq!(
        to_json_line(&ProtocolVersionResponse::current()).expect("protocol version must serialize"),
        golden("protocol-version.json")
    );
}

#[test]
fn normative_schemas_should_be_valid_json_and_cover_the_registries() {
    let request_schema: Value = serde_json::from_str(
        &fs::read_to_string(repository_root().join("docs/schema/v1/request.schema.json"))
            .expect("request schema must be readable"),
    )
    .expect("request schema must be JSON");
    let response_schema: Value = serde_json::from_str(
        &fs::read_to_string(repository_root().join("docs/schema/v1/response.schema.json"))
            .expect("response schema must be readable"),
    )
    .expect("response schema must be JSON");

    assert_eq!(request_schema["properties"]["protocol_version"]["const"], 1);
    let schema_codes = response_schema["$defs"]["error"]["properties"]["code"]["enum"]
        .as_array()
        .expect("error code enum must be an array");
    assert_eq!(json!(schema_codes), json!(ErrorCode::ALL));
    let schema_warnings = response_schema["$defs"]["warning"]["properties"]["code"]["enum"]
        .as_array()
        .expect("warning code enum must be an array");
    assert_eq!(json!(schema_warnings), json!(WarningCode::ALL));
}
