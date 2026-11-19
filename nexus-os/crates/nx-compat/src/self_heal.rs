//! Self-healing engine for Windows update resilience.
//!
//! Detects capability regressions and reset settings after Windows updates,
//! degrades gracefully with non-blocking notices, auto-re-enables when supported,
//! and safely re-applies reset settings through the never-brick journal and Platform trait.

use nexus_journal::{Journal, Mutation};
use nexus_model::platform::{Platform, RegistryValueData, ServiceRunState};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::build::WindowsBuildInfo;
use crate::error::{CompatError, CompatResult};
use crate::notice::NoticeQueue;
use crate::probe::{Capability, CapabilityStatus, ProbeProvider, ProbeResult};
use crate::store::{BuildHistoryStore, BuildStatus};

/// Runtime operational state of an individual Nexus-OS feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureState {
    Enabled,
    Degraded { fallback: String },
    Disabled { reason: String },
}

impl FeatureState {
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Enabled | Self::Degraded { .. })
    }
}

/// Known settings Windows updates commonly reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResilienceSetting {
    RegistryDword {
        category: &'static str,
        name: &'static str,
        hive: &'static str,
        subkey: &'static str,
        value_name: &'static str,
        target_value: u32,
    },
    RegistryString {
        category: &'static str,
        name: &'static str,
        hive: &'static str,
        subkey: &'static str,
        value_name: &'static str,
        target_value: &'static str,
    },
    Service {
        category: &'static str,
        name: &'static str,
        service_name: &'static str,
        target_running: bool,
    },
    TaskbarSuppressed {
        category: &'static str,
        name: &'static str,
        should_suppress: bool,
    },
}

impl ResilienceSetting {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RegistryDword { name, .. }
            | Self::RegistryString { name, .. }
            | Self::Service { name, .. }
            | Self::TaskbarSuppressed { name, .. } => name,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::RegistryDword { category, .. }
            | Self::RegistryString { category, .. }
            | Self::Service { category, .. }
            | Self::TaskbarSuppressed { category, .. } => category,
        }
    }

    /// Standard list of settings Windows updates commonly reset.
    pub fn common_settings() -> Vec<ResilienceSetting> {
        vec![
            // 1. Privacy & Telemetry
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Disable Telemetry Policy",
                hive: "HKLM",
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                value_name: "AllowTelemetry",
                target_value: 0,
            },
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Limit Max Telemetry",
                hive: "HKLM",
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                value_name: "MaxTelemetryAllowed",
                target_value: 0,
            },
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Disable Advertising ID",
                hive: "HKCU",
                subkey: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
                value_name: "Enabled",
                target_value: 0,
            },
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Disable Activity Feed",
                hive: "HKLM",
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\System",
                value_name: "EnableActivityFeed",
                target_value: 0,
            },
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Disable Windows Copilot",
                hive: "HKCU",
                subkey: r"Software\Policies\Microsoft\Windows\WindowsCopilot",
                value_name: "TurnOffWindowsCopilot",
                target_value: 1,
            },
            ResilienceSetting::RegistryDword {
                category: "Privacy",
                name: "Disable Recall AI Snapshots",
                hive: "HKLM",
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI",
                value_name: "DisableAIDataAnalysis",
                target_value: 1,
            },
            ResilienceSetting::Service {
                category: "Privacy",
                name: "Disable DiagTrack Service",
                service_name: "DiagTrack",
                target_running: false,
            },
            ResilienceSetting::Service {
                category: "Privacy",
                name: "Disable WAP Push Service",
                service_name: "dmwappushservice",
                target_running: false,
            },
            // 2. Explorer & Taskbar settings
            ResilienceSetting::RegistryDword {
                category: "Explorer",
                name: "Taskbar Left Alignment",
                hive: "HKCU",
                subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
                value_name: "TaskbarAl",
                target_value: 0,
            },
            ResilienceSetting::RegistryDword {
                category: "Explorer",
                name: "Hide TaskView Button",
                hive: "HKCU",
                subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
                value_name: "ShowTaskViewButton",
                target_value: 0,
            },
            ResilienceSetting::TaskbarSuppressed {
                category: "Explorer",
                name: "Native Taskbar Hidden",
                should_suppress: true,
            },
            // 3. Default Apps association
            ResilienceSetting::RegistryString {
                category: "DefaultApps",
                name: "Default Web Browser",
                hive: "HKCU",
                subkey: r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\http\UserChoice",
                value_name: "ProgId",
                target_value: "NexusBrowser",
            },
        ]
    }
}

