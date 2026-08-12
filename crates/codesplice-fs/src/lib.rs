#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Filesystem boundary types for CodeSplice.
//!
//! Workspace acquisition, path validation, locking, journaling, commit, and
//! recovery are introduced in later phases. This crate depends only on the pure
//! core domain crate.

use std::error::Error;
use std::fmt;

use codesplice_core::CoreError;

/// Typed failures owned by the filesystem boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FsError {
    /// A pure domain failure propagated through the filesystem boundary.
    Core(CoreError),
    /// The requested filesystem capability belongs to a later phase.
    CapabilityUnavailable {
        /// Stable capability name.
        capability: &'static str,
    },
}

impl fmt::Display for FsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "core error: {error}"),
            Self::CapabilityUnavailable { capability } => {
                write!(
                    formatter,
                    "filesystem capability is unavailable: {capability}"
                )
            }
        }
    }
}

impl Error for FsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::CapabilityUnavailable { .. } => None,
        }
    }
}

impl From<CoreError> for FsError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}
