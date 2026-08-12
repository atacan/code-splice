//! Pure recovery classification over synthetic filesystem observations.

use crate::{CommitKind, FsError, GlobalState, RollbackKind, TargetState};

/// Classification of one relevant filesystem location against recorded bytes and identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationObservation {
    /// The location contains the recorded original object and bytes.
    Original,
    /// The location contains the recorded candidate, or an authorized
    /// transaction-owned partial candidate while still `Preparing`.
    Candidate,
    /// The location is absent.
    Absent,
    /// Type, parent, identity, length, or digest differs from every authorized state.
    Unexpected,
}

/// Synthetic observations used by the pure recovery classifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticTargetObservation {
    /// Whether the validated target parent still has its recorded identity.
    pub parent_matches: bool,
    /// Observation at the user target path.
    pub target: LocationObservation,
    /// Observation at the generated candidate name.
    pub candidate: LocationObservation,
    /// Observation at the generated backup name.
    pub backup: LocationObservation,
}

/// Safe recovery actions established for one target observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryDisposition {
    /// Completion may proceed for this target.
    pub complete: bool,
    /// Rollback may proceed for this target.
    pub rollback: bool,
    /// Only terminal cleanup is permitted.
    pub cleanup_only: bool,
}

impl RecoveryDisposition {
    const fn completion_and_rollback() -> Self {
        Self {
            complete: true,
            rollback: true,
            cleanup_only: false,
        }
    }

    const fn rollback_only() -> Self {
        Self {
            complete: false,
            rollback: true,
            cleanup_only: false,
        }
    }

    const fn cleanup_only() -> Self {
        Self {
            complete: false,
            rollback: false,
            cleanup_only: true,
        }
    }
}

/// Classifies one target against the last valid full state snapshot.
///
/// This function performs no filesystem access. Callers must first map actual
/// locations to `original`, `candidate`, `absent`, or `unexpected` using type,
/// parent, identity, length, and digest checks.
///
/// # Errors
///
/// Returns `RecoveryConflict` for every unexpected or ambiguous combination.
pub fn classify_recovery(
    global: GlobalState,
    original_existed: bool,
    recorded: TargetState,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    if !observed.parent_matches
        || [observed.target, observed.candidate, observed.backup]
            .contains(&LocationObservation::Unexpected)
    {
        return Err(conflict("unexpected_location_or_parent"));
    }

    match global {
        GlobalState::Preparing => classify_preparing(original_existed, observed),
        GlobalState::Prepared => classify_prepared(original_existed, observed),
        GlobalState::Committing => classify_committing(original_existed, recorded, observed),
        GlobalState::Committed => classify_committed(original_existed, observed),
        GlobalState::RollingBack => classify_rolling_back(original_existed, recorded, observed),
        GlobalState::RolledBack => classify_rolled_back(original_existed, recorded, observed),
    }
}

fn classify_preparing(
    original_existed: bool,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    let initial_target = if original_existed {
        LocationObservation::Original
    } else {
        LocationObservation::Absent
    };
    if observed.target == initial_target
        && matches!(
            observed.candidate,
            LocationObservation::Absent | LocationObservation::Candidate
        )
        && observed.backup == LocationObservation::Absent
    {
        Ok(RecoveryDisposition::rollback_only())
    } else {
        Err(conflict("preparing_observation_invalid"))
    }
}

fn classify_prepared(
    original_existed: bool,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    let initial_target = if original_existed {
        LocationObservation::Original
    } else {
        LocationObservation::Absent
    };
    if observed.target == initial_target
        && observed.candidate == LocationObservation::Candidate
        && observed.backup == LocationObservation::Absent
    {
        Ok(RecoveryDisposition::completion_and_rollback())
    } else {
        Err(conflict("prepared_observation_invalid"))
    }
}

