use nx_privacy::wfp::{
    WfpAction, WfpCondition, WfpDirection, WfpEngine, WfpProtocol, WfpRule,
    FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION_FLAG_PERSISTENT,
};
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn test_wfp_engine_defaults_to_dynamic_session() {
    let engine = WfpEngine::new();
    assert!(engine.is_dynamic());
    assert_eq!(engine.session.flags, FWPM_SESSION_FLAG_DYNAMIC);
    assert_eq!(engine.session.name, "Nexus-OS Shield Dynamic Session");
}

#[test]
fn test_wfp_engine_persistent_session_opt_in() {
    let engine = WfpEngine::new_persistent();
    assert!(!engine.is_dynamic());
    assert_eq!(engine.session.flags, FWPM_SESSION_FLAG_PERSISTENT);
    assert_eq!(engine.session.name, "Nexus-OS Shield Persistent Session");
}

#[test]
fn test_wfp_rule_builder_per_app_outbound_block() {
    let rule = WfpRule::builder("BlockUntrustedApp")
        .description("Block untrusted binary from internet access")
        .for_app(r"C:\Suspicious\evil.exe")
        .block_outbound()
        .weight(200)
        .build()
        .unwrap();

    assert_eq!(rule.name, "BlockUntrustedApp");
    assert_eq!(rule.action, WfpAction::Block);
    assert_eq!(rule.direction, WfpDirection::Outbound);
    assert_eq!(rule.weight, 200);
    assert!(!rule.persistent);
    assert_eq!(rule.app_path(), Some(r"C:\Suspicious\evil.exe"));
}

#[test]
fn test_wfp_rule_builder_per_app_inbound_permit() {
    let rule = WfpRule::builder("PermitNexusServer")
        .for_app(r"C:\Program Files\Nexus\server.exe")
        .permit_inbound()
        .with_local_port(8080)
        .with_protocol(WfpProtocol::Tcp)
        .build()
        .unwrap();

    assert_eq!(rule.action, WfpAction::Permit);
    assert_eq!(rule.direction, WfpDirection::Inbound);
    assert!(rule.conditions.contains(&WfpCondition::AppPath(
        r"C:\Program Files\Nexus\server.exe".into()
    )));
    assert!(rule.conditions.contains(&WfpCondition::LocalPort(8080)));
    assert!(rule
        .conditions
        .contains(&WfpCondition::Protocol(WfpProtocol::Tcp)));
}

#[test]
fn test_wfp_rule_builder_persistent_opt_in() {
    let rule = WfpRule::builder("PersistentFirewallRule")
        .for_app(r"C:\Windows\System32\cmd.exe")
        .block_outbound()
        .persistent(true)
        .build()
        .unwrap();

    assert!(rule.persistent);
}

#[test]
fn test_wfp_rule_builder_validation_empty_name() {
    let res = WfpRule::builder("").for_app(r"C:\app.exe").build();
    assert!(res.is_err());
}

#[test]
fn test_wfp_rule_builder_validation_no_conditions() {
    let res = WfpRule::builder("NoConditionRule").build();
    assert!(res.is_err());
}

#[test]
fn test_wfp_filter_specs_generation_on_structs() {
    let mut engine = WfpEngine::new();

    let rule1 = WfpRule::builder("TelemetryBlock")
        .for_app(r"C:\Windows\System32\diagtrack.dll")
        .block_outbound()
        .with_remote_port(443)
        .with_remote_ip(IpAddr::V4(Ipv4Addr::new(20, 190, 159, 0)))
        .weight(500)
        .build()
        .unwrap();

    let rule2 = WfpRule::builder("AllowLocalInbound")
        .for_app(r"C:\App\app.exe")
        .permit_inbound()
        .build()
        .unwrap();

    engine.add_rule(rule1);
    engine.add_rule(rule2);

    let specs = engine.generate_filter_specs();
    assert_eq!(specs.len(), 2);

    let outbound_spec = specs
        .iter()
        .find(|s| s.layer_name == "FWPM_LAYER_ALE_AUTH_CONNECT_V4")
        .unwrap();
    assert_eq!(outbound_spec.action_type, "FWP_ACTION_BLOCK");
    assert_eq!(outbound_spec.weight, 500);
    assert_eq!(outbound_spec.flags, FWPM_SESSION_FLAG_DYNAMIC);
    assert_eq!(outbound_spec.condition_count, 3);

    let inbound_spec = specs
        .iter()
        .find(|s| s.layer_name == "FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4")
        .unwrap();
    assert_eq!(inbound_spec.action_type, "FWP_ACTION_PERMIT");
}

#[test]
fn test_wfp_engine_add_and_remove_rules() {
    let mut engine = WfpEngine::new();
    let rule = WfpRule::builder("Rule1")
        .for_app(r"C:\test.exe")
        .build()
        .unwrap();
    let rule_id = rule.rule_id;

    engine.add_rule(rule);
    assert_eq!(engine.rule_count(), 1);
    assert!(engine.get_rule(rule_id).is_some());

    let removed = engine.remove_rule(rule_id).unwrap();
    assert_eq!(removed.name, "Rule1");
    assert_eq!(engine.rule_count(), 0);

    // Removing non-existent rule returns error
    assert!(engine.remove_rule(rule_id).is_err());
}
