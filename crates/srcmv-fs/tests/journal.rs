//! Phase 5 persistent journal, locking, and control-scan tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use codesplice_fs::{
    CandidateKind, CandidateState, CommitKind, CommitState, FsError, GlobalState, Manifest,
    ManifestInput, ManifestSegment, ManifestTarget, MetadataPolicy, PersistedIdentity,
    RecoveryEntryKind, RollbackKind, RollbackState, StateSnapshot, TargetState, TransactionJournal,
    TransactionLimits, Workspace, decode_manifest_record, decode_state_record,
    encode_manifest_record, encode_manifest_record_with_limits, encode_state_record,
    encode_state_record_with_limits, validate_state_transition,
};
use tempfile::TempDir;

const IDENTITY: PersistedIdentity = PersistedIdentity {
    device: 7,
    inode: 11,
};
const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn workspace() -> (TempDir, Workspace) {
    let root = TempDir::new().expect("temporary workspace should be created");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    (root, workspace)
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest(transaction_id: &str) -> Manifest {
    Manifest {
        transaction_version: 1,
        transaction_id: transaction_id.to_owned(),
        workspace_identity: IDENTITY,
        plan_sha256: DIGEST.to_owned(),
        inputs: vec![ManifestInput {
            path: "src/input.rs".to_owned(),
            parent_identity: IDENTITY,
            existed: true,
            file_identity: Some(IDENTITY),
            sha256: Some(DIGEST.to_owned()),
            length: Some(4),
            link_count: Some(1),
        }],
        targets: vec![ManifestTarget {
            target_index: 0,
            path: "src/output.rs".to_owned(),
            parent_identity: IDENTITY,
            original_existed: true,
            original_identity: Some(IDENTITY),
            original_sha256: Some(DIGEST.to_owned()),
            original_length: Some(4),
            candidate_name: "candidate-00000000".to_owned(),
            backup_name: "backup-00000000".to_owned(),
            candidate_sha256: DIGEST.to_owned(),
            candidate_length: 4,
            metadata_policy: MetadataPolicy::PreserveExistingMode,
            new_file_mode: None,
            segments: vec![ManifestSegment {
                input_index: 0,
                start: 0,
                end: 4,
                operation_index: None,
            }],
        }],
        metadata_limitations: vec!["ownership".to_owned(), "xattrs".to_owned()],
    }
}

fn target(candidate: CandidateKind, commit: CommitKind, rollback: RollbackKind) -> TargetState {
    TargetState {
        target_index: 0,
        candidate: CandidateState {
            kind: candidate,
            identity: (candidate == CandidateKind::Ready).then_some(IDENTITY),
        },
        commit: CommitState {
            kind: commit,
            identity: (commit != CommitKind::Untouched).then_some(IDENTITY),
            preserved_mode: (commit == CommitKind::BackedUp).then_some(0o644),
        },
        rollback: RollbackState {
            kind: rollback,
            identity: (rollback == RollbackKind::OriginalRestored).then_some(IDENTITY),
        },
    }
}

fn state(
    sequence: u64,
    manifest_checksum: &str,
    prior: Option<String>,
    global_state: GlobalState,
    target: TargetState,
) -> StateSnapshot {
    StateSnapshot {
        transaction_version: 1,
        sequence,
        manifest_checksum: manifest_checksum.to_owned(),
        prior_state_checksum: prior,
        global_state,
        targets: vec![target],
    }
}

#[test]
fn journal_record_envelopes_should_round_trip_exact_payloads() {
    let manifest = manifest("0123456789abcdef0123456789abcdef");
    let manifest_record = encode_manifest_record(&manifest).expect("manifest should encode");
    let state = state(
        0,
        DIGEST,
        None,
        GlobalState::Preparing,
        target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    let state_record = encode_state_record(&state).expect("state should encode");

    assert_eq!(
        decode_manifest_record(&manifest_record).expect("manifest should decode"),
        manifest
    );
    assert_eq!(
        decode_state_record(&state_record).expect("state should decode"),
        state
    );
    assert!(manifest_record.starts_with(b"CODESPLICE-MANIFEST\0\0\0\0\x01"));
    assert!(state_record.starts_with(b"CODESPLICE-STATE\0\0\0\0\x01"));
}

#[test]
fn journal_record_envelopes_should_match_checked_in_golden_bytes() {
    let golden = repository_root().join("tests/golden/transaction-v1");
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(golden.join("manifest.json")).expect("manifest golden should read"),
    )
    .expect("manifest golden should decode");
    let state: StateSnapshot = serde_json::from_slice(
        &fs::read(golden.join("state-00000000.json")).expect("state golden should read"),
    )
    .expect("state golden should decode");

    assert_eq!(
        encode_manifest_record(&manifest).expect("manifest should encode"),
        decode_hex(
            &fs::read_to_string(golden.join("manifest.rec.hex"))
                .expect("manifest record golden should read")
        )
    );
    assert_eq!(
        encode_state_record(&state).expect("state should encode"),
        decode_hex(
            &fs::read_to_string(golden.join("state-00000000.rec.hex"))
                .expect("state record golden should read")
        )
    );
}

#[test]
fn journal_payload_schemas_should_be_valid_draft_2020_12_json() {
    let schemas = repository_root().join("docs/schema/transaction-v1");
    for name in ["manifest.schema.json", "state.schema.json"] {
        let schema: serde_json::Value = serde_json::from_slice(
            &fs::read(schemas.join(name)).expect("transaction schema should read"),
        )
        .expect("transaction schema should be valid JSON");
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["additionalProperties"], false);
    }
}

