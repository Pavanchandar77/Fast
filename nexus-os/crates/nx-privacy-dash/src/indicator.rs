//! Live hardware indicator state machine (idle / in-use / recently-used / blocked).
//!
//! Suitable for menu-bar chips, status badges, and notification overlays.

use crate::consent_store::{AppIdentifier, CapabilityKind, ConsentRecord, ConsentStoreSnapshot};
use serde::{Deserialize, Serialize};

/// Duration in milliseconds to hold the indicator in "recently used" before returning to idle.
pub const DEFAULT_RECENT_DECAY_MS: u64 = 5_000;

/// Duration in milliseconds to show a "blocked" indicator chip before decaying to idle.
pub const DEFAULT_BLOCKED_DECAY_MS: u64 = 5_000;

/// High-level status classification for UI chips and styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IndicatorStatusKind {
    Idle,
    InUse,
    RecentlyUsed,
    Blocked,
}

impl IndicatorStatusKind {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::InUse)
    }

    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// An active session holding a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveAppSession {
    pub app: AppIdentifier,
    pub started_at_ms: u64,
}

/// A recently completed session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentAppSession {
    pub app: AppIdentifier,
    pub stopped_at_ms: u64,
}

/// A blocked access attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedAppSession {
    pub app: AppIdentifier,
    pub attempted_at_ms: u64,
}

/// State of a single capability indicator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IndicatorState {
    /// Not in use and no recent activity.
    #[default]
    Idle,
    /// Actively in use by one or more applications.
    InUse { active_apps: Vec<ActiveAppSession> },
    /// Finished recently, displaying amber/yellow warning before decaying to idle.
    RecentlyUsed {
        recent_apps: Vec<RecentAppSession>,
        expires_at_ms: u64,
    },
    /// Access was blocked (due to kill-switch or deny rule).
    Blocked {
        blocked_apps: Vec<BlockedAppSession>,
        expires_at_ms: u64,
    },
}

impl IndicatorState {
    pub fn kind(&self) -> IndicatorStatusKind {
        match self {
            Self::Idle => IndicatorStatusKind::Idle,
            Self::InUse { .. } => IndicatorStatusKind::InUse,
            Self::RecentlyUsed { .. } => IndicatorStatusKind::RecentlyUsed,
            Self::Blocked { .. } => IndicatorStatusKind::Blocked,
        }
    }
}

/// Events emitted upon state changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndicatorTransitionEvent {
    StartedInUse {
        capability: CapabilityKind,
        apps: Vec<AppIdentifier>,
    },
    TransitionedToRecentlyUsed {
        capability: CapabilityKind,
        apps: Vec<AppIdentifier>,
        expires_at_ms: u64,
    },
    TransitionedToIdle {
        capability: CapabilityKind,
    },
    AccessBlocked {
        capability: CapabilityKind,
        app: AppIdentifier,
        expires_at_ms: u64,
    },
}

/// A display chip ready for menu-bar or status area rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndicatorChip {
    pub capability: CapabilityKind,
    pub status: IndicatorStatusKind,
    pub icon_name: &'static str,
    pub color_hex: &'static str,
    pub label: String,
    pub badge_count: usize,
    pub tooltip: String,
}

/// State machine for a single capability indicator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityIndicator {
    pub capability: CapabilityKind,
    pub state: IndicatorState,
    pub recent_decay_ms: u64,
    pub blocked_decay_ms: u64,
}

impl CapabilityIndicator {
    pub fn new(capability: CapabilityKind) -> Self {
        Self {
            capability,
            state: IndicatorState::Idle,
            recent_decay_ms: DEFAULT_RECENT_DECAY_MS,
            blocked_decay_ms: DEFAULT_BLOCKED_DECAY_MS,
        }
    }

    /// Advance time and check for decay timeouts.
    pub fn tick(&mut self, now_ms: u64) -> Vec<IndicatorTransitionEvent> {
        let mut events = Vec::new();

        match &self.state {
            IndicatorState::RecentlyUsed { expires_at_ms, .. }
            | IndicatorState::Blocked { expires_at_ms, .. }
                if now_ms >= *expires_at_ms =>
            {
                self.state = IndicatorState::Idle;
                events.push(IndicatorTransitionEvent::TransitionedToIdle {
                    capability: self.capability.clone(),
                });
            }
            _ => {}
        }

        events
    }

