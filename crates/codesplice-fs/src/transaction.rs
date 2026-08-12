//! Single-target transaction execution and conservative recovery.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use codesplice_core::{
    EditPlan, FileIdentity, OutputChange, OutputSegment, Sha256Digest, WorkspaceRelativePath,
    WorkspaceSnapshot,
};
use rustix::fs::{CWD, RenameFlags, renameat_with};
use rustix::io::Errno;
use sha2::{Digest, Sha256};

use crate::control::acquire_existing_mutation_lock;
use crate::journal::sync_directory;
use crate::{
    CandidateKind, CandidateState, CommitKind, CommitState, FsError, GlobalState,
    LocationObservation, Manifest, ManifestInput, ManifestSegment, ManifestTarget, MetadataPolicy,
    MutationLock, PersistedIdentity, RecoveryEntryKind, RequiredPathState, RollbackKind,
    RollbackState, SnapshotLimits, SnapshotRequirement, StateSnapshot, SyntheticTargetObservation,
    TargetState, TransactionDirectory, TransactionJournal, Workspace, classify_recovery,
    decode_manifest_record, decode_state_record,
};

const METADATA_LIMITATIONS: [&str; 7] = [
    "ownership",
    "access_control_lists",
    "extended_attributes",
    "resource_forks",
    "timestamps",
    "platform_flags",
    "hard_link_relationships",
];

/// Successful result of one changing single-target commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitOutcome {
    transaction_id: String,
    changed_path: String,
    preserved_mode: Option<u32>,
}

impl CommitOutcome {
    /// Canonical transaction identifier.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// The single changed workspace-relative path.
    #[must_use]
    pub fn changed_path(&self) -> &str {
        &self.changed_path
    }

    /// Permission bits preserved from an existing target, when applicable.
    #[must_use]
    pub const fn preserved_mode(&self) -> Option<u32> {
        self.preserved_mode
    }
}

/// Successful terminal result of explicit transaction recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryOutcome {
    transaction_id: String,
    state: &'static str,
}

impl RecoveryOutcome {
    /// Canonical transaction identifier.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Terminal recovery state: `committed` or `rolled_back`.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        self.state
    }
}

impl Workspace {
    /// Executes one changed target through the persistent transaction engine.
    ///
    /// The caller must hold `lock`, must have repeated planning while locked, and
    /// must pass the startup-umask-derived new-file mode.
    ///
    /// # Errors
    ///
    /// Returns a validation, journal, collision, recovery, or I/O error. Plans
    /// with zero or more than one changed target are rejected by this boundary.
    pub fn commit_single_target(
        &self,
        lock: &MutationLock,
        snapshot: &WorkspaceSnapshot,
        plan: &EditPlan,
        new_file_mode: u32,
    ) -> Result<CommitOutcome, FsError> {
        ensure_qualified_filesystem(self.canonical_root())?;
        let changed = plan
            .outputs
            .iter()
            .filter(|output| output.change != OutputChange::Unchanged)
            .collect::<Vec<_>>();
        if changed.len() != 1 {
            return Err(FsError::ResourceLimitExceeded {
                resource: "single_target_commit_targets",
                actual: u64::try_from(changed.len()).unwrap_or(u64::MAX),
                limit: 1,
            });
        }
        let directory = lock.create_transaction_directory()?;
        let transaction_id = directory.transaction_id().to_owned();
        let active_path = directory.path().to_path_buf();
        let manifest =
            match build_manifest(snapshot, plan, changed[0], &transaction_id, new_file_mode) {
                Ok(manifest) => manifest,
                Err(error) => {
                    lock.rollback_control_only(&transaction_id)?;
                    return Err(error);
                }
            };
        let result = execute_new_transaction(self, lock, directory, &manifest, snapshot);
        match result {
            Ok(outcome) => Ok(outcome),
            Err(original) => {
                if active_path.exists()
                    && recover_locked(self, lock, &transaction_id, RecoveryMode::Rollback).is_err()
                {
                    return Err(FsError::TransactionRecoveryRequired {
                        transaction_ids: vec![transaction_id],
                    });
                }
                Err(original)
            }
        }
    }

    /// Completes a validated single-target transaction or committed cleanup entry.
    ///
    /// # Errors
    ///
    /// Returns contention, corruption, action, conflict, no-replace, or I/O errors.
    pub fn recovery_complete(&self, transaction_id: &str) -> Result<RecoveryOutcome, FsError> {
        ensure_qualified_filesystem(self.canonical_root())?;
        if self.diagnostic_lock()?.is_none() {
            return Err(FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            });
        }
        let lock = acquire_existing_mutation_lock(self)?;
        recover_locked(self, &lock, transaction_id, RecoveryMode::Complete)
    }

    /// Rolls back a validated single-target transaction or rolled-back cleanup entry.
    ///
    /// # Errors
    ///
    /// Returns contention, corruption, action, conflict, no-replace, or I/O errors.
    pub fn recovery_rollback(&self, transaction_id: &str) -> Result<RecoveryOutcome, FsError> {
        ensure_qualified_filesystem(self.canonical_root())?;
        if self.diagnostic_lock()?.is_none() {
            return Err(FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            });
        }
        let lock = acquire_existing_mutation_lock(self)?;
        recover_locked(self, &lock, transaction_id, RecoveryMode::Rollback)
    }
}

