//! One chrome layer for every Nexus window: rounded client frame, resize
//! band, hide-on-close, and show/restore/focus.
//!
//! Call [`install`] from the window's open closure. App windows (Files,
//! Privacy) get a resize band, DWM transitions off (our genie/bloom runs
//! instead), and hide-on-close. Shell and overlay windows stay non-resizable
//! and are not hidden on close.

use nx_ui::gpui_theme::WINDOW_FRAME_INSET;
#[cfg(windows)]
use tracing::{info, warn};

/// How a Nexus window is managed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Dock, island, pop-up: no resize, stays open.
    Shell,
    /// Spotlight, Mission Control: overlay, no resize, no hide-on-close.
    Overlay,
    /// Files, Privacy: resize, hide-on-close, our own min/close motion.
    App,
}

/// Attach the shared chrome to a just-opened window.
///
/// `cx` is required for [`Kind::App`] (hide-on-close). Overlay and shell
/// windows can pass `None`.
pub fn install(window: &mut gpui::Window, cx: Option<&mut gpui::App>, name: &str, kind: Kind) {
    apply_rounded_native_corners(window);
    let resizable = kind == Kind::App;
    native_chrome_with(window, name, resizable);
    if kind == Kind::App {
        if let Some(cx) = cx {
            hide_on_close(window, cx);
        }
    }
}

#[cfg(windows)]
pub(crate) fn window_hwnd(window: &gpui::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

/// Make the whole window client area and log its handle.
///
/// `present_client_window` (nx-ui) already drops the caption, the accent
/// border and DWM rounding. GPUI's own `WM_NCCALCSIZE` still reserves an
/// invisible resize frame (8 px left/right/bottom, 1 px top) whose hit-test
/// codes swallow clicks near the edges; this subclass returns the full rect
/// as client and maps resize/border hit codes to `HTCLIENT`.
/// Like [`native_chrome`], and `resizable` app windows get a thin resize
/// band (the transparent frame inset plus a few px of the body edge).
fn native_chrome_with(window: &mut gpui::Window, name: &str, resizable: bool) {
    #[cfg(windows)]
    {
        match window_hwnd(window) {
            Some(raw) => {
                win_chrome::subclass_client_only(raw, resizable);
                if resizable {
                    // Our genie/bloom replaces the Windows minimize and
                    // restore animation for app windows.
                    win_chrome::disable_dwm_transitions(raw);
                }
                info!("window '{name}' created hwnd={raw:#x} resizable={resizable}");
            }
            None => warn!("window '{name}': no Win32 handle"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, name, resizable);
    }
}

/// Which edge of a window a point sits on, for the resize band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Pure hit test: `(x, y)` relative to the window's top-left, `band` the
/// edge thickness and `corner` the (larger) corner grab size, all in
/// physical pixels.
pub fn resize_edge(w: i32, h: i32, x: i32, y: i32, band: i32, corner: i32) -> Option<ResizeEdge> {
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    let left = x < band;
    let right = x >= w - band;
    let top = y < band;
    let bottom = y >= h - band;
    let near_left = x < corner;
    let near_right = x >= w - corner;
    let near_top = y < corner;
    let near_bottom = y >= h - corner;
    Some(match () {
        _ if (top && near_left) || (left && near_top) => ResizeEdge::TopLeft,
        _ if (top && near_right) || (right && near_top) => ResizeEdge::TopRight,
        _ if (bottom && near_left) || (left && near_bottom) => ResizeEdge::BottomLeft,
        _ if (bottom && near_right) || (right && near_bottom) => ResizeEdge::BottomRight,
        _ if left => ResizeEdge::Left,
        _ if right => ResizeEdge::Right,
        _ if top => ResizeEdge::Top,
        _ if bottom => ResizeEdge::Bottom,
        _ => return None,
    })
}

#[cfg(windows)]
mod win_chrome {
    use windows::Win32::Foundation::HWND;

    const SUBCLASS_ID: usize = 0x4E58_4253; // "NXBS"

    pub fn disable_dwm_transitions(raw: isize) {
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
        };
        let on: i32 = 1;
        unsafe {
            let _ = DwmSetWindowAttribute(
                HWND(raw as *mut std::ffi::c_void),
                DWMWA_TRANSITIONS_FORCEDISABLED,
                std::ptr::from_ref(&on).cast(),
                std::mem::size_of::<i32>() as u32,
            );
        }
    }

    /// Install the client-only subclass. Styles and DWM attributes stay as
    /// `present_client_window` set them.
    pub fn subclass_client_only(raw: isize, resizable: bool) {
        use windows::Win32::UI::Shell::SetWindowSubclass;
        unsafe {
            let _ = SetWindowSubclass(
                HWND(raw as *mut std::ffi::c_void),
                Some(client_only_proc),
                SUBCLASS_ID,
                usize::from(resizable),
            );
        }
    }

    unsafe extern "system" fn client_only_proc(
        hwnd: HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
        _id: usize,
        data: usize,
    ) -> windows::Win32::Foundation::LRESULT {
        use windows::Win32::Foundation::LRESULT;
        use windows::Win32::UI::Shell::DefSubclassProc;
        use windows::Win32::UI::WindowsAndMessaging::{
            HTBORDER, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
            HTTOPLEFT, HTTOPRIGHT, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT,
        };
        match msg {
            WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
            WM_NCHITTEST => {
                if data == 1 {
                    if let Some(code) = resize_hit(hwnd, lparam) {
                        return LRESULT(code as isize);
                    }
                }
                // Resize/border codes become client; caption and window
                // buttons (from GPUI drag areas) pass through unchanged.
                let hit = DefSubclassProc(hwnd, msg, wparam, lparam);
                match hit.0 as u32 {
                    HTLEFT | HTRIGHT | HTTOP | HTTOPLEFT | HTTOPRIGHT | HTBOTTOM | HTBOTTOMLEFT
                    | HTBOTTOMRIGHT | HTBORDER => LRESULT(HTCLIENT as isize),
                    _ => hit,
                }
            }
            // Start the system sizing loop ourselves so GPUI's non-client
            // mouse handling cannot swallow the press on the resize band.
            windows::Win32::UI::WindowsAndMessaging::WM_NCLBUTTONDOWN
                if data == 1
                    && matches!(
                        wparam.0 as u32,
                        HTLEFT
                            | HTRIGHT
                            | HTTOP
                            | HTTOPLEFT
                            | HTTOPRIGHT
                            | HTBOTTOM
                            | HTBOTTOMLEFT
                            | HTBOTTOMRIGHT
                    ) =>
            {
                windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            // TRUE keeps the activation change; nothing is painted.
            WM_NCACTIVATE => LRESULT(1),
            WM_NCPAINT => LRESULT(0),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }

    /// Resize band for app windows: the transparent frame inset around the
    /// rounded body plus 4 px of the body edge (8 px at the corners).
    unsafe fn resize_hit(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> Option<u32> {
        use super::{resize_edge, ResizeEdge};
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, IsZoomed, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP,
            HTTOPLEFT, HTTOPRIGHT,
        };
        if IsZoomed(hwnd).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32 - r.left;
        let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32 - r.top;
        let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let inset = nx_ui::gpui_theme::WINDOW_FRAME_INSET * dpi;
        let band = (inset + 4.0 * dpi).round() as i32;
        let corner = (inset + 12.0 * dpi).round() as i32;
        let edge = resize_edge(r.right - r.left, r.bottom - r.top, x, y, band, corner)?;
        Some(match edge {
            ResizeEdge::Left => HTLEFT,
            ResizeEdge::Right => HTRIGHT,
            ResizeEdge::Top => HTTOP,
            ResizeEdge::Bottom => HTBOTTOM,
            ResizeEdge::TopLeft => HTTOPLEFT,
            ResizeEdge::TopRight => HTTOPRIGHT,
            ResizeEdge::BottomLeft => HTBOTTOMLEFT,
            ResizeEdge::BottomRight => HTBOTTOMRIGHT,
        })
    }
}

/// Nexus app windows (Files, Privacy) hide on close, like a macOS app whose
/// last window closed: the dock icon brings the same window back. Destroying
/// it left the dock holding a dead handle, so the icon did nothing.
pub(crate) fn hide_on_close(window: &mut gpui::Window, cx: &mut gpui::App) {
    window.on_window_should_close(cx, |window, _cx| {
        #[cfg(windows)]
        if let Some(raw) = window_hwnd(window) {
            nx_ui::platform::hide_async(raw);
            info!("window {raw:#x} hidden on close");
            return false;
        }
        let _ = window;
        true
    });
}

/// Bring a Nexus app window back: unhide, restore from minimized, focus.
pub fn show_app_window(window: &mut gpui::Window) {
    #[cfg(windows)]
    if let Some(raw) = window_hwnd(window) {
        nx_ui::platform::restore_and_focus(raw);
        return;
    }
    window.activate_window();
}

pub(crate) fn apply_rounded_native_corners(window: &mut gpui::Window) {
    window.set_client_inset(gpui::px(WINDOW_FRAME_INSET));
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                // Client chrome only. The system frame is the blue bar and the
                // reason clicks never reach the buttons drawn inside.
                let _ = nx_ui::platform::present_client_window(win32.hwnd.get());
            }
        }
    }
    let _ = window;
}

