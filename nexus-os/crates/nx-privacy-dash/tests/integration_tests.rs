//! Integration and snapshot golden tests for nx-privacy-dash.

use nx_privacy_dash::{
    AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus, ConsentStoreParser,
    ConsentStoreSnapshot, DashboardTab, DashboardViewModel, FileTime, KillSwitchKind,
    NetworkCategory, NetworkPerAppViewModel, NetworkSample, PermissionCapability, PermissionState,
    Rect, SensitiveKind, TimeWindow, TrafficDisposition, WidgetKind,
};
use proptest::prelude::*;

const FULL_REGISTRY_FIXTURE: &str = r#"
Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam]
"Value"="Allow"

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\Microsoft.WindowsCamera_8wekyb3d8bbwe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,40,a2,88,85,9a,bc,01
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\NonPackaged\C:#Program Files#Zoom#bin#Zoom.exe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,50,4b,a2,82,9a,bc,01
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\C:#Program Files#Discord#app-1.0.9000#Discord.exe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,00,00,10,20,30,40,01
"LastUsedTimeStop"=hex(b):00,00,00,50,60,70,80,01

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location\NonPackaged\C:#Program Files#Google#Chrome#Application#chrome.exe]
"Value"="Deny"
"LastUsedTimeStart"=hex(b):00,00,00,00,00,00,00,00
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00
"#;

#[test]
fn test_end_to_end_dashboard_scenario() {
    let mut vm = DashboardViewModel::new();
    let now_ms = 1_700_000_000_000u64;

    // 1. Ingest registry snapshot
    let snapshot = ConsentStoreParser::parse_reg_file(FULL_REGISTRY_FIXTURE).unwrap();
    vm.update_from_consent_store(&snapshot, now_ms);

    // Camera should be in use (Zoom + WindowsCamera)
    assert!(vm.indicators.camera.state.kind().is_active());
    assert_eq!(vm.indicators.camera.chip(now_ms).badge_count, 2);

    // Location should have been denied for Chrome
    let chrome_app = AppIdentifier::from_desktop_path(
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    );
    assert!(!vm
        .permissions
        .profiles
        .get(chrome_app.identifier())
        .unwrap()
        .is_allowed(PermissionCapability::Location));

    // 2. Engage Kill-Switch for Camera
    let ks_req = vm.kill_switches.create_request(
        KillSwitchKind::CameraMaster,
        true,
        "User Privacy Panic",
        now_ms,
    );
    let dry_run = ks_req.dry_run();
    assert!(dry_run.is_reversible);
    assert!(dry_run.target_description.contains("Engage"));
    vm.kill_switches.apply_request(&ks_req);
    assert!(vm.kill_switches.is_engaged(KillSwitchKind::CameraMaster));

    // 3. Feed Network Samples
    vm.network.ingest_samples(vec![
        NetworkSample {
            app: chrome_app.clone(),
            timestamp_ms: now_ms - 1000,
            remote_addr: "142.250.190.46:443".into(),
            domain: Some("google.com".into()),
            bytes_sent: 500,
            bytes_received: 2500,
            disposition: TrafficDisposition::Allowed,
            category: NetworkCategory::Content,
        },
        NetworkSample {
            app: AppIdentifier::from_desktop_path("C:\\Windows\\System32\\diagtrack.dll"),
            timestamp_ms: now_ms - 500,
            remote_addr: "20.190.159.0:443".into(),
            domain: Some("telemetry.microsoft.com".into()),
            bytes_sent: 800,
            bytes_received: 0,
            disposition: TrafficDisposition::BlockedTelemetry,
            category: NetworkCategory::Telemetry,
        },
    ]);

    // Verify network summary
    let net_sum = vm.network.summaries(TimeWindow::LastHour, now_ms);
    assert_eq!(net_sum.len(), 2);
    assert_eq!(
        vm.network
            .global_telemetry_blocked_bytes(TimeWindow::LastHour, now_ms),
        800
    );

    // 4. Test Clipboard Sensitive Copy & Auto-Clear
    let copy_res = vm.clipboard.record_copy(
        "Secret token: sk-proj-1234567890abcdefghijklmnopqrstuvwxyz",
        Some(chrome_app.clone()),
        now_ms,
    );
    assert!(copy_res.is_some());
    assert_eq!(copy_res.unwrap().kind, SensitiveKind::ApiToken);
    assert!(vm.clipboard.is_sensitive_active());

    // Reading by untrusted app emits alert
    let untrusted_app = AppIdentifier::from_desktop_path("C:\\Malware\\spy.exe");
    let alert = vm.clipboard.record_read(untrusted_app, now_ms + 100);
    assert!(alert.is_some());

    // Tick past 60s -> auto clear
    vm.tick(now_ms + 61_000);
    assert!(!vm.clipboard.is_sensitive_active());

    // 5. Verify layout tree generation across all tabs
    let bounds = Rect::new(0.0, 0.0, 1024.0, 768.0);
    for tab in DashboardTab::all() {
        vm.set_tab(tab);
        let tree = vm.compute_layout(bounds, now_ms + 61_000);
        assert_eq!(tree.kind, WidgetKind::Window);
        assert!(tree.rect.approx_eq(&bounds, 0.01));
        assert!(!tree.children.is_empty());
    }
}

