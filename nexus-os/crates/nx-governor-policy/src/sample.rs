//! Telemetry samples fed into the governor policy engine.

use crate::types::Pid;
use serde::{Deserialize, Serialize};

/// Metric sample for an individual active process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSample {
    /// Process identifier.
    pub pid: Pid,
    /// Executable name (e.g. "code.exe", "cargo.exe").
    pub name: String,
    /// Percentage CPU utilization (0.0 to 100.0 * core_count).
    pub cpu_usage_pct: f32,
    /// Cumulative I/O throughput in bytes per second.
    pub io_bytes_sec: u64,
    /// Whether this process owns the current foreground window.
    pub is_foreground: bool,
    /// Whether this process is running in fullscreen or borderless exclusive window.
    pub is_fullscreen: bool,
    /// Idle duration in seconds since last user interaction or foreground activity.
    pub idle_seconds: u64,
}

impl ProcessSample {
    /// Creates a new process sample.
    pub fn new(pid: Pid, name: impl Into<String>, cpu_usage_pct: f32) -> Self {
        Self {
            pid,
            name: name.into(),
            cpu_usage_pct,
            io_bytes_sec: 0,
            is_foreground: false,
            is_fullscreen: false,
            idle_seconds: 0,
        }
    }

    /// Sets foreground flag.
    pub fn with_foreground(mut self, is_foreground: bool) -> Self {
        self.is_foreground = is_foreground;
        self
    }

    /// Sets fullscreen flag.
    pub fn with_fullscreen(mut self, is_fullscreen: bool) -> Self {
        self.is_fullscreen = is_fullscreen;
        self
    }

    /// Sets idle duration in seconds.
    pub fn with_idle_seconds(mut self, idle_seconds: u64) -> Self {
        self.idle_seconds = idle_seconds;
        self
    }

    /// Sets I/O throughput in bytes/second.
    pub fn with_io_bytes_sec(mut self, io_bytes_sec: u64) -> Self {
        self.io_bytes_sec = io_bytes_sec;
        self
    }
}

/// A periodic snapshot of the complete desktop system state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    /// Current foreground process ID, if any.
    pub foreground_pid: Option<Pid>,
    /// Current fullscreen window owner process ID, if any.
    pub fullscreen_pid: Option<Pid>,
    /// Overall system CPU utilization percentage (0.0 to 100.0).
    pub total_cpu_pct: f32,
    /// Hint indicating a known game or 3D fullscreen workload is active.
    pub is_game_active: bool,
    /// Timestamp in milliseconds when snapshot was collected.
    pub timestamp_ms: u64,
    /// Samples for all inspected processes.
    pub processes: Vec<ProcessSample>,
}

impl SystemSnapshot {
    /// Creates a new system snapshot.
    pub fn new(total_cpu_pct: f32, timestamp_ms: u64) -> Self {
        Self {
            foreground_pid: None,
            fullscreen_pid: None,
            total_cpu_pct,
            is_game_active: false,
            timestamp_ms,
            processes: Vec::new(),
        }
    }

    /// Attaches the foreground PID.
    pub fn with_foreground(mut self, pid: Option<Pid>) -> Self {
        self.foreground_pid = pid;
        self
    }

    /// Attaches fullscreen PID.
    pub fn with_fullscreen(mut self, pid: Option<Pid>) -> Self {
        self.fullscreen_pid = pid;
        self
    }

    /// Sets game active flag.
    pub fn with_game_active(mut self, active: bool) -> Self {
        self.is_game_active = active;
        self
    }

    /// Adds a process sample.
    pub fn add_process(&mut self, sample: ProcessSample) {
        self.processes.push(sample);
    }
}
