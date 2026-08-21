//! Workspace control-tree validation, locking, scans, and safe control-only cleanup.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use getrandom::fill;
use rustix::fs::{CWD, FlockOperation, RenameFlags, flock, renameat_with};
use rustix::io::Errno;

use crate::journal::{
    GlobalState, StateSnapshot, TransactionLimits, checksum_text,
    decode_manifest_record_with_checksum, decode_state_record_with_checksum, read_record_bounded,
    sync_directory, validate_state_against_manifest, validate_state_transition,
};
use crate::{FsError, Manifest, Workspace};

/// Maximum transaction directories visited by one bounded scan.
pub const MAX_TRANSACTION_DIRECTORIES: u64 = 100;
/// Maximum bytes read by one recovery command.
pub const MAX_RECOVERY_BYTES: u64 = 256 * 1024 * 1024;

const CONTROL_NAME: &str = ".codesplice";
const TRANSACTIONS_NAME: &str = "transactions";
const COMPLETED_NAME: &str = "completed";
const LOCK_NAME: &str = "lock";
const MAX_ID_ATTEMPTS: usize = 8;

/// Recovery-directory classification exposed to orchestration and reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryEntryKind {
    /// Canonical active directory without a published manifest.
    OrphanRecord,
    /// Valid manifest without state sequence zero.
    ManifestOnly,
    /// Valid nonterminal or terminal journal in the active directory.
    Active,
    /// Suffix-classified directory beneath `completed`.
    CleanupOnly,
}

impl RecoveryEntryKind {
    /// Stable protocol spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrphanRecord => "orphan_record",
            Self::ManifestOnly => "manifest_only",
            Self::Active => "active",
            Self::CleanupOnly => "cleanup_only",
        }
    }
}

/// One validated recovery-list or status entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryEntry {
    transaction_id: String,
    kind: RecoveryEntryKind,
    actions: Vec<&'static str>,
    visibility: &'static str,
    active_path: Option<PathBuf>,
    completed_path: Option<PathBuf>,
}

impl RecoveryEntry {
    /// Canonical transaction identifier.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Validated recovery-directory classification.
    #[must_use]
    pub const fn kind(&self) -> RecoveryEntryKind {
        self.kind
    }

    /// Currently safe protocol recovery actions.
    #[must_use]
    pub fn actions(&self) -> &[&'static str] {
        &self.actions
    }

    /// Current target-visibility classification for recovery reporting.
    #[must_use]
    pub const fn visibility(&self) -> &'static str {
        self.visibility
    }

    pub(crate) fn active_path(&self) -> Option<&Path> {
        self.active_path.as_deref()
    }

    pub(crate) fn completed_path(&self) -> Option<&Path> {
        self.completed_path.as_deref()
    }
}

/// Result of a bounded diagnostic control scan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ControlObservation {
    entries: Vec<RecoveryEntry>,
}

impl ControlObservation {
    /// Validated entries in transaction-ID order.
    #[must_use]
    pub fn entries(&self) -> &[RecoveryEntry] {
        &self.entries
    }

    /// Finds one canonical identifier in active or completed storage.
    #[must_use]
    pub fn find(&self, transaction_id: &str) -> Option<&RecoveryEntry> {
        self.entries
            .binary_search_by(|entry| entry.transaction_id.as_str().cmp(transaction_id))
            .ok()
            .map(|index| &self.entries[index])
    }
}

#[derive(Debug)]
struct ControlPaths {
    control: PathBuf,
    transactions: PathBuf,
    completed: PathBuf,
    lock: PathBuf,
}

impl ControlPaths {
    fn new(workspace: &Workspace) -> Self {
        let control = workspace.canonical_root().join(CONTROL_NAME);
        Self {
            transactions: control.join(TRANSACTIONS_NAME),
            completed: control.join(COMPLETED_NAME),
            lock: control.join(LOCK_NAME),
            control,
        }
    }
}

/// Held nonblocking shared diagnostic lock.
#[derive(Debug)]
pub struct DiagnosticLock {
    _lock_file: File,
    paths: ControlPaths,
}

impl DiagnosticLock {
    /// Performs a bounded, fully validating read-only control scan.
    ///
    /// # Errors
    ///
    /// Returns corruption, invalid-control, I/O, or resource-limit errors. It
    /// never deletes or publishes anything.
    pub fn scan(&self) -> Result<ControlObservation, FsError> {
        self.scan_with_limits(TransactionLimits::default())
    }

    /// Performs a read-only control scan with trusted lower limits.
    ///
    /// # Errors
    ///
    /// Returns corruption, invalid-control, I/O, or resource-limit errors.
    pub fn scan_with_limits(
        &self,
        limits: TransactionLimits,
    ) -> Result<ControlObservation, FsError> {
        scan_control(&self.paths, limits)
    }
}

