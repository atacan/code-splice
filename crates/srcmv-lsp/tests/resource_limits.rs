//! Focused lowerable-boundary coverage for semantic-selection resource limits.

use std::time::Duration;

use gen_lsp_types::{DocumentSymbol, Position, Range, SymbolKind, Uri, WorkspaceFolder};
use serde_json::{Value, json};
use srcmv_core::LineIndex;
use srcmv_lsp::capabilities::SupportedPositionEncoding;
use srcmv_lsp::jsonrpc::{
    EnvelopeLimits, IncomingMessage, JsonRpcError, ResponseCorrelator, classify_message,
};
use srcmv_lsp::position::{PositionConverter, PositionLimits};
use srcmv_lsp::process::{ProcessError, ProcessSpec};
use srcmv_lsp::session::{
    ImmutableDocument, SessionDeadlines, SessionError, SessionInput, SessionLimits, run_session,
};
use srcmv_lsp::symbols::{
    MatchMode, SelectionExtent, SymbolError, SymbolLimits, normalize_hierarchical_symbols,
    resolve_name, resolve_position,
};
use srcmv_lsp::transport::{TransportError, TransportLimits};

fn envelope(method_limit: usize, id_limit: usize, params_limit: usize) -> EnvelopeLimits {
    EnvelopeLimits {
        max_json_depth: 64,
        max_id_bytes: id_limit,
        max_method_bytes: method_limit,
        max_params_bytes: params_limit,
    }
}

#[test]
fn envelope_method_bytes_accept_below_and_at_then_reject_above() {
    for method in ["abc", "abcd"] {
        let result = classify_message(
            json!({"jsonrpc":"2.0", "method":method}),
            envelope(4, 64, 64),
        );
        assert!(
            matches!(result, Ok(IncomingMessage::Notification(_))),
            "method of {} bytes should pass: {result:?}",
            method.len()
        );
    }

    let error = classify_message(
        json!({"jsonrpc":"2.0", "method":"abcde"}),
        envelope(4, 64, 64),
    )
    .expect_err("method above the byte limit must fail");
    assert!(matches!(
        error,
        JsonRpcError::MethodTooLong {
            length: 5,
            limit: 4
        }
    ));
}

#[test]
fn envelope_id_bytes_accept_below_and_at_then_reject_above() {
    // String-ID accounting includes the two JSON quote bytes.
    for id in ["x", "xx"] {
        let result = classify_message(
            json!({"jsonrpc":"2.0", "id":id, "method":"m"}),
            envelope(64, 4, 64),
        );
        assert!(
            matches!(result, Ok(IncomingMessage::Request(_))),
            "encoded ID of {} bytes should pass: {result:?}",
            id.len() + 2
        );
    }

    let error = classify_message(
        json!({"jsonrpc":"2.0", "id":"xxx", "method":"m"}),
        envelope(64, 4, 64),
    )
    .expect_err("encoded ID above the byte limit must fail");
    assert!(matches!(
        error,
        JsonRpcError::IdTooLarge {
            length: 5,
            limit: 4
        }
    ));
}

#[test]
fn envelope_params_bytes_accept_below_and_at_then_reject_above() {
    let params = json!({"x":"\n"});
    let exact = serde_json::to_vec(&params)
        .expect("parameters should serialize")
        .len();
    for limit in [exact + 1, exact] {
        let result = classify_message(
            json!({"jsonrpc":"2.0", "method":"m", "params":params}),
            envelope(64, 64, limit),
        );
        assert!(
            matches!(result, Ok(IncomingMessage::Notification(_))),
            "escaped parameters should fit limit {limit}: {result:?}"
        );
    }

    let error = classify_message(
        json!({"jsonrpc":"2.0", "method":"m", "params":params}),
        envelope(64, 64, exact - 1),
    )
    .expect_err("serialized parameters above the byte limit must fail");
    assert!(matches!(
        error,
        JsonRpcError::ParamsTooLarge { length, limit }
            if length == exact && limit == exact - 1
    ));
}

