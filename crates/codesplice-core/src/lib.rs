#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Immutable domain contracts for CodeSplice.
//!
//! This crate owns the filesystem-independent vocabulary used by snapshotting,
//! planning, protocol conversion, and execution. Planning is a pure transform
//! from an immutable snapshot and batch specification to segment recipes.

mod plan_hash;
mod planner;

pub use plan_hash::{PLAN_HASH_VERSION, encode_plan_record, plan_digest};
pub use planner::plan;

use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// A complete ordered batch of requested operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchSpecification {
    /// Operations in request order.
    pub operations: Arc<[Operation]>,
}

/// A byte-preserving move or copy request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Remove the selected bytes from the source and insert them at the destination.
    Move(OperationSpecification),
    /// Insert the selected bytes at the destination without deleting the source.
    Copy(OperationSpecification),
}

/// Source and destination shared by move and copy operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationSpecification {
    /// Bytes selected from an existing source.
    pub source: SourceSelection,
    /// Destination path, anchor, and precondition.
    pub destination: Destination,
}

/// A source path, selector, and required existing-file precondition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    /// Normalized workspace-relative source path.
    pub path: WorkspaceRelativePath,
    /// Coordinates in the immutable source snapshot.
    pub selector: Selector,
    /// Required digest of the existing source.
    pub precondition: Precondition,
}

/// A destination path, anchor, and existence precondition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Destination {
    /// Normalized workspace-relative destination path.
    pub path: WorkspaceRelativePath,
    /// Coordinates in the immutable destination snapshot.
    pub anchor: Anchor,
    /// Existing digest or required absence.
    pub precondition: Precondition,
}

/// The required initial state of a referenced path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Precondition {
    /// The path must be an existing file with this exact content digest.
    Sha256(Sha256Digest),
    /// The destination path must not exist.
    MustNotExist,
}

/// Coordinates selecting a nonempty source byte range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Selector {
    /// Inclusive one-based first and last line numbers.
    Lines {
        /// First selected line.
        start: u64,
        /// Last selected line.
        end: u64,
    },
    /// A zero-based half-open byte range.
    Bytes {
        /// First selected byte.
        start: u64,
        /// Exclusive end offset.
        end: u64,
    },
}

/// Coordinates at which selected bytes are inserted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Anchor {
    /// Insert before the first byte.
    FileStart,
    /// Insert after the last byte.
    FileEnd,
    /// Insert before a one-based line number.
    BeforeLine(u64),
    /// Insert after a one-based line and its original terminator.
    AfterLine(u64),
    /// Insert at a zero-based byte offset.
    ByteOffset(u64),
}

/// A validated UTF-8 path relative to the workspace root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceRelativePath {
    /// Normalized UTF-8 spelling.
    pub value: String,
}

/// SHA-256 digest bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Sha256Digest(pub [u8; 32]);

impl Sha256Digest {
    /// Returns the exact lowercase protocol spelling for this digest.
    #[must_use]
    pub fn to_prefixed_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        for byte in self.0 {
            value.push(char::from(HEX[usize::from(byte >> 4)]));
            value.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        value
    }
}

/// POSIX physical identity of a file or directory.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileIdentity {
    /// Filesystem device number.
    pub device: u64,
    /// Inode number on the device.
    pub inode: u64,
}

/// Stable index assigned to one file in an immutable workspace snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotFileId(pub u64);

/// Immutable inputs acquired for a single planning attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
    /// Physical identity of the canonical workspace root.
    pub workspace_identity: FileIdentity,
    /// Existing files in normalized path order.
    pub files: Arc<[FileSnapshot]>,
    /// Required-absent paths in normalized path order.
    pub absent_paths: Arc<[AbsentPathSnapshot]>,
}

/// Immutable bytes and identity information for one existing file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSnapshot {
    /// Stable index within the containing workspace snapshot.
    pub id: SnapshotFileId,
    /// Normalized workspace-relative path.
    pub path: WorkspaceRelativePath,
    /// Physical identity of the file's parent directory.
    pub parent_identity: FileIdentity,
    /// Root-to-parent physical identities captured during the no-symlink walk.
    pub parent_identities: Arc<[FileIdentity]>,
    /// Physical identity of the file.
    pub identity: FileIdentity,
    /// Link count observed during acquisition.
    pub link_count: u64,
    /// Exact immutable file bytes shared by all segment references.
    pub bytes: Arc<[u8]>,
    /// SHA-256 digest of the immutable bytes.
    pub digest: Sha256Digest,
    /// Line boundaries derived from the immutable bytes.
    pub line_index: LineIndex,
}