/// Held nonblocking exclusive mutation lock.
#[derive(Debug)]
pub struct MutationLock {
    _lock_file: File,
    paths: ControlPaths,
    identities: ControlIdentities,
}

impl MutationLock {
    pub(crate) const fn control_device(&self) -> u64 {
        self.identities.control.0
    }

    /// Revalidates the root, control directories, and lock entry against the
    /// identities captured immediately after lock acquisition.
    ///
    /// Commit and recovery call this immediately before user-target mutation.
    ///
    /// # Errors
    ///
    /// Returns `ControlDirectoryInvalid` if an entry was replaced.
    pub fn revalidate_control_identities(&self) -> Result<(), FsError> {
        if capture_control_identities(&self.paths)? == self.identities {
            Ok(())
        } else {
            Err(FsError::ControlDirectoryInvalid {
                reason: "control_identity_changed",
            })
        }
    }

    /// Scans for unfinished transactions, removes only validated cleanup-only
    /// directories, and refuses to admit a new transaction while an active one exists.
    ///
    /// # Errors
    ///
    /// Returns recovery-required, corruption, I/O, or resource-limit errors.
    pub fn gate_new_transaction(&self) -> Result<(), FsError> {
        self.gate_new_transaction_with_limits(TransactionLimits::default())
    }

    /// Runs the new-transaction gate with trusted lower scan limits.
    ///
    /// # Errors
    ///
    /// Returns recovery-required, corruption, I/O, or resource-limit errors.
    pub fn gate_new_transaction_with_limits(
        &self,
        limits: TransactionLimits,
    ) -> Result<(), FsError> {
        let observation = scan_control(&self.paths, limits)?;
        let active = observation
            .entries
            .iter()
            .filter(|entry| entry.active_path.is_some())
            .map(|entry| entry.transaction_id.clone())
            .collect::<Vec<_>>();
        if !active.is_empty() {
            return Err(FsError::TransactionRecoveryRequired {
                transaction_ids: active,
            });
        }
        for entry in observation.entries {
            if let Some(path) = entry.completed_path {
                cleanup_completed_directory(&path, &entry.transaction_id)?;
            }
        }
        Ok(())
    }

    /// Allocates an exclusive canonical active transaction directory using a
    /// random 128-bit identifier and bounded collision retry.
    ///
    /// # Errors
    ///
    /// Returns an I/O or bounded-collision error.
    pub fn create_transaction_directory(&self) -> Result<TransactionDirectory, FsError> {
        self.create_transaction_directory_with(|bytes| {
            fill(bytes).map_err(|_| FsError::Io {
                operation: "generate_transaction_id",
                path: None,
                kind: io::ErrorKind::Other,
            })
        })
    }

