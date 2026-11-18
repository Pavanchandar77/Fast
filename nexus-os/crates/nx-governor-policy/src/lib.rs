//! # nx-governor-policy
//!
//! Pure, robust, Process Lasso-grade governor policy engine for Nexus-OS.
//!
//! Provides deterministic, dry-run-able, and strictly reversible scheduling policies:
//! - Foreground priority boost (max 1 priority class step, never `High` unless whitelisted).
//! - ProBalance temporary demotion of sustained CPU hogs with automatic restore.
//! - EcoQoS (efficiency mode / ProcessPowerThrottling) for background apps instead of hard 1% job caps.
//! - Automatic Game Mode detection (pauses governor; never throttles games or helpers).
//! - Process-exit hook to release state so PID reuse cannot inherit stale caps.
//! - Safe defaults: never suspend processes by default, no `BelowNormal` background priority.

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod config;
pub mod engine;
pub mod mutation;
pub mod rules;
pub mod sample;
pub mod state;
pub mod types;

// Re-exports
pub use config::GovernorPolicyConfig;
pub use engine::GovernorEngine;
pub use mutation::{MutationPlan, PolicyMutation, SimulatedProcessState, SimulatedSystemState};
pub use rules::{
    matches_rule, normalize_process_name, GovernorRules, CRITICAL_SYSTEM_PROCESSES,
    KNOWN_GAME_EXECUTABLES, NEXUS_CORE_PROCESSES,
};
pub use sample::{ProcessSample, SystemSnapshot};
pub use state::TrackedProcess;
pub use types::{EcoQosState, GovernorError, Pid, PriorityClass};
