//! Portable listing backend on `std::fs::read_dir` (getdents on Linux).

use super::{Batcher, ListOptions};
use crate::entry::{Attributes, DirEntry, EntryKind};
use crate::error::{FsError, FsResult};
use crate::path::to_native;
use crate::pool::CancelToken;
use std::ops::ControlFlow;
use std::path::Path;

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn list(
    dir: &Path,
    opts: &ListOptions,
    cancel: &CancelToken,
    out: &mut Batcher<'_>,
) -> FsResult<()> {
    let rd = std::fs::read_dir(to_native(dir)).map_err(|e| FsError::from_io(dir, e))?;
    for item in rd {
        if cancel.is_cancelled() {
            return Err(FsError::Cancelled);
        }
        // An entry that vanished between readdir and stat is simply skipped.
        let Ok(item) = item else { continue };
        let Ok(ft) = item.file_type() else { continue };
        let kind = if ft.is_symlink() {
            EntryKind::Symlink
        } else if ft.is_dir() {
            EntryKind::Dir
        } else if ft.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let mut e = DirEntry::new(item.file_name(), kind, 0);
        if opts.with_metadata {
            if let Ok(md) = item.metadata() {
                if kind == EntryKind::File {
                    e.size = md.len();
                }
                e.modified = md.modified().ok();
                e.created = md.created().ok();
                if md.permissions().readonly() {
                    e.attributes.0 |= Attributes::READONLY;
                }
            }
        }
        if out.push(e) == ControlFlow::Break(()) {
            return Err(FsError::Cancelled);
        }
    }
    Ok(())
}
