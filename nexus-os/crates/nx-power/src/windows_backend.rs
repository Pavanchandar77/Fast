use crate::error::PowerResult;
use crate::types::WindowId;

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{BOOL, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE,
    };

    #[link(name = "user32")]
    extern "system" {
        fn AddClipboardFormatListener(hwnd: HWND) -> BOOL;
        fn RemoveClipboardFormatListener(hwnd: HWND) -> BOOL;
    }

    pub fn set_always_on_top(window_id: WindowId, topmost: bool) -> PowerResult<()> {
        let hwnd = HWND(window_id.as_raw() as *mut _);
        let insert_after = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };

        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
        let success = unsafe { SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, flags) };

        if success.is_ok() {
            Ok(())
        } else {
            Err(crate::error::PowerError::WindowNotFound(window_id))
        }
    }

    /// Registers a clipboard listener HWND via AddClipboardFormatListener.
    pub fn register_clipboard_listener(listener_hwnd: WindowId) -> PowerResult<()> {
        let hwnd = HWND(listener_hwnd.as_raw() as *mut _);
        let success = unsafe { AddClipboardFormatListener(hwnd) };
        if success.as_bool() {
            Ok(())
        } else {
            Err(crate::error::PowerError::ClipboardRejected(
                "Failed to register clipboard format listener".to_string(),
            ))
        }
    }

    /// Unregisters clipboard listener HWND.
    pub fn unregister_clipboard_listener(listener_hwnd: WindowId) -> PowerResult<()> {
        let hwnd = HWND(listener_hwnd.as_raw() as *mut _);
        let _ = unsafe { RemoveClipboardFormatListener(hwnd) };
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn set_always_on_top(_window_id: WindowId, _topmost: bool) -> PowerResult<()> {
        // Mock implementation on non-Windows targets
        Ok(())
    }

    pub fn register_clipboard_listener(_listener_hwnd: WindowId) -> PowerResult<()> {
        Ok(())
    }

    pub fn unregister_clipboard_listener(_listener_hwnd: WindowId) -> PowerResult<()> {
        Ok(())
    }
}

pub use imp::*;
