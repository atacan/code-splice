//! Phase 7 filesystem-layer single-target transaction tests.

use std::fs;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use srcmv_core::{
    Anchor, BatchSpecification, Destination, Operation, OperationSpecification, Precondition,
    ResourceBudget, Selector, Sha256Digest, SourceSelection, WorkspaceRelativePath, plan,
};
use srcmv_fs::{RequiredPathState, SnapshotLimits, SnapshotRequirement, Workspace};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest(Sha256::digest(bytes).into())
}

fn path(value: &str) -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: value.to_owned(),
    }
}

#[test]
fn single_target_engine_should_commit_one_existing_output_through_the_journal() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("source"), b"payload").expect("source should be written");
    fs::write(root.path().join("target"), b"before-").expect("target should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    let requirements = vec![
        SnapshotRequirement {
            path: path("source"),
            state: RequiredPathState::Existing(digest(b"payload")),
        },
        SnapshotRequirement {
            path: path("target"),
            state: RequiredPathState::Existing(digest(b"before-")),
        },
    ];
    let batch = BatchSpecification {
        operations: Arc::from([Operation::Copy(OperationSpecification {
            source: SourceSelection {
                path: path("source"),
                selector: Selector::Bytes { start: 0, end: 7 },
                precondition: Precondition::Sha256(digest(b"payload")),
            },
            destination: Destination {
                path: path("target"),
                anchor: Anchor::FileEnd,
                precondition: Precondition::Sha256(digest(b"before-")),
            },
        })]),
    };
    let snapshot = workspace
        .acquire_snapshot(&requirements, SnapshotLimits::default())
        .expect("snapshot should acquire");
    let edit_plan = plan(&snapshot, &batch, ResourceBudget::default()).expect("plan should build");
    let lock = workspace
        .mutation_lock()
        .expect("mutation lock should acquire");
    lock.gate_new_transaction()
        .expect("transaction gate should pass");

    let outcome = workspace
        .commit_single_target(&lock, &snapshot, &edit_plan, 0o644)
        .expect("single target should commit");

    assert_eq!(outcome.changed_path(), "target");
    assert_eq!(
        fs::read(root.path().join("target")).expect("target should read"),
        b"before-payload"
    );
}

#[test]
fn single_target_engine_should_reject_a_zero_target_plan() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("source"), b"abc").expect("source should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    let requirements = vec![SnapshotRequirement {
        path: path("source"),
        state: RequiredPathState::Existing(digest(b"abc")),
    }];
    let batch = BatchSpecification {
        operations: Arc::from([Operation::Move(OperationSpecification {
            source: SourceSelection {
                path: path("source"),
                selector: Selector::Bytes { start: 0, end: 1 },
                precondition: Precondition::Sha256(digest(b"abc")),
            },
            destination: Destination {
                path: path("source"),
                anchor: Anchor::ByteOffset(0),
                precondition: Precondition::Sha256(digest(b"abc")),
            },
        })]),
    };
    let snapshot = workspace
        .acquire_snapshot(&requirements, SnapshotLimits::default())
        .expect("snapshot should acquire");
    let edit_plan = plan(&snapshot, &batch, ResourceBudget::default()).expect("plan should build");
    let lock = workspace
        .mutation_lock()
        .expect("mutation lock should acquire");

    let error = workspace
        .commit_single_target(&lock, &snapshot, &edit_plan, 0o644)
        .expect_err("no-op plans do not enter the transaction engine");

    assert!(matches!(
        error,
        srcmv_fs::FsError::ResourceLimitExceeded {
            resource: "single_target_commit_targets",
            actual: 0,
            limit: 1
        }
    ));
}
