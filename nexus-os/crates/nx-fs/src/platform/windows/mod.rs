//! Windows backends (compile-checked on Linux with the gnu target; runtime
//! tested only on Windows).

pub(crate) mod listing;
pub(crate) mod recycle;
pub mod thumbs;
pub(crate) mod volume;

use crate::error::FsError;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// NUL-terminated UTF-16 for a native (long-path) version of `p`.
pub(crate) fn wide(p: &Path) -> Vec<u16> {
    crate::path::to_native(p)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// NUL-terminated UTF-16 for a raw string (device paths like `\\.\C:`).
pub(crate) fn wide_str(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Map a `windows::core::Error` to a typed error for `path`.
pub(crate) fn win_err(path: &Path, e: &windows::core::Error) -> FsError {
    let hr = e.code().0 as u32;
    // HRESULT_FROM_WIN32: facility 7.
    let code = if hr & 0xffff_0000 == 0x8007_0000 {
        (hr & 0xffff) as i32
    } else {
        hr as i32
    };
    FsError::from_io(path, std::io::Error::from_raw_os_error(code))
}

/// Initialise COM for the calling thread for the guard's lifetime.
pub(crate) struct ComGuard {
    owned: bool,
}

impl ComGuard {
    pub(crate) fn init() -> ComGuard {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        // S_OK / S_FALSE mean we must balance with CoUninitialize;
        // RPC_E_CHANGED_MODE means COM is already up in another mode (fine).
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        ComGuard { owned: hr.is_ok() }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}
