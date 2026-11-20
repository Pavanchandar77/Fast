//! Error types for `nx-compat`.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompatError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("Capability unavailable: {capability:?}, fallback: {fallback}")]
    CapabilityUnavailable {
        capability: crate::probe::Capability,
        fallback: String,
    },

    #[error("Probe failure: {0}")]
    ProbeFailed(String),

    #[error("Journal error: {0}")]
    Journal(String),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Re-apply setting failed: {setting}, rolled back: {rolled_back}")]
    ReapplyFailed { setting: String, rolled_back: bool },
}

pub type CompatResult<T> = Result<T, CompatError>;
