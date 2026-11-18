//! Shell Recycle Bin via `IFileOperation`, with restore for undo.

use super::{wide, win_err, ComGuard};
use crate::error::{FsError, FsResult, IoContext};
use crate::ops::trash::{
    parse_recycle_info, pick_recycled, recycle_data_path, recycle_info_path, Trash, TrashedItem,
};
use crate::path::to_native;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOFX_EARLYFAILURE,
    FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING,
};

/// The Windows Recycle Bin.
#[derive(Debug, Default)]
pub struct RecycleBin;

impl RecycleBin {
    pub fn new() -> Self {
        RecycleBin
    }
}

/// Scan `X:\$Recycle.Bin\<SID>\$I*` files we can read.
fn recycled_candidates(original: &Path) -> Vec<(PathBuf, String, u64)> {
    let Some(letter) = super::volume::drive_letter(original) else {
        return Vec::new();
    };
    let bin = PathBuf::from(format!(r"{letter}:\$Recycle.Bin"));
    let mut out = Vec::new();
    let Ok(sids) = std::fs::read_dir(to_native(&bin)) else {
        return out;
    };
    for sid in sids.flatten() {
        let Ok(items) = std::fs::read_dir(sid.path()) else {
            continue;
        };
        for it in items.flatten() {
            let name = it.file_name();
            if !name.to_string_lossy().starts_with("$I") {
                continue;
            }
            if let Ok(bytes) = std::fs::read(it.path()) {
                if let Ok((orig, _size, time)) = parse_recycle_info(&bytes) {
                    out.push((it.path(), orig, time));
                }
            }
        }
    }
    out
}

impl Trash for RecycleBin {
    fn trash(&self, path: &Path) -> FsResult<TrashedItem> {
        let _com = ComGuard::init();
        let w = wide(path);
        // SAFETY: COM is initialised on this thread; all pointers are valid
        // for the duration of the calls.
        unsafe {
            let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)
                .map_err(|e| win_err(path, &e))?;
            // NUKEWARNING: if an item is too big for the bin, the shell asks
            // instead of silently deleting it for good.
            op.SetOperationFlags(
                FOF_ALLOWUNDO
                    | FOF_NOCONFIRMATION
                    | FOF_SILENT
                    | FOF_NOERRORUI
                    | FOF_WANTNUKEWARNING
                    | FOFX_RECYCLEONDELETE
                    | FOFX_EARLYFAILURE,
            )
            .map_err(|e| win_err(path, &e))?;
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)
                .map_err(|e| win_err(path, &e))?;
            op.DeleteItem(&item, None).map_err(|e| win_err(path, &e))?;
            op.PerformOperations().map_err(|e| win_err(path, &e))?;
            if op
                .GetAnyOperationsAborted()
                .map(|b| b.as_bool())
                .unwrap_or(false)
            {
                return Err(FsError::Cancelled);
            }
        }
        let original = PathBuf::from(crate::path::strip_extended_windows(&path.to_string_lossy()));
        let info = pick_recycled(&recycled_candidates(&original), &original.to_string_lossy());
        let location = info.and_then(|i| recycle_data_path(&i)).unwrap_or_default();
        Ok(TrashedItem { original, location })
    }

    fn restore(&self, item: &TrashedItem) -> FsResult<()> {
        if item.location.as_os_str().is_empty() {
            return Err(FsError::Unsupported(
                "item was not found in the Recycle Bin",
            ));
        }
        if std::fs::symlink_metadata(to_native(&item.original)).is_ok() {
            return Err(FsError::AlreadyExists {
                path: item.original.clone(),
            });
        }
        if let Some(parent) = item.original.parent() {
            std::fs::create_dir_all(to_native(parent)).at(parent)?;
        }
        std::fs::rename(to_native(&item.location), to_native(&item.original)).at(&item.location)?;
        if let Some(info) = recycle_info_path(&item.location) {
            let _ = std::fs::remove_file(to_native(&info));
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Recycle Bin"
    }
}