    fn create_transaction_directory_with<F>(
        &self,
        mut random: F,
    ) -> Result<TransactionDirectory, FsError>
    where
        F: FnMut(&mut [u8; 16]) -> Result<(), FsError>,
    {
        for _ in 0..MAX_ID_ATTEMPTS {
            let mut bytes = [0_u8; 16];
            random(&mut bytes)?;
            let transaction_id = hex_lower(&bytes);
            if self
                .paths
                .completed
                .join(format!("{transaction_id}-committed"))
                .exists()
                || self
                    .paths
                    .completed
                    .join(format!("{transaction_id}-rolledback"))
                    .exists()
            {
                continue;
            }
            let path = self.paths.transactions.join(&transaction_id);
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    sync_directory(&self.paths.transactions)?;
                    return Ok(TransactionDirectory {
                        transaction_id,
                        path,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(control_io("create_transaction_directory", error)),
            }
        }
        Err(FsError::ResourceLimitExceeded {
            resource: "transaction_id_collision_attempts",
            actual: u64::try_from(MAX_ID_ATTEMPTS).unwrap_or(u64::MAX),
            limit: u64::try_from(MAX_ID_ATTEMPTS).unwrap_or(u64::MAX),
        })
    }

    /// Removes a canonical orphan or manifest-only transaction without touching
    /// a user target.
    ///
    /// # Errors
    ///
    /// Returns not-found, action-not-allowed, corruption, or I/O errors.
    pub fn rollback_control_only(&self, transaction_id: &str) -> Result<(), FsError> {
        validate_transaction_id(transaction_id)?;
        let observation = scan_control(&self.paths, TransactionLimits::default())?;
        let entry =
            observation
                .find(transaction_id)
                .ok_or_else(|| FsError::TransactionNotFound {
                    transaction_id: transaction_id.to_owned(),
                })?;
        if !matches!(
            entry.kind,
            RecoveryEntryKind::OrphanRecord | RecoveryEntryKind::ManifestOnly
        ) {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: transaction_id.to_owned(),
                reason: "phase_5_allows_only_control_only_rollback",
            });
        }
        let path = entry
            .active_path
            .as_ref()
            .ok_or(FsError::InternalInvariant {
                invariant: "control_only_recovery_has_active_path",
            })?;
        remove_validated_children(path)?;
        fs::remove_dir(path).map_err(|error| control_io("remove_transaction_directory", error))?;
        sync_directory(&self.paths.transactions)
    }

    pub(crate) fn recovery_entry(&self, transaction_id: &str) -> Result<RecoveryEntry, FsError> {
        validate_transaction_id(transaction_id)?;
        scan_control(&self.paths, TransactionLimits::default())?
            .find(transaction_id)
            .cloned()
            .ok_or_else(|| FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            })
    }

    pub(crate) fn finish_transaction(
        &self,
        transaction_id: &str,
        active_path: &Path,
        committed: bool,
    ) -> Result<(), FsError> {
        validate_transaction_id(transaction_id)?;
        if active_path != self.paths.transactions.join(transaction_id) {
            return Err(FsError::InternalInvariant {
                invariant: "terminal_transaction_path_is_canonical",
            });
        }
        let suffix = if committed { "committed" } else { "rolledback" };
        let completed = self
            .paths
            .completed
            .join(format!("{transaction_id}-{suffix}"));
        crate::test_failpoint("before_terminal_directory_rename")?;
        renameat_with(CWD, active_path, CWD, &completed, RenameFlags::NOREPLACE).map_err(
            |error| match error {
                Errno::EXIST => FsError::RecoveryConflict {
                    reason: "completed_directory_collision",
                },
                Errno::XDEV => FsError::CrossDeviceTransaction,
                Errno::NOSYS | Errno::NOTSUP | Errno::INVAL => FsError::NoReplaceUnavailable,
                _ => control_io(
                    "terminal_directory_rename",
                    io::Error::from_raw_os_error(error.raw_os_error()),
                ),
            },
        )?;
        sync_directory(&self.paths.transactions)?;
        sync_directory(&self.paths.completed)?;
        crate::test_failpoint("after_terminal_directory_rename")?;
        crate::test_failpoint("before_terminal_cleanup")?;
        cleanup_completed_directory(&completed, transaction_id)?;
        crate::test_failpoint("after_terminal_cleanup")
    }

    pub(crate) fn cleanup_completed(&self, entry: &RecoveryEntry) -> Result<(), FsError> {
        let path = entry.completed_path().ok_or(FsError::InternalInvariant {
            invariant: "cleanup_entry_has_completed_path",
        })?;
        cleanup_completed_directory(path, entry.transaction_id())
    }
}

/// Newly allocated active transaction directory.
#[derive(Debug)]
pub struct TransactionDirectory {
    pub(crate) transaction_id: String,
    pub(crate) path: PathBuf,
}

impl TransactionDirectory {
    /// Canonical transaction identifier.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Absolute transaction-directory path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn from_recovery(transaction_id: String, path: PathBuf) -> Self {
        Self {
            transaction_id,
            path,
        }
    }
}

