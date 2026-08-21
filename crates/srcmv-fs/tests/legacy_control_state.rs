//! Legacy-artifact rejection: a former-identity control tree at the workspace
//! root must fail every control-state operation without being enumerated,
//! parsed, locked, migrated, or modified.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use srcmv_fs::{FsError, Workspace};
use tempfile::TempDir;

const SENTINEL_BYTES: &[u8] = b"SRCMV-LEGACY-SENTINEL\0\0\x01";

struct LegacyTree {
    _root: TempDir,
    legacy: std::path::PathBuf,
    sentinel: std::path::PathBuf,
}

fn write_sentinel(path: &std::path::Path) {
    fs::write(path, SENTINEL_BYTES).expect("sentinel should write");
}

fn make_legacy_tree(name: &str) -> LegacyTree {
    let root = TempDir::new().expect("temporary workspace root");
    let legacy = root.path().join(name);
    let transactions = legacy.join("transactions");
    fs::create_dir_all(&transactions).expect("legacy tree should create");
    let sentinel = transactions.join("00000000-sentinel.rec");
    write_sentinel(&sentinel);
    fs::write(legacy.join("lock"), b"stale").expect("legacy lock should write");
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o500))
        .expect("legacy permissions should restrict writes");
    LegacyTree {
        _root: root,
        legacy,
        sentinel,
    }
}

fn snapshot(tree: &LegacyTree) -> (Vec<u8>, u32, u64, (i64, i64)) {
    let metadata = fs::metadata(&tree.sentinel).expect("sentinel metadata should read");
    (
        fs::read(&tree.sentinel).expect("sentinel should read"),
        fs::metadata(&tree.legacy)
            .expect("legacy metadata should read")
            .permissions()
            .mode(),
        metadata.len(),
        (metadata.mtime(), metadata.mtime_nsec()),
    )
}

fn restore_permissions(tree: &LegacyTree) {
    fs::set_permissions(&tree.legacy, fs::Permissions::from_mode(0o700))
        .expect("legacy permissions should restore");
}

#[test]
fn lowercase_legacy_tree_rejects_every_control_state_operation() {
    let tree = make_legacy_tree(".codesplice");
    let before = snapshot(&tree);
    let workspace = Workspace::open(tree._root.path()).expect("workspace root itself should open");

    assert!(matches!(
        workspace.diagnostic_lock(),
        Err(FsError::LegacyControlState)
    ));
    assert!(matches!(
        workspace.mutation_lock(),
        Err(FsError::LegacyControlState)
    ));
    assert!(matches!(
        workspace.recovery_list(),
        Err(FsError::LegacyControlState)
    ));
    assert!(matches!(
        workspace.recovery_status("00000000000000000000000000000000"),
        Err(FsError::LegacyControlState)
    ));
    assert!(matches!(
        workspace.recovery_rollback_control_only("00000000000000000000000000000000"),
        Err(FsError::LegacyControlState)
    ));

    assert_eq!(before, snapshot(&tree));
    assert!(!tree._root.path().join(".srcmv").exists());
    restore_permissions(&tree);
}

#[test]
fn uppercase_legacy_tree_is_rejected_case_insensitively() {
    let tree = make_legacy_tree(".CODESPLICE");
    let before = snapshot(&tree);
    let workspace = Workspace::open(tree._root.path()).expect("workspace root itself should open");

    assert!(matches!(
        workspace.diagnostic_lock(),
        Err(FsError::LegacyControlState)
    ));
    assert!(matches!(
        workspace.mutation_lock(),
        Err(FsError::LegacyControlState)
    ));

    assert_eq!(before, snapshot(&tree));
    assert!(!tree._root.path().join(".srcmv").exists());
    restore_permissions(&tree);
}
