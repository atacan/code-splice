//! Checksummed transaction records and pure state-machine validation.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::CWD;
use rustix::fs::{RenameFlags, renameat_with};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use srcmv_core::{FileIdentity, Sha256Digest};

use crate::{FsError, TransactionDirectory};

/// Record-envelope format version.
pub const RECORD_FORMAT_VERSION: u32 = 1;
/// Maximum manifest or individual state-record size.
pub const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum state records in one transaction.
pub const MAX_STATE_RECORDS: u64 = 512;
/// Maximum cumulative bytes in one state chain.
pub const MAX_STATE_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum targets in one transaction.
pub const MAX_TRANSACTION_TARGETS: u64 = 100;
/// Maximum projected candidate, backup, and record disk use.
pub const MAX_TRANSACTION_DISK_BYTES: u64 = 3 * 1024 * 1024 * 1024;

/// Lowerable limits for transaction records and recovery scans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionLimits {
    pub(crate) record_bytes: u64,
    pub(crate) targets: u64,
    pub(crate) state_records: u64,
    pub(crate) state_bytes: u64,
    pub(crate) transaction_directories: u64,
    pub(crate) recovery_bytes: u64,
    pub(crate) projected_disk_bytes: u64,
}

impl TransactionLimits {
    /// Creates limits for trusted lower-limit configuration and boundary tests.
    #[must_use]
    pub const fn new(
        record_bytes: u64,
        targets: u64,
        state_records: u64,
        state_bytes: u64,
        transaction_directories: u64,
        recovery_bytes: u64,
        projected_disk_bytes: u64,
    ) -> Self {
        Self {
            record_bytes,
            targets,
            state_records,
            state_bytes,
            transaction_directories,
            recovery_bytes,
            projected_disk_bytes,
        }
    }
}

impl Default for TransactionLimits {
    fn default() -> Self {
        Self::new(
            MAX_RECORD_BYTES,
            MAX_TRANSACTION_TARGETS,
            MAX_STATE_RECORDS,
            MAX_STATE_BYTES,
            crate::control::MAX_TRANSACTION_DIRECTORIES,
            crate::control::MAX_RECOVERY_BYTES,
            MAX_TRANSACTION_DISK_BYTES,
        )
    }
}

pub(crate) const MANIFEST_MAGIC: &[u8] = b"SRCMV-MANIFEST\0";
pub(crate) const STATE_MAGIC: &[u8] = b"SRCMV-STATE\0";
const HEADER_TRAILER_BYTES: u64 = 4 + 8 + 32;

/// A persisted POSIX physical identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedIdentity {
    /// Filesystem device number.
    pub device: u64,
    /// Inode number.
    pub inode: u64,
}

impl From<FileIdentity> for PersistedIdentity {
    fn from(value: FileIdentity) -> Self {
        Self {
            device: value.device,
            inode: value.inode,
        }
    }
}

/// One immutable input captured in a transaction manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestInput {
    /// Normalized workspace-relative path.
    pub path: String,
    /// Validated parent identity.
    pub parent_identity: PersistedIdentity,
    /// Whether the input existed at planning time.
    pub existed: bool,
    /// Existing file identity.
    pub file_identity: Option<PersistedIdentity>,
    /// Existing file SHA-256 in protocol spelling.
    pub sha256: Option<String>,
    /// Existing file byte length.
    pub length: Option<u64>,
    /// Existing file link count.
    pub link_count: Option<u64>,
}

/// A source segment retained for candidate streaming.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSegment {
    /// Input index in the manifest.
    pub input_index: u64,
    /// Inclusive byte start in the immutable input.
    pub start: u64,
    /// Exclusive byte end in the immutable input.
    pub end: u64,
    /// Operation index for payload segments, otherwise `None`.
    pub operation_index: Option<u64>,
}

/// Permission policy persisted for one target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPolicy {
    /// Preserve the mode observed on the verified backup.
    PreserveExistingMode,
    /// Apply the startup-umask-derived mode stored in `new_file_mode`.
    NewFileMode,
}

/// One deterministically ordered target in a transaction manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestTarget {
    /// Zero-based index in normalized path order.
    pub target_index: u64,
    /// Normalized workspace-relative target path.
    pub path: String,
    /// Validated target parent identity.
    pub parent_identity: PersistedIdentity,
    /// Whether the target originally existed.
    pub original_existed: bool,
    /// Original physical identity.
    pub original_identity: Option<PersistedIdentity>,
    /// Original content digest.
    pub original_sha256: Option<String>,
    /// Original byte length.
    pub original_length: Option<u64>,
    /// Generated candidate basename.
    pub candidate_name: String,
    /// Generated backup basename.
    pub backup_name: String,
    /// Planned candidate content digest.
    pub candidate_sha256: String,
    /// Planned candidate byte length.
    pub candidate_length: u64,
    /// Permission handling policy.
    pub metadata_policy: MetadataPolicy,
    /// Startup-umask-derived new-file mode when applicable.
    pub new_file_mode: Option<u32>,
    /// Ordered source segments for candidate construction.
    pub segments: Vec<ManifestSegment>,
}

