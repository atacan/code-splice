//! Boundary coverage for the semantic-selection source snapshot cap.

use std::fs;

use srcmv_core::WorkspaceRelativePath;
use srcmv_fs::{FsError, SnapshotLimits, Workspace};
use tempfile::TempDir;

const SELECTION_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

fn limits() -> SnapshotLimits {
    SnapshotLimits::new(
        4_096,
        1,
        SELECTION_SOURCE_BYTES,
        SELECTION_SOURCE_BYTES,
        9_000_000,
        80_000_000,
    )
}

fn path() -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: "source".to_owned(),
    }
}

#[test]
fn selection_snapshot_accepts_below_and_at_eight_mib_then_rejects_above() {
    let root = TempDir::new().expect("workspace should be created");
    let source = root.path().join("source");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    fs::write(
        &source,
        vec![b'x'; usize::try_from(SELECTION_SOURCE_BYTES - 1).expect("limit should fit")],
    )
    .expect("below-limit fixture should be written");
    let below = workspace
        .acquire_existing_file(&path(), limits())
        .expect("source below eight MiB should be acquired");
    assert_eq!(below.bytes.len() as u64, SELECTION_SOURCE_BYTES - 1);

    fs::write(
        &source,
        vec![b'x'; usize::try_from(SELECTION_SOURCE_BYTES).expect("limit should fit")],
    )
    .expect("at-limit fixture should be written");
    let at = workspace
        .acquire_existing_file(&path(), limits())
        .expect("source at eight MiB should be acquired");
    assert_eq!(at.bytes.len() as u64, SELECTION_SOURCE_BYTES);

    fs::write(
        &source,
        vec![b'x'; usize::try_from(SELECTION_SOURCE_BYTES + 1).expect("limit should fit")],
    )
    .expect("above-limit fixture should be written");
    let error = workspace
        .acquire_existing_file(&path(), limits())
        .expect_err("source above eight MiB must be rejected");
    assert_eq!(
        error,
        FsError::ResourceLimitExceeded {
            resource: "snapshot_file_bytes",
            actual: SELECTION_SOURCE_BYTES + 1,
            limit: SELECTION_SOURCE_BYTES,
        }
    );
}
