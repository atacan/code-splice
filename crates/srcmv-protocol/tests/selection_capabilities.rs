//! Golden and boundedness tests for static semantic-selection discovery.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use srcmv_protocol::{MAX_RESPONSE_BYTES, SelectionCapabilitiesResponse, to_json_line};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden() -> String {
    fs::read_to_string(
        repository_root().join("tests/golden/selection-capabilities-v1/capabilities.json"),
    )
    .expect("selection-capabilities golden must be readable")
}

#[test]
fn current_selection_capabilities_should_match_exact_golden_json_line() {
    let actual = to_json_line(&SelectionCapabilitiesResponse::current())
        .expect("static selection capabilities must serialize");

    assert_eq!(actual, golden());
}

#[test]
fn selection_capabilities_should_report_static_server_semantics() {
    let value: Value = serde_json::from_str(&golden()).expect("golden must contain JSON");

    assert_eq!(value["language_server"]["bundled"], false);
    assert_eq!(
        value["language_server"]["availability"],
        "runtime_dependent"
    );
    assert!(value["language_server"].get("installed").is_none());
}

#[test]
fn selection_capabilities_should_have_a_constant_small_response_bound() {
    let line = to_json_line(&SelectionCapabilitiesResponse::current())
        .expect("static selection capabilities must serialize");

    assert!(line.len() < 1_024);
    assert!((line.len() as u64) <= MAX_RESPONSE_BYTES);
}
