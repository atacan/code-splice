#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Strict JSON protocol boundary for CodeSplice protocol version 1.
//!
//! This crate validates request envelopes and converts them to the pure domain
//! model without accessing the filesystem. It also owns the stable error and
//! warning registry shared by the command-line interface and later reports.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use codesplice_core::{
    Anchor, BatchSpecification, Destination, Operation, OperationSpecification, Precondition,
    Selector, Sha256Digest, SourceSelection, WorkspaceRelativePath,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};

/// The only request protocol version supported by this release.
pub const PROTOCOL_VERSION: u64 = 1;
/// The deterministic plan-hash format supported by this release.
pub const PLAN_HASH_VERSION: u64 = 1;
/// Maximum accepted JSON request size in bytes.
pub const MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum accepted JSON array/object nesting depth.
pub const MAX_JSON_DEPTH: u64 = 64;
/// Maximum number of operations in one request.
pub const MAX_OPERATIONS: u64 = 1_000;
/// Maximum number of distinct operation path spellings in one request.
pub const MAX_OPERATION_PATHS: u64 = 1_000;
/// Maximum encoded UTF-8 bytes in one operation path.
pub const MAX_PATH_BYTES: u64 = 4_096;

/// Resource limits enforced while decoding a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestLimits {
    request_bytes: u64,
    json_depth: u64,
    operations: u64,
    operation_paths: u64,
    path_bytes: u64,
}

impl RequestLimits {
    /// Creates a request-limit set, primarily for boundary tests and trusted
    /// configurations that lower the release defaults.
    #[must_use]
    pub const fn new(
        request_bytes: u64,
        json_depth: u64,
        operations: u64,
        operation_paths: u64,
        path_bytes: u64,
    ) -> Self {
        Self {
            request_bytes,
            json_depth,
            operations,
            operation_paths,
            path_bytes,
        }
    }
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self::new(
            MAX_REQUEST_BYTES,
            MAX_JSON_DEPTH,
            MAX_OPERATIONS,
            MAX_OPERATION_PATHS,
            MAX_PATH_BYTES,
        )
    }
}

/// Stable error categories and their process exit status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    /// Malformed command-line or protocol input.
    Request,
    /// A valid request conflicts with observed or expected state.
    Conflict,
    /// A configured bound or platform capability prevents the operation.
    LimitOrSupport,
    /// Transaction coordination or recovery state prevents the operation.
    Transaction,
    /// Persistent control data is invalid.
    Corruption,
    /// An I/O or implementation failure occurred.
    Internal,
}

impl ErrorCategory {
    /// Returns the documented command-line exit status for this category.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Request => 2,
            Self::Conflict => 3,
            Self::LimitOrSupport => 4,
            Self::Transaction => 5,
            Self::Corruption => 6,
            Self::Internal => 8,
        }
    }

    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Conflict => "conflict",
            Self::LimitOrSupport => "limit_or_support",
            Self::Transaction => "transaction",
            Self::Corruption => "corruption",
            Self::Internal => "internal",
        }
    }
}