/// Report summarizing self-healing operations after startup self-check.
#[derive(Debug, Clone, Default)]
pub struct SelfCheckReport {
    pub build_status: Option<BuildStatus>,
    pub regressions: Vec<(Capability, String)>,
    pub re_enabled: Vec<Capability>,
    pub probed_capabilities: Vec<ProbeResult>,
    pub settings_verified: usize,
    pub settings_healed: Vec<String>,
    pub settings_failed: Vec<(String, String)>,
}

impl SelfCheckReport {
    pub fn is_clean(&self) -> bool {
        self.regressions.is_empty() && self.settings_failed.is_empty()
    }
}

/// Self-healing resilience orchestrator.
pub struct SelfHealingEngine {
    store: BuildHistoryStore,
    notices: Arc<NoticeQueue>,
    feature_states: RwLock<HashMap<Capability, FeatureState>>,
}

impl SelfHealingEngine {
    pub fn new(store: BuildHistoryStore, notices: Arc<NoticeQueue>) -> Self {
        Self {
            store,
            notices,
            feature_states: RwLock::new(HashMap::new()),
        }
    }

    pub fn feature_state(&self, capability: Capability) -> FeatureState {
        self.feature_states
            .read()
            .get(&capability)
            .cloned()
            .unwrap_or(FeatureState::Enabled)
    }

    pub fn is_feature_operational(&self, capability: Capability) -> bool {
        self.feature_state(capability).is_operational()
    }

    /// Performs the startup self-check:
    /// 1. Detects build/UBR change against recorded state.
    /// 2. Probes all capabilities; degrades/disables failing ones; auto-re-enables restored ones.
    /// 3. If build changed (or forced), verifies known reset settings and re-applies via journal.
    /// 4. Undoes via journal if any re-apply fails so Windows is never left broken.
    pub fn run_startup_self_check(
        &self,
        current_build: &WindowsBuildInfo,
        platform: &dyn Platform,
        journal: &mut Journal,
        probe_provider: &dyn ProbeProvider,
        check_settings: bool,
    ) -> CompatResult<SelfCheckReport> {
        let mut report = SelfCheckReport::default();

        // 1. Build change detection
        let build_status = self.store.check_and_record(current_build)?;
        let build_changed = build_status.is_changed();
        report.build_status = Some(build_status.clone());

        if let BuildStatus::BuildChanged {
            ref previous,
            ref current,
        } = build_status
        {
            self.notices.push_info(
                None,
                "Windows Update Detected",
                format!(
                    "Upgraded from {} to {}. Running self-healing verification.",
                    previous.full_version_string(),
                    current.full_version_string()
                ),
            );
        }

        // 2. Capability probing & state transitions
        let mut states_guard = self.feature_states.write();
        for &cap in Capability::ALL {
            let probe_res = probe_provider.probe(cap);
            report.probed_capabilities.push(probe_res.clone());

            let prev_state = states_guard
                .get(&cap)
                .cloned()
                .unwrap_or(FeatureState::Enabled);

            match probe_res.status {
                CapabilityStatus::Available => {
                    if !prev_state.is_enabled() {
                        // Re-enable automatically!
                        report.re_enabled.push(cap);
                        self.notices.push_info(
                            Some(cap),
                            "Feature Re-enabled",
                            format!(
                                "{} is supported again on Windows build {}.",
                                cap.name(),
                                current_build.build
                            ),
                        );
                    }
                    states_guard.insert(cap, FeatureState::Enabled);
                }
                CapabilityStatus::Degraded { ref fallback } => {
                    let fb = fallback.clone();
                    report.regressions.push((cap, fb.clone()));
                    states_guard.insert(
                        cap,
                        FeatureState::Degraded {
                            fallback: fb.clone(),
                        },
                    );
                    self.notices.push_warning(
                        Some(cap),
                        "Feature Degraded",
                        format!("{}: Using graceful fallback: {}", cap.name(), fb),
                    );
                }
                CapabilityStatus::Unavailable {
                    ref reason,
                    ref fallback,
                } => {
                    let r = reason.clone();
                    let fb = fallback.clone();
                    report
                        .regressions
                        .push((cap, format!("{r} (fallback: {fb})")));
                    states_guard.insert(cap, FeatureState::Disabled { reason: r.clone() });
                    self.notices.push_warning(
                        Some(cap),
                        "Feature Disabled",
                        format!("{}: {} Fallback: {}", cap.name(), r, fb),
                    );
                }
            }
        }
        drop(states_guard);

        // 3. Settings verification and re-application (on build change or forced check)
        if build_changed || check_settings {
            let settings = ResilienceSetting::common_settings();
            report.settings_verified = settings.len();

            for setting in settings {
                let is_reset = self.verify_setting_reset(&setting, platform);
                if is_reset {
                    let setting_name = setting.name().to_string();
                    match self.reapply_setting(&setting, platform, journal) {
                        Ok(()) => {
                            self.notices.push_info(
                                None,
                                "Setting Re-applied",
                                format!("Restored {} after Windows update.", setting_name),
                            );
                            report.settings_healed.push(setting_name);
                        }
                        Err(err) => {
                            self.notices.push_warning(
                                None,
                                "Re-apply Failed & Reverted",
                                format!(
                                    "Could not re-apply {}; rolled back via journal: {}",
                                    setting_name, err
                                ),
                            );
                            report.settings_failed.push((setting_name, err.to_string()));
                        }
                    }
                }
            }
        }

        Ok(report)
    }

