//! Static discovery metadata for the independently versioned semantic-selection surface.
//!
//! This response describes features implemented by srcmv itself. It does
//! not inspect configuration, search `PATH`, or claim that a compatible
//! language server is installed. Language-server availability is resolved only
//! when a selection command runs, and a launched server is trusted local code.

use serde::Serialize;

use crate::SELECTION_PROTOCOL_VERSION;

/// Successful response for `selection-capabilities --json`.
///
/// Every collection and string is fixed by selection protocol version 1, so
/// serialization has a small constant upper bound independent of the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SelectionCapabilitiesResponse {
    selection_protocol_version: u64,
    features: [&'static str; 3],
    queries: [&'static str; 2],
    extents: [&'static str; 2],
    position_encodings: [&'static str; 3],
    language_server: LanguageServerSemantics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct LanguageServerSemantics {
    bundled: bool,
    availability: &'static str,
    trust: &'static str,
}

impl SelectionCapabilitiesResponse {
    /// Returns the semantic-selection capabilities implemented by this binary.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            selection_protocol_version: SELECTION_PROTOCOL_VERSION,
            features: [
                "document_symbols",
                "all_matches",
                "request_source_composition",
            ],
            queries: ["name", "position"],
            extents: ["symbol", "declaration_lines"],
            position_encodings: ["utf-8", "utf-16", "utf-32"],
            language_server: LanguageServerSemantics {
                bundled: false,
                availability: "runtime_dependent",
                trust: "trusted_local_process",
            },
        }
    }
}
