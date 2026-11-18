use nx_privacy::system::{
    FakeSystem, RegistryHive, RegistryValue, ServiceStartType, ServiceStatus, SystemBackend,
};
use nx_privacy::telemetry::{
    TelemetryKillOptions, TelemetryPlanner, TELEMETRY_SCHEDULED_TASKS, TELEMETRY_SERVICES,
};

#[test]
fn test_telemetry_plan_covers_all_required_services() {
    let sys = FakeSystem::windows_default();
    let opts = TelemetryKillOptions {
        disable_services: true,
        disable_scheduled_tasks: false,
        disable_telemetry_policies: false,
        disable_advertising_id: false,
        disable_activity_history: false,
        disable_copilot: false,
        disable_recall: false,
        block_telemetry_hosts: false,
    };

    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();
    assert_eq!(plan.len(), TELEMETRY_SERVICES.len());

    let descriptions = plan.describe();
    assert!(descriptions.iter().any(|d| d.contains("DiagTrack")));
    assert!(descriptions.iter().any(|d| d.contains("dmwappushservice")));
}

#[test]
fn test_telemetry_plan_covers_all_scheduled_tasks() {
    let sys = FakeSystem::windows_default();
    let opts = TelemetryKillOptions {
        disable_services: false,
        disable_scheduled_tasks: true,
        disable_telemetry_policies: false,
        disable_advertising_id: false,
        disable_activity_history: false,
        disable_copilot: false,
        disable_recall: false,
        block_telemetry_hosts: false,
    };

    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();
    assert_eq!(plan.len(), TELEMETRY_SCHEDULED_TASKS.len());

    let descriptions = plan.describe();
    for task in TELEMETRY_SCHEDULED_TASKS {
        assert!(
            descriptions.iter().any(|d| d.contains(task)),
            "Missing task: {task}"
        );
    }
}

#[test]
fn test_telemetry_plan_covers_policies_advertising_activity() {
    let sys = FakeSystem::windows_default();
    let opts = TelemetryKillOptions {
        disable_services: false,
        disable_scheduled_tasks: false,
        disable_telemetry_policies: true,
        disable_advertising_id: true,
        disable_activity_history: true,
        disable_copilot: false,
        disable_recall: false,
        block_telemetry_hosts: false,
    };

    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();
    let desc = plan.describe();

    assert!(desc.iter().any(|d| d.contains("AllowTelemetry")));
    assert!(desc.iter().any(|d| d.contains("Advertising ID")));
    assert!(desc.iter().any(|d| d.contains("activity feed")));
}

#[test]
fn test_telemetry_plan_covers_copilot_and_recall() {
    let sys = FakeSystem::windows_default();
    let opts = TelemetryKillOptions {
        disable_services: false,
        disable_scheduled_tasks: false,
        disable_telemetry_policies: false,
        disable_advertising_id: false,
        disable_activity_history: false,
        disable_copilot: true,
        disable_recall: true,
        block_telemetry_hosts: false,
    };

    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();
    let desc = plan.describe();

    assert!(desc.iter().any(|d| d.contains("TurnOffWindowsCopilot")));
    assert!(desc.iter().any(|d| d.contains("DisableAIDataAnalysis")));
}

#[test]
fn test_telemetry_apply_disables_services_and_tasks() {
    let mut sys = FakeSystem::windows_default();
    let opts = TelemetryKillOptions::default();

    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();
    let receipt = plan.apply(&mut sys).unwrap();

    // Verify services disabled and stopped
    for service in TELEMETRY_SERVICES {
        let state = sys.get_service_state(service).unwrap().unwrap();
        assert_eq!(state.start_type, ServiceStartType::Disabled);
        assert_eq!(state.status, ServiceStatus::Stopped);
    }

    // Verify scheduled tasks disabled
    for task in TELEMETRY_SCHEDULED_TASKS {
        let state = sys.get_task_state(task).unwrap().unwrap();
        assert!(!state.enabled);
    }

    // Verify telemetry registry keys set
    let allow = sys
        .get_registry_value(
            RegistryHive::Hklm,
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
        )
        .unwrap();
    assert_eq!(allow, Some(RegistryValue::Dword(0)));

    // Verify Copilot disabled
    let copilot = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Policies\Microsoft\Windows\WindowsCopilot",
            "TurnOffWindowsCopilot",
        )
        .unwrap();
    assert_eq!(copilot, Some(RegistryValue::Dword(1)));

    // Verify Recall disabled
    let recall = sys
        .get_registry_value(
            RegistryHive::Hklm,
            r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI",
            "DisableAIDataAnalysis",
        )
        .unwrap();
    assert_eq!(recall, Some(RegistryValue::Dword(1)));

    // Verify hosts blocklist added
    let hosts = sys.read_hosts_file().unwrap();
    assert!(hosts.contains("v10.events.data.microsoft.com"));

    // Rollback and test identity!
    receipt.rollback(&mut sys).unwrap();

    // DiagTrack should be back to Automatic / Running
    let diag = sys.get_service_state("DiagTrack").unwrap().unwrap();
    assert_eq!(diag.start_type, ServiceStartType::Automatic);
    assert_eq!(diag.status, ServiceStatus::Running);

    // Tasks should be back to enabled
    for task in TELEMETRY_SCHEDULED_TASKS {
        assert!(sys.get_task_state(task).unwrap().unwrap().enabled);
    }
}

#[test]
fn test_telemetry_plan_apply_rollback_identity() {
    let mut sys = FakeSystem::windows_default();
    let initial_state = sys.clone();

    let opts = TelemetryKillOptions::default();
    let plan = TelemetryPlanner::build_plan(&opts, &sys).unwrap();

    let receipt = plan.apply(&mut sys).unwrap();
    assert!(!receipt.is_empty());
    assert_ne!(sys, initial_state);

    receipt.rollback(&mut sys).unwrap();
    assert_eq!(sys, initial_state);
}
