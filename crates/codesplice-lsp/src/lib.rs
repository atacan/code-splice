#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bounded Language Server Protocol support for semantic source selection.
//!
//! This crate will own the LSP transport, lifecycle, and conversion from
//! language-server positions to validated [`codesplice_core::ByteRange`] values.

/// Bounded JSON-RPC framing, envelope validation, and response correlation.
pub mod jsonrpc;
/// Bounded child-process transport, supervision, and cleanup.
pub mod process;
/// Deadline-aware composition of process supervision and JSON-RPC framing.
pub mod transport;

use std::path::{Path, PathBuf};

/// Returns the CodeSplice configuration file below a platform configuration directory.
///
/// The caller supplies the platform base so tests and explicit overrides do not
/// need to mutate process-global environment variables.
#[must_use]
pub fn configuration_path_in(configuration_directory: &Path) -> PathBuf {
    configuration_directory
        .join("codesplice")
        .join("config.toml")
}

/// Returns the default CodeSplice configuration file for the current user.
///
/// This uses [`directories::BaseDirs`] rather than [`directories::ProjectDirs`]
/// so the cross-platform contract is exactly `config_dir/codesplice/config.toml`.
/// Returns `None` when the platform cannot resolve the current user's base
/// directories.
#[must_use]
pub fn default_configuration_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|base_directories| configuration_path_in(base_directories.config_dir()))
}