#[cfg(test)]
mod tests {
    use super::{resize_edge, ResizeEdge};

    #[test]
    fn resize_band_finds_edges_and_corners() {
        let (w, h) = (1000, 700);
        assert_eq!(resize_edge(w, h, 500, 350, 24, 32), None);
        assert_eq!(resize_edge(w, h, 2, 350, 24, 32), Some(ResizeEdge::Left));
        assert_eq!(resize_edge(w, h, 999, 350, 24, 32), Some(ResizeEdge::Right));
        assert_eq!(resize_edge(w, h, 500, 0, 24, 32), Some(ResizeEdge::Top));
        assert_eq!(
            resize_edge(w, h, 500, 690, 24, 32),
            Some(ResizeEdge::Bottom)
        );
        assert_eq!(resize_edge(w, h, 3, 3, 24, 32), Some(ResizeEdge::TopLeft));
        assert_eq!(
            resize_edge(w, h, 990, 30, 24, 32),
            Some(ResizeEdge::TopRight)
        );
        assert_eq!(
            resize_edge(w, h, 30, 695, 24, 32),
            Some(ResizeEdge::BottomLeft)
        );
        assert_eq!(
            resize_edge(w, h, 995, 695, 24, 32),
            Some(ResizeEdge::BottomRight)
        );
        assert_eq!(resize_edge(w, h, -1, 10, 24, 32), None);
    }
}
