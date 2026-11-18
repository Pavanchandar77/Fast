//! Per-process tracked state maintained by the governor engine.

use crate::types::{EcoQosState, Pid, PriorityClass};

/// Tracking record for an active process governed by Nexus-OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedProcess {
    /// Process identifier.
    pub pid: Pid,
    /// Executable name.
    pub name: String,
    /// Original un-boosted base priority class (typically Normal).
    pub base_priority: PriorityClass,
    /// Currently assigned priority class.
    pub current_priority: PriorityClass,
    /// Current EcoQoS state.
    pub eco_qos: EcoQosState,
    /// Whether this process is currently elevated due to foreground boost.
    pub is_boosted: bool,
    /// Whether this process is currently demoted due to ProBalance CPU hog detection.
    pub is_demoted: bool,
    /// Consecutive evaluation ticks where this process exceeded the CPU threshold.
    pub consecutive_high_cpu_ticks: u32,
    /// Consecutive evaluation ticks where this process operated at normal CPU levels.
    pub consecutive_normal_cpu_ticks: u32,
    /// Consecutive evaluation ticks where this process has been in the background.
    pub background_idle_ticks: u32,
    /// Evaluation tick number when this process was last observed.
    pub last_seen_tick: u64,
}

impl TrackedProcess {
    /// Creates a new tracked process record with default `Normal` base priority.
    pub fn new(pid: Pid, name: impl Into<String>, tick: u64) -> Self {
        Self {
            pid,
            name: name.into(),
            base_priority: PriorityClass::Normal,
            current_priority: PriorityClass::Normal,
            eco_qos: EcoQosState::Disabled,
            is_boosted: false,
            is_demoted: false,
            consecutive_high_cpu_ticks: 0,
            consecutive_normal_cpu_ticks: 0,
            background_idle_ticks: 0,
            last_seen_tick: tick,
        }
    }
}