fn build_manifest(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    output: &codesplice_core::PlannedOutput,
    transaction_id: &str,
    new_file_mode: u32,
) -> Result<Manifest, FsError> {
    let mut inputs = Vec::with_capacity(snapshot.files.len() + snapshot.absent_paths.len());
    for file in snapshot.files.iter() {
        inputs.push(ManifestInput {
            path: file.path.value.clone(),
            parent_identity: file.parent_identity.into(),
            existed: true,
            file_identity: Some(file.identity.into()),
            sha256: Some(file.digest.to_prefixed_hex()),
            length: Some(u64::try_from(file.bytes.len()).unwrap_or(u64::MAX)),
            link_count: Some(file.link_count),
        });
    }
    for absent in snapshot.absent_paths.iter() {
        inputs.push(ManifestInput {
            path: absent.path.value.clone(),
            parent_identity: absent.parent_identity.into(),
            existed: false,
            file_identity: None,
            sha256: None,
            length: None,
            link_count: None,
        });
    }

    let original = snapshot.files.iter().find(|file| file.path == output.path);
    let absent = snapshot
        .absent_paths
        .iter()
        .find(|path| path.path == output.path);
    let parent_identity = original
        .map(|file| file.parent_identity)
        .or_else(|| absent.map(|path| path.parent_identity))
        .ok_or(FsError::InternalInvariant {
            invariant: "changed_output_has_snapshot_input",
        })?;
    if parent_identity.device != snapshot.workspace_identity.device {
        return Err(FsError::CrossDeviceTransaction);
    }
    let segments = output
        .segments
        .iter()
        .map(|segment| match segment {
            OutputSegment::OriginalSlice {
                snapshot_file_id,
                range,
            } => ManifestSegment {
                input_index: snapshot_file_id.0,
                start: range.start,
                end: range.end,
                operation_index: None,
            },
            OutputSegment::PayloadSlice {
                operation_index,
                snapshot_file_id,
                range,
                ..
            } => ManifestSegment {
                input_index: snapshot_file_id.0,
                start: range.start,
                end: range.end,
                operation_index: Some(*operation_index),
            },
        })
        .collect();
    let target = ManifestTarget {
        target_index: 0,
        path: output.path.value.clone(),
        parent_identity: parent_identity.into(),
        original_existed: original.is_some(),
        original_identity: original.map(|file| file.identity.into()),
        original_sha256: original.map(|file| file.digest.to_prefixed_hex()),
        original_length: original.map(|file| u64::try_from(file.bytes.len()).unwrap_or(u64::MAX)),
        candidate_name: "candidate-00000000".to_owned(),
        backup_name: "backup-00000000".to_owned(),
        candidate_sha256: output.resulting_digest.to_prefixed_hex(),
        candidate_length: output.resulting_length,
        metadata_policy: if original.is_some() {
            MetadataPolicy::PreserveExistingMode
        } else {
            MetadataPolicy::NewFileMode
        },
        new_file_mode: original.is_none().then_some(new_file_mode & 0o666),
        segments,
    };
    Ok(Manifest {
        transaction_version: 1,
        transaction_id: transaction_id.to_owned(),
        workspace_identity: snapshot.workspace_identity.into(),
        plan_sha256: plan.digest.0.to_prefixed_hex(),
        inputs,
        targets: vec![target],
        metadata_limitations: METADATA_LIMITATIONS
            .iter()
            .map(ToString::to_string)
            .collect(),
    })
}

fn execute_new_transaction(
    workspace: &Workspace,
    lock: &MutationLock,
    directory: TransactionDirectory,
    manifest: &Manifest,
    snapshot: &WorkspaceSnapshot,
) -> Result<CommitOutcome, FsError> {
    let active_path = directory.path().to_path_buf();
    let mut journal = TransactionJournal::create(directory, manifest)?;
    let mut progress = Progress {
        state: StateSnapshot {
            transaction_version: 1,
            sequence: 0,
            manifest_checksum: journal.manifest_checksum().to_owned(),
            prior_state_checksum: None,
            global_state: GlobalState::Preparing,
            targets: vec![untouched_target_state()],
        },
        checksum: String::new(),
    };
    progress.checksum = journal.publish_state(&progress.state)?;
    let candidate_identity = create_candidate(&active_path, &manifest.targets[0], snapshot)?;
    publish_transition(
        &mut journal,
        &mut progress,
        GlobalState::Prepared,
        |target| {
            target.candidate = CandidateState {
                kind: CandidateKind::Ready,
                identity: Some(candidate_identity),
            };
        },
    )?;
    revalidate_manifest_inputs(workspace, manifest)?;
    lock.revalidate_control_identities()?;
    publish_transition(&mut journal, &mut progress, GlobalState::Committing, |_| {})?;
    complete_commit_steps(
        workspace,
        lock,
        manifest,
        &active_path,
        &mut journal,
        &mut progress,
    )?;
    let preserved_mode = progress.state.targets[0].commit.preserved_mode;
    lock.finish_transaction(&manifest.transaction_id, &active_path, true)?;
    Ok(CommitOutcome {
        transaction_id: manifest.transaction_id.clone(),
        changed_path: manifest.targets[0].path.clone(),
        preserved_mode,
    })
}

