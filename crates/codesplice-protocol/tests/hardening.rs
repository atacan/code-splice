//! Phase 9 protocol decoder and human-rendering fuzz properties.

use std::fs;
use std::path::{Path, PathBuf};

use codesplice_protocol::{escape_terminal_text, parse_request};
use proptest::prelude::*;
use serde_json::Value;

proptest! {
    #[test]
    fn json_decoder_fuzz_regression_never_panics_or_accepts_non_utf8(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let parsed = parse_request(&bytes);
        if std::str::from_utf8(&bytes).is_err() {
            prop_assert!(parsed.is_err());
        }
    }

    #[test]
    fn human_escaping_fuzz_regression_removes_terminal_and_bidi_controls(value in any::<String>()) {
        let escaped = escape_terminal_text(&value);
        let contains_unsafe_character = escaped.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        });

        prop_assert!(!contains_unsafe_character);
    }
}

#[test]
fn json_decoder_checked_in_fuzz_regressions_remain_rejected() {
    let cases: [&[u8]; 8] = [
        b"{",
        b"[",
        b"\xff\xfe",
        br#"{"protocol_version":1,"protocol_version":1,"operations":[]}"#,
        br#"{"protocol_version":1,"operations":[],"unknown":true}"#,
        br#"{"protocol_version":18446744073709551616,"operations":[]}"#,
        br#"{"protocol_version":1,"operations":[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]"#,
        b"\0",
    ];

    for input in cases {
        assert!(parse_request(input).is_err(), "input={input:?}");
    }
}

#[test]
fn phase9_json_artifacts_are_valid_and_cover_every_fuzz_surface() {
    let baseline: Value = serde_json::from_str(
        &fs::read_to_string(repository_root().join("docs/performance-baseline.json"))
            .expect("performance baseline should be readable"),
    )
    .expect("performance baseline should be JSON");
    let seeds: Value = serde_json::from_str(
        &fs::read_to_string(repository_root().join("tests/fuzz-regressions/seeds.json"))
            .expect("fuzz seed manifest should be readable"),
    )
    .expect("fuzz seed manifest should be JSON");

    assert_eq!(baseline["thresholds"], Value::Null);
    let surfaces = seeds["seeds"]
        .as_array()
        .expect("seed list should be an array")
        .iter()
        .filter_map(|seed| seed["surface"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "json",
        "line_index",
        "deterministic_cbor",
        "manifest_record",
        "state_record",
        "human_escape",
    ] {
        assert!(surfaces.contains(&required), "missing surface {required}");
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("protocol crate should be nested beneath the repository")
        .to_path_buf()
}