/// A validated absent destination in an immutable workspace snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbsentPathSnapshot {
    /// Normalized workspace-relative path.
    pub path: WorkspaceRelativePath,
    /// Physical identity of the existing parent directory.
    pub parent_identity: FileIdentity,
    /// Root-to-parent physical identities captured during the no-symlink walk.
    pub parent_identities: Arc<[FileIdentity]>,
    /// Final path component whose absence was observed.
    pub basename: String,
}

/// Compact line-boundary data derived from one immutable file snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineIndex {
    /// Exclusive byte end of each logical line, including its terminator.
    boundaries: Arc<[u64]>,
}

impl LineIndex {
    /// Builds a line index for immutable bytes while enforcing representation limits.
    ///
    /// LF, CRLF, and lone CR are terminators. A nonempty unterminated suffix is a
    /// line, while an empty file and the suffix after a final terminator are not.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ResourceLimitExceeded`] when adding a boundary would
    /// exceed `maximum_lines` or `maximum_memory_bytes`.
    pub fn from_bytes_with_limits(
        bytes: &[u8],
        maximum_lines: u64,
        maximum_memory_bytes: u64,
    ) -> Result<Self, CoreError> {
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| CoreError::ResourceLimitExceeded {
                resource: "snapshot_file_bytes",
                actual: u64::MAX,
                limit: u64::MAX - 1,
            })?;
        let mut boundaries = Vec::new();
        let mut offset = 0_usize;

        while offset < bytes.len() {
            let boundary = match bytes[offset] {
                b'\n' => Some(offset + 1),
                b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                    offset += 1;
                    Some(offset + 1)
                }
                b'\r' => Some(offset + 1),
                _ => None,
            };
            if let Some(boundary) = boundary {
                push_line_boundary(
                    &mut boundaries,
                    boundary,
                    maximum_lines,
                    maximum_memory_bytes,
                )?;
            }
            offset += 1;
        }

        if boundaries.last().copied() != Some(byte_length) && !bytes.is_empty() {
            push_line_boundary(
                &mut boundaries,
                bytes.len(),
                maximum_lines,
                maximum_memory_bytes,
            )?;
        }

        Ok(Self {
            boundaries: boundaries.into(),
        })
    }

    /// Returns the number of logical lines.
    #[must_use]
    pub fn line_count(&self) -> u64 {
        u64::try_from(self.boundaries.len()).unwrap_or(u64::MAX)
    }

    /// Returns the byte offset before a one-based line number.
    #[must_use]
    pub fn line_start(&self, line: u64) -> Option<u64> {
        if line == 0 || line > self.line_count() {
            return None;
        }
        match line {
            1 => Some(0),
            2.. => usize::try_from(line - 2)
                .ok()
                .and_then(|index| self.boundaries.get(index))
                .copied(),
            _ => None,
        }
    }

    /// Returns the exclusive byte end of a one-based line, including its terminator.
    #[must_use]
    pub fn line_end(&self, line: u64) -> Option<u64> {
        line.checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.boundaries.get(index))
            .copied()
    }

    /// Returns the exact bytes used by the compact boundary representation.
    #[must_use]
    pub fn memory_bytes(&self) -> u64 {
        self.line_count().saturating_mul(8)
    }
}

fn push_line_boundary(
    boundaries: &mut Vec<u64>,
    boundary: usize,
    maximum_lines: u64,
    maximum_memory_bytes: u64,
) -> Result<(), CoreError> {
    let next_line_count = u64::try_from(boundaries.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    enforce_core_limit("line_count", next_line_count, maximum_lines)?;
    let next_memory = next_line_count.saturating_mul(8);
    enforce_core_limit("line_index_memory", next_memory, maximum_memory_bytes)?;
    let boundary = u64::try_from(boundary).map_err(|_| CoreError::ResourceLimitExceeded {
        resource: "snapshot_file_bytes",
        actual: u64::MAX,
        limit: u64::MAX,
    })?;
    boundaries.push(boundary);
    Ok(())
}

fn enforce_core_limit(resource: &'static str, actual: u64, limit: u64) -> Result<(), CoreError> {
    if actual > limit {
        return Err(CoreError::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}

/// A selector and anchor resolved against the initial snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedOperation {
    /// Zero-based request-order index.
    pub operation_index: u64,
    /// Move or copy discriminant.
    pub kind: OperationKind,
    /// Normalized source path.
    pub source_path: WorkspaceRelativePath,
    /// Original selector retained for reports and plan hashing.
    pub selector: Selector,
    /// Source precondition retained in the resolved record.
    pub source_precondition: Precondition,
    /// Source byte range in the initial snapshot.
    pub source_range: ByteRange,
    /// Digest of the exact selected payload bytes.
    pub selected_digest: Sha256Digest,
    /// Normalized destination path.
    pub destination_path: WorkspaceRelativePath,
    /// Original anchor retained for reports and plan hashing.
    pub anchor: Anchor,
    /// Destination precondition retained in the resolved record.
    pub destination_precondition: Precondition,
    /// Destination byte offset in the initial snapshot.
    pub destination_offset: u64,
    /// Whether this operation contributes edit events.
    pub effect: OperationEffect,
}

/// Stable operation discriminant used by resolved plans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    /// Remove from the source and insert at the destination.
    Move,
    /// Insert at the destination without deleting the source.
    Copy,
}

/// A half-open byte range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset.
    pub end: u64,
}

