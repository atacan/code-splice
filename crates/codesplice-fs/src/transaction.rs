//! Multi-target transaction execution and conservative recovery.

use std::collections::BTreeMap;
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

/// Local filesystem configurations qualified for the v0.1 pilot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualifiedFilesystem {
    /// Linux ext4.
    Ext4,
    /// macOS APFS.
    Apfs,
}

impl QualifiedFilesystem {
    /// Stable filesystem spelling used in qualification reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
            Self::Apfs => "apfs",
        }
    }
}

/// Successful result of one changing commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitOutcome {
    transaction_id: String,
    changed_paths: Vec<String>,
    preserved_permission_modes: BTreeMap<String, u32>,
}

impl CommitOutcome {
    /// Canonical transaction identifier.
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// Changed workspace-relative paths in deterministic transaction order.
    #[must_use]
    pub fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }

    /// Permission bits preserved from existing targets, keyed by normalized path.
    #[must_use]
    pub const fn preserved_permission_modes(&self) -> &BTreeMap<String, u32> {
        &self.preserved_permission_modes
    }

    /// The changed path for a single-target outcome.
    #[must_use]
    pub fn changed_path(&self) -> &str {
        self.changed_paths.first().map_or("", String::as_str)
    }

    /// Permission bits preserved for a single-target outcome, when applicable.
    #[must_use]
    pub fn preserved_mode(&self) -> Option<u32> {
        self.preserved_permission_modes.values().next().copied()
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
    /// Detects and validates the workspace's local filesystem for mutation.
    ///
    /// # Errors
    ///
    /// Returns [`FsError::UnsupportedFilesystem`] unless the current platform and
    /// filesystem match a qualified v0.1 pilot row.
    pub fn qualified_filesystem(&self) -> Result<QualifiedFilesystem, FsError> {
        detect_qualified_filesystem(self.canonical_root())
    }

    /// Executes all changed targets through the persistent transaction engine.
    ///
    /// The caller must hold `lock`, must have repeated planning while locked, and
    /// must pass the startup-umask-derived new-file mode.
    ///
    /// # Errors
    ///
    /// Returns a validation, journal, collision, recovery, or I/O error. Plans
    /// with no changed targets are rejected by this boundary.
    pub fn commit(
        &self,
        lock: &MutationLock,
        snapshot: &WorkspaceSnapshot,
        plan: &EditPlan,
        new_file_mode: u32,
    ) -> Result<CommitOutcome, FsError> {
        self.qualified_filesystem()?;
        ensure_control_device(lock, self.identity())?;
        let mut changed = plan
            .outputs
            .iter()
            .filter(|output| output.change != OutputChange::Unchanged)
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Err(FsError::ResourceLimitExceeded {
                resource: "transaction_targets",
                actual: 0,
                limit: crate::journal::MAX_TRANSACTION_TARGETS,
            });
        }
        changed.sort_by(|left, right| left.path.value.as_bytes().cmp(right.path.value.as_bytes()));
        let directory = lock.create_transaction_directory()?;
        let transaction_id = directory.transaction_id().to_owned();
        let active_path = directory.path().to_path_buf();
        let manifest =
            match build_manifest(snapshot, plan, &changed, &transaction_id, new_file_mode) {
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

    /// Executes exactly one changed target through the shared transaction engine.
    ///
    /// This compatibility boundary retains the Phase 7 filesystem-layer API while
    /// delegating all mutation and recovery behavior to [`Workspace::commit`].
    ///
    /// # Errors
    ///
    /// Returns a resource error unless the plan changes exactly one target, or any
    /// error returned by the shared transaction engine.
    pub fn commit_single_target(
        &self,
        lock: &MutationLock,
        snapshot: &WorkspaceSnapshot,
        plan: &EditPlan,
        new_file_mode: u32,
    ) -> Result<CommitOutcome, FsError> {
        let changed_targets = plan
            .outputs
            .iter()
            .filter(|output| output.change != OutputChange::Unchanged)
            .count();
        if changed_targets != 1 {
            return Err(FsError::ResourceLimitExceeded {
                resource: "single_target_commit_targets",
                actual: u64::try_from(changed_targets).unwrap_or(u64::MAX),
                limit: 1,
            });
        }
        self.commit(lock, snapshot, plan, new_file_mode)
    }

    /// Completes a validated transaction or committed cleanup entry.
    ///
    /// # Errors
    ///
    /// Returns contention, corruption, action, conflict, no-replace, or I/O errors.
    pub fn recovery_complete(&self, transaction_id: &str) -> Result<RecoveryOutcome, FsError> {
        self.qualified_filesystem()?;
        if self.diagnostic_lock()?.is_none() {
            return Err(FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            });
        }
        let lock = acquire_existing_mutation_lock(self)?;
        ensure_control_device(&lock, self.identity())?;
        recover_locked(self, &lock, transaction_id, RecoveryMode::Complete)
    }

    /// Rolls back a validated transaction or rolled-back cleanup entry.
    ///
    /// # Errors
    ///
    /// Returns contention, corruption, action, conflict, no-replace, or I/O errors.
    pub fn recovery_rollback(&self, transaction_id: &str) -> Result<RecoveryOutcome, FsError> {
        self.qualified_filesystem()?;
        if self.diagnostic_lock()?.is_none() {
            return Err(FsError::TransactionNotFound {
                transaction_id: transaction_id.to_owned(),
            });
        }
        let lock = acquire_existing_mutation_lock(self)?;
        ensure_control_device(&lock, self.identity())?;
        recover_locked(self, &lock, transaction_id, RecoveryMode::Rollback)
    }
}