    /// Record a blocked access attempt for this capability.
    pub fn record_blocked(&mut self, app: AppIdentifier, now_ms: u64) -> IndicatorTransitionEvent {
        let expires_at_ms = now_ms.saturating_add(self.blocked_decay_ms);

        let mut blocked_apps = match &mut self.state {
            IndicatorState::Blocked { blocked_apps, .. } => std::mem::take(blocked_apps),
            _ => Vec::new(),
        };

        // Update or insert
        if let Some(existing) = blocked_apps.iter_mut().find(|s| s.app == app) {
            existing.attempted_at_ms = now_ms;
        } else {
            blocked_apps.push(BlockedAppSession {
                app: app.clone(),
                attempted_at_ms: now_ms,
            });
        }

        self.state = IndicatorState::Blocked {
            blocked_apps,
            expires_at_ms,
        };

        IndicatorTransitionEvent::AccessBlocked {
            capability: self.capability.clone(),
            app,
            expires_at_ms,
        }
    }

    /// Update state using fresh records for this capability from ConsentStore.
    pub fn update_records(
        &mut self,
        records: &[ConsentRecord],
        now_ms: u64,
    ) -> Vec<IndicatorTransitionEvent> {
        let mut events = Vec::new();

        let active_records: Vec<&ConsentRecord> = records
            .iter()
            .filter(|r| r.capability == self.capability && r.is_currently_active())
            .collect();

        if !active_records.is_empty() {
            // There are currently active applications using the device
            let new_active_sessions: Vec<ActiveAppSession> = active_records
                .iter()
                .map(|r| ActiveAppSession {
                    app: r.app_id.clone(),
                    started_at_ms: r.start_time_ms().unwrap_or(now_ms),
                })
                .collect();

            let should_emit_start = match &self.state {
                IndicatorState::InUse { active_apps } => {
                    // Check if newly introduced apps
                    new_active_sessions
                        .iter()
                        .any(|new_s| !active_apps.iter().any(|a| a.app == new_s.app))
                }
                _ => true,
            };

            let app_ids = new_active_sessions.iter().map(|s| s.app.clone()).collect();
            self.state = IndicatorState::InUse {
                active_apps: new_active_sessions,
            };

            if should_emit_start {
                events.push(IndicatorTransitionEvent::StartedInUse {
                    capability: self.capability.clone(),
                    apps: app_ids,
                });
            }
        } else {
            // No active records right now.
            // Check if we were previously InUse: if so, transition to RecentlyUsed.
            match &self.state {
                IndicatorState::InUse { active_apps } => {
                    let recent_apps: Vec<RecentAppSession> = active_apps
                        .iter()
                        .map(|a| RecentAppSession {
                            app: a.app.clone(),
                            stopped_at_ms: now_ms,
                        })
                        .collect();

                    let expires_at_ms = now_ms.saturating_add(self.recent_decay_ms);
                    let app_ids: Vec<AppIdentifier> =
                        recent_apps.iter().map(|r| r.app.clone()).collect();

                    self.state = IndicatorState::RecentlyUsed {
                        recent_apps,
                        expires_at_ms,
                    };

                    events.push(IndicatorTransitionEvent::TransitionedToRecentlyUsed {
                        capability: self.capability.clone(),
                        apps: app_ids,
                        expires_at_ms,
                    });
                }
                IndicatorState::RecentlyUsed { expires_at_ms, .. } => {
                    if now_ms >= *expires_at_ms {
                        self.state = IndicatorState::Idle;
                        events.push(IndicatorTransitionEvent::TransitionedToIdle {
                            capability: self.capability.clone(),
                        });
                    }
                }
                IndicatorState::Blocked { expires_at_ms, .. } => {
                    if now_ms >= *expires_at_ms {
                        self.state = IndicatorState::Idle;
                        events.push(IndicatorTransitionEvent::TransitionedToIdle {
                            capability: self.capability.clone(),
                        });
                    }
                }
                IndicatorState::Idle => {
                    // Also check if any record recently stopped within threshold
                    let recent_records: Vec<&ConsentRecord> = records
                        .iter()
                        .filter(|r| {
                            r.capability == self.capability
                                && r.was_recently_used(now_ms, self.recent_decay_ms)
                        })
                        .collect();

                    if !recent_records.is_empty() {
                        let recent_apps: Vec<RecentAppSession> = recent_records
                            .iter()
                            .map(|r| RecentAppSession {
                                app: r.app_id.clone(),
                                stopped_at_ms: r.stop_time_ms().unwrap_or(now_ms),
                            })
                            .collect();

                        let expires_at_ms = now_ms.saturating_add(self.recent_decay_ms);
                        let app_ids: Vec<AppIdentifier> =
                            recent_apps.iter().map(|r| r.app.clone()).collect();

                        self.state = IndicatorState::RecentlyUsed {
                            recent_apps,
                            expires_at_ms,
                        };

                        events.push(IndicatorTransitionEvent::TransitionedToRecentlyUsed {
                            capability: self.capability.clone(),
                            apps: app_ids,
                            expires_at_ms,
                        });
                    }
                }
            }
        }

        events
    }

