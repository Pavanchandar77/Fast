//! Dynamic Island: a floating monochrome pill at the top-center of the screen.
//!
//! Behaviour is inspired by (but does not vendor) MIT-licensed DynamicNotchKit
//! and NotchDrop, and the hover-to-expand HUD pattern used by Alcove-style
//! floating islands and GPL-licensed Boring Notch. This module is a native
//! GPUI reimplementation.

use crate::clock::ClockSample;

/// Collapsed pill window height (including transparent padding).
pub const COLLAPSED_HEIGHT: f32 = 60.0;
/// Expanded control-center window height (assistant + activities + controls).
pub const EXPANDED_HEIGHT: f32 = 720.0;
/// Fullscreen slit window height: a few px of pill hugging the top edge.
pub const SLIT_HEIGHT: f32 = 14.0;
/// Peek (notification / pop-up) window height.
pub const PEEK_HEIGHT: f32 = 92.0;
/// Island window width.
pub const ISLAND_WIDTH: f32 = 680.0;

/// What the island is showing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslandShape {
    /// Fullscreen app in front: a thin slit at the very top edge.
    Slit,
    /// Normal resting pill.
    Pill,
    /// A notification / pop-up briefly expanded; retracts on its own.
    Peek,
    /// Click-opened control center + assistant.
    Expanded,
}

impl IslandShape {
    pub fn window_height(self) -> f32 {
        match self {
            IslandShape::Slit => SLIT_HEIGHT,
            IslandShape::Pill => COLLAPSED_HEIGHT,
            IslandShape::Peek => PEEK_HEIGHT,
            IslandShape::Expanded => EXPANDED_HEIGHT,
        }
    }
}

/// Battery notices that peek the island.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryAlert {
    /// Dropped below 50 / 20 / 10 percent.
    Below(u8),
    /// At or below 5 percent.
    Critical,
    Plugged,
    Unplugged,
}

pub const BATTERY_THRESHOLDS: [u8; 3] = [50, 20, 10];
pub const BATTERY_CRITICAL: u8 = 5;

#[derive(Clone, Debug)]
pub struct IslandModel {
    /// Click-opened control center. Hover never expands.
    pub expanded: bool,
    /// Pointer is over the island (drives only a subtle highlight).
    pub hovered: bool,
    /// A fullscreen app is in front: rest as a slit instead of a pill.
    pub fullscreen: bool,
    /// Seconds (view clock) until which a peek is shown.
    pub peek_until: Option<f64>,
    last_battery: Option<u8>,
    last_charging: Option<bool>,
    pub clock: ClockSample,
    pub hour24: bool,
    pub volume: f32,
    pub muted: bool,
    pub brightness: f32,
    pub wifi: bool,
    pub bluetooth: bool,
    pub battery_pct: Option<u8>,
    pub charging: bool,
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    /// Used fraction of the system drive, 0..1.
    pub disk: f32,
    pub now_playing: Option<(String, String)>,
    pub notifications: Vec<String>,
    cpu_prev: Option<(u64, u64)>,
}

impl Default for IslandModel {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandModel {
    pub fn new() -> Self {
        let mut model = Self {
            expanded: false,
            hovered: false,
            fullscreen: false,
            peek_until: None,
            last_battery: None,
            last_charging: None,
            clock: ClockSample::now_local(),
            hour24: true,
            volume: 0.62,
            muted: false,
            brightness: 0.78,
            wifi: true,
            bluetooth: false,
            battery_pct: None,
            charging: false,
            cpu: 0.0,
            ram_used: 0,
            ram_total: 0,
            disk: 0.0,
            now_playing: None,
            notifications: Vec::new(),
            cpu_prev: None,
        };
        model.cpu_prev = sample_cpu();
        model.tick();
        model
    }

