//! Per-app permission audit and toggle model that emits reversible command objects.

use crate::consent_store::{AppIdentifier, CapabilityKind, ConsentStatus, ConsentStoreSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Supported permission capabilities in Nexus-OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionCapability {
    Camera,
    Microphone,
    Location,
    Clipboard,
    Notifications,
}

impl PermissionCapability {
    pub fn all() -> [Self; 5] {
        [
            Self::Camera,
            Self::Microphone,
            Self::Location,
            Self::Clipboard,
            Self::Notifications,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::Clipboard => "Clipboard Access",
            Self::Notifications => "Notifications",
        }
    }

    pub fn is_sensitive_hardware(&self) -> bool {
        matches!(self, Self::Camera | Self::Microphone | Self::Location)
    }
}

impl fmt::Display for PermissionCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Permission grant state for an app.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum PermissionState {
    Allowed,
    #[default]
    Denied,
    Prompt,
    Restricted,
}

impl PermissionState {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    pub fn toggle(&self) -> Self {
        match self {
            Self::Allowed => Self::Denied,
            Self::Denied => Self::Allowed,
            Self::Prompt => Self::Denied,
            Self::Restricted => Self::Restricted, // Restricted cannot be toggled directly
        }
    }
}

/// Calculated risk score for an application's permission posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn color_hex(&self) -> &'static str {
        match self {
            Self::Low => "#2ecc71",      // Green
            Self::Medium => "#f39c12",   // Amber
            Self::High => "#e67e22",     // Orange
            Self::Critical => "#e74c3c", // Red
        }
    }
}

/// Full permission profile of an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPermissionProfile {
    pub app: AppIdentifier,
    pub is_packaged: bool,
    pub permissions: BTreeMap<PermissionCapability, PermissionState>,
    pub last_access_ms: BTreeMap<PermissionCapability, u64>,
    pub risk_level: RiskLevel,
}

impl AppPermissionProfile {
    pub fn new(app: AppIdentifier) -> Self {
        let is_packaged = app.is_packaged();
        let mut permissions = BTreeMap::new();
        for cap in PermissionCapability::all() {
            permissions.insert(cap, PermissionState::Denied);
        }

        Self {
            app,
            is_packaged,
            permissions,
            last_access_ms: BTreeMap::new(),
            risk_level: RiskLevel::Low,
        }
    }

    /// Update calculated risk level based on granted permissions and activity patterns.
    pub fn recalculate_risk(&mut self, now_ms: u64) {
        let camera_allowed = self.is_allowed(PermissionCapability::Camera);
        let mic_allowed = self.is_allowed(PermissionCapability::Microphone);
        let loc_allowed = self.is_allowed(PermissionCapability::Location);
        let clip_allowed = self.is_allowed(PermissionCapability::Clipboard);

        let allowed_count = self
            .permissions
            .values()
            .filter(|&&s| s == PermissionState::Allowed)
            .count();

        // Check dormancy: sensitive hardware allowed but no usage in > 30 days (2,592,000,000 ms)
        const THIRTY_DAYS_MS: u64 = 30 * 24 * 60 * 60 * 1000;
        let mut is_dormant_sensitive = false;

        for cap in [
            PermissionCapability::Camera,
            PermissionCapability::Microphone,
            PermissionCapability::Location,
        ] {
            if self.is_allowed(cap) {
                if let Some(&last_ms) = self.last_access_ms.get(&cap) {
                    if now_ms.saturating_sub(last_ms) > THIRTY_DAYS_MS {
                        is_dormant_sensitive = true;
                    }
                } else {
                    // Never recorded using it, but permission is granted
                    is_dormant_sensitive = true;
                }
            }
        }

        if (camera_allowed && mic_allowed && loc_allowed && clip_allowed)
            || (!self.is_packaged && camera_allowed && mic_allowed && is_dormant_sensitive)
        {
            self.risk_level = RiskLevel::Critical;
        } else if (camera_allowed && mic_allowed)
            || (is_dormant_sensitive && (camera_allowed || mic_allowed))
            || (!self.is_packaged && clip_allowed && loc_allowed)
        {
            self.risk_level = RiskLevel::High;
        } else if allowed_count >= 2 || loc_allowed || clip_allowed {
            self.risk_level = RiskLevel::Medium;
        } else {
            self.risk_level = RiskLevel::Low;
        }
    }

    pub fn is_allowed(&self, cap: PermissionCapability) -> bool {
        self.permissions
            .get(&cap)
            .copied()
            .map(|s| s.is_allowed())
            .unwrap_or(false)
    }

