//! nx-privacy: Reversible Privacy Shield engine for Nexus-OS.
//!
//! Provides transactional, dry-run-able system mutations for Windows telemetry elimination,
//! hardware kill switches (camera, mic, location), idempotent hosts-file tracker blocking,
//! and WFP per-app firewall rules (with dynamic session default).

pub mod backup;
pub mod error;
pub mod hosts;
pub mod killswitch;
pub mod plan;
pub mod shield;
pub mod system;
pub mod telemetry;
pub mod trackers;
pub mod wfp;

#[cfg(windows)]
pub mod windows;

pub use backup::{BackupEntry, RollbackReceipt};
pub use error::ShieldError;
pub use hosts::{HostsPlanner, BLOCK_END_MARKER, BLOCK_START_MARKER};
pub use killswitch::{KillSwitchConfig, KillSwitchPlanner, PrivacyDevice};
pub use plan::{DryRunDiff, DryRunReport, MutationPlan, MutationStep};
pub use shield::{PrivacyShield, PrivacyShieldConfig};
pub use system::{
    FakeSystem, RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus,
    SystemBackend, TaskState,
};
pub use telemetry::{TelemetryKillOptions, TelemetryPlanner};
pub use trackers::{parse_tracker_list, AD_AND_TRACKER_HOSTS, TELEMETRY_HOSTS};
pub use wfp::{
    WfpAction, WfpCondition, WfpDirection, WfpEngine, WfpFilterSpec, WfpProtocol, WfpRule,
    WfpRuleBuilder, WfpSessionConfig, FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION_FLAG_PERSISTENT,
};
#[cfg(windows)]
pub use windows::WindowsSystem;