/// Immutable payload published before candidate creation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Private transaction schema version.
    pub transaction_version: u64,
    /// Canonical 128-bit transaction identifier.
    pub transaction_id: String,
    /// Canonical workspace-root identity.
    pub workspace_identity: PersistedIdentity,
    /// Plan digest in protocol spelling.
    pub plan_sha256: String,
    /// All inputs required for final pre-mutation validation.
    pub inputs: Vec<ManifestInput>,
    /// Changed targets sorted by normalized UTF-8 path bytes.
    pub targets: Vec<ManifestTarget>,
    /// Metadata exclusions acknowledged by the v0.1 contract.
    pub metadata_limitations: Vec<String>,
}

/// Candidate-artifact state tag.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// No complete candidate has been recorded.
    Missing,
    /// A synced and verified candidate identity has been recorded.
    Ready,
}

/// Persisted candidate-artifact state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateState {
    /// Candidate state tag.
    pub kind: CandidateKind,
    /// Recorded ready-candidate identity.
    pub identity: Option<PersistedIdentity>,
}

/// Target commit-progress tag.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitKind {
    /// The target has not been mutated according to the journal.
    Untouched,
    /// The original was moved to the generated backup name.
    BackedUp,
    /// The candidate was installed at the target.
    Installed,
}

/// Persisted target commit progress.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommitState {
    /// Commit-progress tag.
    pub kind: CommitKind,
    /// Recorded backup or final identity.
    pub identity: Option<PersistedIdentity>,
    /// Mode captured from an existing verified backup.
    pub preserved_mode: Option<u32>,
}

/// Target rollback-progress tag.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackKind {
    /// Rollback has not restored this target.
    None,
    /// The original file has been restored.
    OriginalRestored,
    /// Original absence has been restored.
    AbsenceRestored,
}

/// Persisted target rollback progress.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackState {
    /// Rollback-progress tag.
    pub kind: RollbackKind,
    /// Identity of a restored original file.
    pub identity: Option<PersistedIdentity>,
}

/// Full persisted state for one manifest target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetState {
    /// Manifest target index.
    pub target_index: u64,
    /// Candidate-artifact state.
    pub candidate: CandidateState,
    /// Commit progress.
    pub commit: CommitState,
    /// Rollback progress.
    pub rollback: RollbackState,
}

/// Global transaction state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalState {
    /// Manifest exists and candidates are not all ready.
    Preparing,
    /// Every candidate is ready and no target mutation has begun.
    Prepared,
    /// Target commit is in progress.
    Committing,
    /// Every target is installed and verified.
    Committed,
    /// Transaction-wide rollback is in progress.
    RollingBack,
    /// Every original state has been restored and verified.
    RolledBack,
}

/// One append-only full transaction-state snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateSnapshot {
    /// Private transaction schema version.
    pub transaction_version: u64,
    /// Strictly increasing state sequence.
    pub sequence: u64,
    /// Stored envelope checksum of the manifest record.
    pub manifest_checksum: String,
    /// Stored envelope checksum of the prior state record, or `None` for sequence zero.
    pub prior_state_checksum: Option<String>,
    /// Global state.
    pub global_state: GlobalState,
    /// Full state of every manifest target.
    pub targets: Vec<TargetState>,
}

/// An opened transaction journal beneath an exclusively locked control tree.
#[derive(Debug)]
pub struct TransactionJournal {
    directory: PathBuf,
    manifest_checksum: String,
    original_existence: Vec<bool>,
    limits: TransactionLimits,
    projected_disk_bytes: u64,
    state_record_bytes: u64,
    last_state: Option<StateSnapshot>,
    last_checksum: Option<String>,
}

impl TransactionJournal {
    /// SHA-256 protocol spelling for the published manifest envelope.
    #[must_use]
    pub fn manifest_checksum(&self) -> &str {
        &self.manifest_checksum
    }

    /// Publishes a manifest into a newly allocated transaction directory.
    ///
    /// # Errors
    ///
    /// Returns a validation, serialization, publication, or synchronization error.
    pub fn create(directory: TransactionDirectory, manifest: &Manifest) -> Result<Self, FsError> {
        Self::create_with_limits(directory, manifest, TransactionLimits::default())
    }