impl Serialize for ErrorCategory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// Complete protocol-v1 error-code registry.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ErrorCode {
    /// Invalid command-line grammar.
    InvalidCli,
    /// Malformed JSON syntax.
    InvalidJson,
    /// A syntactically valid request uses an unsupported protocol version.
    UnsupportedProtocolVersion,
    /// A request value violates the protocol contract.
    InvalidRequest,
    /// A digest is not the exact lowercase protocol spelling.
    InvalidDigest,
    /// A path precondition does not match.
    PreconditionFailed,
    /// A file changed during a stability-sensitive operation.
    FileChanged,
    /// Distinct paths identify the same existing file.
    FileAlias,
    /// Commit intent omitted both expected-plan policies.
    ExpectedPlanRequired,
    /// The supplied plan digest does not match.
    ExpectedPlanMismatch,
    /// The plan changed between the unlocked and locked pass.
    PlanChangedDuringCommit,
    /// Requested edits cannot be composed.
    EditConflict,
    /// Recovery observations conflict with the journal.
    RecoveryConflict,
    /// A configured resource bound was exceeded.
    ResourceLimitExceeded,
    /// The running platform is unsupported.
    UnsupportedPlatform,
    /// The workspace filesystem is unsupported.
    UnsupportedFilesystem,
    /// A referenced object is not a supported regular file.
    UnsupportedFileType,
    /// A referenced path traverses a symbolic link.
    SymlinkNotAllowed,
    /// A changed target has multiple hard links.
    HardLinkNotSupported,
    /// Transaction paths span filesystem devices.
    CrossDeviceTransaction,
    /// The required no-replace rename primitive is unavailable.
    NoReplaceUnavailable,
    /// Another process holds the mutation lock.
    TransactionBusy,
    /// An unfinished transaction requires explicit recovery.
    TransactionRecoveryRequired,
    /// A requested transaction identifier does not exist.
    TransactionNotFound,
    /// A recovery action is invalid for the recorded state.
    RecoveryActionNotAllowed,
    /// The control directory violates its invariants.
    ControlDirectoryInvalid,
    /// A transaction record is corrupt.
    TransactionRecordCorrupt,
    /// An operating-system I/O operation failed.
    IoError,
    /// An internal invariant or development-only route was reached.
    InternalError,
}

impl ErrorCode {
    /// Every error code registered by protocol version 1.
    pub const ALL: [Self; 29] = [
        Self::InvalidCli,
        Self::InvalidJson,
        Self::UnsupportedProtocolVersion,
        Self::InvalidRequest,
        Self::InvalidDigest,
        Self::PreconditionFailed,
        Self::FileChanged,
        Self::FileAlias,
        Self::ExpectedPlanRequired,
        Self::ExpectedPlanMismatch,
        Self::PlanChangedDuringCommit,
        Self::EditConflict,
        Self::RecoveryConflict,
        Self::ResourceLimitExceeded,
        Self::UnsupportedPlatform,
        Self::UnsupportedFilesystem,
        Self::UnsupportedFileType,
        Self::SymlinkNotAllowed,
        Self::HardLinkNotSupported,
        Self::CrossDeviceTransaction,
        Self::NoReplaceUnavailable,
        Self::TransactionBusy,
        Self::TransactionRecoveryRequired,
        Self::TransactionNotFound,
        Self::RecoveryActionNotAllowed,
        Self::ControlDirectoryInvalid,
        Self::TransactionRecordCorrupt,
        Self::IoError,
        Self::InternalError,
    ];

    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCli => "INVALID_CLI",
            Self::InvalidJson => "INVALID_JSON",
            Self::UnsupportedProtocolVersion => "UNSUPPORTED_PROTOCOL_VERSION",
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::InvalidDigest => "INVALID_DIGEST",
            Self::PreconditionFailed => "PRECONDITION_FAILED",
            Self::FileChanged => "FILE_CHANGED",
            Self::FileAlias => "FILE_ALIAS",
            Self::ExpectedPlanRequired => "EXPECTED_PLAN_REQUIRED",
            Self::ExpectedPlanMismatch => "EXPECTED_PLAN_MISMATCH",
            Self::PlanChangedDuringCommit => "PLAN_CHANGED_DURING_COMMIT",
            Self::EditConflict => "EDIT_CONFLICT",
            Self::RecoveryConflict => "RECOVERY_CONFLICT",
            Self::ResourceLimitExceeded => "RESOURCE_LIMIT_EXCEEDED",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            Self::UnsupportedFilesystem => "UNSUPPORTED_FILESYSTEM",
            Self::UnsupportedFileType => "UNSUPPORTED_FILE_TYPE",
            Self::SymlinkNotAllowed => "SYMLINK_NOT_ALLOWED",
            Self::HardLinkNotSupported => "HARD_LINK_NOT_SUPPORTED",
            Self::CrossDeviceTransaction => "CROSS_DEVICE_TRANSACTION",
            Self::NoReplaceUnavailable => "NO_REPLACE_UNAVAILABLE",
            Self::TransactionBusy => "TRANSACTION_BUSY",
            Self::TransactionRecoveryRequired => "TRANSACTION_RECOVERY_REQUIRED",
            Self::TransactionNotFound => "TRANSACTION_NOT_FOUND",
            Self::RecoveryActionNotAllowed => "RECOVERY_ACTION_NOT_ALLOWED",
            Self::ControlDirectoryInvalid => "CONTROL_DIRECTORY_INVALID",
            Self::TransactionRecordCorrupt => "TRANSACTION_RECORD_CORRUPT",
            Self::IoError => "IO_ERROR",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Returns the documented category for this code.
    #[must_use]
    pub const fn category(self) -> ErrorCategory {
        match self {
            Self::InvalidCli
            | Self::InvalidJson
            | Self::UnsupportedProtocolVersion
            | Self::InvalidRequest
            | Self::InvalidDigest => ErrorCategory::Request,
            Self::PreconditionFailed
            | Self::FileChanged
            | Self::FileAlias
            | Self::ExpectedPlanRequired
            | Self::ExpectedPlanMismatch
            | Self::PlanChangedDuringCommit
            | Self::EditConflict
            | Self::RecoveryConflict => ErrorCategory::Conflict,
            Self::ResourceLimitExceeded
            | Self::UnsupportedPlatform
            | Self::UnsupportedFilesystem
            | Self::UnsupportedFileType
            | Self::SymlinkNotAllowed
            | Self::HardLinkNotSupported
            | Self::CrossDeviceTransaction
            | Self::NoReplaceUnavailable => ErrorCategory::LimitOrSupport,
            Self::TransactionBusy
            | Self::TransactionRecoveryRequired
            | Self::TransactionNotFound
            | Self::RecoveryActionNotAllowed => ErrorCategory::Transaction,
            Self::ControlDirectoryInvalid | Self::TransactionRecordCorrupt => {
                ErrorCategory::Corruption
            }
            Self::IoError | Self::InternalError => ErrorCategory::Internal,
        }
    }