impl Workspace {
    /// Acquires a shared diagnostic lock if a control tree already exists.
    ///
    /// The method creates nothing. A workspace without `.codesplice` returns
    /// `Ok(None)`; a partial or invalid control tree fails closed.
    ///
    /// # Errors
    ///
    /// Returns `TransactionBusy` for contention or a validation/I/O error.
    pub fn diagnostic_lock(&self) -> Result<Option<DiagnosticLock>, FsError> {
        let paths = ControlPaths::new(self);
        match fs::symlink_metadata(&paths.control) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(control_io("inspect_control_directory", error)),
            Ok(_) => {}
        }
        validate_control_tree(&paths)?;
        let lock_file = OpenOptions::new()
            .read(true)
            .open(&paths.lock)
            .map_err(|error| control_io("open_diagnostic_lock", error))?;
        try_flock(&lock_file, FlockOperation::NonBlockingLockShared)?;
        validate_control_tree(&paths)?;
        Ok(Some(DiagnosticLock {
            _lock_file: lock_file,
            paths,
        }))
    }

    /// Creates or validates the control tree and acquires its nonblocking exclusive lock.
    ///
    /// # Errors
    ///
    /// Returns `TransactionBusy` for contention or a validation/I/O error.
    pub fn mutation_lock(&self) -> Result<MutationLock, FsError> {
        let paths = ControlPaths::new(self);
        create_or_validate_control_tree(&paths)?;
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.lock)
            .map_err(|error| control_io("open_mutation_lock", error))?;
        try_flock(&lock_file, FlockOperation::NonBlockingLockExclusive)?;
        validate_control_tree(&paths)?;
        let identities = capture_control_identities(&paths)?;
        Ok(MutationLock {
            _lock_file: lock_file,
            paths,
            identities,
        })
    }

    /// Lists validated recovery entries without creating control artifacts.
    ///
    /// # Errors
    ///
    /// Returns lock contention, corruption, control-tree, I/O, or limit errors.
    pub fn recovery_list(&self) -> Result<ControlObservation, FsError> {
        self.diagnostic_lock()?
            .map_or_else(|| Ok(ControlObservation::default()), |lock| lock.scan())
    }

    /// Returns one validated recovery entry without creating control artifacts.
    ///
    /// # Errors
    ///
    /// Returns not-found, contention, corruption, control-tree, I/O, or limit errors.
    pub fn recovery_status(&self, transaction_id: &str) -> Result<RecoveryEntry, FsError> {
        validate_transaction_id(transaction_id)?;
        self.recovery_list()?
            .find(transaction_id)
            .cloned()
            .ok_or_else(|| FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            })
    }

    /// Performs Phase 5's target-independent rollback cleanup for an orphan or
    /// manifest-only transaction.
    ///
    /// # Errors
    ///
    /// Returns not-found, contention, invalid-action, corruption, or I/O errors.
    pub fn recovery_rollback_control_only(&self, transaction_id: &str) -> Result<(), FsError> {
        validate_transaction_id(transaction_id)?;
        let Some(_) = self.diagnostic_lock()? else {
            return Err(FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            });
        };
        let lock = acquire_existing_mutation_lock(self)?;
        lock.rollback_control_only(transaction_id)
    }
}

pub(crate) fn acquire_existing_mutation_lock(
    workspace: &Workspace,
) -> Result<MutationLock, FsError> {
    let paths = ControlPaths::new(workspace);
    validate_control_tree(&paths)?;
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.lock)
        .map_err(|error| control_io("open_mutation_lock", error))?;
    try_flock(&lock_file, FlockOperation::NonBlockingLockExclusive)?;
    validate_control_tree(&paths)?;
    let identities = capture_control_identities(&paths)?;
    Ok(MutationLock {
        _lock_file: lock_file,
        paths,
        identities,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ControlIdentities {
    root: (u64, u64),
    control: (u64, u64),
    transactions: (u64, u64),
    completed: (u64, u64),
    lock: (u64, u64),
}

fn capture_control_identities(paths: &ControlPaths) -> Result<ControlIdentities, FsError> {
    let root = paths.control.parent().ok_or(FsError::InternalInvariant {
        invariant: "control_directory_has_workspace_parent",
    })?;
    Ok(ControlIdentities {
        root: object_identity(root)?,
        control: object_identity(&paths.control)?,
        transactions: object_identity(&paths.transactions)?,
        completed: object_identity(&paths.completed)?,
        lock: object_identity(&paths.lock)?,
    })
}

fn object_identity(path: &Path) -> Result<(u64, u64), FsError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| control_io("capture_control_identity", error))?;
    Ok((metadata.dev(), metadata.ino()))
}

fn create_or_validate_control_tree(paths: &ControlPaths) -> Result<(), FsError> {
    let control_created = create_directory_if_absent(&paths.control)?;
    let mut changed = control_created;
    changed |= create_directory_if_absent(&paths.transactions)?;
    changed |= create_directory_if_absent(&paths.completed)?;
    if !paths.lock.exists() {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&paths.lock)
        {
            Ok(file) => {
                file.sync_all()
                    .map_err(|error| control_io("sync_lock", error))?;
                changed = true;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(control_io("create_lock", error)),
        }
    }
    if changed {
        sync_directory(&paths.control)?;
    }
    if control_created {
        let workspace_root = paths.control.parent().ok_or(FsError::InternalInvariant {
            invariant: "control_directory_has_workspace_parent",
        })?;
        sync_directory(workspace_root)?;
    }
    validate_control_tree(paths)
}

fn create_directory_if_absent(path: &Path) -> Result<bool, FsError> {
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(control_io("create_control_directory", error)),
    }
}

fn validate_control_tree(paths: &ControlPaths) -> Result<(), FsError> {
    for path in [&paths.control, &paths.transactions, &paths.completed] {
        validate_owned_object(path, true)?;
    }
    validate_owned_object(&paths.lock, false)
}

