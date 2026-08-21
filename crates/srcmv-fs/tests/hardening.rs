//! Phase 9 record, state-folding, and recovery-classifier fuzz properties.

use srcmv_fs::{
    CandidateKind, CandidateState, CommitKind, CommitState, GlobalState, LocationObservation,
    PersistedIdentity, RollbackKind, RollbackState, StateSnapshot, SyntheticTargetObservation,
    TargetState, classify_recovery, decode_manifest_record, decode_state_record,
    validate_state_transition,
};
use proptest::prelude::*;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

proptest! {
    #[test]
    fn record_decoders_fuzz_regression_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        let _ = decode_manifest_record(&bytes);
        let _ = decode_state_record(&bytes);
    }

    #[test]
    fn state_folding_fuzz_regression_never_panics(
        previous_seed in any::<u8>(),
        next_seed in any::<u8>(),
        previous_sequence in any::<u64>(),
        next_sequence in any::<u64>(),
    ) {
        let previous = state(previous_seed, previous_sequence);
        let next = state(next_seed, next_sequence);
        let _ = validate_state_transition(&previous, &next);
    }

    #[test]
    fn recovery_classifier_fuzz_regression_is_total_over_synthetic_observations(
        global_seed in any::<u8>(),
        state_seed in any::<u8>(),
        original_existed in any::<bool>(),
        parent_matches in any::<bool>(),
        target_seed in any::<u8>(),
        candidate_seed in any::<u8>(),
        backup_seed in any::<u8>(),
    ) {
        let recorded = target_state(state_seed);
        let observed = SyntheticTargetObservation {
            parent_matches,
            target: observation(target_seed),
            candidate: observation(candidate_seed),
            backup: observation(backup_seed),
        };

        let _ = classify_recovery(global(global_seed), original_existed, recorded, observed);
    }
}

#[test]
fn record_decoders_checked_in_fuzz_regressions_remain_rejected() {
    let cases: [&[u8]; 7] = [
        b"",
        b"CODESPLICE-MANIFEST\0",
        b"CODESPLICE-STATE\0",
        b"CODESPLICE-MANIFEST\0\0\0\0\x01\xff\xff\xff\xff\xff\xff\xff\xff",
        b"CODESPLICE-STATE\0\0\0\0\x01\0\0\0\0\0\0\0\x02{}",
        b"not-a-record",
        &[0xff; 64],
    ];

    for input in cases {
        assert!(decode_manifest_record(input).is_err(), "input={input:?}");
        assert!(decode_state_record(input).is_err(), "input={input:?}");
    }
}

fn state(seed: u8, sequence: u64) -> StateSnapshot {
    StateSnapshot {
        transaction_version: 1,
        sequence,
        manifest_checksum: DIGEST.to_owned(),
        prior_state_checksum: (sequence != 0).then(|| DIGEST.to_owned()),
        global_state: global(seed),
        targets: vec![target_state(seed.rotate_left(3))],
    }
}

fn target_state(seed: u8) -> TargetState {
    TargetState {
        target_index: u64::from(seed % 2),
        candidate: CandidateState {
            kind: if seed & 1 == 0 {
                CandidateKind::Missing
            } else {
                CandidateKind::Ready
            },
            identity: (seed & 2 != 0).then(identity),
        },
        commit: CommitState {
            kind: match (seed >> 2) % 3 {
                0 => CommitKind::Untouched,
                1 => CommitKind::BackedUp,
                _ => CommitKind::Installed,
            },
            identity: (seed & 16 != 0).then(identity),
            preserved_mode: (seed & 32 != 0).then_some(0o640),
        },
        rollback: RollbackState {
            kind: match (seed >> 6) % 3 {
                0 => RollbackKind::None,
                1 => RollbackKind::OriginalRestored,
                _ => RollbackKind::AbsenceRestored,
            },
            identity: (seed & 8 != 0).then(identity),
        },
    }
}

const fn identity() -> PersistedIdentity {
    PersistedIdentity {
        device: 7,
        inode: 11,
    }
}

const fn global(seed: u8) -> GlobalState {
    match seed % 6 {
        0 => GlobalState::Preparing,
        1 => GlobalState::Prepared,
        2 => GlobalState::Committing,
        3 => GlobalState::Committed,
        4 => GlobalState::RollingBack,
        _ => GlobalState::RolledBack,
    }
}

const fn observation(seed: u8) -> LocationObservation {
    match seed % 4 {
        0 => LocationObservation::Original,
        1 => LocationObservation::Candidate,
        2 => LocationObservation::Absent,
        _ => LocationObservation::Unexpected,
    }
}
