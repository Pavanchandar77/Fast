//! Core domain types for Nexus-OS governor policy.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Strongly-typed Process Identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pid(pub u32);

impl Pid {
    /// PID 0 represents System Idle Process.
    pub const SYSTEM_IDLE: Self = Self(0);
    /// PID 4 represents Windows NT System process.
    pub const SYSTEM: Self = Self(4);

    /// Checks if this PID is a critical kernel/system pseudo-process (PID 0 or 4).
    #[inline]
    pub const fn is_system_kernel(&self) -> bool {
        self.0 == 0 || self.0 == 4
    }

    /// Raw numeric value.
    #[inline]
    pub const fn as_u32(&self) -> u32 {
        self.0
    }
}

impl From<u32> for Pid {
    #[inline]
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<Pid> for u32 {
    #[inline]
    fn from(pid: Pid) -> Self {
        pid.0
    }
}

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PID({})", self.0)
    }
}

/// Windows process priority classes, ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PriorityClass {
    /// `IDLE_PRIORITY_CLASS` (0x00000040)
    Idle = 1,
    /// `BELOW_NORMAL_PRIORITY_CLASS` (0x00004000)
    BelowNormal = 2,
    /// `NORMAL_PRIORITY_CLASS` (0x00000020)
    Normal = 3,
    /// `ABOVE_NORMAL_PRIORITY_CLASS` (0x00008000)
    AboveNormal = 4,
    /// `HIGH_PRIORITY_CLASS` (0x00000080)
    High = 5,
    /// `REALTIME_PRIORITY_CLASS` (0x00000100)
    Realtime = 6,
}

impl PriorityClass {
    /// Step up by one priority class (bounded at Realtime).
    #[must_use]
    pub const fn step_up(self) -> Self {
        match self {
            Self::Idle => Self::BelowNormal,
            Self::BelowNormal => Self::Normal,
            Self::Normal => Self::AboveNormal,
            Self::AboveNormal => Self::High,
            Self::High | Self::Realtime => Self::Realtime,
        }
    }

    /// Step down by one priority class (bounded at Idle).
    #[must_use]
    pub const fn step_down(self) -> Self {
        match self {
            Self::Realtime => Self::High,
            Self::High => Self::AboveNormal,
            Self::AboveNormal => Self::Normal,
            Self::Normal => Self::BelowNormal,
            Self::BelowNormal | Self::Idle => Self::Idle,
        }
    }

    /// Convert to Windows Win32 API priority class constant bitmask.
    pub const fn to_win32(self) -> u32 {
        match self {
            Self::Idle => 0x00000040,
            Self::BelowNormal => 0x00004000,
            Self::Normal => 0x00000020,
            Self::AboveNormal => 0x00008000,
            Self::High => 0x00000080,
            Self::Realtime => 0x00000100,
        }
    }

    /// Parse from Windows Win32 API priority class constant bitmask.
    pub const fn from_win32(val: u32) -> Option<Self> {
        match val {
            0x00000040 => Some(Self::Idle),
            0x00004000 => Some(Self::BelowNormal),
            0x00000020 => Some(Self::Normal),
            0x00008000 => Some(Self::AboveNormal),
            0x00000080 => Some(Self::High),
            0x00000100 => Some(Self::Realtime),
            _ => None,
        }
    }

    /// Case-insensitive parse from string names (e.g. "Normal", "BelowNormal", "High").
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "idle" => Some(Self::Idle),
            "belownormal" | "below_normal" | "below normal" => Some(Self::BelowNormal),
            "normal" => Some(Self::Normal),
            "abovenormal" | "above_normal" | "above normal" => Some(Self::AboveNormal),
            "high" => Some(Self::High),
            "realtime" | "real_time" | "real time" => Some(Self::Realtime),
            _ => None,
        }
    }

    /// Textual display name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::BelowNormal => "BelowNormal",
            Self::Normal => "Normal",
            Self::AboveNormal => "AboveNormal",
            Self::High => "High",
            Self::Realtime => "Realtime",
        }
    }
}

impl fmt::Display for PriorityClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// EcoQoS state (Windows 11 Efficiency Mode / ProcessPowerThrottling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EcoQosState {
    /// Standard execution profile (E-cores + P-cores unconstrained).
    Disabled,
    /// EcoQoS enabled: scheduler prefers efficiency cores, lower clock, reduced power draw.
    Enabled,
}

