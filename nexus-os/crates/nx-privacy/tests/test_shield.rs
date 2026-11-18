use nx_privacy::backup::BackupEntry;
use nx_privacy::killswitch::KillSwitchConfig;
use nx_privacy::shield::{PrivacyShield, PrivacyShieldConfig};
use nx_privacy::system::{FakeSystem, SystemBackend};
use nx_privacy::telemetry::TelemetryKillOptions;
use nx_privacy::wfp::WfpRule;

#[test]
fn test_privacy_shield_default_config() {
    let shield = PrivacyShield::default();
    assert!(shield.config().dynamic_wfp_session);
    assert!(shield.wfp_engine().is_dynamic());
    assert!(shield.config().block_trackers);
}

#[test]
fn test_privacy_shield_dry_run_zero_mutations() {
    let sys = FakeSystem::windows_default();
    let initial_state = sys.clone();

    let shield = PrivacyShield::default();
    let report = shield.dry_run(&sys).unwrap();

    assert!(report.total_steps > 10);
    assert!(report.effective_changes > 0);

    // Backend must remain 100% identical!
    assert_eq!(sys, initial_state);
}

#[test]
fn test_privacy_shield_apply_and_rollback_identity() {
    let mut sys = FakeSystem::windows_default();
    let original = sys.clone();

    let rule = WfpRule::builder("BlockTelemetryBin")
        .for_app(r"C:\Windows\System32\diagtrack.dll")
        .block_outbound()
        .build()
        .unwrap();

    let config = PrivacyShieldConfig {
        kill_switches: KillSwitchConfig {
            camera_blocked: true,
            microphone_blocked: true,
            location_blocked: true,
        },
        custom_blocked_domains: vec!["custom-tracker.internal".into()],
        wfp_rules: vec![rule],
        ..Default::default()
    };

    let shield = PrivacyShield::new(config);

    // 1. Apply
    let receipt = shield.apply(&mut sys).unwrap();
    assert!(!receipt.is_empty());
    assert_ne!(sys, original);

    // 2. Rollback
    shield.rollback(&receipt, &mut sys).unwrap();

    // 3. System MUST equal original state exactly!
    assert_eq!(sys, original);
}

#[test]
fn test_guarantee_no_change_applied_without_backup_entry() {
    let mut sys = FakeSystem::windows_default();
    let shield = PrivacyShield::default();

    let plan = shield.plan(&sys).unwrap();
    let step_count = plan.len();

    let receipt = shield.apply(&mut sys).unwrap();

    // Every planned step that performed a mutation must have recorded a backup entry
    assert_eq!(receipt.len(), step_count);

    for backup in &receipt.backups {
        match backup {
            BackupEntry::RegistryValue {
                hive,
                subkey,
                value_name,
                previous,
            } => {
                // If previous was Some, it was recorded. If None, it means key did not exist.
                // Both are valid backups that can restore the previous state!
                assert!(!subkey.is_empty());
                assert!(!value_name.is_empty());
                let _ = (hive, previous);
            }
            BackupEntry::ServiceState {
                service_name,
                previous,
            } => {
                assert!(!service_name.is_empty());
                assert!(
                    previous.is_some(),
                    "Default fixture services must have previous state"
                );
            }
            BackupEntry::TaskState {
                task_path,
                previous,
            } => {
                assert!(!task_path.is_empty());
                assert!(
                    previous.is_some(),
                    "Default fixture tasks must have previous state"
                );
            }
            BackupEntry::HostsFile { previous_content } => {
                assert!(!previous_content.is_empty());
            }
            BackupEntry::WfpRule { rule_name, .. } => {
                assert!(!rule_name.is_empty());
            }
        }
    }
}

#[test]
fn test_privacy_shield_custom_blocked_domains_included() {
    let mut sys = FakeSystem::windows_default();
    let config = PrivacyShieldConfig {
        custom_blocked_domains: vec!["analytics.badcorp.com".into()],
        ..Default::default()
    };

    let shield = PrivacyShield::new(config);
    let receipt = shield.apply(&mut sys).unwrap();

    let hosts = sys.read_hosts_file().unwrap();
    assert!(hosts.contains("0.0.0.0 analytics.badcorp.com"));

    shield.rollback(&receipt, &mut sys).unwrap();
    let restored_hosts = sys.read_hosts_file().unwrap();
    assert!(!restored_hosts.contains("analytics.badcorp.com"));
}

#[test]
fn test_privacy_shield_selective_telemetry_options() {
    let mut sys = FakeSystem::windows_default();
    let original = sys.clone();

    let config = PrivacyShieldConfig {
        telemetry: TelemetryKillOptions {
            disable_services: true,
            disable_scheduled_tasks: false,
            disable_telemetry_policies: false,
            disable_advertising_id: false,
            disable_activity_history: false,
            disable_copilot: false,
            disable_recall: false,
            block_telemetry_hosts: false,
        },
        block_trackers: false,
        ..Default::default()
    };

    let shield = PrivacyShield::new(config);
    let receipt = shield.apply(&mut sys).unwrap();

    // Only services should have changed
    let diag = sys.get_service_state("DiagTrack").unwrap().unwrap();
    assert_eq!(diag.status, nx_privacy::system::ServiceStatus::Stopped);

    // Scheduled task untouched
    let task = sys
        .get_task_state(r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator")
        .unwrap()
        .unwrap();
    assert!(task.enabled);

    // Hosts untouched
    assert_eq!(
        sys.read_hosts_file().unwrap(),
        original.read_hosts_file().unwrap()
    );

    shield.rollback(&receipt, &mut sys).unwrap();
    assert_eq!(sys, original);
}
