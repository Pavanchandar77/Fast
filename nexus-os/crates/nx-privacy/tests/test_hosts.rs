use nx_privacy::hosts::{HostsPlanner, BLOCK_END_MARKER, BLOCK_START_MARKER};
use nx_privacy::system::{FakeSystem, SystemBackend};

#[test]
fn test_hosts_append_block_to_standard_file() {
    let initial = "# Header\n127.0.0.1 localhost\n::1 localhost\n";
    let domains = vec!["telemetry.microsoft.com".to_string()];

    let (updated, changed) = HostsPlanner::generate_updated_content(initial, &domains);
    assert!(changed);
    assert!(updated.starts_with("# Header\n127.0.0.1 localhost\n::1 localhost\n"));
    assert!(updated.contains(BLOCK_START_MARKER));
    assert!(updated.contains("0.0.0.0 telemetry.microsoft.com"));
    assert!(updated.contains(BLOCK_END_MARKER));
}

#[test]
fn test_hosts_idempotency_same_domains() {
    let initial = "# Header\n127.0.0.1 localhost\n";
    let domains = vec![
        "telemetry.microsoft.com".to_string(),
        "v10.events.data.microsoft.com".to_string(),
    ];

    let (first_run, changed1) = HostsPlanner::generate_updated_content(initial, &domains);
    assert!(changed1);

    let (second_run, changed2) = HostsPlanner::generate_updated_content(&first_run, &domains);
    assert!(!changed2);
    assert_eq!(first_run, second_run);
}

#[test]
fn test_hosts_preserves_user_entries_before_and_after() {
    let before = "# Custom user entries BEFORE\n192.168.1.100 router.local\n\n";
    let after = "\n# Custom user entries AFTER\n10.0.0.1 intranet.corp\n";
    let mut initial = String::new();
    initial.push_str(before);
    initial.push_str(BLOCK_START_MARKER);
    initial.push_str("\n0.0.0.0 old-domain.com\n");
    initial.push_str(BLOCK_END_MARKER);
    initial.push_str(after);

    let new_domains = vec!["new-tracker.com".to_string()];
    let (updated, changed) = HostsPlanner::generate_updated_content(&initial, &new_domains);
    assert!(changed);
    assert!(updated.contains("192.168.1.100 router.local"));
    assert!(updated.contains("10.0.0.1 intranet.corp"));
    assert!(updated.contains("0.0.0.0 new-tracker.com"));
    assert!(!updated.contains("old-domain.com"));
}

#[test]
fn test_hosts_preserves_crlf_line_endings() {
    let initial = "# Windows hosts\r\n127.0.0.1 localhost\r\n::1 localhost\r\n";
    let domains = vec!["ad.domain.com".to_string()];

    let (updated, changed) = HostsPlanner::generate_updated_content(initial, &domains);
    assert!(changed);
    assert!(updated.contains("\r\n0.0.0.0 ad.domain.com\r\n"));
    // Ensure all newlines are CRLF
    for (i, line) in updated.split('\n').enumerate() {
        if i < updated.split('\n').count() - 1 {
            assert!(
                line.ends_with('\r'),
                "Line does not end with CR before LF: {line:?}"
            );
        }
    }
}

#[test]
fn test_hosts_handles_empty_file() {
    let initial = "";
    let domains = vec!["tracker.com".to_string()];

    let (updated, changed) = HostsPlanner::generate_updated_content(initial, &domains);
    assert!(changed);
    assert!(updated.contains(BLOCK_START_MARKER));
    assert!(updated.contains("0.0.0.0 tracker.com"));
    assert!(updated.contains(BLOCK_END_MARKER));
}

#[test]
fn test_hosts_clean_removal() {
    let mut initial = "# User config\n127.0.0.1 myapp.local\n\n".to_string();
    initial.push_str(BLOCK_START_MARKER);
    initial.push_str("\n0.0.0.0 tracker1.com\n0.0.0.0 tracker2.com\n");
    initial.push_str(BLOCK_END_MARKER);
    initial.push_str("\n# End comment\n");

    let (removed, changed) = HostsPlanner::generate_removed_content(&initial);
    assert!(changed);
    assert!(!removed.contains(BLOCK_START_MARKER));
    assert!(!removed.contains("tracker1.com"));
    assert!(!removed.contains(BLOCK_END_MARKER));
    assert!(removed.contains("127.0.0.1 myapp.local"));
    assert!(removed.contains("# End comment"));
}

#[test]
fn test_hosts_removal_noop_when_no_block_present() {
    let initial = "127.0.0.1 localhost\n";
    let (removed, changed) = HostsPlanner::generate_removed_content(initial);
    assert!(!changed);
    assert_eq!(removed, initial);
}

#[test]
fn test_hosts_planner_backend_apply_and_rollback_identity() {
    let mut sys = FakeSystem::windows_default();
    let initial_content = sys.read_hosts_file().unwrap();

    let domains = vec![
        "telemetry.microsoft.com".to_string(),
        "doubleclick.net".to_string(),
    ];

    let step = HostsPlanner::plan_update(&sys, &domains).unwrap().unwrap();
    let mut plan = nx_privacy::plan::MutationPlan::new();
    plan.push(step);

    let receipt = plan.apply(&mut sys).unwrap();
    assert_ne!(sys.read_hosts_file().unwrap(), initial_content);

    receipt.rollback(&mut sys).unwrap();
    assert_eq!(sys.read_hosts_file().unwrap(), initial_content);
}