    /// Publishes a manifest using trusted lower limits.
    ///
    /// # Errors
    ///
    /// Returns a validation, resource, publication, or synchronization error.
    pub fn create_with_limits(
        directory: TransactionDirectory,
        manifest: &Manifest,
        limits: TransactionLimits,
    ) -> Result<Self, FsError> {
        if manifest.transaction_id != directory.transaction_id {
            return Err(corrupt(
                Some(&directory.transaction_id),
                "manifest_id_mismatch",
            ));
        }
        validate_manifest(manifest, limits)?;
        let encoded = encode_manifest_record_with_limits(manifest, limits)?;
        let projected_disk_bytes = projected_artifact_bytes(manifest, limits)?
            .checked_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "projected_transaction_disk_bytes",
                actual: u64::MAX,
                limit: limits.projected_disk_bytes,
            })?;
        let checksum = checksum_text(record_checksum(&encoded)?);
        publish_record(&directory.path, "manifest.tmp", "manifest.rec", &encoded)?;
        Ok(Self {
            directory: directory.path,
            manifest_checksum: checksum,
            original_existence: manifest
                .targets
                .iter()
                .map(|target| target.original_existed)
                .collect(),
            limits,
            projected_disk_bytes,
            state_record_bytes: 0,
            last_state: None,
            last_checksum: None,
        })
    }

    pub(crate) fn resume(
        directory: TransactionDirectory,
        manifest: &Manifest,
        last_state: StateSnapshot,
        last_checksum: String,
        state_record_bytes: u64,
    ) -> Result<Self, FsError> {
        validate_manifest(manifest, TransactionLimits::default())?;
        validate_state_against_manifest(&last_state, manifest)?;
        let manifest_bytes = fs::read(directory.path.join("manifest.rec"))
            .map_err(|error| record_io("read_manifest_for_recovery", error))?;
        let manifest_checksum = checksum_text(record_checksum(&manifest_bytes)?);
        if manifest_checksum != last_state.manifest_checksum {
            return Err(corrupt(
                Some(&directory.transaction_id),
                "recovery_manifest_checksum_mismatch",
            ));
        }
        let projected_disk_bytes =
            projected_artifact_bytes(manifest, TransactionLimits::default())?
                .checked_add(u64::try_from(manifest_bytes.len()).unwrap_or(u64::MAX))
                .and_then(|value| value.checked_add(state_record_bytes))
                .ok_or(FsError::ResourceLimitExceeded {
                    resource: "projected_transaction_disk_bytes",
                    actual: u64::MAX,
                    limit: MAX_TRANSACTION_DISK_BYTES,
                })?;
        Ok(Self {
            directory: directory.path,
            manifest_checksum,
            original_existence: manifest
                .targets
                .iter()
                .map(|target| target.original_existed)
                .collect(),
            limits: TransactionLimits::default(),
            projected_disk_bytes,
            state_record_bytes,
            last_state: Some(last_state),
            last_checksum: Some(last_checksum),
        })
    }

    /// Publishes the next validated full state snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if sequencing, checksum links, state combinations, or the
    /// state transition are invalid, or if durable publication fails.
    pub fn publish_state(&mut self, state: &StateSnapshot) -> Result<String, FsError> {
        if state.manifest_checksum != self.manifest_checksum {
            return Err(corrupt(None, "state_manifest_checksum_mismatch"));
        }
        validate_state_against_originals(state, &self.original_existence)?;
        if state.prior_state_checksum != self.last_checksum {
            return Err(corrupt(None, "state_prior_checksum_mismatch"));
        }
        match &self.last_state {
            Some(previous) => {
                validate_state_transition_with_limits(previous, state, self.limits)?;
            }
            None => {
                if state.sequence != 0 || state.global_state != GlobalState::Preparing {
                    return Err(corrupt(None, "state_zero_must_be_preparing"));
                }
                validate_state_shape(state, self.limits)?;
            }
        }
        let encoded = encode_state_record_with_limits(state, self.limits)?;
        let state_record_bytes = self
            .state_record_bytes
            .checked_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "state_record_bytes",
                actual: u64::MAX,
                limit: self.limits.state_bytes,
            })?;
        if state_record_bytes > self.limits.state_bytes {
            return Err(FsError::ResourceLimitExceeded {
                resource: "state_record_bytes",
                actual: state_record_bytes,
                limit: self.limits.state_bytes,
            });
        }
        let projected_disk_bytes = self
            .projected_disk_bytes
            .checked_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "projected_transaction_disk_bytes",
                actual: u64::MAX,
                limit: self.limits.projected_disk_bytes,
            })?;
        if projected_disk_bytes > self.limits.projected_disk_bytes {
            return Err(FsError::ResourceLimitExceeded {
                resource: "projected_transaction_disk_bytes",
                actual: projected_disk_bytes,
                limit: self.limits.projected_disk_bytes,
            });
        }
        let checksum = checksum_text(record_checksum(&encoded)?);
        let temporary = format!("state-{:08}.tmp", state.sequence);
        let published = format!("state-{:08}.rec", state.sequence);
        publish_record(&self.directory, &temporary, &published, &encoded)?;
        self.last_state = Some(state.clone());
        self.last_checksum = Some(checksum.clone());
        self.projected_disk_bytes = projected_disk_bytes;
        self.state_record_bytes = state_record_bytes;
        Ok(checksum)
    }
}

/// Encodes a strict manifest payload in the version-1 record envelope.
///
/// # Errors
///
/// Returns a record-corruption or resource error for invalid fields or size.
pub fn encode_manifest_record(manifest: &Manifest) -> Result<Vec<u8>, FsError> {
    encode_manifest_record_with_limits(manifest, TransactionLimits::default())
}

/// Encodes a manifest using trusted lower transaction limits.
///
/// # Errors
///
/// Returns a record-corruption or resource error for invalid fields or size.
pub fn encode_manifest_record_with_limits(
    manifest: &Manifest,
    limits: TransactionLimits,
) -> Result<Vec<u8>, FsError> {
    validate_manifest(manifest, limits)?;
    let encoded = encode_record(MANIFEST_MAGIC, manifest, limits)?;
    let projected = projected_artifact_bytes(manifest, limits)?
        .checked_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
        .ok_or(FsError::ResourceLimitExceeded {
            resource: "projected_transaction_disk_bytes",
            actual: u64::MAX,
            limit: limits.projected_disk_bytes,
        })?;
    if projected > limits.projected_disk_bytes {
        return Err(FsError::ResourceLimitExceeded {
            resource: "projected_transaction_disk_bytes",
            actual: projected,
            limit: limits.projected_disk_bytes,
        });
    }
    Ok(encoded)
}

