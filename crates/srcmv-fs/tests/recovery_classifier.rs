//! Exhaustive Phase 5 recovery-classifier tables over synthetic observations.

use codesplice_fs::{
    CandidateKind, CandidateState, CommitKind, CommitState, FsError, GlobalState,
    LocationObservation as L, PersistedIdentity, RollbackKind, RollbackState,
    SyntheticTargetObservation as O, TargetState, classify_recovery,
};

const IDENTITY: PersistedIdentity = PersistedIdentity {
    device: 7,
    inode: 11,
};

fn recorded(commit: CommitKind, rollback: RollbackKind) -> TargetState {
    TargetState {
        target_index: 0,
        candidate: CandidateState {
            kind: CandidateKind::Ready,
            identity: Some(IDENTITY),
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

fn preparing_recorded() -> TargetState {
    let mut state = recorded(CommitKind::Untouched, RollbackKind::None);
    state.candidate = CandidateState {
        kind: CandidateKind::Missing,
        identity: None,
    };
    state
}

fn observation(target: L, candidate: L, backup: L) -> O {
    O {
        parent_matches: true,
        target,
        candidate,
        backup,
    }
}

#[test]
fn recovery_classifier_should_accept_exactly_the_documented_existing_target_table() {
    let original_staged = observation(L::Original, L::Candidate, L::Absent);
    let backup_staged = observation(L::Absent, L::Candidate, L::Original);
    let installed = observation(L::Candidate, L::Absent, L::Original);
    let restored = observation(L::Original, L::Absent, L::Absent);
    let rolling_back = vec![
        restored,
        original_staged,
        observation(L::Absent, L::Absent, L::Original),
        backup_staged,
        installed,
    ];
    let rows = [
        (
            GlobalState::Preparing,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![
                observation(L::Original, L::Absent, L::Absent),
                original_staged,
            ],
        ),
        (
            GlobalState::Prepared,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![original_staged],
        ),
        (
            GlobalState::Committing,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![original_staged, backup_staged, installed],
        ),
        (
            GlobalState::Committing,
            recorded(CommitKind::BackedUp, RollbackKind::None),
            vec![backup_staged, installed],
        ),
        (
            GlobalState::Committing,
            recorded(CommitKind::Installed, RollbackKind::None),
            vec![installed],
        ),
        (
            GlobalState::Committed,
            recorded(CommitKind::Installed, RollbackKind::None),
            vec![installed],
        ),
        (
            GlobalState::RollingBack,
            preparing_recorded(),
            rolling_back.clone(),
        ),
        (
            GlobalState::RollingBack,
            recorded(CommitKind::Installed, RollbackKind::None),
            rolling_back,
        ),
        (
            GlobalState::RollingBack,
            recorded(CommitKind::Installed, RollbackKind::OriginalRestored),
            vec![restored],
        ),
        (
            GlobalState::RolledBack,
            recorded(CommitKind::Installed, RollbackKind::OriginalRestored),
            vec![restored],
        ),
    ];

    for (global, state, expected) in rows {
        assert_exhaustive_row(global, true, state, &expected);
    }
}

#[test]
fn recovery_classifier_should_accept_exactly_the_documented_absent_target_table() {
    let staged = observation(L::Absent, L::Candidate, L::Absent);
    let installed = observation(L::Candidate, L::Absent, L::Absent);
    let restored = observation(L::Absent, L::Absent, L::Absent);
    let rolling_back = vec![restored, staged, installed];
    let rows = [
        (
            GlobalState::Preparing,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![restored, staged],
        ),
        (
            GlobalState::Prepared,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![staged],
        ),
        (
            GlobalState::Committing,
            recorded(CommitKind::Untouched, RollbackKind::None),
            vec![staged, installed],
        ),
        (
            GlobalState::Committing,
            recorded(CommitKind::Installed, RollbackKind::None),
            vec![installed],
        ),
        (
            GlobalState::Committed,
            recorded(CommitKind::Installed, RollbackKind::None),
            vec![installed],
        ),
        (
            GlobalState::RollingBack,
            preparing_recorded(),
            rolling_back.clone(),
        ),
        (
            GlobalState::RollingBack,
            recorded(CommitKind::Installed, RollbackKind::None),
            rolling_back,
        ),
        (
            GlobalState::RollingBack,
            recorded(CommitKind::Installed, RollbackKind::AbsenceRestored),
            vec![restored],
        ),
        (
            GlobalState::RolledBack,
            recorded(CommitKind::Installed, RollbackKind::AbsenceRestored),
            vec![restored],
        ),
    ];

    for (global, state, expected) in rows {
        assert_exhaustive_row(global, false, state, &expected);
    }
}

#[test]
fn recovery_classifier_should_reject_every_parent_mismatch_before_action() {
    for global in [
        GlobalState::Preparing,
        GlobalState::Prepared,
        GlobalState::Committing,
        GlobalState::Committed,
        GlobalState::RollingBack,
        GlobalState::RolledBack,
    ] {
        let result = classify_recovery(
            global,
            true,
            recorded(CommitKind::Untouched, RollbackKind::None),
            O {
                parent_matches: false,
                target: L::Original,
                candidate: L::Candidate,
                backup: L::Absent,
            },
        );
        assert!(matches!(result, Err(FsError::RecoveryConflict { .. })));
    }
}

#[test]
fn recovery_classifier_should_assign_actions_by_global_state() {
    let preparing = classify_recovery(
        GlobalState::Preparing,
        true,
        recorded(CommitKind::Untouched, RollbackKind::None),
        observation(L::Original, L::Absent, L::Absent),
    )
    .expect("preparing should classify");
    let prepared = classify_recovery(
        GlobalState::Prepared,
        true,
        recorded(CommitKind::Untouched, RollbackKind::None),
        observation(L::Original, L::Candidate, L::Absent),
    )
    .expect("prepared should classify");
    let committed = classify_recovery(
        GlobalState::Committed,
        true,
        recorded(CommitKind::Installed, RollbackKind::None),
        observation(L::Candidate, L::Absent, L::Original),
    )
    .expect("committed should classify");

    assert_eq!(
        (
            preparing.complete,
            preparing.rollback,
            preparing.cleanup_only
        ),
        (false, true, false)
    );
    assert_eq!(
        (prepared.complete, prepared.rollback, prepared.cleanup_only),
        (true, true, false)
    );
    assert_eq!(
        (
            committed.complete,
            committed.rollback,
            committed.cleanup_only
        ),
        (false, false, true)
    );
}

fn assert_exhaustive_row(
    global: GlobalState,
    original_existed: bool,
    state: TargetState,
    expected: &[O],
) {
    let locations = [L::Original, L::Candidate, L::Absent, L::Unexpected];
    for target in locations {
        for candidate in locations {
            for backup in locations {
                let observed = observation(target, candidate, backup);
                let accepted = classify_recovery(global, original_existed, state, observed).is_ok();
                assert_eq!(
                    accepted,
                    expected.contains(&observed),
                    "global={global:?} original={original_existed} observed={observed:?}"
                );
            }
        }
    }
}
