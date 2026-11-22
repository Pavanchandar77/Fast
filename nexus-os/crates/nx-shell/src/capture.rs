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
    GetWindowLongPtrW, GetWindowRect, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SystemParametersInfoW, GWL_EXSTYLE, LWA_ALPHA, SPI_GETCLIENTAREAANIMATION,
    SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
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
///
/// `SetLayeredWindowAttributes` is intentionally not used. On a DirectComposition
/// window it replaces the per-pixel surface with a solid plate, which is the
/// dark box, and it keeps the plate on top of every click.
pub fn make_click_through(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let bits = WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
        let want = ex | bits as isize;
        if want != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, want);
            let _ = SetWindowPos(
                h,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            // Topmost goes through os-core (own windows only).
            os_core::window::WindowHandle::new(h).set_own_topmost();
        }
    }
}

/// Keep a shell window above fullscreen apps without activating it.
pub fn raise_topmost(window: &mut gpui::Window) {
    if let Some(raw) = gpui_hwnd(window) {
        // Topmost goes through os-core (own windows only), then show.
        os_core::window::WindowHandle::new(hwnd(raw)).set_own_topmost();
        unsafe {
            let _ = SetWindowPos(
                hwnd(raw),
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

/// Let clicks fall through the dock while it is hidden. Does not move or
/// resize the window: `SetWindowPos` from inside a GPUI update re-enters the
/// window procedure and aborts the process.
pub fn set_click_through(raw: isize, on: bool) {
    unsafe {
        let h = hwnd(raw);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let bit = WS_EX_TRANSPARENT.0 as isize;
        let want = if on { ex | bit } else { ex & !bit };
        if want == ex {
            return;
        }
        SetWindowLongPtrW(h, GWL_EXSTYLE, want);
    }
    // Apply the extended style on the window's thread without borrowing GPUI.
    // A synchronous SetWindowPos from the view update re-enters the window
    // procedure and the process exits.
    std::thread::spawn(move || unsafe {
        let _ = SetWindowPos(
            hwnd(raw),
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_FRAMECHANGED
                | SWP_ASYNCWINDOWPOS,
        );
    });
}

fn gpui_hwnd(window: &gpui::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

/// GPUI can show a `show: false` window after the open callback returns.
/// Hide the fullscreen overlay again once that has happened, or it sits on
/// top of every click and paints a dark sheet.
pub fn keep_overlay_hidden(raw: isize) {
    // One hide after GPUI's first show. Repeating it fought the open animation
    // and made windows appear to open and close several times.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            let _ = ShowWindow(hwnd(raw), SW_HIDE);
        }
    });
}

/// Show the overlay only while an effect runs. Even if click-through were
/// refused, input is blocked at most for the length of one animation.
///
/// Must be driven from outside `render`: Windows sends no `WM_PAINT` to a
/// hidden window, so a hidden overlay never renders again and could never
/// re-show itself (v0.1.0 bug: effects queued forever and their window
/// hides never lifted).
pub fn set_overlay_visible(window: &mut gpui::Window, visible: bool) {
    if let Some(raw) = gpui_hwnd(window) {
        set_overlay_visible_raw(raw, visible);
    }
}

pub fn set_overlay_visible_raw(raw: isize, visible: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };
    make_click_through(raw);
    unsafe {
        if visible {
            let _ = SetWindowPos(
                hwnd(raw),
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(hwnd(raw), SW_SHOWNOACTIVATE);
        } else {
            let _ = ShowWindow(hwnd(raw), SW_HIDE);
        }
    }
    tracing::debug!("motion overlay visible={visible}");
}

/// True when `raw` belongs to this process (Nexus's own GPUI windows).
pub fn is_own_window(raw: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd(raw), Some(&mut pid));
    }
    pid != 0 && pid == std::process::id()
}

/// Default lifetime of a [`BloomHide`] before the watchdog lifts it even if
/// the effect never finished. An open/restore effect runs ~0.5 s.
pub const BLOOM_TTL: std::time::Duration = std::time::Duration::from_millis(1500);

struct LiveHide {
    old_ex: isize,
    deadline: std::time::Instant,
}

static LIVE_HIDES: std::sync::Mutex<Vec<(isize, LiveHide)>> = std::sync::Mutex::new(Vec::new());

fn restore_hide(raw: isize, old_ex: isize) {
    unsafe {
        let h = hwnd(raw);
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
        SetWindowLongPtrW(h, GWL_EXSTYLE, old_ex);
    }
}

/// Lift every hide whose deadline passed. Call often (the compositor loop
/// does, every frame). Returns how many were lifted.
pub fn bloom_watchdog() -> usize {
    let now = std::time::Instant::now();
    let expired: Vec<(isize, isize)> = match LIVE_HIDES.lock() {
        Ok(mut live) => {
            let mut out = Vec::new();
            live.retain(|(raw, h)| {
                if h.deadline <= now {
                    out.push((*raw, h.old_ex));
                    false
                } else {
                    true
                }
            });
            out
        }
        Err(_) => Vec::new(),
    };
    for (raw, old_ex) in &expired {
        tracing::warn!("bloom watchdog: effect never finished, restoring window {raw:#x}");
        restore_hide(*raw, *old_ex);
    }
    expired.len()
}

/// Lift every outstanding hide now (shutdown / fail-safe).
pub fn lift_all_hides() {
    let all: Vec<(isize, LiveHide)> = match LIVE_HIDES.lock() {
        Ok(mut live) => std::mem::take(&mut *live),
        Err(_) => Vec::new(),
    };
    for (raw, h) in all {
        restore_hide(raw, h.old_ex);
    }
}

/// Temporarily makes a just-shown foreign window invisible while its
/// snapshot blooms open. Skips windows that are already layered (their
/// alpha belongs to the app). Restores on drop.
pub struct BloomHide {
    raw: isize,
}

impl BloomHide {
    pub fn new(raw: isize) -> Option<Self> {
        Self::with_ttl(raw, BLOOM_TTL)
    }

    /// Hide for at most `ttl`; the watchdog restores it after that.
    /// Never applies to Nexus's own windows.
    pub fn with_ttl(raw: isize, ttl: std::time::Duration) -> Option<Self> {
        if is_own_window(raw) {
            return None;
        }
        if LIVE_HIDES
            .lock()
            .map(|l| l.iter().any(|(r, _)| *r == raw))
            .unwrap_or(true)
        {
            return None;
        }
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
            if let Ok(mut live) = LIVE_HIDES.lock() {
                live.push((
                    raw,
                    LiveHide {
                        old_ex: ex,
                        deadline: std::time::Instant::now() + ttl,
                    },
                ));
            }
            Some(Self { raw })
        }
    }
}

impl Drop for BloomHide {
    fn drop(&mut self) {
        // Restore only if the watchdog has not already done it.
        let entry = LIVE_HIDES.lock().ok().and_then(|mut live| {
            let i = live.iter().position(|(r, _)| *r == self.raw)?;
            Some(live.swap_remove(i).1)
        });
        if let Some(h) = entry {
            restore_hide(self.raw, h.old_ex);
        }
    }
}
