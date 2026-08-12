#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! JSON protocol boundary types for CodeSplice.
//!
//! Schema-backed parsing, DTO conversion, response models, and rendering are
//! introduced in Phase 2. This crate never depends on the filesystem crate.

use std::error::Error;
use std::fmt;

use codesplice_core::CoreError;

/// Typed failures owned by the protocol boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A pure domain failure encountered during conversion.
    Core(CoreError),
    /// The requested protocol capability belongs to a later phase.
    CapabilityUnavailable {
        /// Stable capability name.
        capability: &'static str,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "core error: {error}"),
            Self::CapabilityUnavailable { capability } => {
                write!(
                    formatter,
                    "protocol capability is unavailable: {capability}"
                )
            }
        }
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::CapabilityUnavailable { .. } => None,
        }
    }
}

impl From<CoreError> for ProtocolError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}
