use nx_privacy::system::{
    FakeSystem, RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus,
    SystemBackend, TaskState,
};

#[test]
fn test_fake_system_initialization_empty() {
    let sys = FakeSystem::new();
    assert!(sys.registry.is_empty());
    assert!(sys.services.is_empty());
    assert!(sys.tasks.is_empty());
    assert!(sys.hosts_file.is_empty());
    assert!(sys.mutation_log.is_empty());
}

#[test]
fn test_fake_system_windows_default_fixture() {
    let sys = FakeSystem::windows_default();

    // DiagTrack and dmwappushservice should be present and running
    let diag = sys.get_service_state("DiagTrack").unwrap();
    assert_eq!(
        diag,
        Some(ServiceState {
            start_type: ServiceStartType::Automatic,
            status: ServiceStatus::Running,
        })
    );

    let dmwap = sys.get_service_state("dmwappushservice").unwrap();
    assert_eq!(
        dmwap,
        Some(ServiceState {
            start_type: ServiceStartType::Automatic,
            status: ServiceStatus::Running,
        })
    );

    // Geolocation service
    let lfsvc = sys.get_service_state("lfsvc").unwrap();
    assert_eq!(
        lfsvc,
        Some(ServiceState {
            start_type: ServiceStartType::Manual,
            status: ServiceStatus::Running,
        })
    );

    // Scheduled task fixture check
    let task = sys
        .get_task_state(
            r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
        )
        .unwrap();
    assert_eq!(task, Some(TaskState { enabled: true }));

    // Advertising ID
    let ad_id = sys
        .get_registry_value(
            RegistryHive::Hkcu,
            r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            "Enabled",
        )
        .unwrap();
    assert_eq!(ad_id, Some(RegistryValue::Dword(1)));

    // Hosts file has localhost
    let hosts = sys.read_hosts_file().unwrap();
    assert!(hosts.contains("localhost"));
}

#[test]
fn test_fake_system_registry_crud() {
    let mut sys = FakeSystem::new();
    let hive = RegistryHive::Hklm;
    let subkey = r"SOFTWARE\TestCorp\Settings";
    let val_name = "TelemetryLevel";

    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        None
    );

    sys.set_registry_value(hive, subkey, val_name, RegistryValue::Dword(42))
        .unwrap();
    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        Some(RegistryValue::Dword(42))
    );

    sys.delete_registry_value(hive, subkey, val_name).unwrap();
    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        None
    );
}

#[test]
fn test_fake_system_service_crud() {
    let mut sys = FakeSystem::new();
    let name = "TestService";

    assert_eq!(sys.get_service_state(name).unwrap(), None);

    let state = ServiceState {
        start_type: ServiceStartType::Disabled,
        status: ServiceStatus::Stopped,
    };
    sys.set_service_state(name, state).unwrap();
    assert_eq!(sys.get_service_state(name).unwrap(), Some(state));
}

#[test]
fn test_fake_system_task_crud() {
    let mut sys = FakeSystem::new();
    let path = r"\Microsoft\Windows\TestTask";

    assert_eq!(sys.get_task_state(path).unwrap(), None);

    let state = TaskState { enabled: false };
    sys.set_task_state(path, state).unwrap();
    assert_eq!(sys.get_task_state(path).unwrap(), Some(state));
}

#[test]
fn test_fake_system_hosts_crud() {
    let mut sys = FakeSystem::new();
    assert_eq!(sys.read_hosts_file().unwrap(), "");

    sys.write_hosts_file("127.0.0.1 test.local\n").unwrap();
    assert_eq!(sys.read_hosts_file().unwrap(), "127.0.0.1 test.local\n");
}

#[test]
fn test_fake_system_failure_injection() {
    let mut sys = FakeSystem::new();
    sys.inject_failure("SERVICE:BrokenService");

    let res = sys.get_service_state("BrokenService");
    assert!(res.is_err());

    sys.clear_failures();
    assert!(sys.get_service_state("BrokenService").is_ok());
}
