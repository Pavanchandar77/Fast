//! Comprehensive resilience integration tests using FakePlatform.
//!
//! Validates:
//! - Simulated Windows build changes (fresh, patch, major build upgrade).
//! - Missing undocumented exports (ntdll, user32) -> correct fallbacks, no panic.
//! - Failing probes (Mica -> Acrylic -> solid theme fallback).
//! - Reset settings detection & re-apply through journal and Platform trait.
//! - Never-brick rollback on re-apply failure.
//! - Mid-session explorer.exe (`TaskbarCreated`) and DWM restarts without user action.

use nexus_journal::Journal;
use nexus_model::platform::{
    FakePlatform, Platform, PlatformError, PowerProfile, PrivacyToggle, RegistryValueData,
    ServiceRunState,
};
use nx_compat::build::WindowsBuildInfo;
use nx_compat::notice::NoticeQueue;
use nx_compat::probe::{Capability, MockProbeProvider, ProbeManager};
use nx_compat::restart::{RestartConfig, RestartHandler};
use nx_compat::self_heal::SelfHealingEngine;
use nx_compat::store::{BuildHistoryStore, BuildStatus};
use std::sync::Arc;

#[test]
fn test_simulated_build_upgrade_workflow() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = BuildHistoryStore::new(temp_dir.path());
    let notices = Arc::new(NoticeQueue::new());
    let engine = SelfHealingEngine::new(store, notices.clone());

    let journal_path = temp_dir.path().join("resilience_journal.nxjr");
    let mut journal = Journal::open(&journal_path).unwrap();
    let platform = FakePlatform::new();
    let probes = MockProbeProvider::new();

    // Stage 1: Initial install on Windows 11 23H2 (Build 22631.4387)
    let build_23h2 = WindowsBuildInfo::new(10, 0, 22631, 4387, "23H2", "Windows 11 Pro");
    let report1 = engine
        .run_startup_self_check(&build_23h2, &platform, &mut journal, &probes, false)
        .unwrap();

    assert_eq!(
        report1.build_status,
        Some(BuildStatus::FreshInstall(build_23h2.clone()))
    );
    assert!(report1.is_clean());

    // Stage 2: Second startup without build change
    let report2 = engine
        .run_startup_self_check(&build_23h2, &platform, &mut journal, &probes, false)
        .unwrap();
    assert_eq!(
        report2.build_status,
        Some(BuildStatus::Unchanged(build_23h2.clone()))
    );

    // Stage 3: Windows Update to Windows 11 24H2 (Build 26100.1742)
    let build_24h2 = WindowsBuildInfo::new(10, 0, 26100, 1742, "24H2", "Windows 11 Pro");
    let report3 = engine
        .run_startup_self_check(&build_24h2, &platform, &mut journal, &probes, false)
        .unwrap();

    match report3.build_status {
        Some(BuildStatus::BuildChanged { previous, current }) => {
            assert_eq!(previous, build_23h2);
            assert_eq!(current, build_24h2);
        }
        _ => panic!("Expected BuildChanged status"),
    }
    assert!(notices.count() >= 1);
    assert!(notices
        .all()
        .iter()
        .any(|n| n.title.contains("Windows Update Detected")));
}

