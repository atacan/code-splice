//! Phase 8 filesystem-layer multi-target transaction tests.

use std::fs;
use std::sync::Arc;

use codesplice_core::{
    Anchor, BatchSpecification, Destination, Operation, OperationSpecification, Precondition,
    ResourceBudget, Selector, Sha256Digest, SourceSelection, WorkspaceRelativePath, plan,
};
use codesplice_fs::{RequiredPathState, SnapshotLimits, SnapshotRequirement, Workspace};
use sha2::{Digest, Sha256};
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
fn multi_target_engine_should_commit_three_outputs_in_normalized_path_order() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("source"), b"abc").expect("source should be written");
    fs::write(root.path().join("target"), b"XYZ").expect("target should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    let requirements = vec![
        SnapshotRequirement {
            path: path("source"),
            state: RequiredPathState::Existing(digest(b"abc")),
        },
        SnapshotRequirement {
            path: path("target"),
            state: RequiredPathState::Existing(digest(b"XYZ")),
        },
        SnapshotRequirement {
            path: path("new"),
            state: RequiredPathState::Absent,
        },
    ];
    let batch = BatchSpecification {
        operations: Arc::from([
            Operation::Move(OperationSpecification {
                source: SourceSelection {
                    path: path("source"),
                    selector: Selector::Bytes { start: 0, end: 1 },
                    precondition: Precondition::Sha256(digest(b"abc")),
                },
                destination: Destination {
                    path: path("target"),
                    anchor: Anchor::FileEnd,
                    precondition: Precondition::Sha256(digest(b"XYZ")),
                },
            }),
            Operation::Copy(OperationSpecification {
                source: SourceSelection {
                    path: path("source"),
                    selector: Selector::Bytes { start: 1, end: 3 },
                    precondition: Precondition::Sha256(digest(b"abc")),
                },
                destination: Destination {
                    path: path("new"),
                    anchor: Anchor::FileStart,
                    precondition: Precondition::MustNotExist,
                },
            }),
        ]),
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
        .commit(&lock, &snapshot, &edit_plan, 0o644)
        .expect("multi-target plan should commit");

    assert_eq!(outcome.changed_paths(), ["new", "source", "target"]);
    assert_eq!(
        fs::read(root.path().join("source")).expect("source should read"),
        b"bc"
    );
    assert_eq!(
        fs::read(root.path().join("target")).expect("target should read"),
        b"XYZa"
    );
    assert_eq!(
        fs::read(root.path().join("new")).expect("new target should read"),
        b"bc"
    );
}
