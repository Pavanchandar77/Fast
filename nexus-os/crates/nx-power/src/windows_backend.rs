use crate::error::PowerResult;
use crate::types::WindowId;

#[cfg(windows)]
mod imp {
    use super::*;
    use nexus_model::layout::WindowId as ModelWindowId;
    use nexus_model::platform::Platform;
    use os_core::platform::Win32Platform;
    use windows::Win32::Foundation::{BOOL, HWND};

    #[link(name = "user32")]
    extern "system" {
        fn AddClipboardFormatListener(hwnd: HWND) -> BOOL;
        fn RemoveClipboardFormatListener(hwnd: HWND) -> BOOL;
    }

    pub fn set_always_on_top(window_id: WindowId, topmost: bool) -> PowerResult<()> {
        Win32Platform::new()
            .set_always_on_top(ModelWindowId::new(window_id.0), topmost)
            .map_err(|_| crate::error::PowerError::WindowNotFound(window_id))
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