fn create_candidate(
    transaction_path: &Path,
    target: &ManifestTarget,
    snapshot: &WorkspaceSnapshot,
) -> Result<PersistedIdentity, FsError> {
    let path = transaction_path.join(&target.candidate_name);
    crate::test_failpoint("before_candidate_create")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| transaction_io("create_candidate", Some(&target.path), error))?;
    crate::test_failpoint("after_candidate_create")?;
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    crate::test_failpoint("before_candidate_write")?;
    for segment in &target.segments {
        let input = usize::try_from(segment.input_index)
            .ok()
            .and_then(|index| snapshot.files.get(index))
            .ok_or(FsError::InternalInvariant {
                invariant: "candidate_segment_input_exists",
            })?;
        let start = usize::try_from(segment.start).map_err(|_| FsError::InternalInvariant {
            invariant: "candidate_segment_start_fits_usize",
        })?;
        let end = usize::try_from(segment.end).map_err(|_| FsError::InternalInvariant {
            invariant: "candidate_segment_end_fits_usize",
        })?;
        let bytes = input
            .bytes
            .get(start..end)
            .ok_or(FsError::InternalInvariant {
                invariant: "candidate_segment_range_valid",
            })?;
        file.write_all(bytes)
            .map_err(|error| transaction_io("write_candidate", Some(&target.path), error))?;
        hasher.update(bytes);
        length = length
            .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .ok_or(FsError::InternalInvariant {
                invariant: "candidate_length_checked",
            })?;
    }
    crate::test_failpoint("after_candidate_write")?;
    file.flush()
        .map_err(|error| transaction_io("flush_candidate", Some(&target.path), error))?;
    crate::test_failpoint("before_candidate_sync")?;
    file.sync_all()
        .map_err(|error| transaction_io("sync_candidate", Some(&target.path), error))?;
    crate::test_failpoint("after_candidate_sync")?;
    drop(file);
    let digest = Sha256Digest(hasher.finalize().into());
    if length != target.candidate_length || digest.to_prefixed_hex() != target.candidate_sha256 {
        return Err(FsError::InternalInvariant {
            invariant: "candidate_matches_planned_output",
        });
    }
    crate::test_failpoint("before_candidate_verification")?;
    let fingerprint = fingerprint(&path, Some(&target.path))?.ok_or(FsError::RecoveryConflict {
        reason: "candidate_missing_after_creation",
    })?;
    if fingerprint.length != length || fingerprint.digest != digest {
        return Err(FsError::RecoveryConflict {
            reason: "candidate_verification_mismatch",
        });
    }
    crate::test_failpoint("after_candidate_verification")?;
    sync_directory(transaction_path)?;
    Ok(fingerprint.identity.into())
}

fn complete_commit_steps(
    workspace: &Workspace,
    lock: &MutationLock,
    manifest: &Manifest,
    active_path: &Path,
    journal: &mut TransactionJournal,
    progress: &mut Progress,
) -> Result<(), FsError> {
    let target = &manifest.targets[0];
    let paths = target_paths(workspace, active_path, target)?;
    let observed = observe_target(target, &paths, &progress.state)?;
    let disposition = classify_recovery(
        progress.state.global_state,
        target.original_existed,
        progress.state.targets[0],
        observed,
    )?;
    if !disposition.complete {
        return Err(FsError::RecoveryActionNotAllowed {
            transaction_id: manifest.transaction_id.clone(),
            reason: "transaction_cannot_be_completed",
        });
    }
    lock.revalidate_control_identities()?;

    if observed.target == LocationObservation::Candidate {
        let final_file =
            verify_candidate_location(&paths.target, target, progress.state.targets[0])?;
        publish_installed(journal, progress, final_file.identity.into())?;
    } else {
        if target.original_existed {
            let preserved_mode = if observed.backup == LocationObservation::Original {
                mode_of(&paths.backup, &target.path)?
            } else {
                crate::test_failpoint("before_backup_rename")?;
                no_replace_rename(&paths.target, &paths.backup, "backup_target")?;
                sync_directory(&paths.parent)?;
                sync_directory(active_path)?;
                crate::test_failpoint("after_backup_rename")?;
                verify_original_location(&paths.backup, target)?;
                mode_of(&paths.backup, &target.path)?
            };
            set_mode(&paths.candidate, preserved_mode, &target.path)?;
            if progress.state.targets[0].commit.kind == CommitKind::Untouched {
                let backup = verify_original_location(&paths.backup, target)?;
                publish_transition(journal, progress, GlobalState::Committing, |state| {
                    state.commit = CommitState {
                        kind: CommitKind::BackedUp,
                        identity: Some(backup.identity.into()),
                        preserved_mode: Some(preserved_mode),
                    };
                })?;
            }
        } else {
            let mode = target.new_file_mode.ok_or(FsError::InternalInvariant {
                invariant: "new_target_has_recorded_mode",
            })?;
            set_mode(&paths.candidate, mode, &target.path)?;
        }
        crate::test_failpoint("before_install_rename")?;
        no_replace_rename(&paths.candidate, &paths.target, "install_candidate")?;
        sync_directory(&paths.parent)?;
        sync_directory(active_path)?;
        crate::test_failpoint("after_install_rename")?;
        let final_file =
            verify_candidate_location(&paths.target, target, progress.state.targets[0])?;
        publish_installed(journal, progress, final_file.identity.into())?;
    }
    crate::test_failpoint("before_final_verification")?;
    verify_candidate_location(&paths.target, target, progress.state.targets[0])?;
    crate::test_failpoint("after_final_verification")?;
    publish_transition(journal, progress, GlobalState::Committed, |_| {})?;
    Ok(())
}

