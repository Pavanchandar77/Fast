use nx_privacy::killswitch::{KillSwitchConfig, KillSwitchPlanner};
use nx_privacy::system::{
    FakeSystem, RegistryHive, RegistryValue, ServiceStartType, ServiceStatus, SystemBackend,
};

#[test]
fn test_camera_kill_switch_blocking() {
    let mut sys = FakeSystem::windows_default();
    let config = KillSwitchConfig {
        camera_blocked: true,
        microphone_blocked: false,
        location_blocked: false,
    };

    let plan = KillSwitchPlanner::build_plan(&config, &sys).unwrap();
    let receipt = plan.apply(&mut sys).unwrap();

    let policy = sys
        .get_registry_value(
            RegistryHive::Hklm,
            r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy",
            "LetAppsAccessCamera",
        )
        .unwrap();
    assert_eq!(policy, Some(RegistryValue::Dword(2)));

    let consent = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam",
            "Value",
        )
        .unwrap();
    assert_eq!(consent, Some(RegistryValue::Sz("Deny".into())));

    // Rollback restores Allow
    receipt.rollback(&mut sys).unwrap();
    let consent_restored = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam",
            "Value",
        )
        .unwrap();
    assert_eq!(consent_restored, Some(RegistryValue::Sz("Allow".into())));
}

#[test]
fn test_microphone_kill_switch_blocking() {
    let mut sys = FakeSystem::windows_default();
    let config = KillSwitchConfig {
        camera_blocked: false,
        microphone_blocked: true,
        location_blocked: false,
    };

    let plan = KillSwitchPlanner::build_plan(&config, &sys).unwrap();
    let receipt = plan.apply(&mut sys).unwrap();

    let policy = sys
        .get_registry_value(
            RegistryHive::Hklm,
            r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy",
            "LetAppsAccessMicrophone",
        )
        .unwrap();
    assert_eq!(policy, Some(RegistryValue::Dword(2)));

    let consent = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone",
            "Value",
        )
        .unwrap();
    assert_eq!(consent, Some(RegistryValue::Sz("Deny".into())));

    receipt.rollback(&mut sys).unwrap();
    let consent_restored = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone",
            "Value",
        )
        .unwrap();
    assert_eq!(consent_restored, Some(RegistryValue::Sz("Allow".into())));
}

#[test]
fn test_location_kill_switch_blocking_and_service() {
    let mut sys = FakeSystem::windows_default();
    let config = KillSwitchConfig {
        camera_blocked: false,
        microphone_blocked: false,
        location_blocked: true,
    };

    let plan = KillSwitchPlanner::build_plan(&config, &sys).unwrap();
    let receipt = plan.apply(&mut sys).unwrap();

    let policy = sys
        .get_registry_value(
            RegistryHive::Hklm,
            r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy",
            "LetAppsAccessLocation",
        )
        .unwrap();
    assert_eq!(policy, Some(RegistryValue::Dword(2)));

    let consent = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location",
            "Value",
        )
        .unwrap();
    assert_eq!(consent, Some(RegistryValue::Sz("Deny".into())));

    // Geolocation service disabled and stopped
    let svc = sys.get_service_state("lfsvc").unwrap().unwrap();
    assert_eq!(svc.start_type, ServiceStartType::Disabled);
    assert_eq!(svc.status, ServiceStatus::Stopped);

    receipt.rollback(&mut sys).unwrap();

    // Restores service to Manual / Running
    let svc_restored = sys.get_service_state("lfsvc").unwrap().unwrap();
    assert_eq!(svc_restored.start_type, ServiceStartType::Manual);
    assert_eq!(svc_restored.status, ServiceStatus::Running);
}

#[test]
fn test_kill_switch_all_apply_and_rollback_identity() {
    let mut sys = FakeSystem::windows_default();
    let original = sys.clone();

    let config = KillSwitchConfig {
        camera_blocked: true,
        microphone_blocked: true,
        location_blocked: true,
    };

    let plan = KillSwitchPlanner::build_plan(&config, &sys).unwrap();
    let receipt = plan.apply(&mut sys).unwrap();
    assert_ne!(sys, original);

    receipt.rollback(&mut sys).unwrap();
    assert_eq!(sys, original);
}
