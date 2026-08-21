//! Pure construction and validation of LSP initialization capabilities.

use std::fmt;

use gen_lsp_types::{
    ClientCapabilities, ClientInfo, DocumentSymbolClientCapabilities, DocumentSymbolProvider,
    GeneralClientCapabilities, InitializeParams, InitializeResult, PositionEncodingKind,
    TextDocumentClientCapabilities, TextDocumentSync, TextDocumentSyncClientCapabilities,
    TextDocumentSyncKind, Uri, WindowClientCapabilities, WorkDoneProgressParams,
    WorkspaceClientCapabilities, WorkspaceFolder, WorkspaceFolders,
    WorkspaceFoldersInitializeParams,
};

/// The client name reported to language servers.
pub const CLIENT_NAME: &str = "codesplice";

/// A position encoding supported by CodeSplice's byte-range conversion layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportedPositionEncoding {
    /// LSP character offsets count UTF-8 code units.
    Utf8,
    /// LSP character offsets count UTF-16 code units.
    Utf16,
    /// LSP character offsets count UTF-32 code units.
    Utf32,
}

/// Nullable language-server identity reported during initialization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerIdentity {
    /// Server name, or `None` when the server omitted `serverInfo`.
    pub name: Option<String>,
    /// Server version, or `None` when it was omitted.
    pub version: Option<String>,
}

/// Capabilities accepted for a semantic-selection session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedCapabilities {
    /// Optional identity reported by the server.
    pub server: ServerIdentity,
    /// Position encoding selected by the server.
    pub position_encoding: SupportedPositionEncoding,
}

/// A non-sensitive reason an initialize result cannot support selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityError {
    /// The server did not advertise static document-symbol support.
    DocumentSymbolsUnavailable,
    /// The server did not advertise usable open/close document synchronization.
    DocumentSyncUnavailable,
    /// The server selected a position encoding CodeSplice did not offer.
    UnsupportedPositionEncoding,
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DocumentSymbolsUnavailable => {
                "language server does not provide static document symbols"
            }
            Self::DocumentSyncUnavailable => {
                "language server does not provide usable open/close document synchronization"
            }
            Self::UnsupportedPositionEncoding => {
                "language server selected an unsupported position encoding"
            }
        })
    }
}

impl std::error::Error for CapabilityError {}

/// Builds the fixed client capabilities advertised by semantic selection.
///
/// CodeSplice deliberately advertises only static hierarchical document
/// symbols and the three encodings its conversion layer understands. It does
/// not allow the server to apply edits or create progress work.
#[must_use]
pub fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            apply_edit: Some(false),
            workspace_folders: Some(true),
            configuration: Some(true),
            ..WorkspaceClientCapabilities::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: Some(false),
                ..TextDocumentSyncClientCapabilities::default()
            }),
            document_symbol: Some(DocumentSymbolClientCapabilities {
                dynamic_registration: Some(false),
                hierarchical_document_symbol_support: Some(true),
                ..DocumentSymbolClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(false),
            ..WindowClientCapabilities::default()
        }),
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF16,
                PositionEncodingKind::UTF32,
            ]),
            ..GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}

/// Builds the fixed initialize parameters for one canonical workspace root.
///
/// The same URI is used as the legacy `rootUri` and as the only workspace
/// folder so servers using either initialization convention see one root.
#[allow(deprecated)]
#[must_use]
pub fn initialize_params(
    process_id: Option<i32>,
    client_version: Option<String>,
    root_uri: Uri,
    workspace_name: String,
) -> InitializeParams {
    InitializeParams {
        process_id,
        client_info: Some(ClientInfo::new(CLIENT_NAME.to_owned(), client_version)),
        locale: None,
        root_path: None,
        root_uri: Some(root_uri.clone()),
        capabilities: client_capabilities(),
        initialization_options: None,
        trace: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
        workspace_folders_initialize_params: WorkspaceFoldersInitializeParams::new(Some(
            WorkspaceFolders::WorkspaceFolderList(vec![WorkspaceFolder::new(
                root_uri,
                workspace_name,
            )]),
        )),
    }
}

/// Validates the server capabilities required before a document is opened.
///
/// An omitted position encoding selects the LSP-mandated UTF-16 default.
/// Legacy numeric full and incremental synchronization are accepted;
/// synchronization options must explicitly enable open/close and select one
/// of those usable change modes. The immutable snapshot is still sent in full
/// by `didOpen`, and semantic selection sends no later `didChange`.
///
/// # Errors
///
/// Returns a typed error when document symbols, synchronization, or the
/// selected position encoding are unsupported.
pub fn validate_initialize_result(
    result: &InitializeResult,
) -> Result<NegotiatedCapabilities, CapabilityError> {
    match result.capabilities.document_symbol_provider.as_ref() {
        Some(
            DocumentSymbolProvider::Bool(true) | DocumentSymbolProvider::DocumentSymbolOptions(_),
        ) => {}
        Some(DocumentSymbolProvider::Bool(false)) | None => {
            return Err(CapabilityError::DocumentSymbolsUnavailable);
        }
    }

    match result.capabilities.text_document_sync {
        Some(TextDocumentSync::Kind(
            TextDocumentSyncKind::Full | TextDocumentSyncKind::Incremental,
        )) => {}
        Some(TextDocumentSync::Options(options))
            if options.open_close == Some(true)
                && matches!(
                    options.change,
                    Some(TextDocumentSyncKind::Full | TextDocumentSyncKind::Incremental)
                ) => {}
        Some(TextDocumentSync::Kind(_)) | Some(TextDocumentSync::Options(_)) | None => {
            return Err(CapabilityError::DocumentSyncUnavailable);
        }
    }

    let position_encoding = match result.capabilities.position_encoding.as_ref() {
        None | Some(PositionEncodingKind::UTF16) => SupportedPositionEncoding::Utf16,
        Some(PositionEncodingKind::UTF8) => SupportedPositionEncoding::Utf8,
        Some(PositionEncodingKind::UTF32) => SupportedPositionEncoding::Utf32,
        Some(PositionEncodingKind::Custom(_)) => {
            return Err(CapabilityError::UnsupportedPositionEncoding);
        }
    };

    let server = result
        .server_info
        .as_ref()
        .map_or_else(ServerIdentity::default, |server_info| ServerIdentity {
            name: Some(server_info.name.clone()),
            version: server_info.version.clone(),
        });

    Ok(NegotiatedCapabilities {
        server,
        position_encoding,
    })
}
