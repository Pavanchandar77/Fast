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

/// The point one pixel diagonally outward from `corner` at (x, y).
pub fn outward(corner: Corner, x: i32, y: i32) -> (i32, i32) {
    match corner {
        Corner::TopLeft => (x - 1, y - 1),
        Corner::TopRight => (x + 1, y - 1),
        Corner::BottomLeft => (x - 1, y + 1),
        Corner::BottomRight => (x + 1, y + 1),
    }
}

/// Windows: sample the pointer against the monitor it is on (every
/// display has its own corners). A corner that touches another display is
/// not a corner the pointer can rest in, so it is skipped.
#[cfg(windows)]
pub fn poll(state: &mut HotCorners, now_ms: u64) -> Option<Corner> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTONULL,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowRect,
    };
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return None;
    }
    let mon = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
        return state.update(None, now_ms, false);
    }
    let m = info.rcMonitor;
    let mut corner = corner_at(p.x, p.y, m.left, m.top, m.right, m.bottom);
    if let Some(c) = corner {
        let (ox, oy) = outward(c, p.x, p.y);
        let beyond = unsafe { MonitorFromPoint(POINT { x: ox, y: oy }, MONITOR_DEFAULTTONULL) };
        if !beyond.is_invalid() {
            corner = None;
        }
    }
    let mut blocked = false;
    if corner.is_some() {
        let held = unsafe {
            (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0
        };
        // A fullscreen game or video on this display owns its corners.
        let fullscreen = unsafe {
            let fg = GetForegroundWindow();
            let mut r = RECT::default();
            !fg.0.is_null()
                && GetWindowRect(fg, &mut r).is_ok()
                && r.left <= m.left
                && r.top <= m.top
                && r.right >= m.right
                && r.bottom >= m.bottom
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
    fn second_monitor_corners_use_its_own_rect() {
        // A display to the right of a 1920-wide primary.
        assert_eq!(
            corner_at(3839, 0, 1920, 0, 3840, 1080),
            Some(Corner::TopRight)
        );
        assert_eq!(
            corner_at(1920, 0, 1920, 0, 3840, 1080),
            Some(Corner::TopLeft)
        );
        assert_eq!(outward(Corner::TopLeft, 1920, 0), (1919, -1));
    }

    #[test]
    fn blocked_never_fires() {
        let mut hc = HotCorners::default();
        let c = Some(Corner::BottomRight);
        hc.update(c, 0, true);
        assert_eq!(hc.update(c, 500, true), None);
    }
}