fn build_manifest(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    outputs: &[&codesplice_core::PlannedOutput],
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

    let mut targets = Vec::with_capacity(outputs.len());
    for (index, output) in outputs.iter().enumerate() {
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
        ensure_same_device(parent_identity.device, snapshot.workspace_identity.device)?;
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
        let target_index = u64::try_from(index).unwrap_or(u64::MAX);
        targets.push(ManifestTarget {
            target_index,
            path: output.path.value.clone(),
            parent_identity: parent_identity.into(),
            original_existed: original.is_some(),
            original_identity: original.map(|file| file.identity.into()),
            original_sha256: original.map(|file| file.digest.to_prefixed_hex()),
            original_length: original
                .map(|file| u64::try_from(file.bytes.len()).unwrap_or(u64::MAX)),
            candidate_name: format!("candidate-{target_index:08}"),
            backup_name: format!("backup-{target_index:08}"),
            candidate_sha256: output.resulting_digest.to_prefixed_hex(),
            candidate_length: output.resulting_length,
            metadata_policy: if original.is_some() {
                MetadataPolicy::PreserveExistingMode
            } else {
                MetadataPolicy::NewFileMode
            },
            new_file_mode: original.is_none().then_some(new_file_mode & 0o666),
            segments,
        });
    }
    Ok(Manifest {
        transaction_version: 1,
        transaction_id: transaction_id.to_owned(),
        workspace_identity: snapshot.workspace_identity.into(),
        plan_sha256: plan.digest.0.to_prefixed_hex(),
        inputs,
        targets,
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
            targets: manifest
                .targets
                .iter()
                .map(|target| untouched_target_state(target.target_index))
                .collect(),
        },
        checksum: String::new(),
    };
    progress.checksum = journal.publish_state(&progress.state)?;
    for (index, target) in manifest.targets.iter().enumerate() {
        let candidate_identity = create_candidate(&active_path, target, snapshot, index)?;
        let global_state = if index + 1 == manifest.targets.len() {
            GlobalState::Prepared
        } else {
            GlobalState::Preparing
        };
        publish_target_transition(&mut journal, &mut progress, global_state, index, |target| {
            target.candidate = CandidateState {
                kind: CandidateKind::Ready,
                identity: Some(candidate_identity),
            };
        })?;
    }
    revalidate_manifest_inputs(workspace, manifest)?;
    lock.revalidate_control_identities()?;
    publish_global_transition(&mut journal, &mut progress, GlobalState::Committing)?;
    complete_commit_steps(
        workspace,
        lock,
        manifest,
        &active_path,
        &mut journal,
        &mut progress,
    )?;
    let preserved_permission_modes = manifest
        .targets
        .iter()
        .zip(&progress.state.targets)
        .filter_map(|(target, state)| {
            state
                .commit
                .preserved_mode
                .map(|mode| (target.path.clone(), mode))
        })
        .collect();
    lock.finish_transaction(&manifest.transaction_id, &active_path, true)?;
    Ok(CommitOutcome {
        transaction_id: manifest.transaction_id.clone(),
        changed_paths: manifest
            .targets
            .iter()
            .map(|target| target.path.clone())
            .collect(),
        preserved_permission_modes,
    })
}