#[test]
fn test_golden_snapshot_layout_tree() {
    let mut vm = DashboardViewModel::new();
    let now_ms = 1_700_000_000_000u64;

    let snapshot = ConsentStoreSnapshot::new(vec![ConsentRecord {
        app_id: AppIdentifier::from_desktop_path("C:\\Program Files\\Zoom\\bin\\Zoom.exe"),
        capability: CapabilityKind::Webcam,
        status: ConsentStatus::Allow,
        last_used_start: Some(FileTime::from_unix_millis(now_ms)),
        last_used_stop: None,
        attribution: None,
    }]);
    vm.update_from_consent_store(&snapshot, now_ms);

    let bounds = Rect::new(0.0, 0.0, 1024.0, 768.0);
    let layout = vm.compute_layout(bounds, now_ms);
    let formatted = layout.formatted_tree(0);

    // Snapshot structure checks
    assert!(formatted.contains(
        "- Window #dashboard_window: [0.0, 0.0, 1024.0x768.0] \"Nexus Privacy Dashboard\""
    ));
    assert!(
        formatted.contains("- Header #header: [16.0, 16.0, 992.0x56.0] \"Privacy & Security Hub\"")
    );
    assert!(formatted.contains("- IndicatorChip #chip_Webcam"));
    assert!(formatted.contains("- TabBar #tab_bar: [16.0, 80.0, 992.0x40.0]"));
    assert!(formatted.contains("- TabButton #tab_button_Overview"));
    assert!(formatted.contains("- Section #overview_pane: [16.0, 132.0, 992.0x620.0]"));
    assert!(formatted.contains("- Card #card_hardware"));
    assert!(formatted.contains("- Card #card_kill_switches"));
    assert!(formatted.contains("- Card #card_telemetry"));
    assert!(formatted.contains("- Card #card_clipboard"));
    assert!(formatted.contains("- Section #section_kill_switches"));
}

#[test]
fn test_responsive_layout_proportions() {
    let vm = DashboardViewModel::new();
    let now_ms = 1_000_000;

    let resolutions = [
        Rect::new(0.0, 0.0, 800.0, 600.0),
        Rect::new(0.0, 0.0, 1024.0, 768.0),
        Rect::new(0.0, 0.0, 1440.0, 900.0),
        Rect::new(0.0, 0.0, 1920.0, 1080.0),
    ];

    for r in resolutions {
        let tree = vm.compute_layout(r, now_ms);
        assert!(tree.rect.approx_eq(&r, 0.001));

        let header = tree.find_node("header").unwrap();
        assert!(header.rect.width <= r.width);
        assert!(header.rect.bottom() <= r.height);

        let overview = tree.find_node("overview_pane").unwrap();
        assert!(overview.rect.bottom() <= r.height);
    }
}

// Property-based testing
proptest! {
    #[test]
    fn proptest_filetime_unix_roundtrip(ms in 0u64..3_000_000_000_000u64) {
        let ft = FileTime::from_unix_millis(ms);
        let converted = ft.to_unix_millis();
        prop_assert_eq!(ms, converted);
    }

    #[test]
    fn proptest_rect_invariants(
        x in 0.0f32..1000.0f32,
        y in 0.0f32..1000.0f32,
        w in 1.0f32..2000.0f32,
        h in 1.0f32..2000.0f32
    ) {
        let r = Rect::new(x, y, w, h);
        prop_assert_eq!(r.right(), x + w);
        prop_assert_eq!(r.bottom(), y + h);
        prop_assert!(r.contains_point(x + (w / 2.0), y + (h / 2.0)));
    }

    #[test]
    fn proptest_network_sample_aggregation(
        sent in 0u64..1_000_000u64,
        recv in 0u64..1_000_000u64,
        is_blocked in any::<bool>()
    ) {
        let mut vm = NetworkPerAppViewModel::new();
        let app = AppIdentifier::from_desktop_path("C:\\App\\test.exe");
        let disp = if is_blocked {
            TrafficDisposition::BlockedTelemetry
        } else {
            TrafficDisposition::Allowed
        };

        vm.ingest_sample(NetworkSample {
            app: app.clone(),
            timestamp_ms: 1000,
            remote_addr: "1.1.1.1:80".into(),
            domain: None,
            bytes_sent: sent,
            bytes_received: recv,
            disposition: disp,
            category: NetworkCategory::Content,
        });

        let summaries = vm.summaries(TimeWindow::AllTime, 2000);
        prop_assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        prop_assert_eq!(s.total_bytes, sent + recv);
        if is_blocked {
            prop_assert_eq!(s.blocked_bytes, sent + recv);
            prop_assert_eq!(s.allowed_bytes, 0);
            prop_assert_eq!(s.telemetry_blocked_pct, 100.0);
        } else {
            prop_assert_eq!(s.allowed_bytes, sent + recv);
            prop_assert_eq!(s.blocked_bytes, 0);
            prop_assert_eq!(s.telemetry_blocked_pct, 0.0);
        }
    }

    #[test]
    fn proptest_permission_toggle_reversibility(
        app_name in "[a-zA-Z0-9_]{3,15}",
        initial_state in prop_oneof![Just(PermissionState::Allowed), Just(PermissionState::Denied), Just(PermissionState::Prompt)],
        desired_state in prop_oneof![Just(PermissionState::Allowed), Just(PermissionState::Denied)]
    ) {
        let path = format!("C:\\App\\{}.exe", app_name);
        let app = AppIdentifier::from_desktop_path(&path);
        let cmd = nx_privacy_dash::PermissionToggleCommand {
            command_id: 1,
            app,
            capability: PermissionCapability::Camera,
            previous_state: initial_state,
            desired_state,
            reason: "Test".into(),
            timestamp_ms: 1000,
        };

        let rev1 = cmd.reverse();
        let rev2 = rev1.reverse();

        prop_assert_eq!(cmd.app, rev2.app);
        prop_assert_eq!(cmd.capability, rev2.capability);
        prop_assert_eq!(cmd.previous_state, rev2.previous_state);
        prop_assert_eq!(cmd.desired_state, rev2.desired_state);
    }
}
