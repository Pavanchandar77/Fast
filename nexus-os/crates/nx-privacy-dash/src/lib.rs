//! nx-privacy-dash: Nexus-OS privacy dashboard model and state machine.
//!
//! Provides pure data models and state machines for:
//! - CapabilityAccessManager ConsentStore parser and live indicators (Camera, Mic, Location)
//! - Per-app permission audit and toggle model emitting reversible dry-run-able commands
//! - Clipboard-read alerts and sensitive content auto-clear pure store
//! - Network-per-app view model fed with discrete traffic samples
//! - Kill-switch request commands (data only, to be applied by shield engine)
//! - Pure layout tree and dashboard view-model with deterministic snapshots

pub mod clipboard;
pub mod consent_store;
pub mod filetime;
pub mod gpui_view;
pub mod indicator;
pub mod kill_switch;
pub mod layout;
pub mod network;
pub mod permissions;
pub mod view_model;

pub use gpui_view::PrivacyDashboardView;

pub use clipboard::{
    AlertSeverity, ClipboardClearCommand, ClipboardPrivacyStore, ClipboardReadAlert,
    ClipboardRestoreCommand, SensitiveCopyDetected, SensitiveKind, SensitivePatternDetector,
};
pub use consent_store::{
    AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus, ConsentStoreParser,
    ConsentStoreReader, ConsentStoreSnapshot,
};
pub use filetime::FileTime;
pub use indicator::{
    ActiveAppSession, CapabilityIndicator, IndicatorChip, IndicatorState, IndicatorStatusKind,
    IndicatorTransitionEvent, MenuBarPrivacyIndicators, RecentAppSession,
};
pub use kill_switch::{KillSwitchKind, KillSwitchRequest, KillSwitchStore};
pub use layout::{LayoutNode, Rect, WidgetKind};
pub use network::{
    AppNetworkSummary, DestinationStats, NetworkCategory, NetworkPerAppViewModel, NetworkSample,
    NetworkTimeBucket, TimeWindow, TrafficDisposition,
};
pub use permissions::{
    AppPermissionProfile, AuditRecommendation, DryRunPlan, PermissionAuditReport,
    PermissionCapability, PermissionState, PermissionStore, PermissionToggleCommand, RiskLevel,
};
pub use view_model::{AuditLogEntry, DashboardTab, DashboardViewModel};

pub fn version() -> &'static str {
    "0.1.0"
}