fn validate_owned_object(path: &Path, directory: bool) -> Result<(), FsError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            FsError::ControlDirectoryInvalid {
                reason: "control_object_missing",
            }
        } else {
            control_io("inspect_control_object", error)
        }
    })?;
    let correct_type = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if metadata.file_type().is_symlink() || !correct_type {
        return Err(FsError::ControlDirectoryInvalid {
            reason: "control_object_type_invalid",
        });
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(FsError::ControlDirectoryInvalid {
            reason: "control_object_not_owned_by_user",
        });
    }
    if metadata.permissions().mode() & 0o022 != 0 {
        return Err(FsError::ControlDirectoryInvalid {
            reason: "control_object_group_or_other_writable",
        });
    }
    Ok(())
}

fn try_flock(file: &File, operation: FlockOperation) -> Result<(), FsError> {
    flock(file, operation).map_err(|error| {
        if error == Errno::AGAIN {
            FsError::TransactionBusy
        } else {
            control_io(
                "acquire_control_lock",
                io::Error::from_raw_os_error(error.raw_os_error()),
            )
        }
    })
}

fn scan_control(
    paths: &ControlPaths,
    limits: TransactionLimits,
) -> Result<ControlObservation, FsError> {
    validate_control_tree(paths)?;
    let mut cumulative_bytes = 0_u64;
    let mut scanned = 0_u64;
    let mut entries = BTreeMap::<String, RecoveryEntry>::new();
    for entry in read_directory_sorted(&paths.transactions)? {
        scanned = charge_directory(scanned, limits)?;
        let name = utf8_name(&entry)?;
        validate_transaction_id(&name)?;
        let path = entry.path();
        validate_transaction_directory(&path)?;
        let recovered = inspect_active_transaction(&path, &name, &mut cumulative_bytes, limits)?;
        if entries.insert(name.clone(), recovered).is_some() {
            return Err(corrupt(Some(&name), "duplicate_transaction_id"));
        }
    }
    for entry in read_directory_sorted(&paths.completed)? {
        scanned = charge_directory(scanned, limits)?;
        let name = utf8_name(&entry)?;
        let (id, terminal_state) =
            parse_completed_name(&name).ok_or_else(|| corrupt(None, "completed_name_invalid"))?;
        validate_transaction_directory(&entry.path())?;
        validate_completed_contents(&entry.path(), &id, &mut cumulative_bytes, limits)?;
        let recovered = RecoveryEntry {
            transaction_id: id.clone(),
            kind: RecoveryEntryKind::CleanupOnly,
            actions: vec!["status", "cleanup"],
            visibility: if terminal_state == "committed" {
                "all_planned"
            } else {
                "all_original"
            },
            active_path: None,
            completed_path: Some(entry.path()),
        };
        if entries.insert(id.clone(), recovered).is_some() {
            return Err(corrupt(Some(&id), "duplicate_transaction_id"));
        }
    }
    Ok(ControlObservation {
        entries: entries.into_values().collect(),
    })
}