fn classify_committing(
    original_existed: bool,
    recorded: TargetState,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    let valid = match (original_existed, recorded.commit.kind) {
        (true, CommitKind::Untouched) => matches!(
            (observed.target, observed.candidate, observed.backup),
            (
                LocationObservation::Original,
                LocationObservation::Candidate,
                LocationObservation::Absent
            ) | (
                LocationObservation::Absent,
                LocationObservation::Candidate,
                LocationObservation::Original
            ) | (
                LocationObservation::Candidate,
                LocationObservation::Absent,
                LocationObservation::Original
            )
        ),
        (false, CommitKind::Untouched) => matches!(
            (observed.target, observed.candidate, observed.backup),
            (
                LocationObservation::Absent,
                LocationObservation::Candidate,
                LocationObservation::Absent
            ) | (
                LocationObservation::Candidate,
                LocationObservation::Absent,
                LocationObservation::Absent
            )
        ),
        (true, CommitKind::BackedUp) => matches!(
            (observed.target, observed.candidate, observed.backup),
            (
                LocationObservation::Absent,
                LocationObservation::Candidate,
                LocationObservation::Original
            ) | (
                LocationObservation::Candidate,
                LocationObservation::Absent,
                LocationObservation::Original
            )
        ),
        (false, CommitKind::BackedUp) => false,
        (true, CommitKind::Installed) => {
            observed.target == LocationObservation::Candidate
                && observed.candidate == LocationObservation::Absent
                && observed.backup == LocationObservation::Original
        }
        (false, CommitKind::Installed) => {
            observed.target == LocationObservation::Candidate
                && observed.candidate == LocationObservation::Absent
                && observed.backup == LocationObservation::Absent
        }
    };
    if valid {
        Ok(RecoveryDisposition::completion_and_rollback())
    } else {
        Err(conflict("committing_observation_invalid"))
    }
}

fn classify_committed(
    original_existed: bool,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    let backup = if original_existed {
        LocationObservation::Original
    } else {
        LocationObservation::Absent
    };
    if observed.target == LocationObservation::Candidate
        && observed.candidate == LocationObservation::Absent
        && observed.backup == backup
    {
        Ok(RecoveryDisposition::cleanup_only())
    } else {
        Err(conflict("committed_observation_invalid"))
    }
}

fn classify_rolling_back(
    original_existed: bool,
    recorded: TargetState,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    match recorded.rollback.kind {
        RollbackKind::OriginalRestored if original_existed => {
            classify_restored_original(observed, RecoveryDisposition::rollback_only())
        }
        RollbackKind::AbsenceRestored if !original_existed => {
            classify_restored_absence(observed, RecoveryDisposition::rollback_only())
        }
        RollbackKind::None => {
            let valid = if original_existed {
                matches!(
                    (observed.target, observed.candidate, observed.backup),
                    (
                        LocationObservation::Original,
                        LocationObservation::Absent | LocationObservation::Candidate,
                        LocationObservation::Absent
                    ) | (
                        LocationObservation::Absent,
                        LocationObservation::Absent | LocationObservation::Candidate,
                        LocationObservation::Original
                    ) | (
                        LocationObservation::Candidate,
                        LocationObservation::Absent,
                        LocationObservation::Original
                    )
                )
            } else {
                matches!(
                    (observed.target, observed.candidate, observed.backup),
                    (
                        LocationObservation::Absent,
                        LocationObservation::Absent | LocationObservation::Candidate,
                        LocationObservation::Absent
                    ) | (
                        LocationObservation::Candidate,
                        LocationObservation::Absent,
                        LocationObservation::Absent
                    )
                )
            };
            if valid {
                Ok(RecoveryDisposition::rollback_only())
            } else {
                Err(conflict("rolling_back_observation_invalid"))
            }
        }
        _ => Err(conflict("rolling_back_record_invalid")),
    }
}

fn classify_rolled_back(
    original_existed: bool,
    recorded: TargetState,
    observed: SyntheticTargetObservation,
) -> Result<RecoveryDisposition, FsError> {
    match recorded.rollback.kind {
        RollbackKind::OriginalRestored if original_existed => {
            classify_restored_original(observed, RecoveryDisposition::cleanup_only())
        }
        RollbackKind::AbsenceRestored if !original_existed => {
            classify_restored_absence(observed, RecoveryDisposition::cleanup_only())
        }
        _ => Err(conflict("rolled_back_record_invalid")),
    }
}

fn classify_restored_original(
    observed: SyntheticTargetObservation,
    disposition: RecoveryDisposition,
) -> Result<RecoveryDisposition, FsError> {
    if observed.target == LocationObservation::Original
        && observed.candidate == LocationObservation::Absent
        && observed.backup == LocationObservation::Absent
    {
        Ok(disposition)
    } else {
        Err(conflict("restored_original_observation_invalid"))
    }
}

fn classify_restored_absence(
    observed: SyntheticTargetObservation,
    disposition: RecoveryDisposition,
) -> Result<RecoveryDisposition, FsError> {
    if observed.target == LocationObservation::Absent
        && observed.candidate == LocationObservation::Absent
        && observed.backup == LocationObservation::Absent
    {
        Ok(disposition)
    } else {
        Err(conflict("restored_absence_observation_invalid"))
    }
}

const fn conflict(reason: &'static str) -> FsError {
    FsError::RecoveryConflict { reason }
}