    /// Returns whether retrying after an external-state refresh can reasonably
    /// resolve this error.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::PreconditionFailed
                | Self::FileChanged
                | Self::ExpectedPlanMismatch
                | Self::PlanChangedDuringCommit
                | Self::TransactionBusy
        )
    }
}

impl Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// A stable structured protocol error response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorDto {
    code: ErrorCode,
    category: ErrorCategory,
    retryable: bool,
    message: String,
    context: BTreeMap<String, Value>,
}

impl ErrorDto {
    /// Creates an error response using the registry-owned category and retry policy.
    #[must_use]
    pub fn new(
        code: ErrorCode,
        message: impl Into<String>,
        context: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            code,
            category: code.category(),
            retryable: code.retryable(),
            message: message.into(),
            context,
        }
    }

    /// Returns the stable code.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the process exit status associated with this error.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        self.category.exit_code()
    }

    /// Returns the human-readable message before terminal escaping.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns structured error context.
    #[must_use]
    pub fn context(&self) -> &BTreeMap<String, Value> {
        &self.context
    }
}

/// Typed protocol parsing or conversion failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError {
    report: ErrorDto,
}

impl ProtocolError {
    /// Creates a protocol failure from a stable response.
    #[must_use]
    pub fn new(report: ErrorDto) -> Self {
        Self { report }
    }

    /// Returns the stable error response.
    #[must_use]
    pub fn report(&self) -> &ErrorDto {
        &self.report
    }

    /// Consumes the failure and returns its stable error response.
    #[must_use]
    pub fn into_report(self) -> ErrorDto {
        self.report
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}",
            self.report.code.as_str(),
            self.report.message
        )
    }
}

impl Error for ProtocolError {}

/// Stable protocol-v1 warning identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WarningCode {
    /// A read-only observation was made without a pre-existing shared lock.
    ObservationMayBeStale,
    /// Metadata outside the content and permission contract is not preserved.
    MetadataNotPreserved,
    /// A bounded diff omitted some requested detail.
    DiffTruncated,
}

