//! Win32 glue for the motion overlay: window snapshots, the OS animation
//! setting, click-through overlay styles, and a reversible hide used while a
//! foreign window blooms open.
//!
//! Every call is best effort. A failed capture falls back to a placeholder
//! silhouette, and every style change made to another app's window is undone
//! by [`BloomHide`]'s `Drop`, so a crash in an effect cannot leave a window
//! invisible.

use windows::Win32::Foundation::{BOOL, COLORREF, HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow,
    SystemParametersInfoW, GWL_EXSTYLE, LWA_ALPHA, SPI_GETCLIENTAREAANIMATION, SW_HIDE,
    SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// `PW_RENDERFULLCONTENT`: capture DirectComposition / GPU content too.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

/// Window rect in screen pixels.
pub fn window_rect(raw: isize) -> Option<nx_ui::Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd(raw), &mut r).ok()? };
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    (w > 0 && h > 0).then(|| nx_ui::Rect::new(r.left as f32, r.top as f32, w as f32, h as f32))
}

/// Capture a window as packed BGRA. Tries `PrintWindow` (works while
/// occluded), then a screen blit of its rect. Downscales by `step` (1 = full).
pub fn capture_bgra(raw: isize, step: u32) -> Option<(u32, u32, Vec<u8>)> {
    let rect = window_rect(raw)?;
    let (w, h) = (rect.w as i32, rect.h as i32);
    if w > 8192 || h > 8192 {
        return None;
    }
    unsafe {
        let screen = GetDC(HWND::default());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        let mut ok = PrintWindow(hwnd(raw), mem, PW_RENDERFULLCONTENT).as_bool();
        if !ok {
            ok = BitBlt(
                mem,
                0,
                0,
                w,
                h,
                screen,
                rect.x as i32,
                rect.y as i32,
                SRCCOPY,
            )
            .is_ok();
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = if ok {
            GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(buf.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), screen);
        if lines != h {
            return None;
        }
        // GDI leaves alpha at 0; the overlay needs opaque pixels.
        for px in buf.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Some(downscale(w as u32, h as u32, buf, step.max(1)))
    }
}

fn downscale(w: u32, h: u32, buf: Vec<u8>, step: u32) -> (u32, u32, Vec<u8>) {
    if step == 1 {
        return (w, h, buf);
    }
    let (nw, nh) = ((w / step).max(1), (h / step).max(1));
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let i = (((y * step) * w + x * step) * 4) as usize;
            out.extend_from_slice(&buf[i..i + 4]);
        }
    }
    (nw, nh, out)
}

/// Windows "Animation effects" (Settings > Accessibility > Visual effects).
/// Defaults to enabled when the query fails.
pub fn os_animations_enabled() -> bool {
    let mut on = BOOL(1);
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut on as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_err() || on.as_bool()
}

/// Turn off DWM's own minimize/restore animation for a window so Nexus's
/// ink genie does not play on top of it. Returns false if DWM refused
/// (some windows reject attribute changes from other processes).
pub fn disable_dwm_transitions(raw: isize) -> bool {
    let on = BOOL(1);
    unsafe {
        DwmSetWindowAttribute(
            hwnd(raw),
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&on as *const BOOL).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
        .is_ok()
    }
}

/// Make the overlay click-through, non-activating, topmost, and hidden from
/// Alt+Tab. Idempotent.
pub fn make_click_through(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let bits = WS_EX_LAYERED.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_TOPMOST.0;
        let want = ex | bits as isize;
        if want != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, want);
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
        }
    }
}

fn gpui_hwnd(window: &gpui::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

/// Show the overlay only while an effect runs. Even if click-through were
/// refused, input is blocked at most for the length of one animation.
pub fn set_overlay_visible(window: &mut gpui::Window, visible: bool) {
    if let Some(raw) = gpui_hwnd(window) {
        make_click_through(raw);
        unsafe {
            let _ = ShowWindow(hwnd(raw), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

/// Temporarily makes a just-shown foreign window invisible while its
/// snapshot blooms open. Skips windows that are already layered (their
/// alpha belongs to the app). Restores on drop.
pub struct BloomHide {
    raw: isize,
    old_ex: isize,
}

impl BloomHide {
    pub fn new(raw: isize) -> Option<Self> {
        unsafe {
            let h = hwnd(raw);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            if ex & WS_EX_LAYERED.0 as isize != 0 {
                return None;
            }
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            if SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA).is_err() {
                SetWindowLongPtrW(h, GWL_EXSTYLE, ex);
                return None;
            }
            Some(Self { raw, old_ex: ex })
        }
    }
}

impl Drop for BloomHide {
    fn drop(&mut self) {
        unsafe {
            let h = hwnd(self.raw);
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
            SetWindowLongPtrW(h, GWL_EXSTYLE, self.old_ex);
        }
    }
}