#[test]
fn journal_decoder_should_reject_torn_truncated_checksum_invalid_and_trailing_records() {
    let encoded = encode_manifest_record(&manifest("0123456789abcdef0123456789abcdef"))
        .expect("manifest should encode");
    let mut checksum_invalid = encoded.clone();
    *checksum_invalid
        .last_mut()
        .expect("checksum byte should exist") ^= 1;
    let mut trailing = encoded.clone();
    trailing.push(0);

    for corrupt in [
        &encoded[..encoded.len() - 1],
        &encoded[..12],
        checksum_invalid.as_slice(),
        trailing.as_slice(),
    ] {
        assert!(matches!(
            decode_manifest_record(corrupt),
            Err(FsError::TransactionRecordCorrupt { .. })
        ));
    }
}

#[test]
fn journal_decoder_should_reject_oversized_persisted_records_without_parsing() {
    let oversized = vec![0_u8; 16 * 1024 * 1024 + 1];

    assert!(matches!(
        decode_manifest_record(&oversized),
        Err(FsError::TransactionRecordCorrupt {
            reason: "record_oversized",
            ..
        })
    ));
}

#[test]
fn journal_limits_should_pass_at_and_reject_below_each_known_schema_boundary() {
    let manifest = manifest("0123456789abcdef0123456789abcdef");
    let manifest_record = encode_manifest_record(&manifest).expect("manifest should encode");
    let manifest_length = u64::try_from(manifest_record.len()).expect("length should fit");
    let disk_at_manifest = manifest_length + 8;
    let at_manifest = transaction_limits(manifest_length, 1, 1, 1024, 1, 4096, disk_at_manifest);
    assert!(encode_manifest_record_with_limits(&manifest, at_manifest).is_ok());
    assert!(matches!(
        encode_manifest_record_with_limits(
            &manifest,
            transaction_limits(manifest_length - 1, 1, 1, 1024, 1, 4096, disk_at_manifest)
        ),
        Err(FsError::ResourceLimitExceeded {
            resource: "transaction_record_bytes",
            ..
        })
    ));
    assert!(matches!(
        encode_manifest_record_with_limits(
            &manifest,
            transaction_limits(manifest_length, 0, 1, 1024, 1, 4096, disk_at_manifest)
        ),
        Err(FsError::ResourceLimitExceeded {
            resource: "transaction_targets",
            ..
        })
    ));
    assert!(matches!(
        encode_manifest_record_with_limits(
            &manifest,
            transaction_limits(manifest_length, 1, 1, 1024, 1, 4096, disk_at_manifest - 1)
        ),
        Err(FsError::ResourceLimitExceeded {
            resource: "projected_transaction_disk_bytes",
            ..
        })
    ));

    let snapshot = state(
        0,
        DIGEST,
        None,
        GlobalState::Preparing,
        target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    let state_record = encode_state_record(&snapshot).expect("state should encode");
    let state_length = u64::try_from(state_record.len()).expect("length should fit");
    assert!(
        encode_state_record_with_limits(
            &snapshot,
            transaction_limits(state_length, 1, 1, state_length, 1, 4096, 8)
        )
        .is_ok()
    );
    assert!(matches!(
        encode_state_record_with_limits(
            &snapshot,
            transaction_limits(state_length, 1, 0, state_length, 1, 4096, 8)
        ),
        Err(FsError::ResourceLimitExceeded {
            resource: "state_records",
            ..
        })
    ));
}

#[test]
fn journal_decoder_should_reject_unknown_and_duplicate_payload_fields() {
    let encoded = encode_manifest_record(&manifest("0123456789abcdef0123456789abcdef"))
        .expect("manifest should encode");
    let payload_start = b"CODESPLICE-MANIFEST\0".len() + 12;
    let payload_end = encoded.len() - 32;
    let payload =
        std::str::from_utf8(&encoded[payload_start..payload_end]).expect("JSON should be UTF-8");
    let unknown = payload.replacen('{', "{\"unknown\":true,", 1);
    let duplicate = payload.replacen(
        "\"transaction_version\":1",
        "\"transaction_version\":1,\"transaction_version\":1",
        1,
    );

    for payload in [unknown, duplicate] {
        let record = raw_manifest_record(payload.as_bytes());
        assert!(matches!(
            decode_manifest_record(&record),
            Err(FsError::TransactionRecordCorrupt { .. })
        ));
    }
}

#[test]
fn journal_publication_should_build_and_scan_one_contiguous_checksum_chain() {
    let (_root, workspace) = workspace();
    let lock = workspace
        .mutation_lock()
        .expect("exclusive lock should be acquired");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction directory should be allocated");
    let transaction_id = directory.transaction_id().to_owned();
    let mut journal = TransactionJournal::create(directory, &manifest(&transaction_id))
        .expect("manifest should publish");
    let preparing = state(
        0,
        journal.manifest_checksum(),
        None,
        GlobalState::Preparing,
        target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    let prior = journal
        .publish_state(&preparing)
        .expect("preparing should publish");
    let prepared = state(
        1,
        journal.manifest_checksum(),
        Some(prior),
        GlobalState::Prepared,
        target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    journal
        .publish_state(&prepared)
        .expect("prepared should publish");
    drop(journal);
    drop(lock);

    let status = workspace
        .recovery_status(&transaction_id)
        .expect("journal should scan");
    assert_eq!(status.kind(), RecoveryEntryKind::Active);
    assert_eq!(status.actions(), ["status", "complete", "rollback"]);
}

#[test]
fn journal_scan_should_reject_gapped_forked_and_checksum_invalid_chains() {
    for corruption in ["gap", "fork", "checksum"] {
        let (_root, workspace) = workspace();
        let lock = workspace.mutation_lock().expect("lock should be acquired");
        let directory = lock
            .create_transaction_directory()
            .expect("directory should be allocated");
        let id = directory.transaction_id().to_owned();
        let path = directory.path().to_path_buf();
        let mut journal =
            TransactionJournal::create(directory, &manifest(&id)).expect("manifest should publish");
        let preparing = state(
            0,
            journal.manifest_checksum(),
            None,
            GlobalState::Preparing,
            target(
                CandidateKind::Missing,
                CommitKind::Untouched,
                RollbackKind::None,
            ),
        );
        let prior = journal
            .publish_state(&preparing)
            .expect("state zero should publish");
        let prepared = state(
            1,
            journal.manifest_checksum(),
            Some(prior),
            GlobalState::Prepared,
            target(
                CandidateKind::Ready,
                CommitKind::Untouched,
                RollbackKind::None,
            ),
        );
        journal
            .publish_state(&prepared)
            .expect("state one should publish");
        drop(journal);
        match corruption {
            "gap" => fs::rename(
                path.join("state-00000001.rec"),
                path.join("state-00000002.rec"),
            )
            .expect("state should be renamed for fixture"),
            "fork" => {
                let forked = state(
                    1,
                    DIGEST,
                    Some(DIGEST.to_owned()),
                    GlobalState::Prepared,
                    target(
                        CandidateKind::Ready,
                        CommitKind::Untouched,
                        RollbackKind::None,
                    ),
                );
                fs::write(
                    path.join("state-00000001.rec"),
                    encode_state_record(&forked).expect("fork fixture should encode"),
                )
                .expect("fork fixture should be written");
            }
            "checksum" => {
                let record = path.join("state-00000001.rec");
                let mut bytes = fs::read(&record).expect("state should read");
                *bytes.last_mut().expect("checksum should exist") ^= 1;
                fs::write(record, bytes).expect("corrupt state should write");
            }
            _ => unreachable!(),
        }
        drop(lock);

        assert!(matches!(
            workspace.recovery_list(),
            Err(FsError::TransactionRecordCorrupt { .. })
        ));
    }
}

#[test]
fn journal_scan_should_validate_but_never_adopt_an_unpublished_state_temporary() {
    let (_root, workspace) = workspace();
    let lock = workspace.mutation_lock().expect("lock should be acquired");
    let directory = lock
        .create_transaction_directory()
        .expect("directory should be allocated");
    let id = directory.transaction_id().to_owned();
    let path = directory.path().to_path_buf();
    let mut journal =
        TransactionJournal::create(directory, &manifest(&id)).expect("manifest should publish");
    let preparing = state(
        0,
        journal.manifest_checksum(),
        None,
        GlobalState::Preparing,
        target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    let prior = journal
        .publish_state(&preparing)
        .expect("state zero should publish");
    let mut unpublished = state(
        1,
        journal.manifest_checksum(),
        Some(prior),
        GlobalState::Prepared,
        target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    fs::write(
        path.join("state-00000001.tmp"),
        encode_state_record(&unpublished).expect("temporary should encode"),
    )
    .expect("temporary should write");
    drop(journal);
    drop(lock);

    let status = workspace
        .recovery_status(&id)
        .expect("valid unpublished temporary should be ignored");
    assert_eq!(status.actions(), ["status", "rollback"]);

    unpublished.prior_state_checksum = Some(DIGEST.to_owned());
    fs::write(
        path.join("state-00000001.tmp"),
        encode_state_record(&unpublished).expect("corrupt-chain temporary should encode"),
    )
    .expect("temporary should rewrite");
    assert!(matches!(
        workspace.recovery_status(&id),
        Err(FsError::TransactionRecordCorrupt { .. })
    ));
}

#[test]
fn journal_state_machine_should_accept_only_documented_global_edges() {
    let states = [
        GlobalState::Preparing,
        GlobalState::Prepared,
        GlobalState::Committing,
        GlobalState::Committed,
        GlobalState::RollingBack,
        GlobalState::RolledBack,
    ];
    let allowed = [
        (GlobalState::Preparing, GlobalState::Preparing),
        (GlobalState::Preparing, GlobalState::Prepared),
        (GlobalState::Preparing, GlobalState::RollingBack),
        (GlobalState::Prepared, GlobalState::Prepared),
        (GlobalState::Prepared, GlobalState::Committing),
        (GlobalState::Prepared, GlobalState::RollingBack),
        (GlobalState::Committing, GlobalState::Committing),
        (GlobalState::Committing, GlobalState::Committed),
        (GlobalState::Committing, GlobalState::RollingBack),
        (GlobalState::RollingBack, GlobalState::RollingBack),
        (GlobalState::RollingBack, GlobalState::RolledBack),
    ];
    for from in states {
        for to in states {
            let previous = valid_state_for_global(4, from);
            let mut next = next_state_for_edge(&previous, to);
            next.prior_state_checksum = Some(DIGEST.to_owned());
            let accepted = validate_state_transition(&previous, &next).is_ok();
            let expected = allowed.contains(&(from, to));
            assert_eq!(accepted, expected, "edge {from:?} -> {to:?}");
        }
    }
}

#[test]
fn journal_state_machine_should_exhaustively_validate_global_target_combinations() {
    let globals = [
        GlobalState::Preparing,
        GlobalState::Prepared,
        GlobalState::Committing,
        GlobalState::Committed,
        GlobalState::RollingBack,
        GlobalState::RolledBack,
    ];
    let candidates = [CandidateKind::Missing, CandidateKind::Ready];
    let commits = [
        CommitKind::Untouched,
        CommitKind::BackedUp,
        CommitKind::Installed,
    ];
    let rollbacks = [
        RollbackKind::None,
        RollbackKind::OriginalRestored,
        RollbackKind::AbsenceRestored,
    ];
    for global in globals {
        for candidate in candidates {
            for commit in commits {
                for rollback in rollbacks {
                    let snapshot = state(
                        1,
                        DIGEST,
                        Some(DIGEST.to_owned()),
                        global,
                        target(candidate, commit, rollback),
                    );
                    let accepted = encode_state_record(&snapshot).is_ok();
                    let expected = match global {
                        GlobalState::Preparing => {
                            commit == CommitKind::Untouched && rollback == RollbackKind::None
                        }
                        GlobalState::Prepared => {
                            candidate == CandidateKind::Ready
                                && commit == CommitKind::Untouched
                                && rollback == RollbackKind::None
                        }
                        GlobalState::Committing => {
                            candidate == CandidateKind::Ready && rollback == RollbackKind::None
                        }
                        GlobalState::Committed => {
                            candidate == CandidateKind::Ready
                                && commit == CommitKind::Installed
                                && rollback == RollbackKind::None
                        }
                        GlobalState::RollingBack => {
                            rollback != RollbackKind::None
                                || commit == CommitKind::Untouched
                                || candidate == CandidateKind::Ready
                        }
                        GlobalState::RolledBack => rollback != RollbackKind::None,
                    };
                    assert_eq!(
                        accepted, expected,
                        "global={global:?} candidate={candidate:?} commit={commit:?} rollback={rollback:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn journal_control_tree_should_enforce_modes_types_and_nonblocking_lock_contention() {
    let (root, workspace) = workspace();
    let exclusive = workspace
        .mutation_lock()
        .expect("exclusive lock should succeed");
    assert_eq!(
        fs::metadata(root.path().join(".codesplice"))
            .expect("control should exist")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(matches!(
        workspace.diagnostic_lock(),
        Err(FsError::TransactionBusy)
    ));
    drop(exclusive);
    let shared = workspace
        .diagnostic_lock()
        .expect("diagnostic open should succeed")
        .expect("control should exist");
    assert!(matches!(
        workspace.mutation_lock(),
        Err(FsError::TransactionBusy)
    ));
    drop(shared);

    fs::set_permissions(
        root.path().join(".codesplice/lock"),
        fs::Permissions::from_mode(0o622),
    )
    .expect("mode fixture should be applied");
    assert!(matches!(
        workspace.diagnostic_lock(),
        Err(FsError::ControlDirectoryInvalid { .. })
    ));
}

#[test]
fn journal_mutation_lock_should_detect_control_entry_replacement() {
    let (root, workspace) = workspace();
    let lock = workspace.mutation_lock().expect("lock should succeed");
    let lock_path = root.path().join(".codesplice/lock");
    fs::rename(&lock_path, root.path().join(".codesplice/old-lock"))
        .expect("locked descriptor should remain open after fixture rename");
    fs::write(&lock_path, b"").expect("replacement lock fixture should be created");

    assert!(matches!(
        lock.revalidate_control_identities(),
        Err(FsError::ControlDirectoryInvalid {
            reason: "control_identity_changed"
        })
    ));
}

#[test]
fn journal_orphan_and_manifest_only_rollback_should_remove_only_control_entries() {
    for manifest_only in [false, true] {
        let (root, workspace) = workspace();
        fs::write(root.path().join("user-target"), b"unchanged")
            .expect("user target should be created");
        let before = fs::read(root.path().join("user-target")).expect("target should read");
        let lock = workspace.mutation_lock().expect("lock should succeed");
        let directory = lock
            .create_transaction_directory()
            .expect("transaction directory should be allocated");
        let id = directory.transaction_id().to_owned();
        if manifest_only {
            TransactionJournal::create(directory, &manifest(&id)).expect("manifest should publish");
        }
        drop(lock);

        let status = workspace
            .recovery_status(&id)
            .expect("status should succeed");
        assert_eq!(
            status.kind(),
            if manifest_only {
                RecoveryEntryKind::ManifestOnly
            } else {
                RecoveryEntryKind::OrphanRecord
            }
        );
        workspace
            .recovery_rollback_control_only(&id)
            .expect("control-only rollback should succeed");
        assert!(matches!(
            workspace.recovery_status(&id),
            Err(FsError::TransactionNotFound { .. })
        ));
        assert_eq!(
            fs::read(root.path().join("user-target")).expect("target should remain"),
            before
        );
    }
}

#[test]
fn journal_gate_should_block_active_and_remove_only_validated_completed_entries() {
    let (root, workspace) = workspace();
    let lock = workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction directory should be allocated");
    let id = directory.transaction_id().to_owned();
    assert!(matches!(
        lock.gate_new_transaction(),
        Err(FsError::TransactionRecoveryRequired { .. })
    ));
    drop(directory);
    fs::remove_dir(root.path().join(".codesplice/transactions").join(&id))
        .expect("empty active directory should be removed for fixture");
    let completed_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let completed = root
        .path()
        .join(".codesplice/completed")
        .join(format!("{completed_id}-committed"));
    fs::create_dir(&completed).expect("completed fixture should be created");
    fs::set_permissions(&completed, fs::Permissions::from_mode(0o700))
        .expect("completed mode should be private");

    lock.gate_new_transaction()
        .expect("validated cleanup-only directory should be removed");
    assert!(!completed.exists());
}

#[test]
fn journal_scan_limits_should_reject_below_directory_recovery_and_state_byte_usage() {
    let (root, workspace) = workspace();
    let lock = workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("transaction directory should allocate");
    let id = directory.transaction_id().to_owned();
    let mut journal =
        TransactionJournal::create(directory, &manifest(&id)).expect("manifest should publish");
    let preparing = state(
        0,
        journal.manifest_checksum(),
        None,
        GlobalState::Preparing,
        target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
    );
    journal
        .publish_state(&preparing)
        .expect("state zero should publish");
    drop(journal);
    drop(lock);
    let diagnostic = workspace
        .diagnostic_lock()
        .expect("diagnostic lock should open")
        .expect("control should exist");

    assert!(matches!(
        diagnostic.scan_with_limits(transaction_limits(
            16 * 1024 * 1024,
            100,
            512,
            128 * 1024 * 1024,
            0,
            256 * 1024 * 1024,
            3 * 1024 * 1024 * 1024,
        )),
        Err(FsError::ResourceLimitExceeded {
            resource: "transaction_directories",
            ..
        })
    ));
    let manifest_length = fs::metadata(
        root.path()
            .join(".codesplice/transactions")
            .join(&id)
            .join("manifest.rec"),
    )
    .expect("manifest metadata should read")
    .len();
    assert!(matches!(
        diagnostic.scan_with_limits(transaction_limits(
            16 * 1024 * 1024,
            100,
            512,
            128 * 1024 * 1024,
            1,
            manifest_length - 1,
            3 * 1024 * 1024 * 1024,
        )),
        Err(FsError::ResourceLimitExceeded {
            resource: "recovery_bytes",
            ..
        })
    ));
    assert!(matches!(
        diagnostic.scan_with_limits(transaction_limits(
            16 * 1024 * 1024,
            100,
            512,
            0,
            1,
            256 * 1024 * 1024,
            3 * 1024 * 1024 * 1024,
        )),
        Err(FsError::ResourceLimitExceeded {
            resource: "state_record_bytes",
            ..
        })
    ));
}

#[test]
fn journal_scan_should_never_delete_unknown_transaction_entries() {
    let (root, workspace) = workspace();
    let lock = workspace.mutation_lock().expect("lock should succeed");
    let directory = lock
        .create_transaction_directory()
        .expect("directory should be allocated");
    let id = directory.transaction_id().to_owned();
    let unknown = directory.path().join("unknown");
    fs::write(&unknown, b"do not delete").expect("unknown fixture should be created");

    assert!(matches!(
        lock.gate_new_transaction(),
        Err(FsError::TransactionRecordCorrupt { .. })
    ));
    assert_eq!(
        fs::read(unknown).expect("unknown entry should remain"),
        b"do not delete"
    );
    assert!(
        root.path()
            .join(".codesplice/transactions")
            .join(id)
            .exists()
    );
}

fn raw_manifest_record(payload: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut record = Vec::new();
    record.extend_from_slice(b"CODESPLICE-MANIFEST\0");
    record.extend_from_slice(&1_u32.to_be_bytes());
    record.extend_from_slice(
        &u64::try_from(payload.len())
            .expect("payload length should fit")
            .to_be_bytes(),
    );
    record.extend_from_slice(payload);
    let checksum: [u8; 32] = Sha256::digest(&record).into();
    record.extend_from_slice(&checksum);
    record
}

fn decode_hex(value: &str) -> Vec<u8> {
    let value = value.trim();
    assert_eq!(value.len() % 2, 0, "golden hex should contain byte pairs");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("hex should be ASCII");
            u8::from_str_radix(text, 16).expect("golden hex should be valid")
        })
        .collect()
}

fn transaction_limits(
    record_bytes: u64,
    targets: u64,
    state_records: u64,
    state_bytes: u64,
    transaction_directories: u64,
    recovery_bytes: u64,
    projected_disk_bytes: u64,
) -> TransactionLimits {
    TransactionLimits::new(
        record_bytes,
        targets,
        state_records,
        state_bytes,
        transaction_directories,
        recovery_bytes,
        projected_disk_bytes,
    )
}

fn valid_state_for_global(sequence: u64, global: GlobalState) -> StateSnapshot {
    let target = match global {
        GlobalState::Preparing => target(
            CandidateKind::Missing,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
        GlobalState::Prepared => target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
        GlobalState::Committing => target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
        GlobalState::Committed => target(
            CandidateKind::Ready,
            CommitKind::Installed,
            RollbackKind::None,
        ),
        GlobalState::RollingBack => target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::None,
        ),
        GlobalState::RolledBack => target(
            CandidateKind::Ready,
            CommitKind::Untouched,
            RollbackKind::OriginalRestored,
        ),
    };
    state(sequence, DIGEST, Some(DIGEST.to_owned()), global, target)
}

fn next_state_for_edge(previous: &StateSnapshot, to: GlobalState) -> StateSnapshot {
    let mut next_target = previous.targets[0];
    match to {
        GlobalState::Preparing => {}
        GlobalState::Prepared | GlobalState::Committing => {
            next_target = target(
                CandidateKind::Ready,
                CommitKind::Untouched,
                RollbackKind::None,
            );
        }
        GlobalState::Committed => {
            next_target = target(
                CandidateKind::Ready,
                CommitKind::Installed,
                RollbackKind::None,
            );
        }
        GlobalState::RollingBack => {}
        GlobalState::RolledBack => {
            next_target.rollback = RollbackState {
                kind: RollbackKind::OriginalRestored,
                identity: Some(IDENTITY),
            };
        }
    }
    state(5, DIGEST, Some(DIGEST.to_owned()), to, next_target)
}