#[test]
fn pending_request_count_accepts_below_and_at_then_rejects_above() {
    let mut correlator = ResponseCorrelator::new(2);
    correlator
        .begin_request()
        .expect("one pending request is below the limit");
    correlator
        .begin_request()
        .expect("two pending requests are at the limit");

    let error = correlator
        .begin_request()
        .expect_err("a third pending request exceeds the limit");
    assert!(matches!(
        error,
        JsonRpcError::PendingRequestLimit { limit: 2 }
    ));
}

fn session_input(
    text: String,
    initialization_options: Option<Value>,
    settings: Option<Value>,
    limits: SessionLimits,
) -> SessionInput {
    let workspace_uri: Uri = "file:///srcmv-limit-workspace"
        .parse()
        .expect("workspace URI should parse");
    let document_uri: Uri = "file:///srcmv-limit-workspace/source.rs"
        .parse()
        .expect("document URI should parse");
    SessionInput {
        process: ProcessSpec::new("/srcmv/definitely-absent-lsp-server"),
        workspace: WorkspaceFolder::new(workspace_uri, "workspace".to_owned()),
        document: ImmutableDocument {
            uri: document_uri,
            language_id: "rust".to_owned(),
            text,
        },
        initialization_options,
        settings,
        deadlines: SessionDeadlines {
            initialize: Duration::from_millis(1),
            document_symbols: Duration::from_millis(1),
            shutdown: Duration::from_millis(1),
            cleanup: Duration::from_millis(1),
            total: Duration::from_millis(10),
        },
        limits,
    }
}

fn assert_payload_preflight_passed(
    result: Result<srcmv_lsp::session::SessionOutput, SessionError>,
) {
    assert!(
        matches!(
            result,
            Err(SessionError::Transport(TransportError::Process(
                ProcessError::Spawn(_)
            )))
        ),
        "an accepted payload should proceed to the deliberately absent executable: {result:?}"
    );
}

#[test]
fn session_source_bytes_accept_below_and_at_then_reject_above_before_spawn() {
    for limit in [4, 3] {
        let limits = SessionLimits {
            source_bytes: limit,
            ..SessionLimits::default()
        };
        assert_payload_preflight_passed(run_session(
            session_input("abc".to_owned(), None, None, limits),
            TransportLimits::default(),
        ));
    }

    let error = run_session(
        session_input(
            "abc".to_owned(),
            None,
            None,
            SessionLimits {
                source_bytes: 2,
                ..SessionLimits::default()
            },
        ),
        TransportLimits::default(),
    )
    .expect_err("source above the limit must fail before spawn");
    assert!(matches!(
        error,
        SessionError::ResourceLimit {
            resource: "source bytes",
            limit: 2
        }
    ));
}

fn assert_session_json_bound(value: Value, initialization_options: bool, resource: &'static str) {
    let exact = serde_json::to_vec(&value)
        .expect("fixture should serialize")
        .len();
    for limit in [exact + 1, exact] {
        let mut limits = SessionLimits::default();
        let (options, settings) = if initialization_options {
            limits.initialization_options_bytes = limit;
            (Some(value.clone()), None)
        } else {
            limits.settings_bytes = limit;
            (None, Some(value.clone()))
        };
        assert_payload_preflight_passed(run_session(
            session_input(String::new(), options, settings, limits),
            TransportLimits::default(),
        ));
    }

    let mut limits = SessionLimits::default();
    let (options, settings) = if initialization_options {
        limits.initialization_options_bytes = exact - 1;
        (Some(value), None)
    } else {
        limits.settings_bytes = exact - 1;
        (None, Some(value))
    };
    let error = run_session(
        session_input(String::new(), options, settings, limits),
        TransportLimits::default(),
    )
    .expect_err("JSON payload above its exact serialized limit must fail");
    assert!(matches!(
        error,
        SessionError::ResourceLimit {
            resource: actual_resource,
            limit
        } if actual_resource == resource && limit == exact - 1
    ));
}