fn create_candidate(
    transaction_path: &Path,
    target: &ManifestTarget,
    snapshot: &WorkspaceSnapshot,
    target_index: usize,
) -> Result<PersistedIdentity, FsError> {
    let path = transaction_path.join(&target.candidate_name);
    target_failpoint("before_candidate_create", target_index)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| transaction_io("create_candidate", Some(&target.path), error))?;
    target_failpoint("after_candidate_create", target_index)?;
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    target_failpoint("before_candidate_write", target_index)?;
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
    target_failpoint("after_candidate_write", target_index)?;
    file.flush()
        .map_err(|error| transaction_io("flush_candidate", Some(&target.path), error))?;
    target_failpoint("before_candidate_sync", target_index)?;
    file.sync_all()
        .map_err(|error| transaction_io("sync_candidate", Some(&target.path), error))?;
    target_failpoint("after_candidate_sync", target_index)?;
    drop(file);
    let digest = Sha256Digest(hasher.finalize().into());
    if length != target.candidate_length || digest.to_prefixed_hex() != target.candidate_sha256 {
        return Err(FsError::InternalInvariant {
            invariant: "candidate_matches_planned_output",
        });
    }
    target_failpoint("before_candidate_verification", target_index)?;
    let fingerprint = fingerprint(&path, Some(&target.path))?.ok_or(FsError::RecoveryConflict {
        reason: "candidate_missing_after_creation",
    })?;
    if fingerprint.length != length || fingerprint.digest != digest {
        return Err(FsError::RecoveryConflict {
            reason: "candidate_verification_mismatch",
        });
    }
    target_failpoint("after_candidate_verification", target_index)?;
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
    classify_all_targets(
        workspace,
        manifest,
        active_path,
        &progress.state,
        RecoveryMode::Complete,
    )?;
    lock.revalidate_control_identities()?;
    for (index, target) in manifest.targets.iter().enumerate() {
        complete_target(workspace, target, index, active_path, journal, progress)?;
    }
    for (index, target) in manifest.targets.iter().enumerate() {
        let paths = target_paths(workspace, active_path, target)?;
        target_failpoint("before_final_verification", index)?;
        verify_candidate_location(&paths.target, target, progress.state.targets[index])?;
        target_failpoint("after_final_verification", index)?;
    }
    publish_global_transition(journal, progress, GlobalState::Committed)?;
    Ok(())
}

