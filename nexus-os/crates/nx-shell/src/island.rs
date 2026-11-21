//! Dynamic Island: a floating monochrome pill at the top-center of the screen.
//!
//! Behaviour is inspired by (but does not vendor) MIT-licensed DynamicNotchKit
//! and NotchDrop, and the hover-to-expand HUD pattern used by Alcove-style
//! floating islands and GPL-licensed Boring Notch. This module is a native
//! GPUI reimplementation.

use crate::clock::ClockSample;

/// Live control-center state for the Dynamic Island.
#[derive(Clone, Debug)]
pub struct IslandModel {
    pub expanded: bool,
    /// Click pins the island open until clicked again.
    pub pinned: bool,
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
            pinned: false,
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
            now_playing: None,
            notifications: Vec::new(),
            cpu_prev: None,
        };
        model.tick();
        model
    }

    pub fn clock_label(&self) -> String {
        self.clock.format(self.hour24).to_ascii_uppercase()
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
        if !expanded {
            self.pinned = false;
        }
    }

    pub fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        self.expanded = self.pinned || self.expanded;
    }

    pub fn hover(&mut self, inside: bool) {
        if self.pinned {
            return;
        }
        self.expanded = inside;
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
        (total.saturating_sub(available), total)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0, 0)
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
    #[cfg(not(target_os = "linux"))]
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
    #[cfg(not(target_os = "linux"))]
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
        assert!(!model.pinned);
        assert!(model.hour24);
    }

    #[test]
    fn hover_expands_and_collapses_unless_pinned() {
        let mut model = IslandModel::new();
        model.hover(true);
        assert!(model.expanded);
        model.hover(false);
        assert!(!model.expanded);
        model.toggle_pin();
        assert!(model.pinned && model.expanded);
        model.hover(false);
        assert!(model.expanded);
        model.toggle_pin();
        model.set_expanded(false);
        assert!(!model.expanded && !model.pinned);
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
}