#[test]
fn test_missing_exports_and_failing_probes_graceful_fallbacks() {
    let probes = MockProbeProvider::new();

    // 1. Simulate ntdll!NtSetTimerResolution missing in a new preview build
    probes.set_degraded(
        Capability::NtSetTimerResolution,
        "winmm!timeBeginPeriod(1) multimedia fallback",
        "Export missing in ntdll.dll",
    );

    // 2. Simulate ntdll!NtSuspendProcess missing
    probes.set_unavailable(
        Capability::NtSuspendProcess,
        "NtSuspendProcess removed by Windows Update",
    );

    // 3. Simulate DWM Mica backdrop rejected
    probes.set_degraded(
        Capability::DwmMica,
        "Downgrade to Acrylic blur shader",
        "DWMWA_SYSTEMBACKDROP_TYPE returned error",
    );

    let pm = ProbeManager::new(probes);

    // Verify probe results and fallbacks
    assert!(!pm.is_supported(Capability::NtSetTimerResolution));
    assert!(pm.is_operational(Capability::NtSetTimerResolution));
    assert!(pm
        .fallback_for(Capability::NtSetTimerResolution)
        .contains("winmm"));

    assert!(!pm.is_supported(Capability::NtSuspendProcess));
    assert!(!pm.is_operational(Capability::NtSuspendProcess));
    assert!(pm
        .fallback_for(Capability::NtSuspendProcess)
        .contains("Skip process suspension"));

    assert!(!pm.is_supported(Capability::DwmMica));
    assert!(pm.is_operational(Capability::DwmMica));
    assert!(pm.fallback_for(Capability::DwmMica).contains("Acrylic"));

    // Verify safe dispatch wrappers never panic with missing exports
    assert!(nx_compat::dispatch::safe_nt_suspend_process(100).is_ok());
    assert!(nx_compat::dispatch::safe_nt_resume_process(100).is_ok());
    assert!(nx_compat::dispatch::safe_nt_set_timer_resolution(5000, true).is_ok());
    assert!(nx_compat::dispatch::safe_nt_terminate_process(100, 0).is_ok());
    assert!(nx_compat::dispatch::safe_apply_dwm_attributes(1, 2, true, true, true).is_ok());
    assert!(nx_compat::dispatch::safe_set_window_cloak(1, true).is_ok());
    assert!(nx_compat::dispatch::safe_set_dwm_transitions(1, false).is_ok());
}

#[test]
fn test_reset_settings_verified_and_healed_with_journal_consistency() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = BuildHistoryStore::new(temp_dir.path());
    let notices = Arc::new(NoticeQueue::new());
    let engine = SelfHealingEngine::new(store, notices.clone());

    let journal_path = temp_dir.path().join("heal_journal.nxjr");
    let mut journal = Journal::open(&journal_path).unwrap();
    let platform = FakePlatform::new();
    let probes = MockProbeProvider::new();

    // Simulate initial state where settings were Nexus-configured
    platform
        .set_registry_value(
            "HKLM",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            RegistryValueData::Dword(0),
        )
        .unwrap();
    platform.set_taskbar_visible(false).unwrap();
    platform
        .set_service_state("DiagTrack", ServiceRunState::Disabled)
        .unwrap();

    let build1 = WindowsBuildInfo::new(10, 0, 22631, 4000, "23H2", "Windows 11 Pro");
    let _ = engine
        .run_startup_self_check(&build1, &platform, &mut journal, &probes, false)
        .unwrap();

    // Now simulate Windows Update resetting settings:
    // - AllowTelemetry reset to 3 (Full)
    // - Taskbar reset to visible (true)
    // - DiagTrack service reset to running (true)
    // - Activity feed re-enabled (1)
    platform
        .set_registry_value(
            "HKLM",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            RegistryValueData::Dword(3),
        )
        .unwrap();
    platform.set_taskbar_visible(true).unwrap();
    platform
        .set_service_state("DiagTrack", ServiceRunState::Running)
        .unwrap();
    platform
        .set_registry_value(
            "HKLM",
            r"SOFTWARE\Policies\Microsoft\Windows\System",
            "EnableActivityFeed",
            RegistryValueData::Dword(1),
        )
        .unwrap();

    // Windows update bump: 4000 -> 4387
    let build2 = WindowsBuildInfo::new(10, 0, 22631, 4387, "23H2", "Windows 11 Pro");
    let report = engine
        .run_startup_self_check(&build2, &platform, &mut journal, &probes, false)
        .unwrap();

    // Verify detection and healing
    assert!(report.settings_healed.len() >= 4);
    assert!(report
        .settings_healed
        .contains(&"Disable Telemetry Policy".to_string()));
    assert!(report
        .settings_healed
        .contains(&"Native Taskbar Hidden".to_string()));
    assert!(report
        .settings_healed
        .contains(&"Disable DiagTrack Service".to_string()));
    assert!(report
        .settings_healed
        .contains(&"Disable Activity Feed".to_string()));

    // Verify live state on platform is healed back to Nexus desired values
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
    assert!(!platform.is_taskbar_visible());
    assert_eq!(
        platform.get_service_state("DiagTrack").unwrap(),
        Some(ServiceRunState::Disabled)
    );
    assert_eq!(
        platform
            .get_registry_value(
                "HKLM",
                r"SOFTWARE\Policies\Microsoft\Windows\System",
                "EnableActivityFeed"
            )
            .unwrap(),
        Some(RegistryValueData::Dword(0))
    );

    // Verify journal consistency: mutations recorded, non-empty, and can be read back
    assert!(!journal.is_empty());
    assert!(journal.entries().len() >= 4);

    // Non-blocking notices were posted
    assert!(notices
        .all()
        .iter()
        .any(|n| n.title == "Setting Re-applied"));
}