    pub fn set_permission(&mut self, cap: PermissionCapability, state: PermissionState) {
        self.permissions.insert(cap, state);
    }
}

/// Dry-run execution preview for a reversible command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunPlan {
    pub target_description: String,
    pub target_registry_key: Option<String>,
    pub old_value: String,
    pub new_value: String,
    pub is_reversible: bool,
    pub safety_notes: String,
}

/// A reversible, dry-run-able command object representing a permission toggle.
///
/// Nexus-OS guarantees that every permission mutation can be planned, inspected,
/// dry-run tested, and safely inverted (undone) without bricking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionToggleCommand {
    pub command_id: u64,
    pub app: AppIdentifier,
    pub capability: PermissionCapability,
    pub previous_state: PermissionState,
    pub desired_state: PermissionState,
    pub reason: String,
    pub timestamp_ms: u64,
}

impl PermissionToggleCommand {
    /// Invert this command to return the system to its previous state.
    pub fn reverse(&self) -> Self {
        Self {
            command_id: self.command_id.wrapping_add(1),
            app: self.app.clone(),
            capability: self.capability,
            previous_state: self.desired_state,
            desired_state: self.previous_state,
            reason: format!("Revert: {}", self.reason),
            timestamp_ms: self.timestamp_ms,
        }
    }

    /// Generate dry-run preview describing the mutation that would occur.
    pub fn dry_run(&self) -> DryRunPlan {
        let registry_key = match (&self.capability, &self.app) {
            (
                PermissionCapability::Camera
                | PermissionCapability::Microphone
                | PermissionCapability::Location,
                AppIdentifier::Packaged {
                    package_family_name,
                    ..
                },
            ) => {
                let cap_str = match self.capability {
                    PermissionCapability::Camera => "webcam",
                    PermissionCapability::Microphone => "microphone",
                    PermissionCapability::Location => "location",
                    _ => "unknown",
                };
                Some(format!(
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{}\\{}\\Value",
                    cap_str, package_family_name
                ))
            }
            (
                PermissionCapability::Camera
                | PermissionCapability::Microphone
                | PermissionCapability::Location,
                AppIdentifier::NonPackaged { raw_key, .. },
            ) => {
                let cap_str = match self.capability {
                    PermissionCapability::Camera => "webcam",
                    PermissionCapability::Microphone => "microphone",
                    PermissionCapability::Location => "location",
                    _ => "unknown",
                };
                Some(format!(
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{}\\NonPackaged\\{}\\Value",
                    cap_str, raw_key
                ))
            }
            (PermissionCapability::Clipboard, app) => Some(format!(
                "Nexus::Shield\\Policy\\Clipboard\\{}\\{:?}",
                app.display_name(),
                self.desired_state
            )),
            (PermissionCapability::Notifications, app) => Some(format!(
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings\\{}\\Enabled",
                app.display_name()
            )),
        };

        DryRunPlan {
            target_description: format!(
                "Toggle {} for '{}' from {:?} to {:?}",
                self.capability,
                self.app.display_name(),
                self.previous_state,
                self.desired_state
            ),
            target_registry_key: registry_key,
            old_value: format!("{:?}", self.previous_state),
            new_value: format!("{:?}", self.desired_state),
            is_reversible: true,
            safety_notes:
                "Reversible command. Applying produces an inverse command in undo history.".into(),
        }
    }
}

/// Security recommendations identified during permission audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecommendation {
    pub app: AppIdentifier,
    pub capability: PermissionCapability,
    pub severity: RiskLevel,
    pub title: String,
    pub explanation: String,
    pub suggested_toggle: PermissionToggleCommand,
}

/// Comprehensive audit summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAuditReport {
    pub total_apps: usize,
    pub critical_risk_apps: Vec<AppPermissionProfile>,
    pub high_risk_apps: Vec<AppPermissionProfile>,
    pub recommendations: Vec<AuditRecommendation>,
}

/// Store holding and auditing per-app permissions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PermissionStore {
    pub profiles: BTreeMap<String, AppPermissionProfile>,
    next_command_id: u64,
}

impl PermissionStore {
    pub fn new() -> Self {
        Self {
            profiles: BTreeMap::new(),
            next_command_id: 1,
        }
    }