fn publish_installed(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    identity: PersistedIdentity,
) -> Result<(), FsError> {
    if progress.state.targets[0].commit.kind != CommitKind::Installed {
        publish_transition(journal, progress, GlobalState::Committing, |target| {
            target.commit.kind = CommitKind::Installed;
            target.commit.identity = Some(identity);
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RecoveryMode {
    Complete,
    Rollback,
}

fn recover_locked(
    workspace: &Workspace,
    lock: &MutationLock,
    transaction_id: &str,
    mode: RecoveryMode,
) -> Result<RecoveryOutcome, FsError> {
    let entry = lock.recovery_entry(transaction_id)?;
    if entry.kind() == RecoveryEntryKind::CleanupOnly {
        let name = entry
            .completed_path()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .ok_or(FsError::TransactionRecordCorrupt {
                transaction_id: Some(transaction_id.to_owned()),
                reason: "completed_name_invalid",
            })?;
        let committed = name.ends_with("-committed");
        if committed != matches!(mode, RecoveryMode::Complete) {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: transaction_id.to_owned(),
                reason: "terminal_cleanup_action_mismatch",
            });
        }
        lock.cleanup_completed(&entry)?;
        return Ok(RecoveryOutcome {
            transaction_id: transaction_id.to_owned(),
            state: if committed {
                "committed"
            } else {
                "rolled_back"
            },
        });
    }
    if matches!(
        entry.kind(),
        RecoveryEntryKind::OrphanRecord | RecoveryEntryKind::ManifestOnly
    ) {
        if matches!(mode, RecoveryMode::Complete) {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: transaction_id.to_owned(),
                reason: "unpublished_transaction_cannot_be_completed",
            });
        }
        lock.rollback_control_only(transaction_id)?;
        return Ok(RecoveryOutcome {
            transaction_id: transaction_id.to_owned(),
            state: "rolled_back",
        });
    }

    let active_path = entry
        .active_path()
        .ok_or(FsError::InternalInvariant {
            invariant: "active_recovery_entry_has_path",
        })?
        .to_path_buf();
    let mut loaded = load_active_transaction(transaction_id, active_path.clone())?;
    if loaded.manifest.targets.len() != 1 {
        return Err(FsError::RecoveryActionNotAllowed {
            transaction_id: transaction_id.to_owned(),
            reason: "phase_7_requires_single_target_transaction",
        });
    }
    match mode {
        RecoveryMode::Complete => {
            recover_complete_active(workspace, lock, &active_path, &mut loaded)?;
            Ok(RecoveryOutcome {
                transaction_id: transaction_id.to_owned(),
                state: "committed",
            })
        }
        RecoveryMode::Rollback => {
            recover_rollback_active(workspace, lock, &active_path, &mut loaded)?;
            Ok(RecoveryOutcome {
                transaction_id: transaction_id.to_owned(),
                state: "rolled_back",
            })
        }
    }
}

fn recover_complete_active(
    workspace: &Workspace,
    lock: &MutationLock,
    active_path: &Path,
    loaded: &mut LoadedTransaction,
) -> Result<(), FsError> {
    match loaded.progress.state.global_state {
        GlobalState::Preparing | GlobalState::RollingBack | GlobalState::RolledBack => {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: loaded.manifest.transaction_id.clone(),
                reason: "recorded_state_cannot_be_completed",
            });
        }
        GlobalState::Committed => {
            let target = &loaded.manifest.targets[0];
            let paths = target_paths(workspace, active_path, target)?;
            let observed = observe_target(target, &paths, &loaded.progress.state)?;
            classify_recovery(
                GlobalState::Committed,
                target.original_existed,
                loaded.progress.state.targets[0],
                observed,
            )?;
        }
        GlobalState::Prepared => {
            revalidate_manifest_inputs(workspace, &loaded.manifest)?;
            lock.revalidate_control_identities()?;
            publish_transition(
                &mut loaded.journal,
                &mut loaded.progress,
                GlobalState::Committing,
                |_| {},
            )?;
            complete_commit_steps(
                workspace,
                lock,
                &loaded.manifest,
                active_path,
                &mut loaded.journal,
                &mut loaded.progress,
            )?;
        }
        GlobalState::Committing => {
            complete_commit_steps(
                workspace,
                lock,
                &loaded.manifest,
                active_path,
                &mut loaded.journal,
                &mut loaded.progress,
            )?;
        }
    }
    lock.finish_transaction(&loaded.manifest.transaction_id, active_path, true)
}