/// A mock platform that fails on specific setting mutations to test rollback.
struct FailingPlatform {
    inner: FakePlatform,
    fail_on_key: String,
}

impl FailingPlatform {
    fn new(fail_on_key: &str) -> Self {
        Self {
            inner: FakePlatform::new(),
            fail_on_key: fail_on_key.to_string(),
        }
    }
}

impl Platform for FailingPlatform {
    fn move_resize_window(
        &self,
        id: nexus_model::layout::WindowId,
        rect: nexus_model::layout::Rect,
    ) -> Result<(), PlatformError> {
        self.inner.move_resize_window(id, rect)
    }
    fn show_window(&self, id: nexus_model::layout::WindowId) -> Result<(), PlatformError> {
        self.inner.show_window(id)
    }
    fn hide_window(&self, id: nexus_model::layout::WindowId) -> Result<(), PlatformError> {
        self.inner.hide_window(id)
    }
    fn set_window_cloaked(
        &self,
        id: nexus_model::layout::WindowId,
        cloaked: bool,
    ) -> Result<(), PlatformError> {
        self.inner.set_window_cloaked(id, cloaked)
    }
    fn set_foreground_window(
        &self,
        id: nexus_model::layout::WindowId,
    ) -> Result<(), PlatformError> {
        self.inner.set_foreground_window(id)
    }
    fn close_window(&self, id: nexus_model::layout::WindowId) -> Result<(), PlatformError> {
        self.inner.close_window(id)
    }
    fn set_dwm_transitions(
        &self,
        id: nexus_model::layout::WindowId,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        self.inner.set_dwm_transitions(id, enabled)
    }
    fn get_window_rect(
        &self,
        id: nexus_model::layout::WindowId,
    ) -> Result<nexus_model::layout::Rect, PlatformError> {
        self.inner.get_window_rect(id)
    }
    fn is_window_visible(&self, id: nexus_model::layout::WindowId) -> bool {
        self.inner.is_window_visible(id)
    }
    fn is_window_cloaked(&self, id: nexus_model::layout::WindowId) -> bool {
        self.inner.is_window_cloaked(id)
    }
    fn set_process_priority(
        &self,
        pid: u32,
        priority: nexus_model::platform::ProcessPriority,
    ) -> Result<(), PlatformError> {
        self.inner.set_process_priority(pid, priority)
    }
    fn set_job_cpu_rate_cap(&self, pid: u32, rate_percent: u32) -> Result<(), PlatformError> {
        self.inner.set_job_cpu_rate_cap(pid, rate_percent)
    }
    fn remove_job_cpu_rate_cap(&self, pid: u32) -> Result<(), PlatformError> {
        self.inner.remove_job_cpu_rate_cap(pid)
    }
    fn suspend_process(&self, pid: u32) -> Result<(), PlatformError> {
        self.inner.suspend_process(pid)
    }
    fn resume_process(&self, pid: u32) -> Result<(), PlatformError> {
        self.inner.resume_process(pid)
    }
    fn empty_process_working_set(&self, pid: u32) -> Result<(), PlatformError> {
        self.inner.empty_process_working_set(pid)
    }
    fn set_taskbar_visible(&self, visible: bool) -> Result<(), PlatformError> {
        if self.fail_on_key == "Taskbar" {
            return Err(PlatformError::OperationFailed(
                "Access denied to Shell_TrayWnd".into(),
            ));
        }
        self.inner.set_taskbar_visible(visible)
    }
    fn is_taskbar_visible(&self) -> bool {
        self.inner.is_taskbar_visible()
    }
    fn register_appbar(
        &self,
        id: nexus_model::layout::WindowId,
        edge: nexus_model::platform::AppBarEdge,
        size: u32,
    ) -> Result<(), PlatformError> {
        self.inner.register_appbar(id, edge, size)
    }
    fn unregister_appbar(&self, id: nexus_model::layout::WindowId) -> Result<(), PlatformError> {
        self.inner.unregister_appbar(id)
    }
    fn set_timer_resolution_ns(&self, resolution_100ns: u32) -> Result<(), PlatformError> {
        self.inner.set_timer_resolution_ns(resolution_100ns)
    }
    fn register_hotkey(
        &self,
        id: u32,
        modifiers: nexus_model::hotkeys::Modifiers,
        key: nexus_model::hotkeys::Key,
    ) -> Result<(), PlatformError> {
        self.inner.register_hotkey(id, modifiers, key)
    }
    fn unregister_hotkey(&self, id: u32) -> Result<(), PlatformError> {
        self.inner.unregister_hotkey(id)
    }
    fn set_always_on_top(
        &self,
        id: nexus_model::layout::WindowId,
        topmost: bool,
    ) -> Result<(), PlatformError> {
        self.inner.set_always_on_top(id, topmost)
    }
    fn is_always_on_top(&self, id: nexus_model::layout::WindowId) -> bool {
        self.inner.is_always_on_top(id)
    }
    fn set_process_affinity(&self, pid: u32, affinity_mask: u64) -> Result<(), PlatformError> {
        self.inner.set_process_affinity(pid, affinity_mask)
    }
    fn get_process_affinity(&self, pid: u32) -> Result<u64, PlatformError> {
        self.inner.get_process_affinity(pid)
    }
    fn set_power_profile(&self, profile: PowerProfile) -> Result<(), PlatformError> {
        self.inner.set_power_profile(profile)
    }
    fn get_power_profile(&self) -> Result<PowerProfile, PlatformError> {
        self.inner.get_power_profile()
    }
    fn set_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
        value: RegistryValueData,
    ) -> Result<(), PlatformError> {
        let key = format!(r"{hive}\{subkey}\{value_name}");
        if key.contains(&self.fail_on_key) {
            return Err(PlatformError::PermissionDenied(format!(
                "Access Denied to {key}"
            )));
        }
        self.inner
            .set_registry_value(hive, subkey, value_name, value)
    }
    fn get_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValueData>, PlatformError> {
        self.inner.get_registry_value(hive, subkey, value_name)
    }
    fn delete_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), PlatformError> {
        self.inner.delete_registry_value(hive, subkey, value_name)
    }
    fn set_service_state(
        &self,
        service_name: &str,
        state: ServiceRunState,
    ) -> Result<(), PlatformError> {
        self.inner.set_service_state(service_name, state)
    }
    fn get_service_state(
        &self,
        service_name: &str,
    ) -> Result<Option<ServiceRunState>, PlatformError> {
        self.inner.get_service_state(service_name)
    }
    fn set_privacy_toggle(
        &self,
        toggle: PrivacyToggle,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        self.inner.set_privacy_toggle(toggle, enabled)
    }
    fn get_privacy_toggle(&self, toggle: PrivacyToggle) -> Result<bool, PlatformError> {
        self.inner.get_privacy_toggle(toggle)
    }
    fn write_system_file(&self, path: &str, content: &[u8]) -> Result<(), PlatformError> {
        self.inner.write_system_file(path, content)
    }
    fn read_system_file(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        self.inner.read_system_file(path)
    }
    fn get_system_snapshot(&self) -> Result<nx_governor_policy::SystemSnapshot, PlatformError> {
        self.inner.get_system_snapshot()
    }
}

