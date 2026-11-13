use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Win32 Error: {0}")]
    Win32(#[from] windows::core::Error),

    #[error("NTSTATUS Error: 0x{0:08X}")]
    NtStatus(u32),

    #[error("Process not found: PID {0}")]
    ProcessNotFound(u32),

    #[error("Window handle invalid: {0:?}")]
    InvalidHwnd(isize),

    #[error("Privilege escalation failed: {0}")]
    PrivilegeEscalation(String),

    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown Error: {0}")]
    Generic(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
