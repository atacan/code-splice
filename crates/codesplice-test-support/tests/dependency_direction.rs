//! Guards the workspace-crate dependency architecture.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("test-support crate must be nested under the workspace crates directory")
        .to_path_buf()
}

fn workspace_dependencies(crate_name: &str) -> BTreeSet<String> {
    let manifest_path = workspace_root()
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", manifest_path.display()));
    let mut reads_dependency_entries = false;
    let mut dependencies = BTreeSet::new();

    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let section = line.trim_matches(['[', ']']);
            reads_dependency_entries = section == "dependencies"
                || section == "build-dependencies"
                || (section.starts_with("target.")
                    && (section.ends_with(".dependencies")
                        || section.ends_with(".build-dependencies")));

            for prefix in ["dependencies.", "build-dependencies."] {
                if let Some(name) = section.strip_prefix(prefix)
                    && name.starts_with("codesplice-")
                {
                    dependencies.insert(name.to_owned());
                }
            }
            continue;
        }
        if !reads_dependency_entries || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, _)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.starts_with("codesplice-") {
            dependencies.insert(name.to_owned());
        }
    }

    dependencies
}

#[test]
fn workspace_crate_dependencies_should_follow_the_declared_architecture() {
    let expected = [
        ("codesplice-core", &[][..]),
        ("codesplice-fs", &["codesplice-core"][..]),
        ("codesplice-lsp", &["codesplice-core"][..]),
        ("codesplice-protocol", &["codesplice-core"][..]),
        (
            "codesplice-cli",
            &["codesplice-core", "codesplice-fs", "codesplice-protocol"][..],
        ),
        ("codesplice-test-support", &[][..]),
    ];

    for (crate_name, expected_dependencies) in expected {
        let actual = workspace_dependencies(crate_name);
        let expected = expected_dependencies
            .iter()
            .map(|dependency| (*dependency).to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            actual, expected,
            "unexpected workspace dependencies for {crate_name}"
        );
    }
}