fn complete_target(
    workspace: &Workspace,
    target: &ManifestTarget,
    target_index: usize,
    active_path: &Path,
    journal: &mut TransactionJournal,
    progress: &mut Progress,
) -> Result<(), FsError> {
    let paths = target_paths(workspace, active_path, target)?;
    let observed = observe_target(
        target,
        &paths,
        progress.state.targets[target_index],
        progress.state.global_state,
    )?;
    if observed.target == LocationObservation::Candidate {
        let final_file =
            verify_candidate_location(&paths.target, target, progress.state.targets[target_index])?;
        publish_installed(journal, progress, target_index, final_file.identity.into())?;
        return Ok(());
    }

    if target.original_existed {
        let preserved_mode = if observed.backup == LocationObservation::Original {
            mode_of(&paths.backup, &target.path)?
        } else {
            target_failpoint("before_backup_rename", target_index)?;
            no_replace_rename(&paths.target, &paths.backup, "backup_target")?;
            sync_directory(&paths.parent)?;
            sync_directory(active_path)?;
            target_failpoint("after_backup_rename", target_index)?;
            verify_original_location(&paths.backup, target)?;
            mode_of(&paths.backup, &target.path)?
        };
        set_mode(&paths.candidate, preserved_mode, &target.path)?;
        if progress.state.targets[target_index].commit.kind == CommitKind::Untouched {
            let backup = verify_original_location(&paths.backup, target)?;
            publish_target_transition(
                journal,
                progress,
                GlobalState::Committing,
                target_index,
                |state| {
                    state.commit = CommitState {
                        kind: CommitKind::BackedUp,
                        identity: Some(backup.identity.into()),
                        preserved_mode: Some(preserved_mode),
                    };
                },
            )?;
        }
    } else {
        let mode = target.new_file_mode.ok_or(FsError::InternalInvariant {
            invariant: "new_target_has_recorded_mode",
        })?;
        set_mode(&paths.candidate, mode, &target.path)?;
    }
    target_failpoint("before_install_rename", target_index)?;
    no_replace_rename(&paths.candidate, &paths.target, "install_candidate")?;
    sync_directory(&paths.parent)?;
    sync_directory(active_path)?;
    target_failpoint("after_install_rename", target_index)?;
    let final_file =
        verify_candidate_location(&paths.target, target, progress.state.targets[target_index])?;
    publish_installed(journal, progress, target_index, final_file.identity.into())
}