    /// Construct UI chip for rendering.
    pub fn chip(&self, now_ms: u64) -> IndicatorChip {
        let (icon_name, base_color) = match self.capability {
            CapabilityKind::Webcam => ("camera", "#2ecc71"), // Emerald Green
            CapabilityKind::Microphone => ("mic", "#e67e22"), // Amber / Orange
            CapabilityKind::Location => ("location-dot", "#3498db"), // Blue
            CapabilityKind::Custom(_) => ("shield", "#9b59b6"),
        };

        match &self.state {
            IndicatorState::Idle => IndicatorChip {
                capability: self.capability.clone(),
                status: IndicatorStatusKind::Idle,
                icon_name,
                color_hex: "#7f8c8d", // Muted Gray
                label: format!("{}: Idle", self.capability),
                badge_count: 0,
                tooltip: format!("{} is not in use", self.capability),
            },
            IndicatorState::InUse { active_apps } => {
                let names: Vec<&str> = active_apps.iter().map(|s| s.app.display_name()).collect();
                let joined = names.join(", ");
                IndicatorChip {
                    capability: self.capability.clone(),
                    status: IndicatorStatusKind::InUse,
                    icon_name,
                    color_hex: base_color,
                    label: format!("{}: {}", self.capability, joined),
                    badge_count: active_apps.len(),
                    tooltip: format!("{} is in use by {}", self.capability, joined),
                }
            }
            IndicatorState::RecentlyUsed {
                recent_apps,
                expires_at_ms,
            } => {
                let remaining_s = expires_at_ms.saturating_sub(now_ms).div_ceil(1000);
                let names: Vec<&str> = recent_apps.iter().map(|s| s.app.display_name()).collect();
                let joined = names.join(", ");
                IndicatorChip {
                    capability: self.capability.clone(),
                    status: IndicatorStatusKind::RecentlyUsed,
                    icon_name,
                    color_hex: "#f1c40f", // Yellow / Warning
                    label: format!("{} used ({}s)", self.capability, remaining_s),
                    badge_count: recent_apps.len(),
                    tooltip: format!(
                        "{} was recently used by {} (closing in {}s)",
                        self.capability, joined, remaining_s
                    ),
                }
            }
            IndicatorState::Blocked {
                blocked_apps,
                expires_at_ms,
            } => {
                let remaining_s = expires_at_ms.saturating_sub(now_ms).div_ceil(1000);
                let names: Vec<&str> = blocked_apps.iter().map(|s| s.app.display_name()).collect();
                let joined = names.join(", ");
                IndicatorChip {
                    capability: self.capability.clone(),
                    status: IndicatorStatusKind::Blocked,
                    icon_name,
                    color_hex: "#e74c3c", // Red / Denied
                    label: format!("{} Blocked", self.capability),
                    badge_count: blocked_apps.len(),
                    tooltip: format!(
                        "{} access was blocked for {} (decaying in {}s)",
                        self.capability, joined, remaining_s
                    ),
                }
            }
        }
    }
}

/// Composite manager tracking indicators for Camera, Microphone, and Location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuBarPrivacyIndicators {
    pub camera: CapabilityIndicator,
    pub microphone: CapabilityIndicator,
    pub location: CapabilityIndicator,
}

impl Default for MenuBarPrivacyIndicators {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuBarPrivacyIndicators {
    pub fn new() -> Self {
        Self {
            camera: CapabilityIndicator::new(CapabilityKind::Webcam),
            microphone: CapabilityIndicator::new(CapabilityKind::Microphone),
            location: CapabilityIndicator::new(CapabilityKind::Location),
        }
    }

    /// Whether any hardware indicator is actively streaming/in use.
    pub fn is_any_in_use(&self) -> bool {
        self.camera.state.kind().is_active()
            || self.microphone.state.kind().is_active()
            || self.location.state.kind().is_active()
    }