#[test]
fn test_reapply_failure_rolls_back_journal_never_brick() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = BuildHistoryStore::new(temp_dir.path());
    let notices = Arc::new(NoticeQueue::new());
    let engine = SelfHealingEngine::new(store, notices.clone());

    let journal_path = temp_dir.path().join("failing_journal.nxjr");
    let mut journal = Journal::open(&journal_path).unwrap();

    // Inject failure on AllowTelemetry setting mutation
    let platform = FailingPlatform::new("AllowTelemetry");
    platform
        .inner
        .set_registry_value(
            "HKLM",
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            RegistryValueData::Dword(3),
        )
        .unwrap();

    let build = WindowsBuildInfo::default_windows_11();
    let probes = MockProbeProvider::new();

    let report = engine
        .run_startup_self_check(&build, &platform, &mut journal, &probes, true)
        .unwrap();

    // Verify the failure was handled gracefully:
    // 1. Reported as failed
    assert!(report
        .settings_failed
        .iter()
        .any(|(s, _)| s.contains("Telemetry")));

    // 2. Journal entry was rolled back immediately (not left broken in journal)
    let has_dangling_telemetry = journal.entries().iter().any(|m| match m {
        nexus_journal::Mutation::RegistryValue { value_name, .. } => value_name == "AllowTelemetry",
        _ => false,
    });
    assert!(
        !has_dangling_telemetry,
        "Dangling entry must be rolled back on failure"
    );

    // 3. Notice was posted alerting the operator non-blockingly
    assert!(notices
        .all()
        .iter()
        .any(|n| n.title.contains("Re-apply Failed & Reverted")));

    // 4. Daemon did NOT panic and continues operating
    assert!(!report.is_clean());
}

