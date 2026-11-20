//! `nx-compat`: Windows update resilience, capability probing, and self-healing for Nexus-OS.
//!
//! # Architecture
//!
//! Nexus-OS must keep working seamlessly through frequent Windows updates.
//! Instead of fragile version checks, `nx-compat` provides:
//! 1. Build & UBR detection (combining `RtlGetVersion` and registry metadata).
//! 2. Runtime capability probing via `GetProcAddress` and feature tests with documented graceful fallbacks.
//! 3. Crash-safe self-healing: detects build changes, degrades only regressed features, auto-re-enables
//!    features on later builds, and safely re-applies reset settings through the never-brick journal.
//! 4. Mid-session restart resilience for `explorer.exe` (`TaskbarCreated`) and `dwm.exe` (`WM_DWMCOMPOSITIONCHANGED`).

#![allow(clippy::manual_c_str_literals)]

pub mod build;
pub mod dispatch;
pub mod error;
pub mod notice;
pub mod probe;
pub mod restart;
pub mod self_heal;
pub mod store;

use std::sync::OnceLock;

pub use build::{
    detect_build, detect_build_from_source, BuildSource, MockBuildSource, WindowsBuildInfo,
};
pub use dispatch::*;
pub use error::{CompatError, CompatResult};
pub use notice::{NoticeQueue, NoticeSeverity, ResilienceNotice};
pub use probe::{
    Capability, CapabilityStatus, LiveProbeProvider, MockProbeProvider, ProbeManager,
    ProbeProvider, ProbeResult,
};
pub use restart::{
    DwmRestartReport, RestartConfig, RestartHandler, WM_DWMCOLORIZATIONCOLORCHANGED,
    WM_DWMCOMPOSITIONCHANGED, WM_DWMNCRENDERINGCHANGED,
};
pub use self_heal::{FeatureState, ResilienceSetting, SelfCheckReport, SelfHealingEngine};
pub use store::{BuildHistoryStore, BuildStatus};

static GLOBAL_PROBES: OnceLock<ProbeManager> = OnceLock::new();
static GLOBAL_NOTICES: OnceLock<NoticeQueue> = OnceLock::new();

/// Returns a shared reference to the global `ProbeManager`.
pub fn global_probe_manager() -> &'static ProbeManager {
    GLOBAL_PROBES.get_or_init(ProbeManager::with_live)
}

/// Returns a shared reference to the global `NoticeQueue`.
pub fn global_notice_queue() -> &'static NoticeQueue {
    GLOBAL_NOTICES.get_or_init(NoticeQueue::new)
}

/// Checks whether a capability is fully supported at runtime.
pub fn is_capability_supported(capability: Capability) -> bool {
    global_probe_manager().is_supported(capability)
}

/// Checks whether a capability is operational (available or degraded with fallback).
pub fn is_capability_operational(capability: Capability) -> bool {
    global_probe_manager().is_operational(capability)
}

/// Returns the documented graceful fallback description for a capability.
pub fn fallback_for(capability: Capability) -> &'static str {
    capability.documented_fallback()
}