#[test]
fn session_initialization_options_use_exact_serialized_byte_boundaries() {
    assert_session_json_bound(
        json!({"escaped":"\n\""}),
        true,
        "initialization-options bytes",
    );
}

#[test]
fn session_settings_use_exact_serialized_byte_boundaries() {
    assert_session_json_bound(json!({"escaped":"\n\""}), false, "settings bytes");
}

#[test]
fn escaped_did_open_body_is_preflighted_exactly_before_process_spawn() {
    let text = "\\\n\"".to_owned();
    let message = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {
            "uri": "file:///srcmv-limit-workspace/source.rs",
            "languageId": "rust",
            "version": 1,
            "text": text,
        }}
    });
    let exact = serde_json::to_vec(&message)
        .expect("didOpen fixture should serialize")
        .len();

    for limit in [exact + 1, exact] {
        let mut transport_limits = TransportLimits::default();
        transport_limits.framing.max_outbound_body_bytes = limit;
        assert_payload_preflight_passed(run_session(
            session_input(text.clone(), None, None, SessionLimits::default()),
            transport_limits,
        ));
    }

    let mut transport_limits = TransportLimits::default();
    transport_limits.framing.max_outbound_body_bytes = exact - 1;
    let error = run_session(
        session_input(text, None, None, SessionLimits::default()),
        transport_limits,
    )
    .expect_err("didOpen above the exact outbound limit must fail before spawn");
    assert!(matches!(
        error,
        SessionError::Transport(TransportError::Protocol(
            JsonRpcError::OutboundBodyTooLarge { length, limit }
        )) if length == exact && limit == exact - 1
    ));
}

fn lsp_range(start: u32, end: u32) -> Range {
    Range::new(Position::new(0, start), Position::new(0, end))
}

fn document_symbol(
    name: &str,
    detail: Option<&str>,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol::new(
        name.to_owned(),
        detail.map(str::to_owned),
        SymbolKind::Function,
        None,
        None,
        lsp_range(0, 1),
        lsp_range(0, 1),
        children,
    )
}

fn normalize(
    roots: Vec<DocumentSymbol>,
    limits: SymbolLimits,
) -> Result<Vec<srcmv_lsp::symbols::NormalizedSymbol>, SymbolError> {
    normalize_text("x", roots, limits)
}

fn normalize_text(
    text: &str,
    roots: Vec<DocumentSymbol>,
    limits: SymbolLimits,
) -> Result<Vec<srcmv_lsp::symbols::NormalizedSymbol>, SymbolError> {
    let index = LineIndex::from_bytes_with_limits(text.as_bytes(), 10, 256)
        .expect("fixture line index should build");
    let mut converter = PositionConverter::new(
        text,
        &index,
        SupportedPositionEncoding::Utf8,
        PositionLimits::default(),
    )
    .expect("fixture converter should build");
    normalize_hierarchical_symbols(roots, &mut converter, limits)
}

#[test]
fn symbol_name_and_detail_bytes_accept_at_then_reject_above() {
    let at = normalize(
        vec![document_symbol("ab", Some("cd"), None)],
        SymbolLimits {
            maximum_name_bytes: 2,
            maximum_detail_bytes: 2,
            ..SymbolLimits::default()
        },
    );
    assert!(at.is_ok(), "at-limit symbol strings should pass: {at:?}");

    let name_error = normalize(
        vec![document_symbol("ab", None, None)],
        SymbolLimits {
            maximum_name_bytes: 1,
            ..SymbolLimits::default()
        },
    )
    .expect_err("name above its limit must fail");
    let detail_error = normalize(
        vec![document_symbol("a", Some("cd"), None)],
        SymbolLimits {
            maximum_detail_bytes: 1,
            ..SymbolLimits::default()
        },
    )
    .expect_err("detail above its limit must fail");

    assert!(matches!(
        name_error,
        SymbolError::ResourceLimitExceeded {
            resource: "symbol_name_bytes",
            maximum: 1
        }
    ));
    assert!(matches!(
        detail_error,
        SymbolError::ResourceLimitExceeded {
            resource: "symbol_detail_bytes",
            maximum: 1
        }
    ));
}