/// Whether a resolved operation changes the event stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationEffect {
    /// The operation contributes insertion or deletion events.
    Changed,
    /// A same-file move anchored at its own start or end.
    NoOp,
}

/// Deterministic, filesystem-independent plan for a snapshot and batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditPlan {
    /// Resolved operations in request order.
    pub operations: Arc<[ResolvedOperation]>,
    /// Output recipes in normalized path order.
    pub outputs: Arc<[PlannedOutput]>,
    /// Deterministic accounting charged while retaining the plan.
    pub usage: PlanningUsage,
    /// Versioned deterministic plan digest.
    pub digest: PlanDigest,
}

/// Segment recipe and classification for one output path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedOutput {
    /// Normalized output path.
    pub path: WorkspaceRelativePath,
    /// Original digest, or `None` when the destination was absent.
    pub original_digest: Option<Sha256Digest>,
    /// Classification based on actual resulting bytes.
    pub change: OutputChange,
    /// Resulting byte length.
    pub resulting_length: u64,
    /// Resulting content digest.
    pub resulting_digest: Sha256Digest,
    /// Ordered references into immutable snapshot bytes.
    pub segments: Arc<[OutputSegment]>,
}

/// Byte-level classification of a planned output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputChange {
    /// Resulting bytes equal the existing snapshot.
    Unchanged,
    /// Resulting bytes differ from an existing snapshot and are nonempty.
    ModifiedExisting,
    /// Resulting bytes create an absent path.
    CreatedNew,
    /// Resulting bytes empty an existing file.
    EmptiedExisting,
}

/// One slice in a streamed output recipe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputSegment {
    /// Surviving original bytes from an input snapshot.
    OriginalSlice {
        /// Referenced snapshot file.
        snapshot_file_id: SnapshotFileId,
        /// Referenced half-open byte range.
        range: ByteRange,
    },
    /// Selected bytes inserted by an operation.
    PayloadSlice {
        /// Request-order operation index.
        operation_index: u64,
        /// Referenced snapshot file.
        snapshot_file_id: SnapshotFileId,
        /// Referenced half-open byte range.
        range: ByteRange,
        /// Digest of the selected payload.
        payload_digest: Sha256Digest,
    },
}

/// SHA-256 digest of the versioned deterministic plan record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PlanDigest(pub Sha256Digest);

/// Configured upper bounds charged by protocol, snapshot, planning, and transaction layers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceBudget {
    /// Maximum operations in one batch.
    pub operations: u64,
    /// Maximum distinct operation paths.
    pub operation_paths: u64,
    /// Maximum aggregate immutable snapshot bytes.
    pub snapshot_bytes: u64,
    /// Maximum resulting bytes for one output.
    pub resulting_bytes_per_output: u64,
    /// Maximum aggregate planned output bytes.
    pub planned_output_bytes: u64,
    /// Maximum segments retained by one output.
    pub segments_per_output: u64,
    /// Maximum aggregate output segments.
    pub segments: u64,
    /// Maximum changed transaction targets.
    pub changed_targets: u64,
    /// Maximum conservative serialized response projection.
    pub projected_response_bytes: u64,
    /// Maximum charged memory retained by planning records.
    pub planning_memory_bytes: u64,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            operations: 1_000,
            operation_paths: 1_000,
            snapshot_bytes: 1024 * 1024 * 1024,
            resulting_bytes_per_output: 512 * 1024 * 1024,
            planned_output_bytes: 1024 * 1024 * 1024,
            segments_per_output: 100_000,
            segments: 250_000,
            changed_targets: 100,
            projected_response_bytes: 16 * 1024 * 1024,
            planning_memory_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// Deterministic resource usage retained or projected by one plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanningUsage {
    /// Largest resulting output length.
    pub maximum_output_bytes: u64,
    /// Aggregate resulting output length.
    pub total_output_bytes: u64,
    /// Largest segment count in one output.
    pub maximum_output_segments: u64,
    /// Aggregate segment count.
    pub total_segments: u64,
    /// Number of non-byte-identical outputs.
    pub changed_targets: u64,
    /// Conservative size of the future serialized plan response.
    pub projected_response_bytes: u64,
    /// Charged bytes retained by planning records, excluding snapshot storage.
    pub planning_memory_bytes: u64,
}