fn publish_installed(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    target_index: usize,
    identity: PersistedIdentity,
) -> Result<(), FsError> {
    if progress.state.targets[target_index].commit.kind != CommitKind::Installed {
        publish_target_transition(
            journal,
            progress,
            GlobalState::Committing,
            target_index,
            |target| {
                target.commit.kind = CommitKind::Installed;
                target.commit.identity = Some(identity);
            },
        )?;
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
            classify_all_targets(
                workspace,
                &loaded.manifest,
                active_path,
                &loaded.progress.state,
                RecoveryMode::Complete,
            )?;
        }
        GlobalState::Prepared => {
            revalidate_manifest_inputs(workspace, &loaded.manifest)?;
            classify_all_targets(
                workspace,
                &loaded.manifest,
                active_path,
                &loaded.progress.state,
                RecoveryMode::Complete,
            )?;
            lock.revalidate_control_identities()?;
            publish_global_transition(
                &mut loaded.journal,
                &mut loaded.progress,
                GlobalState::Committing,
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
    classify_all_targets(
        workspace,
        &loaded.manifest,
        active_path,
        &loaded.progress.state,
        RecoveryMode::Rollback,
    )?;
    if loaded.progress.state.global_state != GlobalState::RollingBack {
        publish_global_transition(
            &mut loaded.journal,
            &mut loaded.progress,
            GlobalState::RollingBack,
        )?;
    }
    lock.revalidate_control_identities()?;
    let rollback_result = rollback_all_targets(workspace, active_path, loaded).and_then(|()| {
        publish_global_transition(
            &mut loaded.journal,
            &mut loaded.progress,
            GlobalState::RolledBack,
        )
    });
    if rollback_result.is_err() {
        return Err(FsError::TransactionRecoveryRequired {
            transaction_ids: vec![loaded.manifest.transaction_id.clone()],
        });
    }
    lock.finish_transaction(&loaded.manifest.transaction_id, active_path, false)
}

fn rollback_all_targets(
    workspace: &Workspace,
    active_path: &Path,
    loaded: &mut LoadedTransaction,
) -> Result<(), FsError> {
    for target_index in (0..loaded.manifest.targets.len()).rev() {
        rollback_target(workspace, active_path, loaded, target_index)?;
    }
    classify_all_targets(
        workspace,
        &loaded.manifest,
        active_path,
        &loaded.progress.state,
        RecoveryMode::Rollback,
    )?;
    Ok(())
}

fn rollback_target(
    workspace: &Workspace,
    active_path: &Path,
    loaded: &mut LoadedTransaction,
    target_index: usize,
) -> Result<(), FsError> {
    let target = &loaded.manifest.targets[target_index];
    let paths = target_paths(workspace, active_path, target)?;
    let state = loaded.progress.state.targets[target_index];
    let observed = observe_target(target, &paths, state, loaded.progress.state.global_state)?;
    target_failpoint("before_rollback_target_step", target_index)?;
    if observed.target == LocationObservation::Candidate {
        verify_candidate_location(&paths.target, target, state)?;
        fs::remove_file(&paths.target).map_err(|error| {
            transaction_io("remove_installed_candidate", Some(&target.path), error)
        })?;
        sync_directory(&paths.parent)?;
    }
    target_failpoint("after_rollback_target_step", target_index)?;
    target_failpoint("before_rollback_restore_step", target_index)?;
    if target.original_existed && paths.backup.exists() {
        verify_original_location(&paths.backup, target)?;
        no_replace_rename(&paths.backup, &paths.target, "restore_backup")?;
        sync_directory(&paths.parent)?;
        sync_directory(active_path)?;
    }
    target_failpoint("after_rollback_restore_step", target_index)?;
    target_failpoint("before_rollback_candidate_cleanup", target_index)?;
    if paths.candidate.exists() {
        if state.candidate.kind == CandidateKind::Ready {
            verify_candidate_location(&paths.candidate, target, state)?;
        } else {
            validate_partial_candidate(&paths.candidate)?;
        }
        fs::remove_file(&paths.candidate).map_err(|error| {
            transaction_io("remove_staged_candidate", Some(&target.path), error)
        })?;
        sync_directory(active_path)?;
    }
    target_failpoint("after_rollback_candidate_cleanup", target_index)?;

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
    target_failpoint("before_rollback_verification", target_index)?;
    target_failpoint("after_rollback_verification", target_index)?;
    if loaded.progress.state.targets[target_index].rollback.kind == RollbackKind::None {
        publish_target_transition(
            &mut loaded.journal,
            &mut loaded.progress,
            GlobalState::RollingBack,
            target_index,
            |state| state.rollback = restored,
        )?;
    }
    Ok(())
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

fn publish_global_transition(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    global_state: GlobalState,
) -> Result<(), FsError> {
    publish_transition(journal, progress, global_state, |_| Ok(()))
}

fn publish_target_transition(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    global_state: GlobalState,
    target_index: usize,
    update: impl FnOnce(&mut TargetState),
) -> Result<(), FsError> {
    publish_transition(journal, progress, global_state, |targets| {
        let target = targets
            .get_mut(target_index)
            .ok_or(FsError::InternalInvariant {
                invariant: "state_target_index_exists",
            })?;
        update(target);
        Ok(())
    })
}

fn publish_transition(
    journal: &mut TransactionJournal,
    progress: &mut Progress,
    global_state: GlobalState,
    update: impl FnOnce(&mut [TargetState]) -> Result<(), FsError>,
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
    update(&mut next.targets)?;
    let checksum = journal.publish_state(&next)?;
    progress.state = next;
    progress.checksum = checksum;
    Ok(())
}

const fn untouched_target_state(target_index: u64) -> TargetState {
    TargetState {
        target_index,
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
    ensure_same_device(
        validated.parent_identity.device,
        workspace.identity().device,
    )?;
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
    recorded: TargetState,
    global_state: GlobalState,
) -> Result<SyntheticTargetObservation, FsError> {
    Ok(SyntheticTargetObservation {
        parent_matches: true,
        target: classify_location(&paths.target, target, recorded, false, global_state)?,
        candidate: classify_location(&paths.candidate, target, recorded, true, global_state)?,
        backup: classify_backup(&paths.backup, target)?,
    })
}

fn classify_all_targets(
    workspace: &Workspace,
    manifest: &Manifest,
    active_path: &Path,
    state: &StateSnapshot,
    mode: RecoveryMode,
) -> Result<(), FsError> {
    for (index, target) in manifest.targets.iter().enumerate() {
        let recorded = state
            .targets
            .get(index)
            .copied()
            .ok_or(FsError::InternalInvariant {
                invariant: "manifest_and_state_target_counts_match",
            })?;
        let paths = target_paths(workspace, active_path, target)?;
        let observed = observe_target(target, &paths, recorded, state.global_state)?;
        let disposition = classify_recovery(
            state.global_state,
            target.original_existed,
            recorded,
            observed,
        )?;
        let allowed = match mode {
            RecoveryMode::Complete => disposition.complete || disposition.cleanup_only,
            RecoveryMode::Rollback => disposition.rollback || disposition.cleanup_only,
        };
        if !allowed {
            return Err(FsError::RecoveryActionNotAllowed {
                transaction_id: manifest.transaction_id.clone(),
                reason: match mode {
                    RecoveryMode::Complete => "transaction_cannot_be_completed",
                    RecoveryMode::Rollback => "transaction_cannot_be_rolled_back",
                },
            });
        }
    }
    Ok(())
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

fn target_failpoint(name: &str, target_index: usize) -> Result<(), FsError> {
    crate::test_failpoint(name)?;
    crate::test_failpoint(&format!("{name}_target-{target_index:08}"))
}

fn ensure_control_device(lock: &MutationLock, workspace: FileIdentity) -> Result<(), FsError> {
    ensure_same_device(lock.control_device(), workspace.device)
}

fn ensure_same_device(control_device: u64, workspace_device: u64) -> Result<(), FsError> {
    if control_device == workspace_device {
        Ok(())
    } else {
        Err(FsError::CrossDeviceTransaction)
    }
}

fn detect_qualified_filesystem(path: &Path) -> Result<QualifiedFilesystem, FsError> {
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
        let filesystem = statistics.f_type as i64;
        classify_linux_filesystem(filesystem)
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
        classify_macos_filesystem(&filesystem)
    }
}

#[cfg(target_os = "linux")]
fn classify_linux_filesystem(filesystem: i64) -> Result<QualifiedFilesystem, FsError> {
    const EXT4_SUPER_MAGIC: i64 = 0xef53;
    if filesystem == EXT4_SUPER_MAGIC {
        Ok(QualifiedFilesystem::Ext4)
    } else {
        Err(FsError::UnsupportedFilesystem {
            filesystem: format!("0x{filesystem:x}"),
        })
    }
}

#[cfg(target_os = "macos")]
fn classify_macos_filesystem(filesystem: &str) -> Result<QualifiedFilesystem, FsError> {
    if filesystem == "apfs" {
        Ok(QualifiedFilesystem::Apfs)
    } else {
        Err(FsError::UnsupportedFilesystem {
            filesystem: filesystem.to_owned(),
        })
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
    use std::fs;

    use tempfile::TempDir;

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

    #[test]
    fn phase9_cross_device_check_rejects_every_device_mismatch() {
        assert_eq!(ensure_same_device(7, 7), Ok(()));
        assert_eq!(
            ensure_same_device(7, 8),
            Err(FsError::CrossDeviceTransaction)
        );
    }

    #[test]
    fn phase9_no_replace_collision_preserves_both_entries() {
        let directory = TempDir::new().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"source").expect("source should be written");
        fs::write(&destination, b"destination").expect("destination should be written");

        let error = no_replace_rename(&source, &destination, "qualification_collision")
            .expect_err("collision must fail closed");

        assert!(matches!(error, FsError::RecoveryConflict { .. }));
        assert_eq!(fs::read(source).expect("source should remain"), b"source");
        assert_eq!(
            fs::read(destination).expect("destination should remain"),
            b"destination"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn phase9_filesystem_detection_accepts_ext4_and_rejects_virtual_or_network_types() {
        assert_eq!(
            classify_linux_filesystem(0xef53),
            Ok(QualifiedFilesystem::Ext4)
        );
        for filesystem in [0x0102_1994, 0x6969, 0x794c_7630, 0x9fa0] {
            assert!(matches!(
                classify_linux_filesystem(filesystem),
                Err(FsError::UnsupportedFilesystem { .. })
            ));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase9_filesystem_detection_accepts_apfs_and_rejects_virtual_or_network_types() {
        assert_eq!(
            classify_macos_filesystem("apfs"),
            Ok(QualifiedFilesystem::Apfs)
        );
        for filesystem in ["nfs", "smbfs", "webdav", "devfs"] {
            assert!(matches!(
                classify_macos_filesystem(filesystem),
                Err(FsError::UnsupportedFilesystem { .. })
            ));
        }
    }
}