impl WarningCode {
    /// Every warning registered by protocol version 1.
    pub const ALL: [Self; 3] = [
        Self::ObservationMayBeStale,
        Self::MetadataNotPreserved,
        Self::DiffTruncated,
    ];

    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObservationMayBeStale => "OBSERVATION_MAY_BE_STALE",
            Self::MetadataNotPreserved => "METADATA_NOT_PRESERVED",
            Self::DiffTruncated => "DIFF_TRUNCATED",
        }
    }
}

impl Serialize for WarningCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// A structured warning emitted by successful reports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WarningDto {
    code: WarningCode,
    message: String,
    context: BTreeMap<String, Value>,
}

impl WarningDto {
    /// Creates a warning response.
    #[must_use]
    pub fn new(
        code: WarningCode,
        message: impl Into<String>,
        context: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            context,
        }
    }
}

/// Successful response for `protocol-version --json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolVersionResponse {
    protocol_version: u64,
    supported_protocol_versions: [u64; 1],
    plan_hash_versions: [u64; 1],
}

impl ProtocolVersionResponse {
    /// Returns the protocol versions implemented by this binary.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            supported_protocol_versions: [PROTOCOL_VERSION],
            plan_hash_versions: [PLAN_HASH_VERSION],
        }
    }
}

/// Development-phase capability availability reported by `capabilities --json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FeatureAvailability {
    request_parsing: bool,
    workspace_inspection: bool,
    preview: bool,
    commit: bool,
    recovery: bool,
}

/// Successful response for `capabilities --json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CapabilitiesResponse {
    protocol_version: u64,
    implementation_phase: u64,
    operations: [&'static str; 2],
    selectors: [&'static str; 2],
    anchors: [&'static str; 5],
    preconditions: [&'static str; 2],
    features: FeatureAvailability,
}

impl CapabilitiesResponse {
    /// Returns the capabilities truthfully available at the Phase 2 checkpoint.
    #[must_use]
    pub const fn phase_two() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            implementation_phase: 2,
            operations: ["move", "copy"],
            selectors: ["lines", "bytes"],
            anchors: [
                "file_start",
                "file_end",
                "before_line",
                "after_line",
                "byte_offset",
            ],
            preconditions: ["sha256", "must_not_exist"],
            features: FeatureAvailability {
                request_parsing: true,
                workspace_inspection: false,
                preview: false,
                commit: false,
                recovery: false,
            },
        }
    }
}

/// Parses and validates one protocol-v1 request using release resource limits.
///
/// # Errors
///
/// Returns a stable request, limit, or digest error when the JSON envelope or a
/// converted domain value violates protocol version 1.
pub fn parse_request(input: &[u8]) -> Result<BatchSpecification, ProtocolError> {
    parse_request_with_limits(input, RequestLimits::default())
}

/// Parses and validates one protocol-v1 request using the supplied lowerable limits.
///
/// # Errors
///
/// Returns a stable request, limit, or digest error when the JSON envelope or a
/// converted domain value violates protocol version 1.
pub fn parse_request_with_limits(
    input: &[u8],
    limits: RequestLimits,
) -> Result<BatchSpecification, ProtocolError> {
    enforce_request_size(input, limits.request_bytes)?;
    enforce_json_depth(input, limits.json_depth)?;

    let mut deserializer = serde_json::Deserializer::from_slice(input);
    let request = RequestDto::deserialize(&mut deserializer)
        .map_err(|error| classify_deserialization_error(&error))?;
    deserializer
        .end()
        .map_err(|error| classify_deserialization_error(&error))?;

    convert_request(request, limits)
}

/// Parses the exact lowercase `sha256:` digest spelling used on the wire.
///
/// # Errors
///
/// Returns `INVALID_DIGEST` unless the input is the prefix followed by exactly
/// 64 lowercase hexadecimal digits.
pub fn parse_sha256(value: &str, field: &'static str) -> Result<Sha256Digest, ProtocolError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(invalid_digest(field));
    };
    if hex.len() != 64
        || !hex
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(invalid_digest(field));
    }

    let mut digest = [0_u8; 32];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(Sha256Digest(digest))
}