    pub fn clock_label(&self) -> String {
        self.clock.format(self.hour24).to_ascii_uppercase()
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// Click toggles the control center (the only way to expand).
    pub fn click(&mut self) {
        self.expanded = !self.expanded;
        if self.expanded {
            self.peek_until = None;
        }
    }

    /// Esc or a click outside the island returns to the resting shape.
    pub fn dismiss(&mut self) {
        self.expanded = false;
    }

    /// Hover only highlights; it never changes the shape.
    pub fn hover(&mut self, inside: bool) {
        self.hovered = inside;
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
    }

    /// Briefly expand for a notification / pop-up, then retract.
    pub fn peek(&mut self, now: f64, secs: f64) {
        let until = now + secs;
        self.peek_until = Some(self.peek_until.map_or(until, |u| u.max(until)));
    }

    pub fn shape(&self, now: f64) -> IslandShape {
        if self.expanded {
            IslandShape::Expanded
        } else if self.peek_until.is_some_and(|u| now < u) {
            IslandShape::Peek
        } else if self.fullscreen {
            IslandShape::Slit
        } else {
            IslandShape::Pill
        }
    }

    /// Feed a battery sample; returns alerts that should peek the island.
    /// Each threshold fires once per downward crossing (not while it stays
    /// below), never while charging; plug/unplug fire on change.
    pub fn battery_sample(&mut self, pct: Option<u8>, charging: bool) -> Vec<BatteryAlert> {
        let mut out = Vec::new();
        let Some(cur) = pct else {
            return out;
        };
        if let Some(was) = self.last_charging {
            if was != charging {
                out.push(if charging {
                    BatteryAlert::Plugged
                } else {
                    BatteryAlert::Unplugged
                });
            }
        }
        if let Some(prev) = self.last_battery {
            if !charging && cur < prev {
                if cur <= BATTERY_CRITICAL && prev > BATTERY_CRITICAL {
                    out.push(BatteryAlert::Critical);
                } else if let Some(t) = BATTERY_THRESHOLDS
                    .iter()
                    .copied()
                    .filter(|t| cur < *t && prev >= *t)
                    .min()
                {
                    out.push(BatteryAlert::Below(t));
                }
            }
        }
        self.last_battery = Some(cur);
        self.last_charging = Some(charging);
        out
    }

    /// Refresh clock, CPU, RAM, and battery from the host.
    pub fn tick(&mut self) {
        self.clock = ClockSample::now_local();
        let (used, total) = sample_memory();
        self.ram_used = used;
        self.ram_total = total;
        if let Some(cur) = sample_cpu() {
            if let Some((prev_idle, prev_total)) = self.cpu_prev {
                let d_total = cur.1.saturating_sub(prev_total);
                let d_idle = cur.0.saturating_sub(prev_idle);
                if d_total > 0 {
                    self.cpu = (1.0 - (d_idle as f32 / d_total as f32)).clamp(0.0, 1.0);
                }
            }
            self.cpu_prev = Some(cur);
        }
        let (pct, charging) = sample_battery();
        self.battery_pct = pct;
        self.charging = charging;
        self.disk = sample_disk();
    }

    pub fn ram_ratio(&self) -> f32 {
        if self.ram_total == 0 {
            0.0
        } else {
            (self.ram_used as f32 / self.ram_total as f32).clamp(0.0, 1.0)
        }
    }

    pub fn volume_label(&self) -> String {
        if self.muted {
            "MUTE".to_string()
        } else {
            format!("{:>3}", (self.volume * 100.0).round() as u8)
        }
    }
}

#[allow(clippy::needless_return)] // one early return per platform block
fn sample_memory() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
            return (0, 0);
        };
        let mut total = 0u64;
        let mut available = 0u64;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total = parse_kb(rest);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                available = parse_kb(rest);
            }
        }
        return (total.saturating_sub(available), total);
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..MEMORYSTATUSEX::default()
        };
        if unsafe { GlobalMemoryStatusEx(&raw mut status) }.is_err() {
            return (0, 0);
        }
        let total = status.ullTotalPhys;
        (total.saturating_sub(status.ullAvailPhys), total)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        (0, 0)
    }
}

/// Current clipboard text, read straight from Windows. Empty when nothing
/// text-like is on the clipboard.
pub fn system_clipboard() -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let Some(handle) = GetClipboardData(CF_UNICODETEXT).ok() else {
                let _ = CloseClipboard();
                return None;
            };
            let mem = HGLOBAL(handle.0);
            let ptr = GlobalLock(mem);
            if ptr.is_null() {
                let _ = CloseClipboard();
                return None;
            }
            let words = ptr as *const u16;
            let mut len = 0usize;
            while len < 8_000 && *words.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(words, len));
            let _ = GlobalUnlock(mem);
            let _ = CloseClipboard();
            let text = text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn sample_disk() -> f32 {
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut free = 0u64;
        let mut total = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(w!("C:\\"), None, Some(&raw mut total), Some(&raw mut free)).is_ok()
        };
        if ok && total > 0 {
            return (1.0 - free as f32 / total as f32).clamp(0.0, 1.0);
        }
        0.0
    }
    #[cfg(not(windows))]
    {
        0.0
    }
}

