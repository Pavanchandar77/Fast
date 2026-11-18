#![allow(clippy::field_reassign_with_default)]

use nx_governor_policy::{
    EcoQosState, GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, PriorityClass,
    ProcessSample, SystemSnapshot,
};

#[test]
fn test_foreground_boost_one_class_only_normal_to_abovenormal() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    config.max_boost_class = PriorityClass::AboveNormal;
    let mut engine = GovernorEngine::new(config);

    let plan = engine.on_foreground_changed(Some(Pid(100)), 1000);
    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(100),
            previous: PriorityClass::Normal,
            new: PriorityClass::AboveNormal,
        }
    );

    let proc = engine.get_tracked_process(Pid(100)).expect("Tracked");
    assert_eq!(proc.current_priority, PriorityClass::AboveNormal);
    assert!(proc.is_boosted);
}

#[test]
fn test_foreground_boost_never_high_without_whitelist() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    // Even if max_boost_class were high, standard step_up from Normal is AboveNormal
    config.max_boost_class = PriorityClass::High;
    let mut engine = GovernorEngine::new(config);

    let plan = engine.on_foreground_changed(Some(Pid(101)), 1000);
    assert_eq!(plan.len(), 1);
    // MUST be AboveNormal, never High!
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(101),
            previous: PriorityClass::Normal,
            new: PriorityClass::AboveNormal,
        }
    );
}

#[test]
fn test_foreground_boost_high_when_whitelisted() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    config.whitelist_high_priority = vec!["pro_daw.exe".into(), "vr_runtime.exe".into()];
    let mut engine = GovernorEngine::new(config);

    // Provide process name in a snapshot first
    let mut snapshot = SystemSnapshot::new(20.0, 1000);
    snapshot.add_process(ProcessSample::new(Pid(102), "pro_daw.exe", 10.0));
    let _ = engine.evaluate_tick(&snapshot);

    let plan = engine.on_foreground_changed(Some(Pid(102)), 1001);
    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(102),
            previous: PriorityClass::Normal,
            new: PriorityClass::High,
        }
    );
}

#[test]
fn test_foreground_boost_restored_when_focus_leaves() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    let mut engine = GovernorEngine::new(config);

    // 1. Focus on PID 100 -> boosted to AboveNormal
    let _ = engine.on_foreground_changed(Some(Pid(100)), 1000);

    // 2. Focus moves to PID 200
    let plan = engine.on_foreground_changed(Some(Pid(200)), 2000);

    // Plan must contain restore of PID 100 and boost of PID 200
    assert_eq!(plan.len(), 2);
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(100),
            previous: PriorityClass::AboveNormal,
            new: PriorityClass::Normal,
        }
    );
    assert_eq!(
        plan.mutations[1],
        PolicyMutation::SetPriorityClass {
            pid: Pid(200),
            previous: PriorityClass::Normal,
            new: PriorityClass::AboveNormal,
        }
    );

    let proc1 = engine.get_tracked_process(Pid(100)).expect("Tracked");
    assert_eq!(proc1.current_priority, PriorityClass::Normal);
    assert!(!proc1.is_boosted);
}

#[test]
fn test_previous_foreground_is_never_demoted_to_below_normal() {
    // Audit finding: Old booster demoted previous foreground to BELOW_NORMAL!
    // Our smart governor must NEVER demote previous foreground to BelowNormal.
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    let mut engine = GovernorEngine::new(config);

    let _ = engine.on_foreground_changed(Some(Pid(55)), 1000);
    let plan = engine.on_foreground_changed(Some(Pid(56)), 2000);

    for mutation in &plan.mutations {
        if let PolicyMutation::SetPriorityClass { pid, new, .. } = mutation {
            if *pid == Pid(55) {
                assert_eq!(
                    *new,
                    PriorityClass::Normal,
                    "Previous foreground must restore to Normal, NEVER BelowNormal"
                );
            }
        }
    }
}

#[test]
fn test_focus_moves_to_desktop_restores_previous() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    let mut engine = GovernorEngine::new(config);

    let _ = engine.on_foreground_changed(Some(Pid(70)), 1000);
    let plan = engine.on_foreground_changed(None, 2000);

    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(70),
            previous: PriorityClass::AboveNormal,
            new: PriorityClass::Normal,
        }
    );
}

#[test]
fn test_critical_process_foreground_not_altered() {
    let mut config = GovernorPolicyConfig::default();
    config.boost_foreground = true;
    let mut engine = GovernorEngine::new(config);

    // svchost.exe is critical
    let mut snapshot = SystemSnapshot::new(10.0, 1000);
    snapshot.add_process(ProcessSample::new(Pid(12), "svchost.exe", 1.0));
    let _ = engine.evaluate_tick(&snapshot);

    let plan = engine.on_foreground_changed(Some(Pid(12)), 2000);
    // Should NOT boost svchost
    assert!(plan.is_empty());
}

#[test]
fn test_demoted_process_becoming_foreground_immediately_restored() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Simulate process demoted by ProBalance
    let mut snapshot1 = SystemSnapshot::new(90.0, 1000);
    snapshot1.add_process(ProcessSample::new(Pid(80), "blender.exe", 80.0));
    let _ = engine.evaluate_tick(&snapshot1);

    let mut snapshot2 = SystemSnapshot::new(90.0, 2000);
    snapshot2.add_process(ProcessSample::new(Pid(80), "blender.exe", 80.0));
    let _ = engine.evaluate_tick(&snapshot2);

    let proc = engine.get_tracked_process(Pid(80)).expect("Tracked");
    assert!(proc.is_demoted);

    // User switches to Blender as foreground
    let plan = engine.on_foreground_changed(Some(Pid(80)), 3000);

    // Blender must be restored from BelowNormal and then boosted
    let restored = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid: p,
                previous: PriorityClass::BelowNormal,
                new: PriorityClass::Normal
            } if *p == Pid(80)
        )
    });
    assert!(
        restored,
        "Demotion must immediately be restored on foreground focus"
    );
}

#[test]
fn test_ecoqos_process_becoming_foreground_immediately_disabled() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Trigger EcoQoS in background (idle ticks >= 2)
    for t in 1..=3 {
        let mut snap = SystemSnapshot::new(10.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(90), "worker.exe", 0.5));
        let _ = engine.evaluate_tick(&snap);
    }

    let proc = engine.get_tracked_process(Pid(90)).expect("Tracked");
    assert_eq!(proc.eco_qos, EcoQosState::Enabled);

    // Now bring worker.exe to foreground
    let plan = engine.on_foreground_changed(Some(Pid(90)), 4000);
    let eco_disabled = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetEcoQos {
                pid: p,
                previous: EcoQosState::Enabled,
                new: EcoQosState::Disabled
            } if *p == Pid(90)
        )
    });
    assert!(
        eco_disabled,
        "EcoQoS must immediately be disabled on foreground focus"
    );
}
