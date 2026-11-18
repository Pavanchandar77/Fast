//! Pure governor policy decision engine for Nexus-OS.
//! Decides process priority class, Job Object CPU throttling, RAM trimming, and App Nap suspension.
//! Strictly pure: takes process metadata and returns policy decisions. Zero OS/Win32 dependencies.

use serde::{Deserialize, Serialize};

/// Windows NT process priority class targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessPriorityTarget {
    NoChange,
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThrottleDecision {
    NoChange,
    Throttle { rate_percent: u32 },
    Unthrottle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryDecision {
    NoChange,
    PurgeWorkingSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionDecision {
    NoChange,
    Suspend,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub priority: ProcessPriorityTarget,
    pub throttle: ThrottleDecision,
    pub memory: MemoryDecision,
    pub execution: ExecutionDecision,
    pub reason: &'static str,
}

impl PolicyDecision {
    pub const fn noop(reason: &'static str) -> Self {
        Self {
            priority: ProcessPriorityTarget::NoChange,
            throttle: ThrottleDecision::NoChange,
            memory: MemoryDecision::NoChange,
            execution: ExecutionDecision::NoChange,
            reason,
        }
    }
}

/// Snapshot of a process to evaluate
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot<'a> {
    pub pid: u32,
    pub name: &'a str,
    pub is_foreground: bool,
    pub is_minimized: bool,
    pub is_cloaked: bool,
    pub idle_seconds: u64,
}

/// Immutable list of critical Windows NT and Nexus shell processes that must NEVER be penalized.
pub const CRITICAL_PROCESSES: &[&str] = &[
    "csrss.exe",
    "dwm.exe",
    "lsass.exe",
    "smss.exe",
    "winlogon.exe",
    "services.exe",
    "svchost.exe",
    "audiodg.exe",
    "system",
    "registry",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
    "conhost.exe",
    "explorer.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "spoolsv.exe",
    "securityhealthservice.exe",
    "nexus-os.exe",
    "nexus-compositor.exe",
    "nexus-governor.exe",
    "nexus-vfs.exe",
    "nexus-watchdog.exe",
];

#[derive(Debug, Clone)]
pub struct GovernorPolicyConfig {
    pub dynamic_priority_boost: bool,
    pub app_nap_enabled: bool,
    pub app_nap_idle_secs: u64,
    pub cpu_throttle_percent: u32,
    pub trim_on_minimize: bool,
    pub trim_on_unfocused: bool,
    pub user_exclusions: Vec<String>,
    pub own_pid: u32,
}

impl Default for GovernorPolicyConfig {
    fn default() -> Self {
        Self {
            dynamic_priority_boost: true,
            app_nap_enabled: true,
            app_nap_idle_secs: 30,
            cpu_throttle_percent: 1,
            trim_on_minimize: true,
            trim_on_unfocused: false,
            user_exclusions: vec![
                "spotify.exe".into(),
                "discord.exe".into(),
                "slack.exe".into(),
                "obs64.exe".into(),
                "steam.exe".into(),
                "cargo.exe".into(),
                "rustc.exe".into(),
                "cl.exe".into(),
            ],
            own_pid: 0,
        }
    }
}

pub struct GovernorPolicy;

impl GovernorPolicy {
    /// Checks if a process is exempt from all throttling, suspension, and demotion.
    pub fn is_exempt(pid: u32, name: &str, config: &GovernorPolicyConfig) -> bool {
        if pid == 0 || pid == 4 || pid == config.own_pid {
            return true;
        }

        let name_lower = name.to_lowercase();
        if CRITICAL_PROCESSES.iter().any(|&crit| name_lower == crit) {
            return true;
        }

        // Wildcard match for nexus-* processes
        if name_lower.starts_with("nexus-") {
            return true;
        }

        false
    }

    /// Checks if a process matches user-configured App Nap exclusions
    pub fn is_user_excluded(name: &str, config: &GovernorPolicyConfig) -> bool {
        let name_lower = name.to_lowercase();
        config
            .user_exclusions
            .iter()
            .any(|excl| name_lower.contains(&excl.to_lowercase()))
    }

    /// Evaluates policy decision when a process becomes the active foreground application.
    pub fn evaluate_foreground(
        snapshot: &ProcessSnapshot,
        config: &GovernorPolicyConfig,
    ) -> PolicyDecision {
        if Self::is_exempt(snapshot.pid, snapshot.name, config) {
            return PolicyDecision {
                priority: if config.dynamic_priority_boost {
                    ProcessPriorityTarget::High
                } else {
                    ProcessPriorityTarget::NoChange
                },
                throttle: ThrottleDecision::Unthrottle,
                memory: MemoryDecision::NoChange,
                execution: ExecutionDecision::Resume,
                reason: "Exempt process brought to foreground",
            };
        }

        PolicyDecision {
            priority: if config.dynamic_priority_boost {
                ProcessPriorityTarget::High
            } else {
                ProcessPriorityTarget::NoChange
            },
            throttle: ThrottleDecision::Unthrottle,
            memory: MemoryDecision::NoChange,
            execution: ExecutionDecision::Resume,
            reason: "Foreground process receives priority elevation and unthrottle",
        }
    }

    /// Evaluates policy decision when a process loses foreground focus.
    pub fn evaluate_unfocused(
        snapshot: &ProcessSnapshot,
        config: &GovernorPolicyConfig,
    ) -> PolicyDecision {
        if Self::is_exempt(snapshot.pid, snapshot.name, config) {
            return PolicyDecision::noop("Exempt process demotion skipped");
        }

        let priority = if config.dynamic_priority_boost {
            ProcessPriorityTarget::BelowNormal
        } else {
            ProcessPriorityTarget::NoChange
        };

        let memory = if config.trim_on_unfocused {
            MemoryDecision::PurgeWorkingSet
        } else {
            MemoryDecision::NoChange
        };

        PolicyDecision {
            priority,
            throttle: ThrottleDecision::NoChange,
            memory,
            execution: ExecutionDecision::NoChange,
            reason: "Unfocused process demoted to BelowNormal",
        }
    }

    /// Evaluates policy decision when a window is minimized.
    pub fn evaluate_minimize(
        snapshot: &ProcessSnapshot,
        config: &GovernorPolicyConfig,
    ) -> PolicyDecision {
        if Self::is_exempt(snapshot.pid, snapshot.name, config) {
            return PolicyDecision::noop("Exempt process minimize handling skipped");
        }

        let memory = if config.trim_on_minimize {
            MemoryDecision::PurgeWorkingSet
        } else {
            MemoryDecision::NoChange
        };

        let (throttle, execution) =
            if config.app_nap_enabled && !Self::is_user_excluded(snapshot.name, config) {
                (
                    ThrottleDecision::Throttle {
                        rate_percent: config.cpu_throttle_percent.clamp(1, 10),
                    },
                    ExecutionDecision::NoChange,
                )
            } else {
                (ThrottleDecision::NoChange, ExecutionDecision::NoChange)
            };

        PolicyDecision {
            priority: ProcessPriorityTarget::NoChange,
            throttle,
            memory,
            execution,
            reason: "Minimized window memory purge and rate cap",
        }
    }

    /// Evaluates background periodic App Nap check.
    pub fn evaluate_idle(
        snapshot: &ProcessSnapshot,
        config: &GovernorPolicyConfig,
    ) -> PolicyDecision {
        if snapshot.is_foreground {
            return PolicyDecision::noop("Foreground process is immune from idle throttling");
        }

        if Self::is_exempt(snapshot.pid, snapshot.name, config) {
            return PolicyDecision::noop("Exempt process immune from idle throttling");
        }

        if Self::is_user_excluded(snapshot.name, config) {
            return PolicyDecision::noop("User excluded process immune from idle throttling");
        }

        if !config.app_nap_enabled {
            return PolicyDecision::noop("App Nap disabled");
        }

        if snapshot.idle_seconds >= config.app_nap_idle_secs {
            PolicyDecision {
                priority: ProcessPriorityTarget::Idle,
                throttle: ThrottleDecision::Throttle {
                    rate_percent: config.cpu_throttle_percent.clamp(1, 10),
                },
                memory: MemoryDecision::PurgeWorkingSet,
                execution: ExecutionDecision::NoChange,
                reason: "Idle threshold exceeded: App Nap rate throttle applied",
            }
        } else {
            PolicyDecision::noop("Process not yet idle")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critical_processes_are_exempt() {
        let cfg = GovernorPolicyConfig::default();
        for &proc in CRITICAL_PROCESSES {
            assert!(
                GovernorPolicy::is_exempt(1234, proc, &cfg),
                "{} must be exempt",
                proc
            );
        }
        assert!(GovernorPolicy::is_exempt(0, "anything.exe", &cfg));
        assert!(GovernorPolicy::is_exempt(4, "anything.exe", &cfg));
        assert!(GovernorPolicy::is_exempt(555, "nexus-custom.exe", &cfg));
    }

    #[test]
    fn test_foreground_boost_decision() {
        let cfg = GovernorPolicyConfig::default();
        let snap = ProcessSnapshot {
            pid: 9999,
            name: "notepad.exe",
            is_foreground: true,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 0,
        };

        let decision = GovernorPolicy::evaluate_foreground(&snap, &cfg);
        assert_eq!(decision.priority, ProcessPriorityTarget::High);
        assert_eq!(decision.throttle, ThrottleDecision::Unthrottle);
        assert_eq!(decision.execution, ExecutionDecision::Resume);
        assert_eq!(decision.memory, MemoryDecision::NoChange);
    }

    #[test]
    fn test_unfocused_demotion_decision() {
        let cfg = GovernorPolicyConfig::default();
        let snap = ProcessSnapshot {
            pid: 9999,
            name: "notepad.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 0,
        };

        let decision = GovernorPolicy::evaluate_unfocused(&snap, &cfg);
        assert_eq!(decision.priority, ProcessPriorityTarget::BelowNormal);
    }

    #[test]
    fn test_idle_app_nap_decision() {
        let cfg = GovernorPolicyConfig::default();
        let snap = ProcessSnapshot {
            pid: 9999,
            name: "chrome.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 45, // Exceeds default 30s
        };

        let decision = GovernorPolicy::evaluate_idle(&snap, &cfg);
        assert_eq!(decision.priority, ProcessPriorityTarget::Idle);
        assert_eq!(
            decision.throttle,
            ThrottleDecision::Throttle { rate_percent: 1 }
        );
        assert_eq!(decision.memory, MemoryDecision::PurgeWorkingSet);
    }

    #[test]
    fn test_user_excluded_app_nap_immune() {
        let cfg = GovernorPolicyConfig::default();
        let snap = ProcessSnapshot {
            pid: 9999,
            name: "spotify.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 120,
        };

        let decision = GovernorPolicy::evaluate_idle(&snap, &cfg);
        assert_eq!(decision.throttle, ThrottleDecision::NoChange);
        assert_eq!(decision.priority, ProcessPriorityTarget::NoChange);
    }

    #[test]
    fn test_foreground_immune_from_idle() {
        let cfg = GovernorPolicyConfig::default();
        let snap = ProcessSnapshot {
            pid: 9999,
            name: "code.exe",
            is_foreground: true,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 100,
        };

        let decision = GovernorPolicy::evaluate_idle(&snap, &cfg);
        assert_eq!(decision.throttle, ThrottleDecision::NoChange);
    }

    #[test]
    fn test_exempt_own_pid_and_system() {
        let cfg = GovernorPolicyConfig {
            own_pid: 4040,
            ..Default::default()
        };
        assert!(GovernorPolicy::is_exempt(4040, "nexus-os.exe", &cfg));
        assert!(GovernorPolicy::is_exempt(0, "system", &cfg));
        assert!(GovernorPolicy::is_exempt(4, "system", &cfg));
    }

    #[test]
    fn test_evaluate_foreground_boost_disabled() {
        let cfg = GovernorPolicyConfig {
            dynamic_priority_boost: false,
            ..Default::default()
        };
        let snap = ProcessSnapshot {
            pid: 8888,
            name: "app.exe",
            is_foreground: true,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 0,
        };
        let decision = GovernorPolicy::evaluate_foreground(&snap, &cfg);
        assert_eq!(decision.priority, ProcessPriorityTarget::NoChange);
        assert_eq!(decision.throttle, ThrottleDecision::Unthrottle);
    }

    #[test]
    fn test_evaluate_unfocused_trim_enabled() {
        let cfg = GovernorPolicyConfig {
            trim_on_unfocused: true,
            ..Default::default()
        };
        let snap = ProcessSnapshot {
            pid: 8888,
            name: "app.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 0,
        };
        let decision = GovernorPolicy::evaluate_unfocused(&snap, &cfg);
        assert_eq!(decision.memory, MemoryDecision::PurgeWorkingSet);
    }

    #[test]
    fn test_evaluate_minimize_options() {
        let cfg = GovernorPolicyConfig {
            trim_on_minimize: false,
            app_nap_enabled: false,
            ..Default::default()
        };
        let snap = ProcessSnapshot {
            pid: 8888,
            name: "app.exe",
            is_foreground: false,
            is_minimized: true,
            is_cloaked: false,
            idle_seconds: 0,
        };
        let decision = GovernorPolicy::evaluate_minimize(&snap, &cfg);
        assert_eq!(decision.memory, MemoryDecision::NoChange);
        assert_eq!(decision.throttle, ThrottleDecision::NoChange);
    }

    #[test]
    fn test_evaluate_idle_not_yet_idle_or_disabled() {
        let mut cfg = GovernorPolicyConfig {
            app_nap_idle_secs: 60,
            ..Default::default()
        };
        let snap = ProcessSnapshot {
            pid: 8888,
            name: "app.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 30, // not yet 60s
        };
        let decision = GovernorPolicy::evaluate_idle(&snap, &cfg);
        assert_eq!(decision.throttle, ThrottleDecision::NoChange);

        cfg.app_nap_enabled = false;
        let snap2 = ProcessSnapshot {
            pid: 8888,
            name: "app.exe",
            is_foreground: false,
            is_minimized: false,
            is_cloaked: false,
            idle_seconds: 120,
        };
        let decision2 = GovernorPolicy::evaluate_idle(&snap2, &cfg);
        assert_eq!(decision2.throttle, ThrottleDecision::NoChange);
    }
}