fn recover_rollback_active(
    workspace: &Workspace,
    lock: &MutationLock,
    active_path: &Path,
    loaded: &mut LoadedTransaction,
) -> Result<(), FsError> {
    match loaded.progress.state.global_state {
        GlobalState::Committed => {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: loaded.manifest.transaction_id.clone(),
                reason: "committed_transaction_cannot_be_rolled_back",
            });
        }
        GlobalState::RolledBack => {
            return lock.finish_transaction(&loaded.manifest.transaction_id, active_path, false);
        }
        GlobalState::Preparing
        | GlobalState::Prepared
        | GlobalState::Committing
        | GlobalState::RollingBack => {}
    }
    let target = &loaded.manifest.targets[0];
    let paths = target_paths(workspace, active_path, target)?;
    let observed = observe_target(target, &paths, &loaded.progress.state)?;
    let disposition = classify_recovery(
        loaded.progress.state.global_state,
        target.original_existed,
        loaded.progress.state.targets[0],
        observed,
    )?;
    if !disposition.rollback {
        return Err(FsError::RecoveryActionNotAllowed {
            transaction_id: loaded.manifest.transaction_id.clone(),
            reason: "transaction_cannot_be_rolled_back",
        });
    }
    if loaded.progress.state.global_state != GlobalState::RollingBack {
        publish_transition(
            &mut loaded.journal,
            &mut loaded.progress,
            GlobalState::RollingBack,
            |_| {},
        )?;
    }
    lock.revalidate_control_identities()?;
    crate::test_failpoint("before_rollback_target_step")?;
    if observed.target == LocationObservation::Candidate {
        verify_candidate_location(&paths.target, target, loaded.progress.state.targets[0])?;
        fs::remove_file(&paths.target).map_err(|error| {
            transaction_io("remove_installed_candidate", Some(&target.path), error)
        })?;
        sync_directory(&paths.parent)?;
    }
    crate::test_failpoint("after_rollback_target_step")?;
    crate::test_failpoint("before_rollback_restore_step")?;
    if target.original_existed && paths.backup.exists() {
        verify_original_location(&paths.backup, target)?;
        no_replace_rename(&paths.backup, &paths.target, "restore_backup")?;
        sync_directory(&paths.parent)?;
        sync_directory(active_path)?;
    }
    crate::test_failpoint("after_rollback_restore_step")?;
    crate::test_failpoint("before_rollback_candidate_cleanup")?;
    if paths.candidate.exists() {
        if loaded.progress.state.targets[0].candidate.kind == CandidateKind::Ready {
            verify_candidate_location(&paths.candidate, target, loaded.progress.state.targets[0])?;
        } else {
            validate_partial_candidate(&paths.candidate)?;
        }
        fs::remove_file(&paths.candidate).map_err(|error| {
            transaction_io("remove_staged_candidate", Some(&target.path), error)
        })?;
        sync_directory(active_path)?;
    }
    crate::test_failpoint("after_rollback_candidate_cleanup")?;

    let restored = if target.original_existed {
        let original = verify_original_location(&paths.target, target)?;
        if paths.backup.exists() {
            return Err(FsError::RecoveryConflict {
                reason: "backup_remains_after_original_restore",
            });
        }
        RollbackState {
            kind: RollbackKind::OriginalRestored,
            identity: Some(original.identity.into()),
        }
    } else {
        if paths.target.exists() || paths.backup.exists() {
            return Err(FsError::RecoveryConflict {
                reason: "absence_not_restored",
            });
        }
        RollbackState {
            kind: RollbackKind::AbsenceRestored,
            identity: None,
        }
    };
    crate::test_failpoint("before_rollback_verification")?;
    crate::test_failpoint("after_rollback_verification")?;
    if loaded.progress.state.targets[0].rollback.kind == RollbackKind::None {
        publish_transition(
            &mut loaded.journal,
            &mut loaded.progress,
            GlobalState::RollingBack,
            |state| state.rollback = restored,
        )?;
    }
    publish_transition(
        &mut loaded.journal,
        &mut loaded.progress,
        GlobalState::RolledBack,
        |_| {},
    )?;
    lock.finish_transaction(&loaded.manifest.transaction_id, active_path, false)
}

