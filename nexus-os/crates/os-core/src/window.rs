#[cfg(windows)]
use crate::error::{CoreError, CoreResult};
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::hash::{Hash, Hasher};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, RECT};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SetWindowPos,
    GA_ROOTOWNER, GWL_EXSTYLE, GWL_STYLE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOZORDER,
    WINDOW_LONG_PTR_INDEX, WS_CHILD, WS_EX_TOOLWINDOW, WS_POPUP, WS_THICKFRAME,
};

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowHandle(pub HWND);

#[cfg(windows)]
impl Hash for WindowHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0 .0 as usize).hash(state);
    }
}

#[cfg(windows)]
unsafe impl Send for WindowHandle {}
#[cfg(windows)]
unsafe impl Sync for WindowHandle {}

#[cfg(windows)]
impl WindowHandle {
    pub fn new(hwnd: HWND) -> Self {
        Self(hwnd)
    }

    pub fn raw(&self) -> HWND {
        self.0
    }

    pub fn is_valid(&self) -> bool {
        unsafe { IsWindow(self.0).as_bool() }
    }

    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.0).as_bool() }
    }

    pub fn is_minimized(&self) -> bool {
        unsafe { IsIconic(self.0).as_bool() }
    }

    /// Checks if a window is cloaked by DWM (e.g., suspended UWP apps, virtual desktops)
    pub fn is_cloaked(&self) -> bool {
        let mut cloaked: u32 = 0;
        let hr = unsafe {
            DwmGetWindowAttribute(
                self.0,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        hr.is_ok() && cloaked != 0
    }

    pub fn pid_and_tid(&self) -> (u32, u32) {
        let mut pid: u32 = 0;
        let tid = unsafe { GetWindowThreadProcessId(self.0, Some(&mut pid)) };
        (pid, tid)
    }

    pub fn pid(&self) -> u32 {
        self.pid_and_tid().0
    }

    /// Keep one of Nexus's *own* shell windows (island, dock) above normal
    /// windows. Refuses windows of other processes: changing their z-band
    /// would need a journal entry, and a crashed session must not leave a
    /// foreign window stuck on top.
    pub fn set_own_topmost(&self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        if self.pid() != std::process::id() {
            return false;
        }
        unsafe {
            SetWindowPos(
                self.0,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .is_ok()
        }
    }

    pub fn title(&self) -> String {
        unsafe {
            let length = GetWindowTextLengthW(self.0);
            if length == 0 {
                return String::new();
            }
            let mut buffer = vec![0u16; (length + 1) as usize];
            let read = GetWindowTextW(self.0, &mut buffer);
            String::from_utf16_lossy(&buffer[..read as usize])
        }
    }

    pub fn class_name(&self) -> String {
        unsafe {
            let mut buffer = [0u16; 256];
            let read = GetClassNameW(self.0, &mut buffer);
            if read > 0 {
                String::from_utf16_lossy(&buffer[..read as usize])
            } else {
                String::new()
            }
        }
    }

    pub fn rect(&self) -> CoreResult<RECT> {
        let mut rect = RECT::default();
        let hr = unsafe {
            DwmGetWindowAttribute(
                self.0,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut rect as *mut _ as *mut c_void,
                std::mem::size_of::<RECT>() as u32,
            )
        };
        if hr.is_err() {
            unsafe {
                GetWindowRect(self.0, &mut rect)?;
            }
        }
        Ok(rect)
    }

    pub fn set_rect(&self, rect: RECT) -> CoreResult<()> {
        let previous = match self.rect() {
            Ok(current) => crate::Rect {
                x: current.left,
                y: current.top,
                width: current.right.saturating_sub(current.left),
                height: current.bottom.saturating_sub(current.top),
            },
            Err(_) => rect_to_journal(rect),
        };
        crate::recovery::place_foreign_window(hwnd_bits(self.0), previous, rect_to_journal(rect))
    }

    /// Move a foreign window without waiting for it to process `WM_WINDOWPOSCHANGING`.
    /// A hung target must not stall the shell thread.
    pub(crate) fn place_async(&self, rect: RECT) -> CoreResult<()> {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        unsafe {
            SetWindowPos(
                self.0,
                HWND::default(),
                rect.left,
                rect.top,
                width,
                height,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER,
            )?;
        }
        Ok(())
    }

    pub fn disable_dwm_transitions(&self) -> CoreResult<()> {
        nx_compat::dispatch::safe_set_dwm_transitions(self.0 .0 as isize, false)
            .map_err(|e| CoreError::Generic(e.to_string()))?;
        Ok(())
    }

    /// Comprehensive window manager eligibility filter.
    /// Guards against:
    /// - UWP ghost/invisible cloaked windows
    /// - Context menus, splash screens, tooltips, combobox popups
    /// - Child windows and non-root owner dialogs
    pub fn is_manageable(&self) -> bool {
        if !self.is_valid() || !self.is_visible() {
            return false;
        }

        // 1. Guard against UWP ghost windows (ApplicationFrameWindow) cloaked by DWM
        if self.is_cloaked() {
            return false;
        }

        // 2. Must be its own root owner - prevents hijacking child dialogs, modals, and popups
        let root_owner = unsafe { GetAncestor(self.0, GA_ROOTOWNER) };
        if root_owner != self.0 {
            return false;
        }

        let style = unsafe { GetWindowLongW(self.0, WINDOW_LONG_PTR_INDEX(GWL_STYLE.0)) } as u32;
        let ex_style =
            unsafe { GetWindowLongW(self.0, WINDOW_LONG_PTR_INDEX(GWL_EXSTYLE.0)) } as u32;

        // 3. Skip child windows and tool windows
        if (style & WS_CHILD.0) != 0 {
            return false;
        }
        if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
            return false;
        }

        // 4. Guard against menus, splash screens, and unresizable popups:
        // If WS_POPUP is set, only accept if it has WS_THICKFRAME (resizable modern app with custom chrome)
        if (style & WS_POPUP.0) != 0 && (style & WS_THICKFRAME.0) == 0 {
            return false;
        }

        // 5. Filter known system shell classes
        let class = self.class_name();
        let excluded_classes = [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "Windows.UI.Core.CoreWindow",
            "XamlExplorerHostIslandWindow",
            "PopupHost",
            "DropShadow",
            "tooltips_class32",
        ];
        if excluded_classes.contains(&class.as_str()) {
            return false;
        }

        // 6. Must have non-zero dimension
        if let Ok(rect) = self.rect() {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 20 || height <= 20 {
                return false;
            }
        } else {
            return false;
        }

        true
    }
}

#[cfg(windows)]
pub(crate) fn hwnd_bits(hwnd: HWND) -> u64 {
    hwnd.0 as usize as u64
}

#[cfg(windows)]
pub(crate) fn hwnd_from_bits(bits: u64) -> HWND {
    HWND(bits as usize as *mut std::ffi::c_void)
}

#[cfg(windows)]
fn rect_to_journal(rect: RECT) -> crate::Rect {
    crate::Rect {
        x: rect.left,
        y: rect.top,
        width: rect.right.saturating_sub(rect.left),
        height: rect.bottom.saturating_sub(rect.top),
    }
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HWND(pub *mut std::ffi::c_void);

#[cfg(not(windows))]
unsafe impl Send for HWND {}
#[cfg(not(windows))]
unsafe impl Sync for HWND {}

#[cfg(not(windows))]
impl Default for HWND {
    fn default() -> Self {
        Self(std::ptr::null_mut())
    }
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WindowHandle(pub HWND);

#[cfg(not(windows))]
impl WindowHandle {
    pub fn new(hwnd: HWND) -> Self {
        Self(hwnd)
    }
    pub fn raw(&self) -> HWND {
        self.0
    }
    pub fn is_valid(&self) -> bool {
        true
    }
    pub fn is_visible(&self) -> bool {
        true
    }
    pub fn is_minimized(&self) -> bool {
        false
    }
    pub fn is_cloaked(&self) -> bool {
        false
    }
    pub fn pid_and_tid(&self) -> (u32, u32) {
        (0, 0)
    }
    pub fn pid(&self) -> u32 {
        0
    }
    pub fn title(&self) -> String {
        String::new()
    }
    pub fn class_name(&self) -> String {
        String::new()
    }
    pub fn rect(&self) -> crate::error::CoreResult<RECT> {
        Ok(RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        })
    }
    pub fn set_rect(&self, _rect: RECT) -> crate::error::CoreResult<()> {
        Ok(())
    }
    pub fn disable_dwm_transitions(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }
    pub fn is_manageable(&self) -> bool {
        true
    }
}
