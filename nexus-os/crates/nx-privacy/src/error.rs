use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ShieldError {
    #[error("Backend error: {0}")]
    Backend(String),

    #[error("Hosts format error: {0}")]
    HostsFormat(String),

    #[error("WFP rule validation error: {0}")]
    WfpValidation(String),

    #[error("Rollback execution error: {0}")]
    Rollback(String),

    #[error("Tracker list parse error: {0}")]
    TrackerParse(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Invalid system state: {0}")]
    InvalidState(String),
}