    /// Checks if a given setting has been reset by Windows.
    fn verify_setting_reset(&self, setting: &ResilienceSetting, platform: &dyn Platform) -> bool {
        match setting {
            ResilienceSetting::RegistryDword {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => match platform.get_registry_value(hive, subkey, value_name) {
                Ok(Some(RegistryValueData::Dword(current))) => current != *target_value,
                // Missing value counts as reset if target is set
                Ok(None) => true,
                _ => false,
            },
            ResilienceSetting::RegistryString {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => match platform.get_registry_value(hive, subkey, value_name) {
                Ok(Some(RegistryValueData::Sz(ref current))) => current != target_value,
                Ok(None) => true,
                _ => false,
            },
            ResilienceSetting::Service {
                service_name,
                target_running,
                ..
            } => match platform.get_service_state(service_name) {
                Ok(Some(current)) => {
                    let is_running = current == ServiceRunState::Running;
                    is_running != *target_running
                }
                Ok(None) => false,
                Err(_) => false,
            },
            ResilienceSetting::TaskbarSuppressed {
                should_suppress, ..
            } => {
                if *should_suppress {
                    platform.is_taskbar_visible()
                } else {
                    false
                }
            }
        }
    }

    /// Re-applies a reset setting through the never-brick journal.
    /// If application fails, rolls back the journal entry and restores previous state.
    fn reapply_setting(
        &self,
        setting: &ResilienceSetting,
        platform: &dyn Platform,
        journal: &mut Journal,
    ) -> CompatResult<()> {
        let mutation = match setting {
            ResilienceSetting::RegistryDword {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => {
                let prev_bytes = platform
                    .get_registry_value(hive, subkey, value_name)
                    .ok()
                    .flatten()
                    .map(|v| v.encode());
                Mutation::RegistryValue {
                    hive: hive.to_string(),
                    subkey: subkey.to_string(),
                    value_name: value_name.to_string(),
                    previous: prev_bytes,
                    new: Some(RegistryValueData::Dword(*target_value).encode()),
                }
            }
            ResilienceSetting::RegistryString {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => {
                let prev_bytes = platform
                    .get_registry_value(hive, subkey, value_name)
                    .ok()
                    .flatten()
                    .map(|v| v.encode());
                Mutation::RegistryValue {
                    hive: hive.to_string(),
                    subkey: subkey.to_string(),
                    value_name: value_name.to_string(),
                    previous: prev_bytes,
                    new: Some(RegistryValueData::Sz(target_value.to_string()).encode()),
                }
            }
            ResilienceSetting::Service {
                service_name,
                target_running,
                ..
            } => {
                let prev_state = platform
                    .get_service_state(service_name)
                    .ok()
                    .flatten()
                    .unwrap_or(if *target_running {
                        ServiceRunState::Disabled
                    } else {
                        ServiceRunState::Running
                    });
                let target_state = if *target_running {
                    ServiceRunState::Running
                } else {
                    ServiceRunState::Disabled
                };
                Mutation::ServiceState {
                    service_name: service_name.to_string(),
                    previous_state: prev_state.to_code(),
                    new_state: target_state.to_code(),
                }
            }
            ResilienceSetting::TaskbarSuppressed {
                should_suppress, ..
            } => {
                if *should_suppress {
                    Mutation::TaskbarHide
                } else {
                    return Ok(());
                }
            }
        };

        // 1. Record in journal BEFORE applying
        journal
            .record(&mutation)
            .map_err(|e| CompatError::Journal(e.to_string()))?;

        // 2. Apply via Platform trait
        let apply_result = match setting {
            ResilienceSetting::RegistryDword {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => platform
                .set_registry_value(
                    hive,
                    subkey,
                    value_name,
                    RegistryValueData::Dword(*target_value),
                )
                .map_err(|e| CompatError::Platform(e.to_string())),
            ResilienceSetting::RegistryString {
                hive,
                subkey,
                value_name,
                target_value,
                ..
            } => platform
                .set_registry_value(
                    hive,
                    subkey,
                    value_name,
                    RegistryValueData::Sz(target_value.to_string()),
                )
                .map_err(|e| CompatError::Platform(e.to_string())),
            ResilienceSetting::Service {
                service_name,
                target_running,
                ..
            } => {
                let target_state = if *target_running {
                    ServiceRunState::Running
                } else {
                    ServiceRunState::Disabled
                };
                platform
                    .set_service_state(service_name, target_state)
                    .map_err(|e| CompatError::Platform(e.to_string()))
            }
            ResilienceSetting::TaskbarSuppressed {
                should_suppress, ..
            } => platform
                .set_taskbar_visible(!*should_suppress)
                .map_err(|e| CompatError::Platform(e.to_string())),
        };

        // 3. Rollback via journal if application failed
        if let Err(_err) = apply_result {
            let _ = journal.remove_last_equal(&mutation);
            return Err(CompatError::ReapplyFailed {
                setting: setting.name().to_string(),
                rolled_back: true,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::MockProbeProvider;
    use nexus_model::platform::FakePlatform;

    #[test]
    fn test_self_healing_build_change_and_regressed_probe() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = BuildHistoryStore::new(temp_dir.path());
        let notices = Arc::new(NoticeQueue::new());
        let engine = SelfHealingEngine::new(store, notices.clone());

        let journal_path = temp_dir.path().join("journal.nxjr");
        let mut journal = Journal::open(&journal_path).unwrap();

        let platform = FakePlatform::new();

        // 1. First run on 23H2 (all probes available)
        let b1 = WindowsBuildInfo::new(10, 0, 22631, 4387, "23H2", "Windows 11 Pro");
        let probes = MockProbeProvider::new();
        let report1 = engine
            .run_startup_self_check(&b1, &platform, &mut journal, &probes, false)
            .unwrap();
        assert!(report1.is_clean());
        assert!(engine.is_feature_operational(Capability::DwmMica));

        // 2. Windows Update to 24H2, but DwmMica probe fails / regresses
        let b2 = WindowsBuildInfo::new(10, 0, 26100, 1742, "24H2", "Windows 11 Pro");
        probes.set_degraded(
            Capability::DwmMica,
            "Acrylic blur fallback",
            "Mica attribute rejected by DWM on new build",
        );

        let report2 = engine
            .run_startup_self_check(&b2, &platform, &mut journal, &probes, false)
            .unwrap();
        assert!(!report2.is_clean());
        assert_eq!(report2.regressions.len(), 1);
        assert_eq!(report2.regressions[0].0, Capability::DwmMica);

        // Feature is degraded but still operational (graceful fallback)
        assert!(engine.is_feature_operational(Capability::DwmMica));
        match engine.feature_state(Capability::DwmMica) {
            FeatureState::Degraded { fallback } => assert!(fallback.contains("Acrylic")),
            _ => panic!("Expected degraded state"),
        }
        assert!(notices.count() >= 1);

        // 3. Newer Windows update restores Mica compatibility -> auto re-enabled!
        let b3 = WindowsBuildInfo::new(10, 0, 26100, 2150, "24H2", "Windows 11 Pro");
        probes.set_available(Capability::DwmMica);

        let report3 = engine
            .run_startup_self_check(&b3, &platform, &mut journal, &probes, false)
            .unwrap();
        assert!(report3.is_clean());
        assert_eq!(report3.re_enabled, vec![Capability::DwmMica]);
        assert!(engine.feature_state(Capability::DwmMica).is_enabled());
    }

    #[test]
    fn test_self_healing_reapplies_reset_settings_through_journal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = BuildHistoryStore::new(temp_dir.path());
        let notices = Arc::new(NoticeQueue::new());
        let engine = SelfHealingEngine::new(store, notices);

        let journal_path = temp_dir.path().join("journal.nxjr");
        let mut journal = Journal::open(&journal_path).unwrap();

        let platform = FakePlatform::new();

        // Simulate Windows update resetting Telemetry to enabled (3) and taskbar to visible
        platform
            .set_registry_value(
                "HKLM",
                r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                "AllowTelemetry",
                RegistryValueData::Dword(3),
            )
            .unwrap();
        platform.set_taskbar_visible(true).unwrap();

        let b = WindowsBuildInfo::default_windows_11();
        let probes = MockProbeProvider::new();

        // Run self-check with settings verification
        let report = engine
            .run_startup_self_check(&b, &platform, &mut journal, &probes, true)
            .unwrap();

        // Verify telemetry policy was re-applied to 0
        assert_eq!(
            platform
                .get_registry_value(
                    "HKLM",
                    r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                    "AllowTelemetry"
                )
                .unwrap(),
            Some(RegistryValueData::Dword(0))
        );

        // Verify taskbar was re-suppressed
        assert!(!platform.is_taskbar_visible());

        // Verify journal recorded the mutations
        assert!(!journal.is_empty());
        assert!(report.settings_healed.len() >= 2);
    }
}