    /// Update all indicators from a fresh ConsentStore snapshot.
    pub fn update_snapshot(
        &mut self,
        snapshot: &ConsentStoreSnapshot,
        now_ms: u64,
    ) -> Vec<IndicatorTransitionEvent> {
        let mut events = Vec::new();
        events.extend(self.camera.update_records(&snapshot.records, now_ms));
        events.extend(self.microphone.update_records(&snapshot.records, now_ms));
        events.extend(self.location.update_records(&snapshot.records, now_ms));
        events
    }

    /// Tick all indicators for time-based transitions.
    pub fn tick(&mut self, now_ms: u64) -> Vec<IndicatorTransitionEvent> {
        let mut events = Vec::new();
        events.extend(self.camera.tick(now_ms));
        events.extend(self.microphone.tick(now_ms));
        events.extend(self.location.tick(now_ms));
        events
    }

    /// Get visible chips (only those in InUse, RecentlyUsed, or Blocked state).
    pub fn visible_chips(&self, now_ms: u64) -> Vec<IndicatorChip> {
        let mut chips = Vec::new();
        for ind in [&self.camera, &self.microphone, &self.location] {
            if ind.state.kind().is_visible() {
                chips.push(ind.chip(now_ms));
            }
        }
        chips
    }

    /// Get all chips (including idle).
    pub fn all_chips(&self, now_ms: u64) -> Vec<IndicatorChip> {
        vec![
            self.camera.chip(now_ms),
            self.microphone.chip(now_ms),
            self.location.chip(now_ms),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent_store::ConsentStatus;
    use crate::filetime::FileTime;

    #[test]
    fn indicator_idle_to_in_use() {
        let mut indicator = CapabilityIndicator::new(CapabilityKind::Webcam);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::Idle);

        let now_ms = 1_000_000;
        let app = AppIdentifier::from_desktop_path("C:\\Program Files\\Zoom\\bin\\Zoom.exe");
        let record = ConsentRecord {
            app_id: app.clone(),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(now_ms)),
            last_used_stop: None, // Still active
            attribution: None,
        };

        let events = indicator.update_records(&[record], now_ms);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            IndicatorTransitionEvent::StartedInUse { capability, apps }
                if *capability == CapabilityKind::Webcam && apps[0] == app
        ));

