//! Focused tests for unconditioned single-file snapshot acquisition.

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};

use codesplice_core::{FileIdentity, Sha256Digest, SnapshotFileId, WorkspaceRelativePath};
use codesplice_fs::{FsError, SnapshotLimits, Workspace};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn path(value: &str) -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: value.to_owned(),
    }
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest(Sha256::digest(bytes).into())
}

fn limits(file_bytes: u64) -> SnapshotLimits {
    SnapshotLimits::new(4_096, 1, file_bytes, file_bytes, 1_000, 1_000_000)
}

#[test]
fn acquire_existing_file_returns_exact_bytes_digest_index_and_identity() {
    let root = TempDir::new().expect("workspace should be created");
    let bytes = b"a\r\nb\rc\n";
    fs::create_dir(root.path().join("src")).expect("parent should be created");
    fs::write(root.path().join("src/input"), bytes).expect("fixture should be written");
    let metadata = fs::metadata(root.path().join("src/input")).expect("metadata should be read");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let snapshot = workspace
        .acquire_existing_file(&path("src/input"), SnapshotLimits::default())
        .expect("existing file should be acquired");

    assert_eq!(snapshot.id, SnapshotFileId(0));
    assert_eq!(snapshot.path, path("src/input"));
    assert_eq!(&*snapshot.bytes, bytes);
    assert_eq!(snapshot.digest, digest(bytes));
    assert_eq!(snapshot.line_index.line_count(), 3);
    assert_eq!(
        snapshot.identity,
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    );
    assert_eq!(snapshot.parent_identities.len(), 2);
}

#[test]
fn acquire_existing_file_preserves_non_utf8_bytes() {
    let root = TempDir::new().expect("workspace should be created");
    let bytes = [0xff, 0x00, b'\r', b'\n', 0x80];
    fs::write(root.path().join("binary"), bytes).expect("fixture should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let snapshot = workspace
        .acquire_existing_file(&path("binary"), SnapshotLimits::default())
        .expect("binary file should be acquired");

    assert_eq!(&*snapshot.bytes, &bytes);
}

#[test]
fn acquire_existing_file_returns_typed_relative_path_error_for_absence() {
    let root = TempDir::new().expect("workspace should be created");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let error = workspace
        .acquire_existing_file(&path("missing"), SnapshotLimits::default())
        .expect_err("absent source should fail");

    assert_eq!(
        error,
        FsError::PathNotFound {
            path: "missing".to_owned(),
        }
    );
}

#[test]
fn acquire_existing_file_rejects_non_normalized_path() {
    let root = TempDir::new().expect("workspace should be created");
    fs::create_dir(root.path().join("src")).expect("parent should be created");
    fs::write(root.path().join("source"), b"bytes").expect("fixture should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let error = workspace
        .acquire_existing_file(&path("src/../source"), SnapshotLimits::default())
        .expect_err("non-normalized path should fail");

    assert_eq!(
        error,
        FsError::InvalidPath {
            path: "src/../source".to_owned(),
            reason: "path_parent_component",
        }
    );
}

#[test]
fn acquire_existing_file_rejects_final_and_parent_symlinks() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("real"), b"bytes").expect("fixture should be written");
    symlink("real", root.path().join("link")).expect("file symlink should be created");
    fs::create_dir(root.path().join("real-parent")).expect("parent should be created");
    fs::write(root.path().join("real-parent/input"), b"bytes")
        .expect("nested fixture should be written");
    symlink("real-parent", root.path().join("parent-link"))
        .expect("parent symlink should be created");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let final_error = workspace
        .acquire_existing_file(&path("link"), SnapshotLimits::default())
        .expect_err("final symlink should fail");
    let parent_error = workspace
        .acquire_existing_file(&path("parent-link/input"), SnapshotLimits::default())
        .expect_err("parent symlink should fail");

    assert!(matches!(final_error, FsError::SymlinkNotAllowed { .. }));
    assert!(matches!(parent_error, FsError::SymlinkNotAllowed { .. }));
}

#[test]
fn acquire_existing_file_rejects_special_file() {
    let root = TempDir::new().expect("workspace should be created");
    fs::create_dir(root.path().join("directory")).expect("directory should be created");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let error = workspace
        .acquire_existing_file(&path("directory"), SnapshotLimits::default())
        .expect_err("special file should fail");

    assert_eq!(
        error,
        FsError::UnsupportedFileType {
            path: "directory".to_owned(),
        }
    );
}

#[test]
fn acquire_existing_file_accepts_below_and_at_source_byte_limit() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("source"), b"12345678").expect("fixture should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let below = workspace
        .acquire_existing_file(&path("source"), limits(9))
        .expect("source below limit should be acquired");
    let at = workspace
        .acquire_existing_file(&path("source"), limits(8))
        .expect("source at limit should be acquired");

    assert_eq!(below.bytes.len(), 8);
    assert_eq!(at.bytes.len(), 8);
}

#[test]
fn acquire_existing_file_rejects_source_above_byte_limit() {
    let root = TempDir::new().expect("workspace should be created");
    fs::write(root.path().join("source"), b"12345678").expect("fixture should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");

    let error = workspace
        .acquire_existing_file(&path("source"), limits(7))
        .expect_err("source above limit should fail");

    assert_eq!(
        error,
        FsError::ResourceLimitExceeded {
            resource: "snapshot_file_bytes",
            actual: 8,
            limit: 7,
        }
    );
}

#[test]
fn acquire_existing_file_does_not_modify_workspace() {
    let root = TempDir::new().expect("workspace should be created");
    let source_path = root.path().join("source");
    fs::write(&source_path, b"immutable\n").expect("fixture should be written");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    let before_entries = directory_entries(root.path());
    let before_metadata = fs::metadata(&source_path).expect("metadata should be read");

    workspace
        .acquire_existing_file(&path("source"), SnapshotLimits::default())
        .expect("source should be acquired");

    let after_metadata = fs::metadata(&source_path).expect("metadata should be read");
    assert_eq!(directory_entries(root.path()), before_entries);
    assert_eq!(
        fs::read(source_path).expect("source should be read"),
        b"immutable\n"
    );
    assert_eq!(after_metadata.len(), before_metadata.len());
    assert_eq!(after_metadata.mode(), before_metadata.mode());
    assert_eq!(after_metadata.mtime(), before_metadata.mtime());
    assert_eq!(after_metadata.mtime_nsec(), before_metadata.mtime_nsec());
}

fn directory_entries(path: &std::path::Path) -> Vec<String> {
    let mut entries = fs::read_dir(path)
        .expect("directory should be readable")
        .map(|entry| {
            entry
                .expect("entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
