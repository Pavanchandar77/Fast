//! # nx-fs — the Nexus-OS file-system engine
//!
//! Platform-free core with `cfg(windows)` backends:
//!
//! | Module | What |
//! |---|---|
//! | [`listing`] | streaming directory listing (first rows < 16 ms), `FindFirstFileExW` + `FIND_FIRST_EX_LARGE_FETCH` on Windows |
//! | [`index`] | Everything-style global search: compact path index, fuzzy/prefix matching, frecency, MFT/USN or walker sources |
//! | [`preview`] | Quick Look previews: text, code, images, prioritised + cached + cancellable; `IThumbnailCache` on Windows |
//! | [`ops`] | copy/move/delete/rename jobs: progress, pause/resume, conflicts, Recycle Bin, crash-safe journal, undo |
//! | [`size`] | background folder sizes |
//! | [`tags`] | colour tags and favourites |
//! | [`git`] | optional git status badges |
//! | [`volumes`] | drives for the sidebar |
//!
//! Robustness rules: no `unwrap`/`expect` outside tests, every failure is a
//! typed [`FsError`], long paths go through [`path::to_native`] (`\\?\`),
//! and links/junctions are never followed implicitly.

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod entry;
pub mod error;
pub mod git;
pub mod index;
pub mod listing;
pub mod ops;
pub mod path;
pub mod pool;
pub mod preview;
pub mod size;
pub mod sort;
pub mod tags;
pub mod volumes;
pub mod walk;

mod platform;

pub use entry::{Attributes, DirEntry, EntryKind};
pub use error::{FsError, FsResult};
pub use pool::{CancelToken, Lane, ThreadPool};

/// Windows-only backends (MFT/USN indexer, Recycle Bin, shell thumbnails).
#[cfg(windows)]
pub mod windows {
    pub use crate::platform::windows::recycle::RecycleBin;
    pub use crate::platform::windows::thumbs::ShellThumbnails;
    pub use crate::platform::windows::volume::{drive_letter, NtfsIndexer};
}