fn inspect_active_transaction(
    path: &Path,
    id: &str,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<RecoveryEntry, FsError> {
    let children = read_directory_sorted(path)?;
    let names = children
        .iter()
        .map(utf8_name)
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !names.contains("manifest.rec") {
        if names.iter().any(|name| name != "manifest.tmp") {
            return Err(corrupt(Some(id), "orphan_record_unknown_entry"));
        }
        if names.contains("manifest.tmp") {
            let bytes = read_record_bounded(&path.join("manifest.tmp"), cumulative, limits)?;
            let decoded = decode_manifest_record_with_checksum(&bytes, Some(id), limits)?;
            if decoded.payload.transaction_id != id {
                return Err(corrupt(Some(id), "manifest_id_mismatch"));
            }
        }
        return Ok(RecoveryEntry {
            transaction_id: id.to_owned(),
            kind: RecoveryEntryKind::OrphanRecord,
            actions: vec!["status", "rollback"],
            visibility: "all_original",
            active_path: Some(path.to_path_buf()),
            completed_path: None,
        });
    }
    let manifest_bytes = read_record_bounded(&path.join("manifest.rec"), cumulative, limits)?;
    let manifest_record = decode_manifest_record_with_checksum(&manifest_bytes, Some(id), limits)?;
    let manifest = manifest_record.payload;
    if manifest.transaction_id != id {
        return Err(corrupt(Some(id), "manifest_id_mismatch"));
    }
    let state_names = names
        .iter()
        .filter_map(|name| parse_state_name(name, "rec").map(|sequence| (sequence, name.clone())))
        .collect::<BTreeMap<_, _>>();
    if state_names.is_empty() {
        validate_manifest_only_entries(
            path,
            &names,
            id,
            &manifest,
            manifest_record.checksum,
            cumulative,
            limits,
        )?;
        return Ok(RecoveryEntry {
            transaction_id: id.to_owned(),
            kind: RecoveryEntryKind::ManifestOnly,
            actions: vec!["status", "rollback"],
            visibility: "all_original",
            active_path: Some(path.to_path_buf()),
            completed_path: None,
        });
    }
    validate_active_names(path, &names, &manifest, id)?;
    let (last_state, last_checksum) = fold_state_chain(
        path,
        id,
        &manifest,
        manifest_record.checksum,
        &state_names,
        cumulative,
        limits,
    )?;
    validate_temporaries(
        path,
        id,
        &names,
        TemporaryContext {
            manifest: &manifest,
            manifest_checksum: manifest_record.checksum,
            last_state: &last_state,
            last_checksum: &last_checksum,
        },
        cumulative,
        limits,
    )?;
    let actions = actions_for_state(last_state.global_state);
    Ok(RecoveryEntry {
        transaction_id: id.to_owned(),
        kind: RecoveryEntryKind::Active,
        actions,
        visibility: visibility_for_state(last_state.global_state),
        active_path: Some(path.to_path_buf()),
        completed_path: None,
    })
}

fn fold_state_chain(
    path: &Path,
    id: &str,
    manifest: &Manifest,
    manifest_checksum: srcmv_core::Sha256Digest,
    state_names: &BTreeMap<u64, String>,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<(StateSnapshot, String), FsError> {
    let count = u64::try_from(state_names.len()).unwrap_or(u64::MAX);
    if count > limits.state_records {
        return Err(FsError::ResourceLimitExceeded {
            resource: "state_records",
            actual: count,
            limit: limits.state_records,
        });
    }
    let mut prior: Option<(StateSnapshot, String)> = None;
    let mut state_bytes = 0_u64;
    for expected in 0..count {
        let name = state_names
            .get(&expected)
            .ok_or_else(|| corrupt(Some(id), "state_chain_gap"))?;
        let before = *cumulative;
        let bytes = read_record_bounded(&path.join(name), cumulative, limits)?;
        state_bytes = state_bytes
            .checked_add(cumulative.saturating_sub(before))
            .ok_or_else(|| corrupt(Some(id), "state_bytes_overflow"))?;
        if state_bytes > limits.state_bytes {
            return Err(FsError::ResourceLimitExceeded {
                resource: "state_record_bytes",
                actual: state_bytes,
                limit: limits.state_bytes,
            });
        }
        let decoded = decode_state_record_with_checksum(&bytes, Some(id), limits)?;
        let state = decoded.payload;
        if state.sequence != expected
            || state.manifest_checksum != checksum_text(manifest_checksum)
            || state.targets.len() != manifest.targets.len()
        {
            return Err(corrupt(
                Some(id),
                "state_chain_manifest_or_sequence_mismatch",
            ));
        }
        validate_state_against_manifest(&state, manifest)?;
        if let Some((previous, prior_checksum)) = &prior {
            if state.prior_state_checksum.as_deref() != Some(prior_checksum) {
                return Err(corrupt(Some(id), "state_chain_fork"));
            }
            validate_state_transition(previous, &state)?;
        } else if state.sequence != 0
            || state.global_state != GlobalState::Preparing
            || state.prior_state_checksum.is_some()
        {
            return Err(corrupt(Some(id), "state_zero_invalid"));
        }
        prior = Some((state, checksum_text(decoded.checksum)));
    }
    prior.ok_or_else(|| corrupt(Some(id), "state_chain_empty"))
}

fn validate_manifest_only_entries(
    path: &Path,
    names: &BTreeSet<String>,
    id: &str,
    manifest: &Manifest,
    manifest_checksum: srcmv_core::Sha256Digest,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<(), FsError> {
    for name in names {
        if name == "manifest.rec" {
            continue;
        }
        if name == "state-00000000.tmp" {
            let bytes = read_record_bounded(&path.join(name), cumulative, limits)?;
            let state = decode_state_record_with_checksum(&bytes, Some(id), limits)?.payload;
            if state.sequence != 0
                || state.global_state != GlobalState::Preparing
                || state.manifest_checksum != checksum_text(manifest_checksum)
            {
                return Err(corrupt(Some(id), "manifest_only_temporary_invalid"));
            }
            validate_state_against_manifest(&state, manifest)?;
            continue;
        }
        return Err(corrupt(Some(id), "manifest_only_unknown_entry"));
    }
    Ok(())
}

fn validate_active_names(
    path: &Path,
    names: &BTreeSet<String>,
    manifest: &Manifest,
    id: &str,
) -> Result<(), FsError> {
    let authorized = manifest
        .targets
        .iter()
        .flat_map(|target| [&target.candidate_name, &target.backup_name])
        .collect::<BTreeSet<_>>();
    for name in names {
        let recognized = name == "manifest.rec"
            || name == "manifest.tmp"
            || parse_state_name(name, "rec").is_some()
            || parse_state_name(name, "tmp").is_some()
            || authorized.contains(name);
        if !recognized {
            return Err(corrupt(Some(id), "active_transaction_unknown_entry"));
        }
        if authorized.contains(name) {
            let metadata = fs::symlink_metadata(path.join(name))
                .map_err(|error| control_io("inspect_transaction_artifact", error))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(corrupt(Some(id), "transaction_artifact_not_regular"));
            }
        }
    }
    Ok(())
}

struct TemporaryContext<'a> {
    manifest: &'a Manifest,
    manifest_checksum: srcmv_core::Sha256Digest,
    last_state: &'a StateSnapshot,
    last_checksum: &'a str,
}

fn validate_temporaries(
    path: &Path,
    id: &str,
    names: &BTreeSet<String>,
    context: TemporaryContext<'_>,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<(), FsError> {
    if names.contains("manifest.tmp") {
        return Err(corrupt(Some(id), "published_manifest_has_temporary"));
    }
    let mut temporary_count = 0_u64;
    let expected_sequence = context
        .last_state
        .sequence
        .checked_add(1)
        .ok_or_else(|| corrupt(Some(id), "state_sequence_overflow"))?;
    for name in names {
        if let Some(sequence) = parse_state_name(name, "tmp") {
            temporary_count = temporary_count.saturating_add(1);
            let state = decode_state_record_with_checksum(
                &read_record_bounded(&path.join(name), cumulative, limits)?,
                Some(id),
                limits,
            )?
            .payload;
            if state.sequence != sequence
                || state.sequence != expected_sequence
                || state.manifest_checksum != checksum_text(context.manifest_checksum)
                || state.prior_state_checksum.as_deref() != Some(context.last_checksum)
            {
                return Err(corrupt(Some(id), "state_temporary_sequence_mismatch"));
            }
            validate_state_against_manifest(&state, context.manifest)?;
            validate_state_transition(context.last_state, &state)?;
        }
    }
    if temporary_count > 1 {
        return Err(corrupt(Some(id), "multiple_state_temporaries"));
    }
    Ok(())
}

fn actions_for_state(state: GlobalState) -> Vec<&'static str> {
    match state {
        GlobalState::Preparing => vec!["status", "rollback"],
        GlobalState::Prepared | GlobalState::Committing => vec!["status", "complete", "rollback"],
        GlobalState::Committed | GlobalState::RolledBack => vec!["status", "cleanup"],
        GlobalState::RollingBack => vec!["status", "rollback"],
    }
}