/// Decodes and validates a complete manifest record envelope.
///
/// # Errors
///
/// Returns corruption for bad magic, version, length, JSON, checksum, or semantics.
pub fn decode_manifest_record(bytes: &[u8]) -> Result<Manifest, FsError> {
    decode_manifest_record_with_limits(bytes, TransactionLimits::default())
}

/// Decodes a manifest using trusted lower transaction limits.
///
/// # Errors
///
/// Returns corruption or a resource error for an invalid persisted record.
pub fn decode_manifest_record_with_limits(
    bytes: &[u8],
    limits: TransactionLimits,
) -> Result<Manifest, FsError> {
    let decoded = decode_record::<Manifest>(bytes, MANIFEST_MAGIC, None, limits)?;
    validate_manifest(&decoded.payload, limits)?;
    Ok(decoded.payload)
}

/// Encodes a strict state payload in the version-1 record envelope.
///
/// # Errors
///
/// Returns a record-corruption or resource error for invalid fields or size.
pub fn encode_state_record(state: &StateSnapshot) -> Result<Vec<u8>, FsError> {
    encode_state_record_with_limits(state, TransactionLimits::default())
}

/// Encodes a state snapshot using trusted lower transaction limits.
///
/// # Errors
///
/// Returns a record-corruption or resource error for invalid fields or size.
pub fn encode_state_record_with_limits(
    state: &StateSnapshot,
    limits: TransactionLimits,
) -> Result<Vec<u8>, FsError> {
    validate_state_shape(state, limits)?;
    encode_record(STATE_MAGIC, state, limits)
}

/// Decodes and validates a complete state record envelope.
///
/// # Errors
///
/// Returns corruption for bad magic, version, length, JSON, checksum, or semantics.
pub fn decode_state_record(bytes: &[u8]) -> Result<StateSnapshot, FsError> {
    decode_state_record_with_limits(bytes, TransactionLimits::default())
}

/// Decodes a state snapshot using trusted lower transaction limits.
///
/// # Errors
///
/// Returns corruption or a resource error for an invalid persisted record.
pub fn decode_state_record_with_limits(
    bytes: &[u8],
    limits: TransactionLimits,
) -> Result<StateSnapshot, FsError> {
    let decoded = decode_record::<StateSnapshot>(bytes, STATE_MAGIC, None, limits)?;
    validate_state_shape(&decoded.payload, limits)?;
    Ok(decoded.payload)
}

/// Validates one pure state-machine transition and monotonic per-target progress.
///
/// # Errors
///
/// Returns record corruption for any transition not represented by Section 8.4.
pub fn validate_state_transition(
    previous: &StateSnapshot,
    next: &StateSnapshot,
) -> Result<(), FsError> {
    validate_state_transition_with_limits(previous, next, TransactionLimits::default())
}

fn validate_state_transition_with_limits(
    previous: &StateSnapshot,
    next: &StateSnapshot,
    limits: TransactionLimits,
) -> Result<(), FsError> {
    validate_state_shape(previous, limits)?;
    validate_state_shape(next, limits)?;
    if next.sequence
        != previous
            .sequence
            .checked_add(1)
            .ok_or_else(|| corrupt(None, "state_sequence_overflow"))?
    {
        return Err(corrupt(None, "state_sequence_not_contiguous"));
    }
    let allowed = matches!(
        (previous.global_state, next.global_state),
        (
            GlobalState::Preparing,
            GlobalState::Preparing | GlobalState::Prepared | GlobalState::RollingBack
        ) | (
            GlobalState::Prepared,
            GlobalState::Prepared | GlobalState::Committing | GlobalState::RollingBack
        ) | (
            GlobalState::Committing,
            GlobalState::Committing | GlobalState::Committed | GlobalState::RollingBack
        ) | (
            GlobalState::RollingBack,
            GlobalState::RollingBack | GlobalState::RolledBack
        )
    );
    if !allowed {
        return Err(corrupt(None, "invalid_global_state_transition"));
    }
    if previous.targets.len() != next.targets.len() {
        return Err(corrupt(None, "state_target_count_changed"));
    }
    for (before, after) in previous.targets.iter().zip(&next.targets) {
        if before.target_index != after.target_index {
            return Err(corrupt(None, "state_target_order_changed"));
        }
        validate_target_progress(*before, *after, next.global_state)?;
    }
    Ok(())
}

pub(crate) struct DecodedRecord<T> {
    pub(crate) payload: T,
    pub(crate) checksum: Sha256Digest,
}

pub(crate) fn decode_manifest_record_with_checksum(
    bytes: &[u8],
    transaction_id: Option<&str>,
    limits: TransactionLimits,
) -> Result<DecodedRecord<Manifest>, FsError> {
    let decoded = decode_record(bytes, MANIFEST_MAGIC, transaction_id, limits)?;
    validate_manifest(&decoded.payload, limits)?;
    Ok(decoded)
}

pub(crate) fn decode_state_record_with_checksum(
    bytes: &[u8],
    transaction_id: Option<&str>,
    limits: TransactionLimits,
) -> Result<DecodedRecord<StateSnapshot>, FsError> {
    let decoded = decode_record(bytes, STATE_MAGIC, transaction_id, limits)?;
    validate_state_shape(&decoded.payload, limits)?;
    Ok(decoded)
}

pub(crate) fn checksum_text(digest: Sha256Digest) -> String {
    digest.to_prefixed_hex()
}