fn revalidate_manifest_inputs(workspace: &Workspace, manifest: &Manifest) -> Result<(), FsError> {
    if PersistedIdentity::from(workspace.identity()) != manifest.workspace_identity {
        return Err(FsError::PreconditionFailed {
            path: ".".to_owned(),
            expected: None,
            actual: None,
        });
    }
    let requirements = manifest
        .inputs
        .iter()
        .map(|input| {
            let state = if input.existed {
                RequiredPathState::Existing(parse_digest(input.sha256.as_deref().ok_or(
                    FsError::InternalInvariant {
                        invariant: "existing_manifest_input_has_digest",
                    },
                )?)?)
            } else {
                RequiredPathState::Absent
            };
            Ok(SnapshotRequirement {
                path: WorkspaceRelativePath {
                    value: input.path.clone(),
                },
                state,
            })
        })
        .collect::<Result<Vec<_>, FsError>>()?;
    let fresh = workspace.acquire_snapshot(&requirements, SnapshotLimits::default())?;
    for input in &manifest.inputs {
        if input.existed {
            let file = fresh
                .files
                .iter()
                .find(|file| file.path.value == input.path)
                .ok_or(FsError::PreconditionFailed {
                    path: input.path.clone(),
                    expected: input.sha256.as_deref().map(parse_digest).transpose()?,
                    actual: None,
                })?;
            let exact = PersistedIdentity::from(file.parent_identity) == input.parent_identity
                && input.file_identity == Some(file.identity.into())
                && input.sha256.as_deref() == Some(&file.digest.to_prefixed_hex())
                && input.length == Some(u64::try_from(file.bytes.len()).unwrap_or(u64::MAX))
                && input.link_count == Some(file.link_count);
            if !exact {
                return Err(FsError::FileChanged {
                    path: input.path.clone(),
                    attempts: 1,
                });
            }
        } else {
            let absent = fresh
                .absent_paths
                .iter()
                .find(|path| path.path.value == input.path)
                .ok_or(FsError::PreconditionFailed {
                    path: input.path.clone(),
                    expected: None,
                    actual: None,
                })?;
            if PersistedIdentity::from(absent.parent_identity) != input.parent_identity {
                return Err(FsError::FileChanged {
                    path: input.path.clone(),
                    attempts: 1,
                });
            }
        }
    }
    Ok(())
}

struct Progress {
    state: StateSnapshot,
    checksum: String,
}

fn publish_transition(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    global_state: GlobalState,
    update: impl FnOnce(&mut TargetState),
) -> Result<(), FsError> {
    let mut next = progress.state.clone();
    next.sequence = next
        .sequence
        .checked_add(1)
        .ok_or(FsError::InternalInvariant {
            invariant: "state_sequence_checked",
        })?;
    next.prior_state_checksum = Some(progress.checksum.clone());
    next.global_state = global_state;
    update(&mut next.targets[0]);
    let checksum = journal.publish_state(&next)?;
    progress.state = next;
    progress.checksum = checksum;
    Ok(())
}

const fn untouched_target_state() -> TargetState {
    TargetState {
        target_index: 0,
        candidate: CandidateState {
            kind: CandidateKind::Missing,
            identity: None,
        },
        commit: CommitState {
            kind: CommitKind::Untouched,
            identity: None,
            preserved_mode: None,
        },
        rollback: RollbackState {
            kind: RollbackKind::None,
            identity: None,
        },
    }
}

struct LoadedTransaction {
    manifest: Manifest,
    journal: TransactionJournal,
    progress: Progress,
}

fn load_active_transaction(
    transaction_id: &str,
    active_path: PathBuf,
) -> Result<LoadedTransaction, FsError> {
    let manifest = decode_manifest_record(
        &fs::read(active_path.join("manifest.rec"))
            .map_err(|error| transaction_io("read_recovery_manifest", None, error))?,
    )?;
    if manifest.transaction_id != transaction_id {
        return Err(FsError::TransactionRecordCorrupt {
            transaction_id: Some(transaction_id.to_owned()),
            reason: "manifest_id_mismatch",
        });
    }
    let mut records = fs::read_dir(&active_path)
        .map_err(|error| transaction_io("read_recovery_transaction", None, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| transaction_io("read_recovery_entry", None, error))?;
    records.sort_by_key(fs::DirEntry::file_name);
    let mut last: Option<(StateSnapshot, String)> = None;
    let mut state_record_bytes = 0_u64;
    for record in records {
        let name = record.file_name();
        let Some(name) = name.to_str() else {
            return Err(FsError::TransactionRecordCorrupt {
                transaction_id: Some(transaction_id.to_owned()),
                reason: "control_entry_name_not_utf8",
            });
        };
        if name.starts_with("state-") && name.ends_with(".tmp") {
            fs::remove_file(record.path())
                .map_err(|error| transaction_io("remove_state_temporary", None, error))?;
            continue;
        }
        if !(name.starts_with("state-") && name.ends_with(".rec")) {
            continue;
        }
        let bytes = fs::read(record.path())
            .map_err(|error| transaction_io("read_recovery_state", None, error))?;
        state_record_bytes = state_record_bytes
            .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "state_record_bytes",
                actual: u64::MAX,
                limit: crate::journal::MAX_STATE_BYTES,
            })?;
        let state = decode_state_record(&bytes)?;
        let checksum = record_checksum_text(&bytes)?;
        last = Some((state, checksum));
    }
    sync_directory(&active_path)?;
    let (state, checksum) = last.ok_or(FsError::TransactionRecordCorrupt {
        transaction_id: Some(transaction_id.to_owned()),
        reason: "state_chain_empty",
    })?;
    let directory = TransactionDirectory::from_recovery(transaction_id.to_owned(), active_path);
    let journal = TransactionJournal::resume(
        directory,
        &manifest,
        state.clone(),
        checksum.clone(),
        state_record_bytes,
    )?;
    Ok(LoadedTransaction {
        manifest,
        journal,
        progress: Progress { state, checksum },
    })
}

