#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Common fixture locations and deterministic test values for srcmv tests.

use std::path::{Path, PathBuf};

pub mod fake_lsp;

/// Repository-owned fixture, golden, and scenario roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestRoots {
    workspace_root: PathBuf,
}

impl TestRoots {
    /// Creates paths rooted at a workspace checkout without accessing the filesystem.
    #[must_use]
    pub fn from_workspace_root(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    /// Returns the workspace checkout root.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Returns the shared fixture directory.
    #[must_use]
    pub fn fixtures(&self) -> PathBuf {
        self.workspace_root.join("tests/fixtures")
    }

    /// Returns the shared fake-LSP fixture directory.
    #[must_use]
    pub fn lsp_fixtures(&self) -> PathBuf {
        self.fixtures().join("lsp")
    }

    /// Returns the shared golden-vector directory.
    #[must_use]
    pub fn golden(&self) -> PathBuf {
        self.workspace_root.join("tests/golden")
    }

    /// Returns the shared end-to-end scenario directory.
    #[must_use]
    pub fn scenarios(&self) -> PathBuf {
        self.workspace_root.join("tests/scenarios")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::TestRoots;

    #[test]
    fn fixtures_should_be_beneath_workspace_tests_directory() {
        let roots = TestRoots::from_workspace_root("/checkout");

        assert_eq!(roots.fixtures(), PathBuf::from("/checkout/tests/fixtures"));
    }

    #[test]
    fn golden_should_be_beneath_workspace_tests_directory() {
        let roots = TestRoots::from_workspace_root("/checkout");

        assert_eq!(roots.golden(), PathBuf::from("/checkout/tests/golden"));
    }

    #[test]
    fn lsp_fixtures_should_be_beneath_shared_fixture_directory() {
        let roots = TestRoots::from_workspace_root("/checkout");

        assert_eq!(
            roots.lsp_fixtures(),
            PathBuf::from("/checkout/tests/fixtures/lsp")
        );
    }

    #[test]
    fn scenarios_should_be_beneath_workspace_tests_directory() {
        let roots = TestRoots::from_workspace_root("/checkout");

        assert_eq!(
            roots.scenarios(),
            PathBuf::from("/checkout/tests/scenarios")
        );
    }
}
