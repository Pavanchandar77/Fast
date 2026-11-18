//! `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` listing.
//!
//! The basic info level skips 8.3 short names and LARGE_FETCH asks the
//! file system for big directory buffers, so size, times and attributes come
//! back with the names in very few kernel calls (no per-file stat).

use super::{wide, win_err};
use crate::entry::{filetime_to_system, Attributes, DirEntry, EntryKind};
use crate::error::{FsError, FsResult};
use crate::listing::{Batcher, ListOptions};
use crate::pool::CancelToken;
use std::ffi::OsString;
use std::ops::ControlFlow;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, FILETIME, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
};

const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_REPARSE: u32 = 0x400;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

struct FindGuard(HANDLE);

impl Drop for FindGuard {
    fn drop(&mut self) {
        let _ = unsafe { FindClose(self.0) };
    }
}

fn ft(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

fn entry_from(data: &WIN32_FIND_DATAW) -> Option<DirEntry> {
    let len = data
        .cFileName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(data.cFileName.len());
    let name = &data.cFileName[..len];
    if name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16] {
        return None;
    }
    let attrs = data.dwFileAttributes;
    let kind = if attrs & ATTR_REPARSE != 0 && data.dwReserved0 == IO_REPARSE_TAG_SYMLINK {
        EntryKind::Symlink
    } else if attrs & ATTR_REPARSE != 0 && data.dwReserved0 == IO_REPARSE_TAG_MOUNT_POINT {
        EntryKind::Junction
    } else if attrs & ATTR_DIRECTORY != 0 {
        // Other reparse tags (OneDrive placeholders, dedup) are normal items.
        EntryKind::Dir
    } else {
        EntryKind::File
    };
    let size = if kind == EntryKind::File {
        (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)
    } else {
        0
    };
    let mut e = DirEntry::new(OsString::from_wide(name), kind, size);
    // Windows semantics: hidden comes from the attribute, not a leading dot.
    e.attributes = Attributes(attrs);
    e.modified = filetime_to_system(ft(data.ftLastWriteTime));
    e.created = filetime_to_system(ft(data.ftCreationTime));
    Some(e)
}

pub(crate) fn list(
    dir: &Path,
    _opts: &ListOptions,
    cancel: &CancelToken,
    out: &mut Batcher<'_>,
) -> FsResult<()> {
    let pattern = wide(&PathBuf::from(dir).join("*"));
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` is NUL-terminated and outlives the call; `data` is a
    // valid WIN32_FIND_DATAW for the basic info level.
    let handle = unsafe {
        FindFirstFileExW(
            windows::core::PCWSTR(pattern.as_ptr()),
            FindExInfoBasic,
            (&mut data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    }
    .map_err(|e| win_err(dir, &e))?;
    let guard = FindGuard(handle);
    loop {
        if cancel.is_cancelled() {
            return Err(FsError::Cancelled);
        }
        if let Some(e) = entry_from(&data) {
            if out.push(e) == ControlFlow::Break(()) {
                return Err(FsError::Cancelled);
            }
        }
        // SAFETY: the handle is valid until `guard` drops.
        if let Err(e) = unsafe { FindNextFileW(guard.0, &mut data) } {
            if e.code() == ERROR_NO_MORE_FILES.to_hresult() {
                return Ok(());
            }
            return Err(win_err(dir, &e));
        }
    }
}
