use nx_privacy::backup::BackupEntry;
use nx_privacy::plan::{MutationPlan, MutationStep};
use nx_privacy::system::{
    FakeSystem, RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus,
    SystemBackend, TaskState,
};

#[test]
fn test_dry_run_performs_zero_mutations() {
    let sys = FakeSystem::windows_default();
    let initial_clone = sys.clone();

    let mut plan = MutationPlan::new();
    plan.push(MutationStep::SetRegistry {
        hive: RegistryHive::Hklm,
        subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection".into(),
        value_name: "AllowTelemetry".into(),
        target: RegistryValue::Dword(0),
        description: "Disable telemetry".into(),
    });
    plan.push(MutationStep::SetService {
        service_name: "DiagTrack".into(),
        target: ServiceState {
            start_type: ServiceStartType::Disabled,
            status: ServiceStatus::Stopped,
        },
        description: "Stop DiagTrack".into(),
    });

    let report = plan.dry_run(&sys).unwrap();
    assert_eq!(report.total_steps, 2);
    assert_eq!(report.effective_changes, 2);
    assert_eq!(report.noops, 0);

    // State MUST be completely untouched
    assert_eq!(sys, initial_clone);
}

#[test]
fn test_dry_run_identifies_noops() {
    let mut sys = FakeSystem::new();
    sys.set_registry_value(
        RegistryHive::Hklm,
        r"SOFTWARE\Test",
        "Key",
        RegistryValue::Dword(0),
    )
    .unwrap();

    let mut plan = MutationPlan::new();
    plan.push(MutationStep::SetRegistry {
        hive: RegistryHive::Hklm,
        subkey: r"SOFTWARE\Test".into(),
        value_name: "Key".into(),
        target: RegistryValue::Dword(0),
        description: "Already 0".into(),
    });

    let report = plan.dry_run(&sys).unwrap();
    assert_eq!(report.total_steps, 1);
    assert_eq!(report.effective_changes, 0);
    assert_eq!(report.noops, 1);
    assert!(report.is_clean());
}

#[test]
fn test_apply_records_none_for_missing_key_and_deletes_on_rollback() {
    let mut sys = FakeSystem::new();
    let hive = RegistryHive::Hklm;
    let subkey = r"SOFTWARE\NewKey";
    let val_name = "NewVal";

    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        None
    );

    let mut plan = MutationPlan::new();
    plan.push(MutationStep::SetRegistry {
        hive,
        subkey: subkey.into(),
        value_name: val_name.into(),
        target: RegistryValue::Dword(100),
        description: "Add new key".into(),
    });

    let receipt = plan.apply(&mut sys).unwrap();
    assert_eq!(receipt.len(), 1);
    assert_eq!(
        receipt.backups[0],
        BackupEntry::RegistryValue {
            hive,
            subkey: subkey.into(),
            value_name: val_name.into(),
            previous: None,
        }
    );

    // Key now exists with value 100
    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        Some(RegistryValue::Dword(100))
    );

    // Rollback deletes the key
    receipt.rollback(&mut sys).unwrap();
    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        None
    );
}

#[test]
fn test_apply_records_some_for_existing_key_and_restores_on_rollback() {
    let mut sys = FakeSystem::new();
    let hive = RegistryHive::Hklm;
    let subkey = r"SOFTWARE\Existing";
    let val_name = "OldVal";

    sys.set_registry_value(hive, subkey, val_name, RegistryValue::Dword(555))
        .unwrap();

    let mut plan = MutationPlan::new();
    plan.push(MutationStep::SetRegistry {
        hive,
        subkey: subkey.into(),
        value_name: val_name.into(),
        target: RegistryValue::Dword(0),
        description: "Overwrite to 0".into(),
    });

    let receipt = plan.apply(&mut sys).unwrap();
    assert_eq!(
        receipt.backups[0],
        BackupEntry::RegistryValue {
            hive,
            subkey: subkey.into(),
            value_name: val_name.into(),
            previous: Some(RegistryValue::Dword(555)),
        }
    );

    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        Some(RegistryValue::Dword(0))
    );

    receipt.rollback(&mut sys).unwrap();
    assert_eq!(
        sys.get_registry_value(hive, subkey, val_name).unwrap(),
        Some(RegistryValue::Dword(555))
    );
}

#[test]
fn test_transactional_abort_and_rollback_on_failure() {
    let mut sys = FakeSystem::new();
    sys.set_registry_value(
        RegistryHive::Hklm,
        r"SOFTWARE\Step1",
        "V",
        RegistryValue::Dword(1),
    )
    .unwrap();

    // Inject failure on step 2
    sys.inject_failure("SERVICE:ExplodingService");

    let mut plan = MutationPlan::new();
    plan.push(MutationStep::SetRegistry {
        hive: RegistryHive::Hklm,
        subkey: r"SOFTWARE\Step1".into(),
        value_name: "V".into(),
        target: RegistryValue::Dword(999),
        description: "Step 1".into(),
    });
    plan.push(MutationStep::SetService {
        service_name: "ExplodingService".into(),
        target: ServiceState {
            start_type: ServiceStartType::Disabled,
            status: ServiceStatus::Stopped,
        },
        description: "Step 2".into(),
    });

    let res = plan.apply(&mut sys);
    assert!(res.is_err());

    // Step 1 MUST have been rolled back automatically!
    assert_eq!(
        sys.get_registry_value(RegistryHive::Hklm, r"SOFTWARE\Step1", "V")
            .unwrap(),
        Some(RegistryValue::Dword(1))
    );
}

#[test]
fn test_backup_entry_descriptions() {
    let reg_backup = BackupEntry::RegistryValue {
        hive: RegistryHive::Hklm,
        subkey: r"SOFTWARE\Test".into(),
        value_name: "Foo".into(),
        previous: None,
    };
    assert!(reg_backup.description().contains("<did not exist>"));

    let svc_backup = BackupEntry::ServiceState {
        service_name: "DiagTrack".into(),
        previous: Some(ServiceState {
            start_type: ServiceStartType::Automatic,
            status: ServiceStatus::Running,
        }),
    };
    assert!(svc_backup.description().contains("DiagTrack"));

    let task_backup = BackupEntry::TaskState {
        task_path: r"\Microsoft\Task".into(),
        previous: Some(TaskState { enabled: true }),
    };
    assert!(task_backup.description().contains("enabled=true"));
}