fn encode_record<T: Serialize>(
    magic: &[u8],
    payload: &T,
    limits: TransactionLimits,
) -> Result<Vec<u8>, FsError> {
    let payload = serde_json::to_vec(payload).map_err(|_| corrupt(None, "record_json_encode"))?;
    let total = checked_record_length(magic, payload.len())?;
    if total > limits.record_bytes {
        return Err(FsError::ResourceLimitExceeded {
            resource: "transaction_record_bytes",
            actual: total,
            limit: limits.record_bytes,
        });
    }
    let mut encoded = Vec::with_capacity(usize::try_from(total).unwrap_or(usize::MAX));
    encoded.extend_from_slice(magic);
    encoded.extend_from_slice(&RECORD_FORMAT_VERSION.to_be_bytes());
    encoded.extend_from_slice(
        &u64::try_from(payload.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    encoded.extend_from_slice(&payload);
    let checksum: [u8; 32] = Sha256::digest(&encoded).into();
    encoded.extend_from_slice(&checksum);
    Ok(encoded)
}

fn decode_record<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    magic: &[u8],
    transaction_id: Option<&str>,
    limits: TransactionLimits,
) -> Result<DecodedRecord<T>, FsError> {
    let id = transaction_id.map(str::to_owned);
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > limits.record_bytes {
        return Err(corrupt(id.as_deref(), "record_oversized"));
    }
    let header = magic
        .len()
        .checked_add(12)
        .ok_or_else(|| corrupt(id.as_deref(), "record_length_overflow"))?;
    if bytes.len() < header + 32 || !bytes.starts_with(magic) {
        return Err(corrupt(id.as_deref(), "record_magic_or_truncation"));
    }
    let version = u32::from_be_bytes(
        bytes[magic.len()..magic.len() + 4]
            .try_into()
            .map_err(|_| corrupt(id.as_deref(), "record_version_truncated"))?,
    );
    if version != RECORD_FORMAT_VERSION {
        return Err(corrupt(id.as_deref(), "record_version_unsupported"));
    }
    let payload_length = u64::from_be_bytes(
        bytes[magic.len() + 4..header]
            .try_into()
            .map_err(|_| corrupt(id.as_deref(), "record_length_truncated"))?,
    );
    let expected = u64::try_from(header)
        .unwrap_or(u64::MAX)
        .checked_add(payload_length)
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| corrupt(id.as_deref(), "record_length_overflow"))?;
    if actual != expected {
        return Err(corrupt(id.as_deref(), "record_length_mismatch"));
    }
    let payload_end = header
        + usize::try_from(payload_length)
            .map_err(|_| corrupt(id.as_deref(), "record_length_unrepresentable"))?;
    let expected_checksum: [u8; 32] = Sha256::digest(&bytes[..payload_end]).into();
    if bytes[payload_end..] != expected_checksum {
        return Err(corrupt(id.as_deref(), "record_checksum_invalid"));
    }
    let mut decoder = serde_json::Deserializer::from_slice(&bytes[header..payload_end]);
    let payload = T::deserialize(&mut decoder)
        .map_err(|_| corrupt(id.as_deref(), "record_payload_invalid"))?;
    decoder
        .end()
        .map_err(|_| corrupt(id.as_deref(), "record_payload_trailing_data"))?;
    Ok(DecodedRecord {
        payload,
        checksum: Sha256Digest(expected_checksum),
    })
}

fn validate_manifest(manifest: &Manifest, limits: TransactionLimits) -> Result<(), FsError> {
    if manifest.transaction_version != 1 {
        return Err(corrupt(
            Some(&manifest.transaction_id),
            "manifest_version_unsupported",
        ));
    }
    validate_transaction_id(&manifest.transaction_id)?;
    validate_digest(&manifest.plan_sha256, Some(&manifest.transaction_id))?;
    let target_count = u64::try_from(manifest.targets.len()).unwrap_or(u64::MAX);
    if target_count == 0 {
        return Err(corrupt(
            Some(&manifest.transaction_id),
            "manifest_targets_empty",
        ));
    }
    if target_count > limits.targets {
        return Err(FsError::ResourceLimitExceeded {
            resource: "transaction_targets",
            actual: target_count,
            limit: limits.targets,
        });
    }
    let mut prior_path: Option<&str> = None;
    let mut projected_disk_bytes = 0_u64;
    for (index, target) in manifest.targets.iter().enumerate() {
        validate_normalized_path(&target.path, &manifest.transaction_id)?;
        if target.target_index != u64::try_from(index).unwrap_or(u64::MAX) {
            return Err(corrupt(
                Some(&manifest.transaction_id),
                "manifest_target_index_invalid",
            ));
        }
        if prior_path.is_some_and(|prior| prior.as_bytes() >= target.path.as_bytes()) {
            return Err(corrupt(
                Some(&manifest.transaction_id),
                "manifest_targets_not_sorted",
            ));
        }
        prior_path = Some(&target.path);
        validate_generated_name(&target.candidate_name, "candidate", index)?;
        validate_generated_name(&target.backup_name, "backup", index)?;
        validate_digest(&target.candidate_sha256, Some(&manifest.transaction_id))?;
        validate_optional_original(target, &manifest.transaction_id)?;
        projected_disk_bytes = projected_disk_bytes
            .checked_add(target.candidate_length)
            .and_then(|value| value.checked_add(target.original_length.unwrap_or(0)))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "projected_transaction_disk_bytes",
                actual: u64::MAX,
                limit: limits.projected_disk_bytes,
            })?;
        let metadata_valid = match target.metadata_policy {
            MetadataPolicy::PreserveExistingMode => {
                target.original_existed && target.new_file_mode.is_none()
            }
            MetadataPolicy::NewFileMode => {
                !target.original_existed
                    && target
                        .new_file_mode
                        .is_some_and(|mode| mode <= 0o666 && mode & 0o111 == 0)
            }
        };
        if !metadata_valid {
            return Err(corrupt(
                Some(&manifest.transaction_id),
                "manifest_metadata_policy_invalid",
            ));
        }
        for segment in &target.segments {
            let input = usize::try_from(segment.input_index)
                .ok()
                .and_then(|value| manifest.inputs.get(value));
            if segment.start >= segment.end || input.is_none() {
                return Err(corrupt(
                    Some(&manifest.transaction_id),
                    "manifest_segment_invalid",
                ));
            }
            if input
                .and_then(|input| input.length)
                .is_none_or(|length| segment.end > length)
            {
                return Err(corrupt(
                    Some(&manifest.transaction_id),
                    "manifest_segment_out_of_bounds",
                ));
            }
        }
    }
    if projected_disk_bytes > limits.projected_disk_bytes {
        return Err(FsError::ResourceLimitExceeded {
            resource: "projected_transaction_disk_bytes",
            actual: projected_disk_bytes,
            limit: limits.projected_disk_bytes,
        });
    }
    for input in &manifest.inputs {
        validate_normalized_path(&input.path, &manifest.transaction_id)?;
        validate_optional_input(input, &manifest.transaction_id)?;
    }
    Ok(())
}