#[test]
fn test_mid_session_explorer_and_dwm_restarts() {
    let notices = Arc::new(NoticeQueue::new());
    let handler = RestartHandler::new(RestartConfig::default(), notices.clone());
    let platform = FakePlatform::new();

    // Initial state: taskbar suppressed
    platform.set_taskbar_visible(false).unwrap();
    assert!(!platform.is_taskbar_visible());

    // 1. explorer.exe crashes and restarts: taskbar becomes visible
    platform.set_taskbar_visible(true).unwrap();
    assert!(platform.is_taskbar_visible());

    // TaskbarCreated message received -> re-attach
    let restarted = handler.handle_explorer_restart(&platform).unwrap();
    assert!(restarted);

    // Taskbar is re-suppressed without user action!
    assert!(!platform.is_taskbar_visible());
    assert!(notices
        .all()
        .iter()
        .any(|n| n.title.contains("Explorer Restarted")));

    // 2. DWM restarts -> WM_DWMCOMPOSITIONCHANGED received
    let probes = MockProbeProvider::new();
    probes.set_available(Capability::DwmMica);
    probes.set_available(Capability::DwmThumbnails);

    let report = handler.handle_dwm_restart(&probes).unwrap();
    assert!(report.dwm_operational);
    assert!(report.mica_supported);
    assert!(report.thumbnails_supported);

    assert!(notices
        .all()
        .iter()
        .any(|n| n.title.contains("DWM Composition Re-attached")));
}