    /// Ingest from a ConsentStore snapshot.
    pub fn ingest_consent_snapshot(&mut self, snapshot: &ConsentStoreSnapshot, now_ms: u64) {
        for record in &snapshot.records {
            let key = record.app_id.identifier().to_string();
            let profile = self
                .profiles
                .entry(key)
                .or_insert_with(|| AppPermissionProfile::new(record.app_id.clone()));

            let perm_cap = match record.capability {
                CapabilityKind::Webcam => Some(PermissionCapability::Camera),
                CapabilityKind::Microphone => Some(PermissionCapability::Microphone),
                CapabilityKind::Location => Some(PermissionCapability::Location),
                CapabilityKind::Custom(_) => None,
            };

            if let Some(cap) = perm_cap {
                let state = match record.status {
                    ConsentStatus::Allow => PermissionState::Allowed,
                    ConsentStatus::Deny => PermissionState::Denied,
                    ConsentStatus::Prompt => PermissionState::Prompt,
                    ConsentStatus::Unspecified => PermissionState::Denied,
                };
                profile.set_permission(cap, state);

                if let Some(start) = record.start_time_ms() {
                    profile.last_access_ms.insert(cap, start);
                }
            }
        }

        // Recalculate risks for all apps
        for profile in self.profiles.values_mut() {
            profile.recalculate_risk(now_ms);
        }
    }

    /// Create a toggle command for an app's permission.
    pub fn create_toggle_command(
        &mut self,
        app: &AppIdentifier,
        capability: PermissionCapability,
        desired_state: PermissionState,
        reason: &str,
        now_ms: u64,
    ) -> Option<PermissionToggleCommand> {
        let profile = self.profiles.get(app.identifier())?;
        let previous_state = profile
            .permissions
            .get(&capability)
            .copied()
            .unwrap_or(PermissionState::Denied);

        let cmd = PermissionToggleCommand {
            command_id: self.next_command_id,
            app: app.clone(),
            capability,
            previous_state,
            desired_state,
            reason: reason.to_string(),
            timestamp_ms: now_ms,
        };
        self.next_command_id += 1;
        Some(cmd)
    }

    /// Apply a command to the in-memory store model (mutates pure model, does NOT touch OS).
    pub fn apply_command(&mut self, cmd: &PermissionToggleCommand, now_ms: u64) {
        if let Some(profile) = self.profiles.get_mut(cmd.app.identifier()) {
            profile.set_permission(cmd.capability, cmd.desired_state);
            profile.recalculate_risk(now_ms);
        }
    }