fn projected_artifact_bytes(
    manifest: &Manifest,
    limits: TransactionLimits,
) -> Result<u64, FsError> {
    manifest.targets.iter().try_fold(0_u64, |total, target| {
        total
            .checked_add(target.candidate_length)
            .and_then(|value| value.checked_add(target.original_length.unwrap_or(0)))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "projected_transaction_disk_bytes",
                actual: u64::MAX,
                limit: limits.projected_disk_bytes,
            })
    })
}

fn validate_normalized_path(value: &str, id: &str) -> Result<(), FsError> {
    if value.is_empty()
        || value.len() > 4_096
        || value.starts_with('/')
        || value.contains('\0')
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || value
            .split('/')
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case(".srcmv"))
    {
        Err(corrupt(Some(id), "manifest_path_invalid"))
    } else {
        Ok(())
    }
}

fn validate_state_shape(state: &StateSnapshot, limits: TransactionLimits) -> Result<(), FsError> {
    if state.transaction_version != 1 {
        return Err(corrupt(None, "state_version_unsupported"));
    }
    if state.sequence >= limits.state_records {
        return Err(FsError::ResourceLimitExceeded {
            resource: "state_records",
            actual: state.sequence.saturating_add(1),
            limit: limits.state_records,
        });
    }
    validate_digest(&state.manifest_checksum, None)?;
    if let Some(checksum) = &state.prior_state_checksum {
        validate_digest(checksum, None)?;
    }
    if state.sequence == 0 && state.prior_state_checksum.is_some() {
        return Err(corrupt(None, "state_zero_prior_checksum_present"));
    }
    if state.sequence != 0 && state.prior_state_checksum.is_none() {
        return Err(corrupt(None, "state_prior_checksum_missing"));
    }
    let count = u64::try_from(state.targets.len()).unwrap_or(u64::MAX);
    if count > limits.targets {
        return Err(FsError::ResourceLimitExceeded {
            resource: "transaction_targets",
            actual: count,
            limit: limits.targets,
        });
    }
    for (index, target) in state.targets.iter().enumerate() {
        if target.target_index != u64::try_from(index).unwrap_or(u64::MAX) {
            return Err(corrupt(None, "state_target_index_invalid"));
        }
        validate_target_fields(*target)?;
    }
    validate_global_constraints(state)
}

fn validate_global_constraints(state: &StateSnapshot) -> Result<(), FsError> {
    let all_ready = state
        .targets
        .iter()
        .all(|target| target.candidate.kind == CandidateKind::Ready);
    let all_untouched = state
        .targets
        .iter()
        .all(|target| target.commit.kind == CommitKind::Untouched);
    let none_rolled_back = state
        .targets
        .iter()
        .all(|target| target.rollback.kind == RollbackKind::None);
    match state.global_state {
        GlobalState::Preparing if all_untouched && none_rolled_back => Ok(()),
        GlobalState::Prepared if all_ready && all_untouched && none_rolled_back => Ok(()),
        GlobalState::Committing if all_ready && none_rolled_back => Ok(()),
        GlobalState::Committed
            if all_ready
                && none_rolled_back
                && state
                    .targets
                    .iter()
                    .all(|target| target.commit.kind == CommitKind::Installed) =>
        {
            Ok(())
        }
        GlobalState::RollingBack
            if state.targets.iter().all(|target| {
                target.rollback.kind != RollbackKind::None
                    || target.commit.kind == CommitKind::Untouched
                    || target.candidate.kind == CandidateKind::Ready
            }) =>
        {
            Ok(())
        }
        GlobalState::RolledBack
            if state
                .targets
                .iter()
                .all(|target| target.rollback.kind != RollbackKind::None) =>
        {
            Ok(())
        }
        _ => Err(corrupt(None, "global_state_target_combination_invalid")),
    }
}

