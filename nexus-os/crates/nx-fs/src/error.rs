//! Typed errors for every fallible operation in `nx-fs`.
//!
//! Every error carries the path it relates to (when there is one) so the UI
//! can show a precise, actionable message instead of a generic failure.

use std::io;
use std::path::{Path, PathBuf};

/// Result alias used across the crate.
pub type FsResult<T> = Result<T, FsError>;

/// All errors surfaced by the file-system engine.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    /// The caller lacks permission (ACL, read-only volume, UAC, ...).
    #[error("permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    /// The path does not exist (or vanished while we were working on it).
    #[error("not found: {path}")]
    NotFound { path: PathBuf },

    /// The destination already exists and the conflict policy forbids replacing it.
    #[error("already exists: {path}")]
    AlreadyExists { path: PathBuf },

    /// A symlink / junction cycle was detected.
    #[error("symlink or junction loop at {path}")]
    LinkLoop { path: PathBuf },

    /// A copy or move would place a directory inside itself.
    #[error("cannot copy or move {src} into itself ({dest})")]
    IntoItself { src: PathBuf, dest: PathBuf },

    /// The path is syntactically invalid for this platform.
    #[error("invalid path {path}: {reason}")]
    InvalidPath { path: PathBuf, reason: &'static str },

    /// The operation was cancelled by the user.
    #[error("operation cancelled")]
    Cancelled,

    /// The op journal is unreadable or was written by an incompatible version.
    #[error("op journal {path} is corrupt at line {line}: {reason}")]
    JournalCorrupt {
        path: PathBuf,
        line: usize,
        reason: String,
    },

    /// A persisted store (tags, frecency, ...) could not be parsed.
    #[error("store {path} is corrupt at line {line}")]
    StoreCorrupt { path: PathBuf, line: usize },

    /// Malformed binary data (USN records, image headers, ...).
    #[error("malformed data: {0}")]
    Malformed(String),

    /// The requested feature is not available on this platform or volume.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A background worker went away (thread pool shut down).
    #[error("background worker disconnected")]
    Disconnected,

    /// A native OS call failed.
    #[error("{op} failed (os error {code}) on {path}")]
    Os {
        op: &'static str,
        code: i32,
        path: PathBuf,
    },

    /// Any other I/O error, with the path it happened on.
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl FsError {
    /// Classify an `io::Error` for `path` into the most specific variant.
    pub fn from_io(path: impl AsRef<Path>, err: io::Error) -> Self {
        let path = path.as_ref().to_path_buf();
        match err.kind() {
            io::ErrorKind::PermissionDenied => FsError::PermissionDenied { path },
            io::ErrorKind::NotFound => FsError::NotFound { path },
            io::ErrorKind::AlreadyExists => FsError::AlreadyExists { path },
            _ => {
                // ELOOP (40 on Linux, 62 on macOS) / ERROR_CANT_RESOLVE_FILENAME (1921).
                if matches!(err.raw_os_error(), Some(40) | Some(62) | Some(1921)) {
                    FsError::LinkLoop { path }
                } else {
                    FsError::Io { path, source: err }
                }
            }
        }
    }

    /// True when retrying cannot help and the item should be skipped.
    pub fn is_permission(&self) -> bool {
        matches!(self, FsError::PermissionDenied { .. })
    }

    /// The path this error refers to, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            FsError::PermissionDenied { path }
            | FsError::NotFound { path }
            | FsError::AlreadyExists { path }
            | FsError::LinkLoop { path }
            | FsError::InvalidPath { path, .. }
            | FsError::JournalCorrupt { path, .. }
            | FsError::StoreCorrupt { path, .. }
            | FsError::Os { path, .. }
            | FsError::Io { path, .. } => Some(path),
            FsError::IntoItself { src, .. } => Some(src),
            FsError::Cancelled
            | FsError::Malformed(_)
            | FsError::Unsupported(_)
            | FsError::Disconnected => None,
        }
    }
}

/// Extension to attach a path to `io::Result`s concisely.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> FsResult<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> FsResult<T> {
        self.map_err(|e| FsError::from_io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_are_classified() {
        let e = FsError::from_io("/x", io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(e.is_permission());
        let e = FsError::from_io("/x", io::Error::from(io::ErrorKind::NotFound));
        assert!(matches!(e, FsError::NotFound { .. }));
        let e = FsError::from_io("/x", io::Error::from_raw_os_error(40));
        #[cfg(target_os = "linux")]
        assert!(matches!(e, FsError::LinkLoop { .. }));
        assert_eq!(e.path(), Some(Path::new("/x")));
    }
}
