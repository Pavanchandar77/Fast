//! Hot corners: park the pointer in a screen corner for a beat and Nexus
//! fires that corner's action once (top-left Mission Control, top-right
//! Spotlight, bottom-right Show Desktop). The corner re-arms only after the
//! pointer leaves it, so it never repeats while you sit there.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Pointer must stay this long in the corner (ms).
pub const DWELL_MS: u64 = 140;
/// Corner hit box, physical px from the edge.
pub const ZONE_PX: i32 = 2;

#[derive(Debug, Default)]
pub struct HotCorners {
    inside: Option<(Corner, u64)>,
    fired: bool,
}

pub fn corner_at(x: i32, y: i32, left: i32, top: i32, right: i32, bottom: i32) -> Option<Corner> {
    let l = x < left + ZONE_PX;
    let r = x >= right - ZONE_PX;
    let t = y < top + ZONE_PX;
    let b = y >= bottom - ZONE_PX;
    match (l, r, t, b) {
        (true, _, true, _) => Some(Corner::TopLeft),
        (_, true, true, _) => Some(Corner::TopRight),
        (true, _, _, true) => Some(Corner::BottomLeft),
        (_, true, _, true) => Some(Corner::BottomRight),
        _ => None,
    }
}

impl HotCorners {
    /// Feed the current corner (or None) and the time; returns a corner the
    /// moment its dwell completes. `blocked` (a button held, a fullscreen
    /// app in front) keeps the corner from firing.
    pub fn update(&mut self, corner: Option<Corner>, now_ms: u64, blocked: bool) -> Option<Corner> {
        match (corner, self.inside) {
            (None, _) => {
                self.inside = None;
                self.fired = false;
                None
            }
            (Some(c), Some((prev, since))) if c == prev => {
                if !self.fired && !blocked && now_ms.saturating_sub(since) >= DWELL_MS {
                    self.fired = true;
                    return Some(c);
                }
                None
            }
            (Some(c), _) => {
                self.inside = Some((c, now_ms));
                self.fired = false;
                None
            }
        }
    }
}

/// Windows: sample the pointer against the primary monitor and report a
/// corner whose dwell just completed.
#[cfg(windows)]
pub fn poll(state: &mut HotCorners, now_ms: u64) -> Option<Corner> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN,
        SM_CYSCREEN,
    };
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return None;
    }
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let corner = corner_at(p.x, p.y, 0, 0, w, h);
    let mut blocked = false;
    if corner.is_some() {
        let held = unsafe {
            (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0
        };
        // A fullscreen game or video in front owns the corners.
        let fullscreen = unsafe {
            let fg = GetForegroundWindow();
            let mut r = RECT::default();
            !fg.0.is_null()
                && GetWindowRect(fg, &mut r).is_ok()
                && r.left <= 0
                && r.top <= 0
                && r.right >= w
                && r.bottom >= h
                && !crate::capture::is_own_window(fg.0 as isize)
                && !is_desktop(fg)
        };
        blocked = held || fullscreen;
    }
    state.update(corner, now_ms, blocked)
}

#[cfg(windows)]
fn is_desktop(hwnd: windows::Win32::Foundation::HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf) };
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
}

/// Windows: Show Desktop (Win+D), the same toggle Explorer uses.
#[cfg(windows)]
pub fn show_desktop() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_D, VK_LWIN,
    };
    unsafe {
        keybd_event(VK_LWIN.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_D.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_D.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_map_from_the_edges() {
        assert_eq!(corner_at(0, 0, 0, 0, 1920, 1080), Some(Corner::TopLeft));
        assert_eq!(corner_at(1919, 0, 0, 0, 1920, 1080), Some(Corner::TopRight));
        assert_eq!(
            corner_at(1919, 1079, 0, 0, 1920, 1080),
            Some(Corner::BottomRight)
        );
        assert_eq!(
            corner_at(0, 1079, 0, 0, 1920, 1080),
            Some(Corner::BottomLeft)
        );
        assert_eq!(corner_at(5, 0, 0, 0, 1920, 1080), None);
    }

    #[test]
    fn fires_once_after_dwell_and_rearms_on_leave() {
        let mut hc = HotCorners::default();
        let c = Some(Corner::TopLeft);
        assert_eq!(hc.update(c, 0, false), None);
        assert_eq!(hc.update(c, 100, false), None);
        assert_eq!(hc.update(c, 150, false), c);
        assert_eq!(hc.update(c, 400, false), None);
        assert_eq!(hc.update(None, 410, false), None);
        assert_eq!(hc.update(c, 420, false), None);
        assert_eq!(hc.update(c, 600, false), c);
    }

    #[test]
    fn blocked_never_fires() {
        let mut hc = HotCorners::default();
        let c = Some(Corner::BottomRight);
        hc.update(c, 0, true);
        assert_eq!(hc.update(c, 500, true), None);
    }
}