/// Typed failures owned by the pure domain layer.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreError {
    /// A domain value violates its documented invariant.
    InvalidDomainValue {
        /// Stable field or concept name.
        field: &'static str,
    },
    /// Requested edits conflict under immutable-snapshot semantics.
    EditConflict {
        /// Stable machine-readable reason.
        reason: &'static str,
        /// Request-order operation when one operation owns the conflict.
        operation_index: Option<u64>,
    },
    /// A changing existing output has multiple hard links.
    HardLinkNotSupported {
        /// Normalized output path.
        path: WorkspaceRelativePath,
        /// Link count captured in the immutable snapshot.
        link_count: u64,
    },
    /// A configured resource budget was exceeded.
    ResourceLimitExceeded {
        /// Stable resource name.
        resource: &'static str,
        /// Observed or projected usage.
        actual: u64,
        /// Configured maximum.
        limit: u64,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDomainValue { field } => {
                write!(formatter, "invalid domain value for {field}")
            }
            Self::EditConflict {
                reason,
                operation_index,
            } => match operation_index {
                Some(index) => write!(formatter, "edit conflict ({reason}) at operation {index}"),
                None => write!(formatter, "edit conflict ({reason})"),
            },
            Self::HardLinkNotSupported { path, link_count } => write!(
                formatter,
                "changing hard-linked output {} with link count {link_count} is unsupported",
                path.value
            ),
            Self::ResourceLimitExceeded {
                resource,
                actual,
                limit,
            } => {
                write!(
                    formatter,
                    "resource limit exceeded for {resource}: {actual} > {limit}"
                )
            }
        }
    }
}

impl Error for CoreError {}

#[cfg(test)]
mod line_index_tests {
    use super::{CoreError, LineIndex};

    fn index(bytes: &[u8]) -> LineIndex {
        LineIndex::from_bytes_with_limits(bytes, u64::MAX, u64::MAX)
            .expect("unlimited test index should build")
    }

    #[test]
    fn line_index_should_handle_empty_and_unterminated_bytes() {
        let empty = index(b"");
        let unterminated = index(b"abc");

        assert_eq!(empty.line_count(), 0);
        assert_eq!(unterminated.line_count(), 1);
        assert_eq!(unterminated.line_start(1), Some(0));
        assert_eq!(unterminated.line_end(1), Some(3));
        assert_eq!(unterminated.line_start(2), None);
    }

    #[test]
    fn line_index_should_recognize_lf_crlf_lone_cr_and_mixed_terminators() {
        let line_index = index(b"a\nb\r\nc\rd");

        assert_eq!(line_index.line_count(), 4);
        assert_eq!(line_index.line_start(1), Some(0));
        assert_eq!(line_index.line_end(1), Some(2));
        assert_eq!(line_index.line_start(2), Some(2));
        assert_eq!(line_index.line_end(2), Some(5));
        assert_eq!(line_index.line_start(3), Some(5));
        assert_eq!(line_index.line_end(3), Some(7));
        assert_eq!(line_index.line_start(4), Some(7));
        assert_eq!(line_index.line_end(4), Some(8));
    }

    #[test]
    fn line_index_should_not_create_a_phantom_line_after_a_terminator() {
        assert_eq!(index(b"a\n").line_count(), 1);
        assert_eq!(index(b"a\r\n").line_count(), 1);
        assert_eq!(index(b"a\r").line_count(), 1);
    }

    #[test]
    fn line_index_should_treat_non_utf8_and_long_lines_as_bytes() {
        let mut bytes = vec![b'x'; 128 * 1024];
        bytes.extend_from_slice(&[0xff, b'\n']);
        let line_index = index(&bytes);

        assert_eq!(line_index.line_count(), 1);
        assert_eq!(line_index.line_end(1), Some(131_074));
        assert_eq!(line_index.memory_bytes(), 8);
    }

    #[test]
    fn line_index_should_enforce_line_and_memory_limits_before_a_boundary() {
        let line_error = LineIndex::from_bytes_with_limits(b"a\nb\n", 1, u64::MAX)
            .expect_err("second line should exceed limit");
        let memory_error = LineIndex::from_bytes_with_limits(b"a\n", u64::MAX, 7)
            .expect_err("one boundary needs eight bytes");

        assert!(matches!(
            line_error,
            CoreError::ResourceLimitExceeded {
                resource: "line_count",
                actual: 2,
                limit: 1
            }
        ));
        assert!(matches!(
            memory_error,
            CoreError::ResourceLimitExceeded {
                resource: "line_index_memory",
                actual: 8,
                limit: 7
            }
        ));
    }
}