#[cfg(target_os = "linux")]
fn parse_kb(rest: &str) -> u64 {
    rest.split_whitespace()
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        * 1024
}

/// Returns (idle, total) jiffies.
fn sample_cpu() -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        let line = text.lines().next()?;
        let mut parts = line.split_whitespace();
        if parts.next()? != "cpu" {
            return None;
        }
        let nums: Vec<u64> = parts.filter_map(|s| s.parse().ok()).collect();
        if nums.len() < 4 {
            return None;
        }
        let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
        let total = nums.iter().sum();
        Some((idle, total))
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetSystemTimes;
        unsafe {
            let mut idle = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetSystemTimes(
                Some(&raw mut idle),
                Some(&raw mut kernel),
                Some(&raw mut user),
            )
            .ok()?;
            let idle = ((idle.dwHighDateTime as u64) << 32) | idle.dwLowDateTime as u64;
            // Kernel time already includes idle.
            let kernel = ((kernel.dwHighDateTime as u64) << 32) | kernel.dwLowDateTime as u64;
            let user = ((user.dwHighDateTime as u64) << 32) | user.dwLowDateTime as u64;
            Some((idle, kernel.saturating_add(user)))
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

fn sample_battery() -> (Option<u8>, bool) {
    #[cfg(target_os = "linux")]
    {
        let base = std::path::Path::new("/sys/class/power_supply");
        let Ok(entries) = std::fs::read_dir(base) else {
            return (None, false);
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("BAT") {
                continue;
            }
            let dir = entry.path();
            let pct = std::fs::read_to_string(dir.join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<u8>().ok());
            let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
            let charging = status.to_ascii_lowercase().contains("charg");
            return (pct, charging);
        }
        (None, false)
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut s = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
            return (None, false);
        }
        // BatteryFlag 128 = no system battery, 255 = unknown.
        let has_battery =
            s.BatteryFlag != 128 && s.BatteryFlag != 255 && s.BatteryLifePercent <= 100;
        let pct = has_battery.then_some(s.BatteryLifePercent);
        (pct, s.ACLineStatus == 1)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        (None, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_island_starts_collapsed() {
        let model = IslandModel::new();
        assert!(!model.expanded);
        assert!(model.hour24);
        assert_eq!(model.shape(0.0), IslandShape::Pill);
    }

    #[test]
    fn hover_never_expands_only_click_does() {
        let mut m = IslandModel::new();
        m.hover(true);
        assert!(m.hovered);
        assert_eq!(m.shape(0.0), IslandShape::Pill);
        m.click();
        assert_eq!(m.shape(0.0), IslandShape::Expanded);
        m.hover(false);
        assert_eq!(m.shape(0.0), IslandShape::Expanded);
        m.click();
        assert_eq!(m.shape(0.0), IslandShape::Pill);
    }

    #[test]
    fn fullscreen_rests_as_slit_and_click_expands_then_dismiss_returns() {
        let mut m = IslandModel::new();
        m.set_fullscreen(true);
        assert_eq!(m.shape(0.0), IslandShape::Slit);
        m.hover(true);
        assert_eq!(m.shape(0.0), IslandShape::Slit);
        m.click();
        assert_eq!(m.shape(0.0), IslandShape::Expanded);
        m.dismiss();
        assert_eq!(m.shape(0.0), IslandShape::Slit);
        m.set_fullscreen(false);
        assert_eq!(m.shape(0.0), IslandShape::Pill);
    }

    #[test]
    fn peeks_expand_then_retract_to_resting_shape() {
        let mut m = IslandModel::new();
        m.set_fullscreen(true);
        m.peek(10.0, 3.0);
        assert_eq!(m.shape(11.0), IslandShape::Peek);
        assert_eq!(m.shape(13.5), IslandShape::Slit);
        m.set_fullscreen(false);
        m.peek(20.0, 2.0);
        assert_eq!(m.shape(21.0), IslandShape::Peek);
        assert_eq!(m.shape(22.5), IslandShape::Pill);
        assert_eq!(IslandShape::Slit.window_height(), SLIT_HEIGHT);
        const { assert!(SLIT_HEIGHT < 16.0) };
    }

    #[test]
    fn battery_thresholds_fire_once_per_crossing() {
        let mut m = IslandModel::new();
        let mut fired = Vec::new();
        for pct in [
            60u8, 55, 51, 50, 49, 48, 30, 21, 20, 19, 15, 11, 10, 9, 6, 5, 4, 3,
        ] {
            fired.extend(m.battery_sample(Some(pct), false));
        }
        assert_eq!(
            fired,
            vec![
                BatteryAlert::Below(50),
                BatteryAlert::Below(20),
                BatteryAlert::Below(10),
                BatteryAlert::Critical
            ]
        );
        // Charge back over 50 (plug event only), unplug, drop again: fires again.
        let mut again = m.battery_sample(Some(3), true);
        again.extend(m.battery_sample(Some(52), true));
        again.extend(m.battery_sample(Some(52), false));
        again.extend(m.battery_sample(Some(49), false));
        assert_eq!(
            again,
            vec![
                BatteryAlert::Plugged,
                BatteryAlert::Unplugged,
                BatteryAlert::Below(50)
            ]
        );
    }

    #[test]
    fn big_battery_drop_reports_the_lowest_threshold_once() {
        let mut m = IslandModel::new();
        m.battery_sample(Some(60), false);
        assert_eq!(
            m.battery_sample(Some(15), false),
            vec![BatteryAlert::Below(20)]
        );
        assert!(m.battery_sample(Some(15), false).is_empty());
    }

    #[test]
    fn ram_ratio_is_zero_without_a_sample() {
        let mut model = IslandModel::new();
        model.ram_used = 4;
        model.ram_total = 8;
        assert!((model.ram_ratio() - 0.5).abs() < f32::EPSILON);
        model.ram_total = 0;
        assert_eq!(model.ram_ratio(), 0.0);
    }

    #[test]
    fn windows_meters_read_the_machine() {
        if !cfg!(windows) {
            return;
        }
        let (used, total) = sample_memory();
        assert!(
            total > 512 * 1024 * 1024,
            "RAM total was {total}, used {used}"
        );
        assert!(used > 0, "RAM used was 0");
        let disk = sample_disk();
        assert!(disk > 0.01 && disk < 0.999, "disk used fraction was {disk}");
        let first = sample_cpu().expect("GetSystemTimes");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let second = sample_cpu().expect("GetSystemTimes again");
        assert!(second.1 > first.1, "CPU counters did not move");
    }

    #[test]
    fn meters_stay_honest_across_a_burst() {
        if !cfg!(windows) {
            return;
        }
        let mut model = IslandModel::new();
        let started = std::time::Instant::now();
        for _ in 0..300 {
            model.tick();
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(400),
            "300 meter ticks took {elapsed:?}"
        );
        assert!(model.ram_total > 512 * 1024 * 1024);
        assert!(model.ram_used > 0);
        assert!(model.disk > 0.01 && model.disk < 0.999);
        assert!((0.0..=1.0).contains(&model.cpu));
    }

    #[test]
    fn clipboard_roundtrip_is_instant() {
        if !cfg!(windows) {
            return;
        }
        let previous = system_clipboard();
        let marker = format!("nexus-stress-{}", std::process::id());
        let started = std::time::Instant::now();
        set_clipboard(&marker);
        let read = system_clipboard();
        if let Some(text) = previous {
            set_clipboard(&text);
        }
        assert!(
            started.elapsed() < std::time::Duration::from_millis(250),
            "clipboard took {:?}",
            started.elapsed()
        );
        assert_eq!(read.as_deref(), Some(marker.as_str()));
    }

    #[cfg(not(windows))]
    fn set_clipboard(_text: &str) {}

    #[cfg(windows)]
    fn set_clipboard(text: &str) {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
        };

        let wide: Vec<u16> = std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            for _ in 0..8 {
                if OpenClipboard(None).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = EmptyClipboard();
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).expect("clipboard alloc");
            let ptr = GlobalLock(mem);
            assert!(!ptr.is_null());
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr.cast(), wide.len());
            let _ = GlobalUnlock(mem);
            let _ = SetClipboardData(13, windows::Win32::Foundation::HANDLE(mem.0));
            let _ = CloseClipboard();
        }
    }
}
