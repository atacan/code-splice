//! Phase 9 qualification checks that must run on each claimed platform row.

use codesplice_fs::{QualifiedFilesystem, Workspace};
use tempfile::TempDir;

#[test]
fn phase9_platform_qualification_matches_the_declared_pilot_row() {
    let root = TempDir::new().expect("qualification workspace should be created");
    let workspace = Workspace::open(root.path()).expect("workspace should open");
    let filesystem = workspace
        .qualified_filesystem()
        .expect("runner filesystem must be qualified");

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    assert_eq!(filesystem, QualifiedFilesystem::Ext4);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert_eq!(filesystem, QualifiedFilesystem::Apfs);
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    )))]
    panic!("Phase 9 supports only Linux x86_64/ext4 and macOS arm64/APFS");
}
