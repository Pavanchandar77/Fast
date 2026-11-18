use crate::types::WindowId;
use std::path::PathBuf;
use thiserror::Error;

/// Errors produced across power-feature operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PowerError {
    #[error("Clipboard item rejected: {0}")]
    ClipboardRejected(String),

    #[error("Window {0:?} not found")]
    WindowNotFound(WindowId),

    #[error("Snap layout error: {0}")]
    SnapError(String),

    #[error("Zone {0} not found in layout '{1}'")]
    ZoneNotFound(u32, String),

    #[error("Batch rename error: {0}")]
    RenameError(String),

    #[error("Rename collision for target path: {0}")]
    RenameCollision(PathBuf),

    #[error("Invalid path name: {0}")]
    InvalidPathName(String),

    #[error("Protected system shortcut cannot be stolen: {0}")]
    ProtectedShortcut(String),

    #[error("Circular key remap detected: {0}")]
    CircularRemap(String),

    #[error("Audio session '{0}' not found")]
    AudioSessionNotFound(String),

    #[error("Volume level {0:.2} out of bounds (must be 0.0..=1.0)")]
    InvalidVolume(String),

    #[error("Note {0} not found")]
    NoteNotFound(u64),

    #[error("Tab group {0} not found")]
    TabGroupNotFound(u64),

    #[error("Invalid color format: {0}")]
    InvalidColor(String),

    #[error("Alt-Tab error: {0}")]
    AltTabError(String),

    #[error("Operation is a no-op")]
    NoOp,
}

pub type PowerResult<T> = Result<T, PowerError>;