fn record_checksum_text(bytes: &[u8]) -> Result<String, FsError> {
    let checksum: [u8; 32] = bytes
        .get(
            bytes
                .len()
                .checked_sub(32)
                .ok_or(FsError::TransactionRecordCorrupt {
                    transaction_id: None,
                    reason: "record_truncated",
                })?..,
        )
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "record_checksum_truncated",
        })?;
    Ok(Sha256Digest(checksum).to_prefixed_hex())
}

struct TargetPaths {
    target: PathBuf,
    parent: PathBuf,
    candidate: PathBuf,
    backup: PathBuf,
}

fn target_paths(
    workspace: &Workspace,
    active_path: &Path,
    target: &ManifestTarget,
) -> Result<TargetPaths, FsError> {
    let relative = WorkspaceRelativePath {
        value: target.path.clone(),
    };
    let validated = workspace.validate_path(&relative)?;
    if PersistedIdentity::from(validated.parent_identity) != target.parent_identity {
        return Err(FsError::RecoveryConflict {
            reason: "target_parent_identity_changed",
        });
    }
    if validated.parent_identity.device != workspace.identity().device {
        return Err(FsError::CrossDeviceTransaction);
    }
    Ok(TargetPaths {
        target: validated.full_path,
        parent: validated.parent_path,
        candidate: active_path.join(&target.candidate_name),
        backup: active_path.join(&target.backup_name),
    })
}

fn observe_target(
    target: &ManifestTarget,
    paths: &TargetPaths,
    state: &StateSnapshot,
) -> Result<SyntheticTargetObservation, FsError> {
    let recorded = state.targets[0];
    Ok(SyntheticTargetObservation {
        parent_matches: true,
        target: classify_location(&paths.target, target, recorded, false, state.global_state)?,
        candidate: classify_location(&paths.candidate, target, recorded, true, state.global_state)?,
        backup: classify_backup(&paths.backup, target)?,
    })
}

fn classify_location(
    path: &Path,
    target: &ManifestTarget,
    state: TargetState,
    candidate_location: bool,
    global: GlobalState,
) -> Result<LocationObservation, FsError> {
    let Some(actual) = fingerprint(path, Some(&target.path))? else {
        return Ok(LocationObservation::Absent);
    };
    if target.original_existed && matches_original(&actual, target)? {
        return Ok(LocationObservation::Original);
    }
    if state.candidate.kind == CandidateKind::Ready && matches_candidate(&actual, target, state)? {
        return Ok(LocationObservation::Candidate);
    }
    if candidate_location && global == GlobalState::Preparing {
        return Ok(LocationObservation::Candidate);
    }
    Ok(LocationObservation::Unexpected)
}

fn classify_backup(path: &Path, target: &ManifestTarget) -> Result<LocationObservation, FsError> {
    let Some(actual) = fingerprint(path, Some(&target.path))? else {
        return Ok(LocationObservation::Absent);
    };
    if target.original_existed && matches_original(&actual, target)? {
        Ok(LocationObservation::Original)
    } else {
        Ok(LocationObservation::Unexpected)
    }
}

fn verify_original_location(path: &Path, target: &ManifestTarget) -> Result<Fingerprint, FsError> {
    let actual = fingerprint(path, Some(&target.path))?.ok_or(FsError::RecoveryConflict {
        reason: "recorded_original_missing",
    })?;
    if matches_original(&actual, target)? {
        Ok(actual)
    } else {
        Err(FsError::RecoveryConflict {
            reason: "recorded_original_mismatch",
        })
    }
}

fn verify_candidate_location(
    path: &Path,
    target: &ManifestTarget,
    state: TargetState,
) -> Result<Fingerprint, FsError> {
    let actual = fingerprint(path, Some(&target.path))?.ok_or(FsError::RecoveryConflict {
        reason: "recorded_candidate_missing",
    })?;
    if matches_candidate(&actual, target, state)? {
        Ok(actual)
    } else {
        Err(FsError::RecoveryConflict {
            reason: "recorded_candidate_mismatch",
        })
    }
}

fn matches_original(actual: &Fingerprint, target: &ManifestTarget) -> Result<bool, FsError> {
    Ok(target.original_identity == Some(actual.identity.into())
        && target.original_length == Some(actual.length)
        && target.original_sha256.as_deref() == Some(&actual.digest.to_prefixed_hex()))
}

fn matches_candidate(
    actual: &Fingerprint,
    target: &ManifestTarget,
    state: TargetState,
) -> Result<bool, FsError> {
    Ok(state.candidate.identity == Some(actual.identity.into())
        && target.candidate_length == actual.length
        && target.candidate_sha256 == actual.digest.to_prefixed_hex())
}

fn validate_partial_candidate(path: &Path) -> Result<(), FsError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| transaction_io("inspect_partial_candidate", None, error))?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(FsError::RecoveryConflict {
            reason: "partial_candidate_type_invalid",
        })
    }
}

