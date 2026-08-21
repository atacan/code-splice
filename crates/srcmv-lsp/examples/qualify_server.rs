//! Optional real-server lifecycle qualification and phase timing utility.

use std::error::Error;
use std::path::PathBuf;

use gen_lsp_types::WorkspaceFolder;
use serde_json::json;
use srcmv_core::LineIndex;
use srcmv_lsp::position::{PositionConverter, PositionLimits};
use srcmv_lsp::process::ProcessSpec;
use srcmv_lsp::session::{
    ImmutableDocument, SessionDeadlines, SessionInput, SessionLimits, run_session,
};
use srcmv_lsp::symbols::{
    KnownSymbolKind, MatchMode, SelectionExtent, SymbolLimits, normalize_document_symbols,
    resolve_name,
};
use srcmv_lsp::transport::TransportLimits;
use url::Url;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let program = arguments.next().ok_or("missing PROGRAM")?;
    let language_id = arguments
        .next()
        .ok_or("missing LANGUAGE_ID")?
        .into_string()
        .map_err(|_| "LANGUAGE_ID must be UTF-8")?;
    let workspace = PathBuf::from(arguments.next().ok_or("missing WORKSPACE")?).canonicalize()?;
    let source = PathBuf::from(arguments.next().ok_or("missing SOURCE")?);
    let source_path = workspace.join(source).canonicalize()?;
    if !source_path.starts_with(&workspace) {
        return Err("SOURCE must remain within WORKSPACE".into());
    }
    let text = std::fs::read_to_string(&source_path)?;
    let workspace_uri = Url::from_directory_path(&workspace).map_err(|()| "invalid WORKSPACE")?;
    let source_uri = Url::from_file_path(&source_path).map_err(|()| "invalid SOURCE")?;
    let process = ProcessSpec::new(program)
        .args(arguments)
        .current_directory(&workspace);

    let output = run_session(
        SessionInput {
            process,
            workspace: WorkspaceFolder::new(workspace_uri, "qualification".to_owned()),
            document: ImmutableDocument {
                uri: source_uri,
                language_id,
                text: text.clone(),
            },
            initialization_options: None,
            settings: None,
            deadlines: SessionDeadlines::default(),
            limits: SessionLimits::default(),
        },
        TransportLimits::default(),
    )?;

    let symbol_result_bytes = serde_json::to_vec(&output.symbols)?.len();
    let line_index = LineIndex::from_bytes_with_limits(text.as_bytes(), u64::MAX, u64::MAX)?;
    let mut converter = PositionConverter::new(
        &text,
        &line_index,
        output.capabilities.position_encoding,
        PositionLimits::default(),
    )?;
    let symbols =
        normalize_document_symbols(output.symbols, &mut converter, SymbolLimits::default())?;
    let matched = resolve_name(
        &symbols,
        &text,
        "add",
        Some(KnownSymbolKind::Function),
        SelectionExtent::Symbol,
        MatchMode::Unique,
        SymbolLimits::default(),
    )?;
    let selected = matched
        .first()
        .ok_or("qualification did not resolve `add`")?;

    println!(
        "{}",
        serde_json::to_string(&json!({
            "initialize_ms": milliseconds(output.timings.initialize),
            "document_symbols_ms": milliseconds(output.timings.document_symbols),
            "shutdown_ms": milliseconds(output.timings.shutdown),
            "symbol_result_bytes": symbol_result_bytes,
            "selected_start": selected.selected_range.start,
            "selected_end": selected.selected_range.end,
        }))?
    );
    Ok(())
}

fn milliseconds(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