pub(crate) fn validate_state_against_manifest(
    state: &StateSnapshot,
    manifest: &Manifest,
) -> Result<(), FsError> {
    let originals = manifest
        .targets
        .iter()
        .map(|target| target.original_existed)
        .collect::<Vec<_>>();
    validate_state_against_originals(state, &originals)
}

fn validate_state_against_originals(
    state: &StateSnapshot,
    original_existence: &[bool],
) -> Result<(), FsError> {
    if state.targets.len() != original_existence.len() {
        return Err(corrupt(None, "state_target_count_mismatch"));
    }
    for (target, original_existed) in state.targets.iter().zip(original_existence) {
        if (!original_existed && target.commit.kind == CommitKind::BackedUp)
            || (target.rollback.kind == RollbackKind::OriginalRestored && !original_existed)
            || (target.rollback.kind == RollbackKind::AbsenceRestored && *original_existed)
        {
            return Err(corrupt(None, "state_original_existence_mismatch"));
        }
    }
    Ok(())
}

fn validate_target_fields(target: TargetState) -> Result<(), FsError> {
    if (target.candidate.kind == CandidateKind::Ready) != target.candidate.identity.is_some() {
        return Err(corrupt(None, "candidate_identity_invalid"));
    }
    match target.commit.kind {
        CommitKind::Untouched
            if target.commit.identity.is_none() && target.commit.preserved_mode.is_none() => {}
        CommitKind::BackedUp
            if target.commit.identity.is_some() && target.commit.preserved_mode.is_some() => {}
        CommitKind::Installed if target.commit.identity.is_some() => {}
        _ => return Err(corrupt(None, "commit_fields_invalid")),
    }
    match target.rollback.kind {
        RollbackKind::None | RollbackKind::AbsenceRestored
            if target.rollback.identity.is_none() =>
        {
            Ok(())
        }
        RollbackKind::OriginalRestored if target.rollback.identity.is_some() => Ok(()),
        _ => Err(corrupt(None, "rollback_fields_invalid")),
    }
}

fn validate_target_progress(
    before: TargetState,
    after: TargetState,
    global: GlobalState,
) -> Result<(), FsError> {
    let candidate_monotonic = before.candidate == after.candidate
        || (before.candidate.kind == CandidateKind::Missing
            && after.candidate.kind == CandidateKind::Ready);
    if !candidate_monotonic {
        return Err(corrupt(None, "candidate_state_regressed"));
    }
    if !matches!(global, GlobalState::RollingBack | GlobalState::RolledBack)
        && before.rollback != after.rollback
    {
        return Err(corrupt(None, "rollback_progress_outside_rollback"));
    }
    let commit_progress = commit_rank(after.commit.kind) >= commit_rank(before.commit.kind)
        && (!matches!(global, GlobalState::RollingBack | GlobalState::RolledBack)
            || before.commit == after.commit);
    if !commit_progress {
        return Err(corrupt(None, "commit_state_regressed"));
    }
    if before.rollback.kind != RollbackKind::None && before.rollback != after.rollback {
        return Err(corrupt(None, "rollback_state_changed_after_restore"));
    }
    Ok(())
}

const fn commit_rank(kind: CommitKind) -> u8 {
    match kind {
        CommitKind::Untouched => 0,
        CommitKind::BackedUp => 1,
        CommitKind::Installed => 2,
    }
}

fn validate_optional_input(input: &ManifestInput, id: &str) -> Result<(), FsError> {
    let fields = (
        input.file_identity.is_some(),
        input.sha256.is_some(),
        input.length.is_some(),
        input.link_count.is_some(),
    );
    if input.existed != (fields == (true, true, true, true))
        || (!input.existed && fields != (false, false, false, false))
    {
        return Err(corrupt(Some(id), "manifest_input_existence_fields_invalid"));
    }
    if let Some(digest) = &input.sha256 {
        validate_digest(digest, Some(id))?;
    }
    Ok(())
}

fn validate_optional_original(target: &ManifestTarget, id: &str) -> Result<(), FsError> {
    let fields = (
        target.original_identity.is_some(),
        target.original_sha256.is_some(),
        target.original_length.is_some(),
    );
    if target.original_existed != (fields == (true, true, true))
        || (!target.original_existed && fields != (false, false, false))
    {
        return Err(corrupt(
            Some(id),
            "manifest_target_existence_fields_invalid",
        ));
    }
    if let Some(digest) = &target.original_sha256 {
        validate_digest(digest, Some(id))?;
    }
    Ok(())
}

fn validate_transaction_id(value: &str) -> Result<(), FsError> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(corrupt(None, "transaction_id_invalid"))
    }
}

fn validate_digest(value: &str, id: Option<&str>) -> Result<(), FsError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(corrupt(id, "record_digest_invalid"));
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(corrupt(id, "record_digest_invalid"))
    }
}

fn validate_generated_name(value: &str, prefix: &str, index: usize) -> Result<(), FsError> {
    if value == format!("{prefix}-{index:08}") {
        Ok(())
    } else {
        Err(corrupt(None, "generated_artifact_name_invalid"))
    }
}