#[derive(Clone, Copy)]
struct Fingerprint {
    identity: FileIdentity,
    length: u64,
    digest: Sha256Digest,
}

fn fingerprint(path: &Path, report_path: Option<&str>) -> Result<Option<Fingerprint>, FsError> {
    let entry = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(transaction_io(
                "inspect_transaction_location",
                report_path,
                error,
            ));
        }
    };
    if !entry.is_file() || entry.file_type().is_symlink() {
        return Err(FsError::RecoveryConflict {
            reason: "transaction_location_type_invalid",
        });
    }
    let mut file = File::open(path)
        .map_err(|error| transaction_io("open_transaction_location", report_path, error))?;
    let before = file
        .metadata()
        .map_err(|error| transaction_io("inspect_open_transaction_location", report_path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut length = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| transaction_io("read_transaction_location", report_path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length = length
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or(FsError::ResourceLimitExceeded {
                resource: "recovery_bytes",
                actual: u64::MAX,
                limit: crate::control::MAX_RECOVERY_BYTES,
            })?;
    }
    let after = file
        .metadata()
        .map_err(|error| transaction_io("reinspect_transaction_location", report_path, error))?;
    let entry_after = fs::symlink_metadata(path)
        .map_err(|error| transaction_io("confirm_transaction_location", report_path, error))?;
    let identity = FileIdentity {
        device: before.dev(),
        inode: before.ino(),
    };
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || identity
            != (FileIdentity {
                device: entry_after.dev(),
                inode: entry_after.ino(),
            })
        || length != before.len()
    {
        return Err(FsError::RecoveryConflict {
            reason: "transaction_location_changed_during_read",
        });
    }
    Ok(Some(Fingerprint {
        identity,
        length,
        digest: Sha256Digest(hasher.finalize().into()),
    }))
}

fn mode_of(path: &Path, report_path: &str) -> Result<u32, FsError> {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .map_err(|error| transaction_io("read_target_mode", Some(report_path), error))
}

fn set_mode(path: &Path, mode: u32, report_path: &str) -> Result<(), FsError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|error| transaction_io("set_candidate_mode", Some(report_path), error))?;
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| transaction_io("sync_candidate_mode", Some(report_path), error))
}

fn no_replace_rename(
    source: &Path,
    destination: &Path,
    operation: &'static str,
) -> Result<(), FsError> {
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(
        |error| match error {
            Errno::EXIST => FsError::RecoveryConflict {
                reason: "no_replace_destination_collision",
            },
            Errno::XDEV => FsError::CrossDeviceTransaction,
            Errno::NOSYS | Errno::NOTSUP | Errno::INVAL => FsError::NoReplaceUnavailable,
            _ => transaction_io(
                operation,
                None,
                io::Error::from_raw_os_error(error.raw_os_error()),
            ),
        },
    )
}

fn ensure_qualified_filesystem(path: &Path) -> Result<(), FsError> {
    let directory = File::open(path)
        .map_err(|error| transaction_io("open_workspace_filesystem", None, error))?;
    let statistics = rustix::fs::fstatfs(&directory).map_err(|error| {
        transaction_io(
            "inspect_workspace_filesystem",
            None,
            io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })?;
    #[cfg(target_os = "linux")]
    {
        const EXT4_SUPER_MAGIC: i64 = 0xef53;
        let filesystem = statistics.f_type as i64;
        if filesystem == EXT4_SUPER_MAGIC {
            Ok(())
        } else {
            Err(FsError::UnsupportedFilesystem {
                filesystem: format!("0x{filesystem:x}"),
            })
        }
    }
    #[cfg(target_os = "macos")]
    {
        let bytes = statistics
            .f_fstypename
            .iter()
            .copied()
            .take_while(|byte| *byte != 0)
            .map(|byte| byte as u8)
            .collect::<Vec<_>>();
        let filesystem = String::from_utf8(bytes).unwrap_or_else(|_| "non_utf8".to_owned());
        if filesystem == "apfs" {
            Ok(())
        } else {
            Err(FsError::UnsupportedFilesystem { filesystem })
        }
    }
}

fn parse_digest(value: &str) -> Result<Sha256Digest, FsError> {
    let hex = value
        .strip_prefix("sha256:")
        .ok_or(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "record_digest_invalid",
        })?;
    if hex.len() != 64 {
        return Err(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "record_digest_invalid",
        });
    }
    let mut bytes = [0_u8; 32];
    for (index, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_digit(chunk[0]).ok_or(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "record_digest_invalid",
        })?;
        let low = hex_digit(chunk[1]).ok_or(FsError::TransactionRecordCorrupt {
            transaction_id: None,
            reason: "record_digest_invalid",
        })?;
        bytes[index] = (high << 4) | low;
    }
    Ok(Sha256Digest(bytes))
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn transaction_io(operation: &'static str, path: Option<&str>, error: io::Error) -> FsError {
    FsError::Io {
        operation,
        path: path.map(str::to_owned),
        kind: error.kind(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_target_digest_parser_should_accept_protocol_spelling() {
        let digest =
            parse_digest("sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .expect("digest should parse");

        assert_eq!(
            digest.to_prefixed_hex(),
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }
}