/// Serializes one JSON value followed by exactly one LF.
///
/// # Errors
///
/// Returns `INTERNAL_ERROR` if serialization unexpectedly fails.
pub fn to_json_line<T: Serialize>(value: &T) -> Result<String, ProtocolError> {
    serde_json::to_string(value)
        .map(|mut json| {
            json.push('\n');
            json
        })
        .map_err(|_| {
            ProtocolError::new(ErrorDto::new(
                ErrorCode::InternalError,
                "failed to serialize the JSON response",
                BTreeMap::new(),
            ))
        })
}

/// Replaces an absolute path spelling with a non-sensitive placeholder.
#[must_use]
pub fn redact_path(path: &str) -> &str {
    let windows_absolute = path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if path.starts_with('/') || path.starts_with("\\\\") || windows_absolute {
        "<redacted-absolute-path>"
    } else {
        path
    }
}

/// Escapes terminal controls and bidirectional-formatting characters visibly.
#[must_use]
pub fn escape_terminal_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() || is_bidi_control(character) {
            for replacement in character.escape_unicode() {
                escaped.push(replacement);
            }
        } else {
            escaped.push(character);
        }
    }
    escaped
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDto {
    protocol_version: u64,
    operations: Vec<OperationDto>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OperationDto {
    Move {
        source: SourceDto,
        destination: DestinationDto,
    },
    Copy {
        source: SourceDto,
        destination: DestinationDto,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDto {
    path: String,
    selector: SelectorDto,
    precondition: PreconditionDto,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DestinationDto {
    path: String,
    anchor: AnchorDto,
    precondition: PreconditionDto,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SelectorDto {
    Lines { start: u64, end: u64 },
    Bytes { start: u64, end: u64 },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum AnchorDto {
    FileStart,
    FileEnd,
    BeforeLine { line: u64 },
    AfterLine { line: u64 },
    ByteOffset { offset: u64 },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PreconditionDto {
    Sha256 { value: String },
    MustNotExist,
}

fn convert_request(
    request: RequestDto,
    limits: RequestLimits,
) -> Result<BatchSpecification, ProtocolError> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::new(ErrorDto::new(
            ErrorCode::UnsupportedProtocolVersion,
            "the request protocol version is not supported",
            context([
                ("received", json!(request.protocol_version)),
                ("supported", json!([PROTOCOL_VERSION])),
            ]),
        )));
    }

    let operation_count = u64::try_from(request.operations.len()).unwrap_or(u64::MAX);
    if operation_count == 0 {
        return Err(invalid_request("operations_empty", None));
    }
    enforce_limit("operations", operation_count, limits.operations)?;

    let mut paths = BTreeSet::new();
    for operation in &request.operations {
        let (source, destination) = operation.endpoints();
        enforce_path(source, limits.path_bytes)?;
        enforce_path(destination, limits.path_bytes)?;
        paths.insert(source.as_str());
        paths.insert(destination.as_str());
    }
    let path_count = u64::try_from(paths.len()).unwrap_or(u64::MAX);
    enforce_limit("operation_paths", path_count, limits.operation_paths)?;

    let operations = request
        .operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| convert_operation(operation, index))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(BatchSpecification {
        operations: operations.into(),
    })
}

impl OperationDto {
    fn endpoints(&self) -> (&String, &String) {
        let (source, destination) = match self {
            Self::Move {
                source,
                destination,
            }
            | Self::Copy {
                source,
                destination,
            } => (source, destination),
        };
        (&source.path, &destination.path)
    }
}

fn convert_operation(operation: OperationDto, index: usize) -> Result<Operation, ProtocolError> {
    let (kind, source, destination) = match operation {
        OperationDto::Move {
            source,
            destination,
        } => (1_u8, source, destination),
        OperationDto::Copy {
            source,
            destination,
        } => (2_u8, source, destination),
    };

    let specification = OperationSpecification {
        source: convert_source(source, index)?,
        destination: convert_destination(destination, index)?,
    };
    Ok(if kind == 1 {
        Operation::Move(specification)
    } else {
        Operation::Copy(specification)
    })
}

fn convert_source(source: SourceDto, index: usize) -> Result<SourceSelection, ProtocolError> {
    let precondition = match source.precondition {
        PreconditionDto::Sha256 { value } => Precondition::Sha256(parse_sha256(
            &value,
            "operations[].source.precondition.value",
        )?),
        PreconditionDto::MustNotExist => {
            return Err(invalid_request(
                "source_must_exist",
                Some(u64::try_from(index).unwrap_or(u64::MAX)),
            ));
        }
    };

    Ok(SourceSelection {
        path: WorkspaceRelativePath { value: source.path },
        selector: convert_selector(source.selector, index)?,
        precondition,
    })
}

fn convert_destination(
    destination: DestinationDto,
    index: usize,
) -> Result<Destination, ProtocolError> {
    let precondition = match destination.precondition {
        PreconditionDto::Sha256 { value } => Precondition::Sha256(parse_sha256(
            &value,
            "operations[].destination.precondition.value",
        )?),
        PreconditionDto::MustNotExist => Precondition::MustNotExist,
    };

    Ok(Destination {
        path: WorkspaceRelativePath {
            value: destination.path,
        },
        anchor: convert_anchor(destination.anchor, index)?,
        precondition,
    })
}

fn convert_selector(selector: SelectorDto, index: usize) -> Result<Selector, ProtocolError> {
    match selector {
        SelectorDto::Lines { start, end } if start > 0 && start <= end => {
            Ok(Selector::Lines { start, end })
        }
        SelectorDto::Bytes { start, end } if start < end => Ok(Selector::Bytes { start, end }),
        SelectorDto::Lines { .. } => Err(invalid_request(
            "invalid_line_selector",
            Some(u64::try_from(index).unwrap_or(u64::MAX)),
        )),
        SelectorDto::Bytes { .. } => Err(invalid_request(
            "empty_byte_selector",
            Some(u64::try_from(index).unwrap_or(u64::MAX)),
        )),
    }
}

fn convert_anchor(anchor: AnchorDto, index: usize) -> Result<Anchor, ProtocolError> {
    match anchor {
        AnchorDto::FileStart => Ok(Anchor::FileStart),
        AnchorDto::FileEnd => Ok(Anchor::FileEnd),
        AnchorDto::BeforeLine { line } if line > 0 => Ok(Anchor::BeforeLine(line)),
        AnchorDto::AfterLine { line } if line > 0 => Ok(Anchor::AfterLine(line)),
        AnchorDto::ByteOffset { offset } => Ok(Anchor::ByteOffset(offset)),
        AnchorDto::BeforeLine { .. } | AnchorDto::AfterLine { .. } => Err(invalid_request(
            "invalid_line_anchor",
            Some(u64::try_from(index).unwrap_or(u64::MAX)),
        )),
    }
}

fn enforce_request_size(input: &[u8], limit: u64) -> Result<(), ProtocolError> {
    let actual = u64::try_from(input.len()).unwrap_or(u64::MAX);
    enforce_limit("request_bytes", actual, limit)
}

fn enforce_json_depth(input: &[u8], limit: u64) -> Result<(), ProtocolError> {
    let mut depth = 0_u64;
    let mut in_string = false;
    let mut escaped = false;

    for byte in input {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                enforce_limit("json_depth", depth, limit)?;
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn enforce_path(path: &str, limit: u64) -> Result<(), ProtocolError> {
    let actual = u64::try_from(path.len()).unwrap_or(u64::MAX);
    enforce_limit("path_bytes", actual, limit)?;
    if path.is_empty() {
        return Err(invalid_request("path_empty", None));
    }
    if path.contains('\0') {
        return Err(invalid_request("path_contains_nul", None));
    }
    Ok(())
}

fn enforce_limit(resource: &'static str, actual: u64, limit: u64) -> Result<(), ProtocolError> {
    if actual <= limit {
        return Ok(());
    }
    Err(ProtocolError::new(ErrorDto::new(
        ErrorCode::ResourceLimitExceeded,
        "a protocol resource limit was exceeded",
        context([
            ("actual", json!(actual)),
            ("limit", json!(limit)),
            ("resource", json!(resource)),
        ]),
    )))
}

fn classify_deserialization_error(error: &serde_json::Error) -> ProtocolError {
    let code = if error.is_syntax() || error.is_eof() {
        ErrorCode::InvalidJson
    } else {
        ErrorCode::InvalidRequest
    };
    let message = if code == ErrorCode::InvalidJson {
        "the request is not valid JSON"
    } else {
        "the request does not match protocol version 1"
    };
    ProtocolError::new(ErrorDto::new(
        code,
        message,
        context([
            ("column", json!(error.column())),
            ("line", json!(error.line())),
            ("reason", json!(error.to_string())),
        ]),
    ))
}

fn invalid_request(reason: &'static str, operation_index: Option<u64>) -> ProtocolError {
    let mut details = context([("reason", json!(reason))]);
    if let Some(index) = operation_index {
        details.insert("operation_index".to_string(), json!(index));
    }
    ProtocolError::new(ErrorDto::new(
        ErrorCode::InvalidRequest,
        "the request contains an invalid domain value",
        details,
    ))
}

fn invalid_digest(field: &'static str) -> ProtocolError {
    ProtocolError::new(ErrorDto::new(
        ErrorCode::InvalidDigest,
        "a digest must be `sha256:` followed by 64 lowercase hexadecimal digits",
        context([("field", json!(field))]),
    ))
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn context<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn request_with_operation(operation: &str) -> String {
        format!(r#"{{"protocol_version":1,"operations":[{operation}]}}"#)
    }

    #[test]
    fn parse_request_should_reject_duplicate_keys() {
        let request = r#"{"protocol_version":1,"protocol_version":1,"operations":[]}"#.to_string();

        let error = parse_request(request.as_bytes()).expect_err("duplicate key must fail");

        assert_eq!(error.report().code(), ErrorCode::InvalidRequest);
    }

    #[test]
    fn parse_request_should_reject_unknown_fields() {
        let request = r#"{"protocol_version":1,"operations":[],"extra":true}"#;

        let error = parse_request(request.as_bytes()).expect_err("unknown field must fail");

        assert_eq!(error.report().code(), ErrorCode::InvalidRequest);
    }

    #[test]
    fn parse_request_should_reject_source_absence_precondition() {
        let operation = r#"{"kind":"copy","source":{"path":"a","selector":{"kind":"bytes","start":0,"end":1},"precondition":{"kind":"must_not_exist"}},"destination":{"path":"b","anchor":{"kind":"file_start"},"precondition":{"kind":"must_not_exist"}}}"#;
        let request = request_with_operation(operation);

        let error = parse_request(request.as_bytes()).expect_err("absent source must fail");

        assert_eq!(error.report().code(), ErrorCode::InvalidRequest);
    }

    #[test]
    fn parse_request_should_accept_maximum_u64_coordinate() {
        let operation = format!(
            r#"{{"kind":"copy","source":{{"path":"a","selector":{{"kind":"bytes","start":0,"end":18446744073709551615}},"precondition":{{"kind":"sha256","value":"{DIGEST}"}}}},"destination":{{"path":"b","anchor":{{"kind":"byte_offset","offset":18446744073709551615}},"precondition":{{"kind":"must_not_exist"}}}}}}"#
        );
        let request = request_with_operation(&operation);

        let batch = parse_request(request.as_bytes()).expect("u64 maximum must parse");

        assert_eq!(batch.operations.len(), 1);
    }

    #[test]
    fn escape_terminal_text_should_escape_controls_and_bidi() {
        assert_eq!(
            escape_terminal_text("safe\n\u{202e}text"),
            "safe\\u{a}\\u{202e}text"
        );
    }

    #[test]
    fn redact_path_should_hide_absolute_spellings_only() {
        assert_eq!(
            redact_path("/private/request.json"),
            "<redacted-absolute-path>"
        );
        assert_eq!(redact_path(r"C:\\request.json"), "<redacted-absolute-path>");
        assert_eq!(redact_path("requests/apply.json"), "requests/apply.json");
    }

    #[test]
    fn every_error_code_should_map_to_its_category_exit() {
        for code in ErrorCode::ALL {
            assert!(matches!(code.category().exit_code(), 2 | 3 | 4 | 5 | 6 | 8));
        }
    }
}