fn checked_record_length(magic: &[u8], payload: usize) -> Result<u64, FsError> {
    u64::try_from(magic.len())
        .unwrap_or(u64::MAX)
        .checked_add(HEADER_TRAILER_BYTES)
        .and_then(|value| value.checked_add(u64::try_from(payload).unwrap_or(u64::MAX)))
        .ok_or(FsError::ResourceLimitExceeded {
            resource: "transaction_record_bytes",
            actual: u64::MAX,
            limit: MAX_RECORD_BYTES,
        })
}

fn record_checksum(bytes: &[u8]) -> Result<Sha256Digest, FsError> {
    let checksum = bytes
        .get(
            bytes
                .len()
                .checked_sub(32)
                .ok_or_else(|| corrupt(None, "record_truncated"))?..,
        )
        .ok_or_else(|| corrupt(None, "record_truncated"))?;
    let array: [u8; 32] = checksum
        .try_into()
        .map_err(|_| corrupt(None, "record_checksum_truncated"))?;
    Ok(Sha256Digest(array))
}

fn publish_record(
    directory: &Path,
    temporary: &str,
    published: &str,
    bytes: &[u8],
) -> Result<(), FsError> {
    let temporary_path = directory.join(temporary);
    let published_path = directory.join(published);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary_path)
        .map_err(|error| record_io("create_record_temporary", error))?;
    file.write_all(bytes)
        .map_err(|error| record_io("write_record", error))?;
    file.flush()
        .map_err(|error| record_io("flush_record", error))?;
    file.sync_all()
        .map_err(|error| record_io("sync_record", error))?;
    crate::test_failpoint("before_record_publication")?;
    crate::test_failpoint(&format!("before_{published}_publication"))?;
    renameat_with(
        CWD,
        &temporary_path,
        CWD,
        &published_path,
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        record_io(
            "publish_record",
            io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })?;
    crate::test_failpoint(&format!("after_{published}_publication"))?;
    crate::test_failpoint("after_record_publication")?;
    sync_directory(directory)?;
    Ok(())
}

pub(crate) fn read_record_bounded(
    path: &Path,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<Vec<u8>, FsError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| record_io("inspect_record", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(corrupt(None, "record_not_regular_file"));
    }
    if metadata.len() > limits.record_bytes {
        return Err(corrupt(None, "record_oversized"));
    }
    *cumulative = cumulative
        .checked_add(metadata.len())
        .ok_or(FsError::ResourceLimitExceeded {
            resource: "recovery_bytes",
            actual: u64::MAX,
            limit: limits.recovery_bytes,
        })?;
    if *cumulative > limits.recovery_bytes {
        return Err(FsError::ResourceLimitExceeded {
            resource: "recovery_bytes",
            actual: *cumulative,
            limit: limits.recovery_bytes,
        });
    }
    let mut file = File::open(path).map_err(|error| record_io("open_record", error))?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|error| record_io("read_record", error))?;
    Ok(bytes)
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), FsError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| record_io("sync_directory", error))
}

fn record_io(operation: &'static str, error: io::Error) -> FsError {
    FsError::Io {
        operation,
        path: None,
        kind: error.kind(),
    }
}

fn corrupt(transaction_id: Option<&str>, reason: &'static str) -> FsError {
    FsError::TransactionRecordCorrupt {
        transaction_id: transaction_id.map(str::to_owned),
        reason,
    }
}

#[cfg(test)]
mod failpoint_tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn journal_failpoint_should_interrupt_only_a_test_cfg_subprocess() {
        let output = Command::new(std::env::current_exe().expect("test executable should resolve"))
            .args([
                "--exact",
                "journal::failpoint_tests::subprocess_should_stop_before_record_publication",
            ])
            .env("SRCMV_TEST_CHILD", "1")
            .env("SRCMV_TEST_FAILPOINT", "before_record_publication")
            .output()
            .expect("test subprocess should run");

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn subprocess_should_stop_before_record_publication() {
        if std::env::var_os("SRCMV_TEST_CHILD").is_none() {
            return;
        }
        let root = TempDir::new().expect("temporary directory should be created");
        let transaction = root.path().join(ID);
        fs::create_dir(&transaction).expect("transaction directory should be created");
        let directory = TransactionDirectory {
            transaction_id: ID.to_owned(),
            path: transaction.clone(),
        };
        let manifest = Manifest {
            transaction_version: 1,
            transaction_id: ID.to_owned(),
            workspace_identity: PersistedIdentity {
                device: 1,
                inode: 2,
            },
            plan_sha256: DIGEST.to_owned(),
            inputs: Vec::new(),
            targets: vec![ManifestTarget {
                target_index: 0,
                path: "target".to_owned(),
                parent_identity: PersistedIdentity {
                    device: 1,
                    inode: 2,
                },
                original_existed: false,
                original_identity: None,
                original_sha256: None,
                original_length: None,
                candidate_name: "candidate-00000000".to_owned(),
                backup_name: "backup-00000000".to_owned(),
                candidate_sha256: DIGEST.to_owned(),
                candidate_length: 0,
                metadata_policy: MetadataPolicy::NewFileMode,
                new_file_mode: Some(0o644),
                segments: Vec::new(),
            }],
            metadata_limitations: Vec::new(),
        };

        assert!(matches!(
            TransactionJournal::create(directory, &manifest),
            Err(FsError::Io {
                operation: "test_failpoint",
                kind: io::ErrorKind::Interrupted,
                ..
            })
        ));
        assert!(transaction.join("manifest.tmp").exists());
        assert!(!transaction.join("manifest.rec").exists());
    }
}