const fn visibility_for_state(state: GlobalState) -> &'static str {
    match state {
        GlobalState::Preparing | GlobalState::Prepared | GlobalState::RolledBack => "all_original",
        GlobalState::Committing | GlobalState::RollingBack => "mixed_old_new_possible",
        GlobalState::Committed => "all_planned",
    }
}

fn validate_completed_contents(
    path: &Path,
    id: &str,
    cumulative: &mut u64,
    limits: TransactionLimits,
) -> Result<(), FsError> {
    let names = read_directory_sorted(path)?
        .iter()
        .map(utf8_name)
        .collect::<Result<BTreeSet<_>, _>>()?;
    for name in &names {
        let recognized = name == "manifest.rec"
            || parse_state_name(name, "rec").is_some()
            || parse_state_name(name, "tmp").is_some()
            || is_artifact_name(name, "candidate")
            || is_artifact_name(name, "backup");
        if !recognized {
            return Err(corrupt(Some(id), "completed_transaction_unknown_entry"));
        }
        let child = path.join(name);
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| control_io("inspect_completed_entry", error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(corrupt(Some(id), "completed_entry_not_regular"));
        }
        if name == "manifest.rec" {
            decode_manifest_record_with_checksum(
                &read_record_bounded(&child, cumulative, limits)?,
                Some(id),
                limits,
            )?;
        } else if let Some(sequence) =
            parse_state_name(name, "rec").or_else(|| parse_state_name(name, "tmp"))
        {
            let state = decode_state_record_with_checksum(
                &read_record_bounded(&child, cumulative, limits)?,
                Some(id),
                limits,
            )?
            .payload;
            if state.sequence != sequence {
                return Err(corrupt(Some(id), "completed_state_sequence_mismatch"));
            }
        } else {
            *cumulative =
                cumulative
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
        }
    }
    Ok(())
}