        assert_eq!(indicator.state.kind(), IndicatorStatusKind::InUse);
        let chip = indicator.chip(now_ms);
        assert_eq!(chip.status, IndicatorStatusKind::InUse);
        assert_eq!(chip.badge_count, 1);
        assert!(chip.label.contains("Zoom"));
    }

    #[test]
    fn indicator_in_use_to_recently_used_then_idle() {
        let mut indicator = CapabilityIndicator::new(CapabilityKind::Microphone);
        let app = AppIdentifier::from_desktop_path("C:\\Discord\\Discord.exe");

        // 1. App starts using mic
        let start_ms = 1_000_000;
        let active_record = ConsentRecord {
            app_id: app.clone(),
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(start_ms)),
            last_used_stop: None,
            attribution: None,
        };
        indicator.update_records(&[active_record], start_ms);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::InUse);

        // 2. App stops using mic
        let stop_ms = start_ms + 10_000;
        let stopped_record = ConsentRecord {
            app_id: app.clone(),
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(start_ms)),
            last_used_stop: Some(FileTime::from_unix_millis(stop_ms)),
            attribution: None,
        };

        let events = indicator.update_records(&[stopped_record], stop_ms);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            IndicatorTransitionEvent::TransitionedToRecentlyUsed { capability, .. }
                if *capability == CapabilityKind::Microphone
        ));
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::RecentlyUsed);

        // Check chip during decay
        let chip_during = indicator.chip(stop_ms + 1_000);
        assert_eq!(chip_during.status, IndicatorStatusKind::RecentlyUsed);
        assert!(chip_during.tooltip.contains("Discord"));

        // 3. Tick before decay expiry (at +4s) -> remains RecentlyUsed
        let events_mid = indicator.tick(stop_ms + 4_000);
        assert!(events_mid.is_empty());
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::RecentlyUsed);

        // 4. Tick after decay expiry (at +5s) -> transitions to Idle
        let events_end = indicator.tick(stop_ms + 5_000);
        assert_eq!(events_end.len(), 1);
        assert!(matches!(
            &events_end[0],
            IndicatorTransitionEvent::TransitionedToIdle { capability }
                if *capability == CapabilityKind::Microphone
        ));
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::Idle);
    }

    #[test]
    fn indicator_blocked_transition_and_decay() {
        let mut indicator = CapabilityIndicator::new(CapabilityKind::Location);
        let app = AppIdentifier::from_desktop_path("C:\\Chrome\\chrome.exe");

        let event = indicator.record_blocked(app.clone(), 50_000);
        assert!(matches!(
            event,
            IndicatorTransitionEvent::AccessBlocked { capability, .. }
                if capability == CapabilityKind::Location
        ));
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::Blocked);

        let chip = indicator.chip(51_000);
        assert_eq!(chip.status, IndicatorStatusKind::Blocked);
        assert_eq!(chip.color_hex, "#e74c3c");

        // Tick past 5000ms
        let events = indicator.tick(55_000);
        assert_eq!(events.len(), 1);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::Idle);
    }

    #[test]
    fn menu_bar_privacy_indicators_aggregate() {
        let mut indicators = MenuBarPrivacyIndicators::new();
        assert!(!indicators.is_any_in_use());
        assert_eq!(indicators.visible_chips(1000).len(), 0);

        let snap = ConsentStoreSnapshot::new(vec![
            ConsentRecord {
                app_id: AppIdentifier::from_desktop_path("C:\\Obs\\obs.exe"),
                capability: CapabilityKind::Webcam,
                status: ConsentStatus::Allow,
                last_used_start: Some(FileTime::from_unix_millis(1000)),
                last_used_stop: None,
                attribution: None,
            },
            ConsentRecord {
                app_id: AppIdentifier::from_desktop_path("C:\\Discord\\discord.exe"),
                capability: CapabilityKind::Microphone,
                status: ConsentStatus::Allow,
                last_used_start: Some(FileTime::from_unix_millis(1000)),
                last_used_stop: None,
                attribution: None,
            },
        ]);

        indicators.update_snapshot(&snap, 1000);
        assert!(indicators.is_any_in_use());

        let visible = indicators.visible_chips(1000);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].capability, CapabilityKind::Webcam);
        assert_eq!(visible[1].capability, CapabilityKind::Microphone);
    }

    #[test]
    fn concurrent_apps_partial_stop() {
        let mut indicator = CapabilityIndicator::new(CapabilityKind::Webcam);
        let app1 = AppIdentifier::from_desktop_path("C:\\Zoom\\Zoom.exe");
        let app2 = AppIdentifier::from_desktop_path("C:\\Obs\\Obs.exe");

        let rec1 = ConsentRecord {
            app_id: app1.clone(),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(1000)),
            last_used_stop: None,
            attribution: None,
        };
        let rec2 = ConsentRecord {
            app_id: app2.clone(),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(2000)),
            last_used_stop: None,
            attribution: None,
        };

        // Both running
        indicator.update_records(&[rec1.clone(), rec2.clone()], 2500);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::InUse);
        assert_eq!(indicator.chip(2500).badge_count, 2);

        // App 1 stops, App 2 still running
        let rec1_stopped = ConsentRecord {
            app_id: app1,
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(1000)),
            last_used_stop: Some(FileTime::from_unix_millis(3000)),
            attribution: None,
        };

        indicator.update_records(&[rec1_stopped, rec2], 3500);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::InUse);
        assert_eq!(indicator.chip(3500).badge_count, 1);
    }

    #[test]
    fn recently_used_reopened_to_in_use() {
        let mut indicator = CapabilityIndicator::new(CapabilityKind::Microphone);
        let app = AppIdentifier::from_desktop_path("C:\\App\\Voice.exe");

        let stopped_rec = ConsentRecord {
            app_id: app.clone(),
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(1000)),
            last_used_stop: Some(FileTime::from_unix_millis(2000)),
            attribution: None,
        };

        // Recently stopped
        indicator.update_records(&[stopped_rec], 3000);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::RecentlyUsed);

        // Reopened at 4000
        let active_rec = ConsentRecord {
            app_id: app,
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(4000)),
            last_used_stop: None,
            attribution: None,
        };

        let events = indicator.update_records(&[active_rec], 4500);
        assert_eq!(indicator.state.kind(), IndicatorStatusKind::InUse);
        assert!(events
            .iter()
            .any(|e| matches!(e, IndicatorTransitionEvent::StartedInUse { .. })));
    }
}