impl EcoQosState {
    /// Returns true if EcoQoS is enabled.
    #[inline]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    /// Invert the state.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Disabled => Self::Enabled,
            Self::Enabled => Self::Disabled,
        }
    }
}

impl fmt::Display for EcoQosState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("Disabled"),
            Self::Enabled => f.write_str("Enabled"),
        }
    }
}

/// Errors originating in governor policy evaluation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GovernorError {
    #[error("Invalid priority class name: {0}")]
    InvalidPriorityName(String),

    #[error("Process not found in policy registry: {0}")]
    ProcessNotFound(Pid),

    #[error("Refused dangerous action on critical system process: {0}")]
    RefusedCriticalProcess(Pid),

    #[error("Invalid sample data: {0}")]
    InvalidSample(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_conversions() {
        let p = Pid(1234);
        assert_eq!(p.as_u32(), 1234);
        assert_eq!(u32::from(p), 1234);
        assert_eq!(Pid::from(1234), p);
        assert_eq!(format!("{}", p), "PID(1234)");
        assert!(!p.is_system_kernel());
        assert!(Pid::SYSTEM_IDLE.is_system_kernel());
        assert!(Pid::SYSTEM.is_system_kernel());
    }

    #[test]
    fn test_priority_class_steps() {
        assert_eq!(PriorityClass::Idle.step_up(), PriorityClass::BelowNormal);
        assert_eq!(PriorityClass::BelowNormal.step_up(), PriorityClass::Normal);
        assert_eq!(PriorityClass::Normal.step_up(), PriorityClass::AboveNormal);
        assert_eq!(PriorityClass::AboveNormal.step_up(), PriorityClass::High);
        assert_eq!(PriorityClass::High.step_up(), PriorityClass::Realtime);
        assert_eq!(PriorityClass::Realtime.step_up(), PriorityClass::Realtime);

        assert_eq!(PriorityClass::Realtime.step_down(), PriorityClass::High);
        assert_eq!(PriorityClass::High.step_down(), PriorityClass::AboveNormal);
        assert_eq!(
            PriorityClass::AboveNormal.step_down(),
            PriorityClass::Normal
        );
        assert_eq!(
            PriorityClass::Normal.step_down(),
            PriorityClass::BelowNormal
        );
        assert_eq!(PriorityClass::BelowNormal.step_down(), PriorityClass::Idle);
        assert_eq!(PriorityClass::Idle.step_down(), PriorityClass::Idle);
    }

    #[test]
    fn test_priority_class_win32_roundtrip() {
        let classes = [
            PriorityClass::Idle,
            PriorityClass::BelowNormal,
            PriorityClass::Normal,
            PriorityClass::AboveNormal,
            PriorityClass::High,
            PriorityClass::Realtime,
        ];
        for c in classes {
            let win32 = c.to_win32();
            let parsed = PriorityClass::from_win32(win32).expect("Must parse win32");
            assert_eq!(parsed, c);
        }
        assert!(PriorityClass::from_win32(0x99999).is_none());
    }

    #[test]
    fn test_priority_class_from_name() {
        assert_eq!(
            PriorityClass::from_name("Normal"),
            Some(PriorityClass::Normal)
        );
        assert_eq!(
            PriorityClass::from_name("abovenormal"),
            Some(PriorityClass::AboveNormal)
        );
        assert_eq!(
            PriorityClass::from_name("BELOW_NORMAL"),
            Some(PriorityClass::BelowNormal)
        );
        assert_eq!(PriorityClass::from_name("High"), Some(PriorityClass::High));
        assert_eq!(
            PriorityClass::from_name("RealTime"),
            Some(PriorityClass::Realtime)
        );
        assert_eq!(PriorityClass::from_name("Invalid"), None);
    }

    #[test]
    fn test_eco_qos_state() {
        let s = EcoQosState::Disabled;
        assert!(!s.is_enabled());
        let toggled = s.toggle();
        assert!(toggled.is_enabled());
        assert_eq!(format!("{}", toggled), "Enabled");
        assert_eq!(format!("{}", s), "Disabled");
    }
}
