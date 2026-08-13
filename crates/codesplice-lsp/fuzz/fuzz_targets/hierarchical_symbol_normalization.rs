#![no_main]

use codesplice_core::LineIndex;
use codesplice_lsp::capabilities::SupportedPositionEncoding;
use codesplice_lsp::position::{PositionConverter, PositionLimits};
use codesplice_lsp::symbols::{SymbolLimits, normalize_document_symbols};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

const SNAPSHOT: &str = "pub mod café {\r\n\tpub fn rocket🙂() {}\r}\n";

fuzz_target!(|data: &[u8]| {
    let Ok(response) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let Ok(index) = LineIndex::from_bytes_with_limits(SNAPSHOT.as_bytes(), u64::MAX, u64::MAX)
    else {
        return;
    };
    let encoding = match data.len() % 3 {
        0 => SupportedPositionEncoding::Utf8,
        1 => SupportedPositionEncoding::Utf16,
        _ => SupportedPositionEncoding::Utf32,
    };
    let Ok(mut converter) = PositionConverter::new(
        SNAPSHOT,
        &index,
        encoding,
        PositionLimits {
            maximum_code_points_scanned: 16_384,
        },
    ) else {
        return;
    };
    let limits = SymbolLimits {
        maximum_raw_symbols: 256,
        maximum_flattened_symbols: 256,
        maximum_depth: 64,
        maximum_name_bytes: 4_096,
        maximum_detail_bytes: 16_384,
        maximum_path_bytes: 32_768,
        maximum_candidate_storage_bytes: 1_048_576,
        maximum_matches: 256,
        maximum_ambiguity_candidates: 50,
    };

    let _ = normalize_document_symbols(response, &mut converter, limits);
});
