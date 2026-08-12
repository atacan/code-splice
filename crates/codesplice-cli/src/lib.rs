#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Orchestration boundary for the CodeSplice command-line application.
//!
//! The command grammar and executable behavior are introduced in Phase 2.

use std::error::Error;
use std::fmt;

use codesplice_core::CoreError;
use codesplice_fs::FsError;
use codesplice_protocol::ProtocolError;

/// Typed failures at the command-line orchestration boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CliError {
    /// A pure domain failure.
    Core(CoreError),
    /// A filesystem-boundary failure.
    Fs(FsError),
    /// A protocol-boundary failure.
    Protocol(ProtocolError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "core error: {error}"),
            Self::Fs(error) => write!(formatter, "filesystem error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Fs(error) => Some(error),
            Self::Protocol(error) => Some(error),
        }
    }
}

impl From<CoreError> for CliError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<FsError> for CliError {
    fn from(error: FsError) -> Self {
        Self::Fs(error)
    }
}

impl From<ProtocolError> for CliError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}