    /// Perform privacy audit and generate report.
    pub fn audit(&self, now_ms: u64) -> PermissionAuditReport {
        let mut critical_apps = Vec::new();
        let mut high_apps = Vec::new();
        let mut recommendations = Vec::new();

        let mut cmd_id = self.next_command_id;

        for profile in self.profiles.values() {
            match profile.risk_level {
                RiskLevel::Critical => critical_apps.push(profile.clone()),
                RiskLevel::High => high_apps.push(profile.clone()),
                _ => {}
            }

            // Generate recommendations for dormant sensitive permissions
            for cap in [
                PermissionCapability::Camera,
                PermissionCapability::Microphone,
                PermissionCapability::Location,
            ] {
                if profile.is_allowed(cap) {
                    let last_ms = profile.last_access_ms.get(&cap).copied();
                    let is_dormant = match last_ms {
                        Some(last) => now_ms.saturating_sub(last) > (30 * 24 * 60 * 60 * 1000),
                        None => true,
                    };

                    if is_dormant {
                        let cmd = PermissionToggleCommand {
                            command_id: cmd_id,
                            app: profile.app.clone(),
                            capability: cap,
                            previous_state: PermissionState::Allowed,
                            desired_state: PermissionState::Denied,
                            reason: format!(
                                "Revoke unused {} access from dormant application '{}'",
                                cap,
                                profile.app.display_name()
                            ),
                            timestamp_ms: now_ms,
                        };
                        cmd_id += 1;

                        recommendations.push(AuditRecommendation {
                            app: profile.app.clone(),
                            capability: cap,
                            severity: if profile.is_packaged {
                                RiskLevel::Medium
                            } else {
                                RiskLevel::High
                            },
                            title: format!("Unused {} Permission", cap),
                            explanation: format!(
                                "'{}' has not accessed {} in >30 days. Revoke permission to minimize attack surface.",
                                profile.app.display_name(),
                                cap
                            ),
                            suggested_toggle: cmd,
                        });
                    }
                }
            }
        }

        PermissionAuditReport {
            total_apps: self.profiles.len(),
            critical_risk_apps: critical_apps,
            high_risk_apps: high_apps,
            recommendations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent_store::ConsentRecord;
    use crate::filetime::FileTime;

    #[test]
    fn permission_toggle_command_reversibility() {
        let app = AppIdentifier::from_desktop_path("C:\\Program Files\\Zoom\\bin\\Zoom.exe");
        let cmd = PermissionToggleCommand {
            command_id: 100,
            app: app.clone(),
            capability: PermissionCapability::Camera,
            previous_state: PermissionState::Allowed,
            desired_state: PermissionState::Denied,
            reason: "User revoked camera access".into(),
            timestamp_ms: 1234567,
        };

        let rev = cmd.reverse();
        assert_eq!(rev.app, app);
        assert_eq!(rev.capability, PermissionCapability::Camera);
        assert_eq!(rev.previous_state, PermissionState::Denied);
        assert_eq!(rev.desired_state, PermissionState::Allowed);
        assert!(rev.reason.starts_with("Revert:"));

        // Dry run test
        let plan = cmd.dry_run();
        assert!(plan.is_reversible);
        assert!(plan.target_description.contains("Zoom"));
        assert!(plan.target_registry_key.unwrap().contains("webcam"));
    }

    #[test]
    fn permission_store_audit_and_recommendations() {
        let mut store = PermissionStore::new();
        let now_ms = 10_000_000_000u64;
        let old_ms = now_ms - (45 * 24 * 60 * 60 * 1000); // 45 days ago

        let snapshot = ConsentStoreSnapshot::new(vec![
            ConsentRecord {
                app_id: AppIdentifier::from_desktop_path("C:\\Tools\\legacy_tool.exe"),
                capability: CapabilityKind::Webcam,
                status: ConsentStatus::Allow,
                last_used_start: Some(FileTime::from_unix_millis(old_ms)),
                last_used_stop: Some(FileTime::from_unix_millis(old_ms + 1000)),
                attribution: None,
            },
            ConsentRecord {
                app_id: AppIdentifier::from_desktop_path("C:\\Tools\\legacy_tool.exe"),
                capability: CapabilityKind::Microphone,
                status: ConsentStatus::Allow,
                last_used_start: Some(FileTime::from_unix_millis(old_ms)),
                last_used_stop: Some(FileTime::from_unix_millis(old_ms + 1000)),
                attribution: None,
            },
        ]);

        store.ingest_consent_snapshot(&snapshot, now_ms);
        let audit = store.audit(now_ms);

        assert_eq!(audit.total_apps, 1);
        assert_eq!(audit.critical_risk_apps.len(), 1);
        assert_eq!(audit.critical_risk_apps[0].risk_level, RiskLevel::Critical);
        assert_eq!(audit.recommendations.len(), 2);

        // Apply recommendation toggle to verify in-memory update
        let rec = &audit.recommendations[0];
        store.apply_command(&rec.suggested_toggle, now_ms);

        let app_prof = store.profiles.get("C:\\Tools\\legacy_tool.exe").unwrap();
        assert_eq!(
            app_prof.permissions.get(&rec.capability),
            Some(&PermissionState::Denied)
        );
    }

    #[test]
    fn command_double_reversal_is_identity() {
        let app = AppIdentifier::from_desktop_path("C:\\App\\app.exe");
        let cmd = PermissionToggleCommand {
            command_id: 42,
            app,
            capability: PermissionCapability::Microphone,
            previous_state: PermissionState::Allowed,
            desired_state: PermissionState::Denied,
            reason: "Revoke mic".into(),
            timestamp_ms: 100,
        };

        let rev1 = cmd.reverse();
        let rev2 = rev1.reverse();

        assert_eq!(cmd.app, rev2.app);
        assert_eq!(cmd.capability, rev2.capability);
        assert_eq!(cmd.previous_state, rev2.previous_state);
        assert_eq!(cmd.desired_state, rev2.desired_state);
    }

    #[test]
    fn packaged_app_risk_assessment() {
        let app = AppIdentifier::from_package_family_name("Microsoft.WindowsCamera_8wekyb3d8bbwe");
        let mut profile = AppPermissionProfile::new(app);
        assert!(profile.is_packaged);

        // Only notifications allowed -> Low risk
        profile.set_permission(
            PermissionCapability::Notifications,
            PermissionState::Allowed,
        );
        profile.recalculate_risk(1000);
        assert_eq!(profile.risk_level, RiskLevel::Low);

        // Add Camera -> Medium risk
        profile.set_permission(PermissionCapability::Camera, PermissionState::Allowed);
        profile
            .last_access_ms
            .insert(PermissionCapability::Camera, 950);
        profile.recalculate_risk(1000);
        assert_eq!(profile.risk_level, RiskLevel::Medium);
    }
}