#[test]
fn symbol_counts_depth_path_and_storage_accept_at_then_reject_above() {
    let at_limits = SymbolLimits {
        maximum_raw_symbols: 1,
        maximum_flattened_symbols: 1,
        maximum_depth: 1,
        maximum_path_bytes: 1,
        maximum_candidate_storage_bytes: 3,
        ..SymbolLimits::default()
    };
    let at = normalize(vec![document_symbol("a", None, None)], at_limits);
    assert!(at.is_ok(), "all exact symbol bounds should pass: {at:?}");

    let cases = [
        (
            SymbolLimits {
                maximum_raw_symbols: 0,
                ..at_limits
            },
            "raw_document_symbols",
        ),
        (
            SymbolLimits {
                maximum_flattened_symbols: 0,
                ..at_limits
            },
            "flattened_document_symbols",
        ),
        (
            SymbolLimits {
                maximum_depth: 0,
                ..at_limits
            },
            "symbol_nesting_depth",
        ),
        (
            SymbolLimits {
                maximum_path_bytes: 0,
                ..at_limits
            },
            "symbol_path_bytes",
        ),
        (
            SymbolLimits {
                maximum_candidate_storage_bytes: 2,
                ..at_limits
            },
            "symbol_candidate_storage_bytes",
        ),
    ];
    for (limits, resource) in cases {
        let error = normalize(vec![document_symbol("a", None, None)], limits)
            .expect_err("one-above symbol bound must fail");
        assert!(
            matches!(
                error,
                SymbolError::ResourceLimitExceeded {
                    resource: actual,
                    ..
                } if actual == resource
            ),
            "unexpected error for {resource}: {error:?}"
        );
    }
}

#[test]
fn successful_match_count_accepts_at_then_rejects_above() {
    let symbols = normalize(
        vec![document_symbol("a", None, None)],
        SymbolLimits::default(),
    )
    .expect("fixture symbol should normalize");
    let at = resolve_position(
        &symbols,
        "x",
        0,
        None,
        SelectionExtent::Symbol,
        MatchMode::All,
        SymbolLimits {
            maximum_matches: 1,
            ..SymbolLimits::default()
        },
    );
    assert!(at.is_ok(), "one exact match should fit: {at:?}");

    let error = resolve_position(
        &symbols,
        "x",
        0,
        None,
        SelectionExtent::Symbol,
        MatchMode::All,
        SymbolLimits {
            maximum_matches: 0,
            ..SymbolLimits::default()
        },
    )
    .expect_err("one match above a zero-match limit must fail");
    assert!(matches!(
        error,
        SymbolError::ResourceLimitExceeded {
            resource: "selection_matches",
            maximum: 0
        }
    ));
}

#[test]
fn ambiguity_candidate_cap_retains_below_at_and_truncates_above() {
    let roots = (0..3)
        .map(|line| {
            DocumentSymbol::new(
                "same".to_owned(),
                None,
                SymbolKind::Function,
                None,
                None,
                Range::new(Position::new(line, 0), Position::new(line, 1)),
                Range::new(Position::new(line, 0), Position::new(line, 1)),
                None,
            )
        })
        .collect();
    let symbols = normalize_text("x\nx\nx", roots, SymbolLimits::default())
        .expect("fixture symbols should normalize");

    for (limit, expected_candidates) in [(4, 3), (3, 3), (2, 2)] {
        let error = resolve_name(
            &symbols,
            "x\nx\nx",
            "same",
            None,
            SelectionExtent::Symbol,
            MatchMode::Unique,
            SymbolLimits {
                maximum_ambiguity_candidates: limit,
                ..SymbolLimits::default()
            },
        )
        .expect_err("three name matches should be ambiguous");
        let SymbolError::Ambiguous { total, candidates } = error else {
            panic!("expected bounded ambiguity error, found {error:?}");
        };
        assert_eq!(total, 3);
        assert_eq!(candidates.len(), expected_candidates);
    }
}
