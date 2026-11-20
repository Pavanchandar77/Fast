//! Win32 / DWM host integrations behind `#[cfg(windows)]`, with mock/simulated
//! fallbacks on non-Windows platforms so all host lifecycle logic is testable on Linux.

use crate::backdrop::BackdropChoice;
use crate::error::{ShellError, ShellResult};
use crate::expose::ThumbnailSlot;
use nx_wm::WindowId;
use std::collections::HashMap;

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{BOOL, HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmFlush, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
        DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
        DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
    };

    pub fn apply_backdrop(hwnd: isize, choice: BackdropChoice) -> ShellResult<()> {
        if hwnd == 0 {
            return Err(ShellError::Backdrop("null hwnd".to_string()));
        }
        nx_ui::platform::apply_hwnd(hwnd, choice.request)
            .map_err(|e| ShellError::Backdrop(e.to_string()))
    }

    pub fn dwm_flush() -> ShellResult<()> {
        if !nx_compat::is_capability_operational(nx_compat::Capability::DwmFlush) {
            return Ok(());
        }
        unsafe { DwmFlush().map_err(|e| ShellError::Thumbnail(format!("DwmFlush: {e}"))) }
    }

    pub fn register_thumbnail(dest_hwnd: isize, src_hwnd: isize) -> ShellResult<isize> {
        if dest_hwnd == 0 || src_hwnd == 0 {
            return Err(ShellError::Thumbnail(
                "null dest or source hwnd".to_string(),
            ));
        }
        if !nx_compat::is_capability_operational(nx_compat::Capability::DwmThumbnails) {
            return Err(ShellError::Thumbnail(
                "DWM thumbnail capability unavailable on this build".to_string(),
            ));
        }
        unsafe {
            DwmRegisterThumbnail(HWND(dest_hwnd as *mut _), HWND(src_hwnd as *mut _))
                .map_err(|e| ShellError::Thumbnail(format!("DwmRegisterThumbnail: {e}")))
        }
    }

    pub fn update_thumbnail(thumb_id: isize, slot: &ThumbnailSlot) -> ShellResult<()> {
        if thumb_id == 0 {
            return Err(ShellError::Thumbnail(
                "invalid thumbnail handle".to_string(),
            ));
        }
        let dest = RECT {
            left: slot.dest.x.round() as i32,
            top: slot.dest.y.round() as i32,
            right: slot.dest.right().round() as i32,
            bottom: slot.dest.bottom().round() as i32,
        };
        let flags = DWM_TNP_RECTDESTINATION
            | DWM_TNP_OPACITY
            | DWM_TNP_VISIBLE
            | if slot.client_only {
                DWM_TNP_SOURCECLIENTAREAONLY
            } else {
                0
            };
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: flags,
            rcDestination: dest,
            rcSource: RECT::default(),
            opacity: slot.opacity,
            fVisible: BOOL(if slot.opacity > 0 { 1 } else { 0 }),
            fSourceClientAreaOnly: BOOL(if slot.client_only { 1 } else { 0 }),
        };
        unsafe {
            DwmUpdateThumbnailProperties(thumb_id, &props)
                .map_err(|e| ShellError::Thumbnail(format!("DwmUpdateThumbnailProperties: {e}")))
        }
    }

    pub fn unregister_thumbnail(thumb_id: isize) -> ShellResult<()> {
        if thumb_id == 0 {
            return Ok(());
        }
        unsafe {
            DwmUnregisterThumbnail(thumb_id)
                .map_err(|e| ShellError::Thumbnail(format!("DwmUnregisterThumbnail: {e}")))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicIsize, Ordering};

    static NEXT_THUMB_ID: AtomicIsize = AtomicIsize::new(100);

    pub fn apply_backdrop(hwnd: isize, _choice: BackdropChoice) -> ShellResult<()> {
        if hwnd == 0 {
            return Err(ShellError::Backdrop("null hwnd".to_string()));
        }
        Ok(())
    }

    pub fn dwm_flush() -> ShellResult<()> {
        Ok(())
    }

    pub fn register_thumbnail(dest_hwnd: isize, src_hwnd: isize) -> ShellResult<isize> {
        if dest_hwnd == 0 || src_hwnd == 0 {
            return Err(ShellError::Thumbnail(
                "null dest or source hwnd".to_string(),
            ));
        }
        Ok(NEXT_THUMB_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn update_thumbnail(thumb_id: isize, _slot: &ThumbnailSlot) -> ShellResult<()> {
        if thumb_id == 0 {
            return Err(ShellError::Thumbnail(
                "invalid thumbnail handle".to_string(),
            ));
        }
        Ok(())
    }

    pub fn unregister_thumbnail(_thumb_id: isize) -> ShellResult<()> {
        Ok(())
    }
}

pub use imp::{
    apply_backdrop, dwm_flush, register_thumbnail, unregister_thumbnail, update_thumbnail,
};

/// Manages live DwmRegisterThumbnail handles synchronized with Expose layout slots.
/// Automatically registers new thumbnails, updates properties per frame, and unregisters
/// handles when windows disappear or the manager is dropped.
#[derive(Debug, Default)]
pub struct LiveThumbnailManager {
    handles: HashMap<WindowId, isize>,
}

impl LiveThumbnailManager {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.handles.len()
    }

    pub fn handle_for(&self, window: WindowId) -> Option<isize> {
        self.handles.get(&window).copied()
    }

    /// Synchronize active registrations with the current slots from `ExposeModel::slots`.
    pub fn sync(&mut self, dest_hwnd: isize, slots: &[ThumbnailSlot]) -> ShellResult<()> {
        let active_windows: std::collections::HashSet<WindowId> =
            slots.iter().map(|s| s.window).collect();

        // 1. Unregister thumbnails for windows that left the workspace / Expose
        let removed: Vec<(WindowId, isize)> = self
            .handles
            .iter()
            .filter(|(w, _)| !active_windows.contains(w))
            .map(|(w, h)| (*w, *h))
            .collect();

        for (window, handle) in removed {
            let _ = unregister_thumbnail(handle);
            self.handles.remove(&window);
        }

        // 2. Register or update each slot
        for slot in slots {
            if slot.source_hwnd == 0 {
                continue;
            }
            let handle = match self.handles.get(&slot.window) {
                Some(&h) => h,
                None => {
                    let h = register_thumbnail(dest_hwnd, slot.source_hwnd as isize)?;
                    self.handles.insert(slot.window, h);
                    h
                }
            };
            update_thumbnail(handle, slot)?;
        }

        Ok(())
    }

    /// Clear and unregister all thumbnails.
    pub fn unregister_all(&mut self) {
        for (_, handle) in self.handles.drain() {
            let _ = unregister_thumbnail(handle);
        }
    }
}

impl Drop for LiveThumbnailManager {
    fn drop(&mut self) {
        self.unregister_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_ui::geom::Rect;

    #[test]
    fn manager_registers_updates_and_unregisters() {
        let mut mgr = LiveThumbnailManager::new();
        let slot1 = ThumbnailSlot {
            window: WindowId(1),
            source_hwnd: 0x1234,
            dest: Rect::new(10.0, 10.0, 200.0, 150.0),
            opacity: 255,
            client_only: true,
        };
        let slot2 = ThumbnailSlot {
            window: WindowId(2),
            source_hwnd: 0x5678,
            dest: Rect::new(220.0, 10.0, 200.0, 150.0),
            opacity: 255,
            client_only: true,
        };

        mgr.sync(0x9999, &[slot1, slot2]).expect("sync");
        assert_eq!(mgr.active_count(), 2);
        assert!(mgr.handle_for(WindowId(1)).is_some());
        assert!(mgr.handle_for(WindowId(2)).is_some());

        // Now remove window 1, leaving only window 2
        mgr.sync(0x9999, &[slot2]).expect("sync 2");
        assert_eq!(mgr.active_count(), 1);
        assert!(mgr.handle_for(WindowId(1)).is_none());
        assert!(mgr.handle_for(WindowId(2)).is_some());

        mgr.unregister_all();
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn invalid_hwnd_returns_error() {
        assert!(register_thumbnail(0, 0x1234).is_err());
        assert!(register_thumbnail(0x1234, 0).is_err());
        assert!(apply_backdrop(
            0,
            crate::backdrop::choose(
                &nx_ui::theme::Theme::dark(nx_ui::theme::Accent::Blue),
                nx_ui::platform::SystemBackdrop::None,
                false,
            ),
        )
        .is_err());
    }
}