fn cleanup_completed_directory(path: &Path, id: &str) -> Result<(), FsError> {
    let mut cumulative = 0;
    validate_completed_contents(path, id, &mut cumulative, TransactionLimits::default())?;
    remove_validated_children(path)?;
    fs::remove_dir(path).map_err(|error| control_io("remove_completed_directory", error))?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn remove_validated_children(path: &Path) -> Result<(), FsError> {
    for entry in read_directory_sorted(path)? {
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| control_io("inspect_cleanup_entry", error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(corrupt(None, "cleanup_entry_not_regular"));
        }
        fs::remove_file(entry.path()).map_err(|error| control_io("remove_cleanup_entry", error))?;
    }
    sync_directory(path)
}

fn read_directory_sorted(path: &Path) -> Result<Vec<fs::DirEntry>, FsError> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| control_io("read_control_directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| control_io("read_control_entry", error))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn validate_transaction_directory(path: &Path) -> Result<(), FsError> {
    validate_owned_object(path, true)?;
    let mode = fs::symlink_metadata(path)
        .map_err(|error| control_io("inspect_transaction_directory", error))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(FsError::ControlDirectoryInvalid {
            reason: "transaction_directory_not_private",
        });
    }
    Ok(())
}

fn charge_directory(current: u64, limits: TransactionLimits) -> Result<u64, FsError> {
    let actual = current.saturating_add(1);
    if actual > limits.transaction_directories {
        return Err(FsError::ResourceLimitExceeded {
            resource: "transaction_directories",
            actual,
            limit: limits.transaction_directories,
        });
    }
    Ok(actual)
}

fn parse_state_name(name: &str, extension: &str) -> Option<u64> {
    let digits = name
        .strip_prefix("state-")?
        .strip_suffix(&format!(".{extension}"))?;
    (digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
        .filter(|sequence| *sequence < crate::journal::MAX_STATE_RECORDS)
}

fn is_artifact_name(name: &str, prefix: &str) -> bool {
    let Some(digits) = name.strip_prefix(&format!("{prefix}-")) else {
        return false;
    };
    digits.len() == 8
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits
            .parse::<u64>()
            .is_ok_and(|index| index < crate::journal::MAX_TRANSACTION_TARGETS)
}

fn parse_completed_name(name: &str) -> Option<(String, &'static str)> {
    for (suffix, state) in [("-committed", "committed"), ("-rolledback", "rolledback")] {
        if let Some(id) = name.strip_suffix(suffix)
            && validate_transaction_id(id).is_ok()
        {
            return Some((id.to_owned(), state));
        }
    }
    None
}

fn validate_transaction_id(value: &str) -> Result<(), FsError> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "transaction_id_invalid",
        })
    }
}

fn utf8_name(entry: &fs::DirEntry) -> Result<String, FsError> {
    entry
        .file_name()
        .into_string()
        .map_err(|_| corrupt(None, "control_entry_name_not_utf8"))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn control_io(operation: &'static str, error: io::Error) -> FsError {
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
mod journal_control_tests {
    use std::cell::Cell;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn journal_transaction_id_generation_should_retry_active_and_completed_collisions() {
        let root = TempDir::new().expect("temporary workspace should be created");
        let workspace = Workspace::open(root.path()).expect("workspace should open");
        let lock = workspace.mutation_lock().expect("lock should succeed");
        let completed_collision = root
            .path()
            .join(".codesplice/completed/00000000000000000000000000000000-committed");
        fs::create_dir(&completed_collision).expect("collision fixture should be created");
        let calls = Cell::new(0_u8);

        let directory = lock
            .create_transaction_directory_with(|bytes| {
                let value = calls.get();
                calls.set(value.saturating_add(1));
                bytes.fill(value);
                Ok(())
            })
            .expect("second random identifier should allocate");

        assert_eq!(calls.get(), 2);
        assert_eq!(
            directory.transaction_id(),
            "01010101010101010101010101010101"
        );
    }

    #[test]
    fn journal_transaction_id_generation_should_stop_after_eight_collisions() {
        let root = TempDir::new().expect("temporary workspace should be created");
        let workspace = Workspace::open(root.path()).expect("workspace should open");
        let lock = workspace.mutation_lock().expect("lock should succeed");
        let collision = root
            .path()
            .join(".codesplice/completed/00000000000000000000000000000000-committed");
        fs::create_dir(collision).expect("collision fixture should be created");
        let calls = Cell::new(0_u64);

        let error = lock
            .create_transaction_directory_with(|bytes| {
                calls.set(calls.get().saturating_add(1));
                bytes.fill(0);
                Ok(())
            })
            .expect_err("bounded collision retries should fail");

        assert_eq!(calls.get(), 8);
        assert!(matches!(
            error,
            FsError::ResourceLimitExceeded {
                resource: "transaction_id_collision_attempts",
                ..
            }
        ));
    }
}
