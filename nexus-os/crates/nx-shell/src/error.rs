//! Failures the shell can report. None of them mutate the OS.

use std::fmt;

/// Recoverable shell error. Hot paths return this instead of panicking.
#[derive(Debug)]
pub enum ShellError {
    Font(String),
    Index(String),
    Backdrop(String),
    Thumbnail(String),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(msg) => write!(f, "font error: {msg}"),
            Self::Index(msg) => write!(f, "index error: {msg}"),
            Self::Backdrop(msg) => write!(f, "backdrop error: {msg}"),
            Self::Thumbnail(msg) => write!(f, "thumbnail error: {msg}"),
        }
    }
}

impl std::error::Error for ShellError {}

pub type ShellResult<T> = Result<T, ShellError>;
