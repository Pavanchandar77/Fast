use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WmError {
    #[error("Monitor {0} not found")]
    MonitorNotFound(u32),

    #[error("Workspace {0} not found")]
    WorkspaceNotFound(u8),

    #[error("Window {0:X} not found")]
    WindowNotFound(u64),

    #[error("Keybinding error: {0}")]
    Keybinding(String),

    #[error("Layout error: {0}")]
    Layout(String),
}
