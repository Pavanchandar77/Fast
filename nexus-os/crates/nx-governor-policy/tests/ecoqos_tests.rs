#![allow(clippy::field_reassign_with_default)]

use nx_governor_policy::{
    EcoQosState, GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, ProcessSample,
    SystemSnapshot,
};

#[test]
fn test_ecoqos_applied_to_idle_background_app() {
    let mut config = GovernorPolicyConfig::default();
    config.eco_qos_enabled = true;
    config.eco_qos_background_idle_ticks = 2;
    let mut engine = GovernorEngine::new(config);

    // Tick 1
    let mut snap1 = SystemSnapshot::new(10.0, 1000);
    snap1.add_process(ProcessSample::new(Pid(100), "background.exe", 1.0));
    let plan1 = engine.evaluate_tick(&snap1);
    assert!(plan1.is_empty()); // Not yet idle enough

    // Tick 2: Reaches idle threshold
    let mut snap2 = SystemSnapshot::new(10.0, 2000);
    snap2.add_process(ProcessSample::new(Pid(100), "background.exe", 1.0));
    let plan2 = engine.evaluate_tick(&snap2);

    assert_eq!(plan2.len(), 1);
    assert_eq!(
        plan2.mutations[0],
        PolicyMutation::SetEcoQos {
            pid: Pid(100),
            previous: EcoQosState::Disabled,
            new: EcoQosState::Enabled,
        }
    );

    let proc = engine.get_tracked_process(Pid(100)).expect("Tracked");
    assert_eq!(proc.eco_qos, EcoQosState::Enabled);
}

#[test]
fn test_ecoqos_never_applied_to_critical_processes() {
    let mut config = GovernorPolicyConfig::default();
    config.eco_qos_enabled = true;
    config.eco_qos_background_idle_ticks = 1;
    let mut engine = GovernorEngine::new(config);

    for t in 1..=5 {
        let mut snap = SystemSnapshot::new(10.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(10), "explorer.exe", 0.1));
        snap.add_process(ProcessSample::new(Pid(20), "audiodg.exe", 0.1));
        let plan = engine.evaluate_tick(&snap);

        for m in &plan.mutations {
            if let PolicyMutation::SetEcoQos { pid, new, .. } = m {
                if *pid == Pid(10) || *pid == Pid(20) {
                    assert_ne!(
                        *new,
                        EcoQosState::Enabled,
                        "Critical processes must never be placed in EcoQoS!"
                    );
                }
            }
        }
    }
}

#[test]
fn test_ecoqos_never_applied_to_excluded_processes() {
    let mut config = GovernorPolicyConfig::default();
    config.eco_qos_enabled = true;
    config.eco_qos_background_idle_ticks = 1;
    let mut engine = GovernorEngine::new(config);

    // Spotify is excluded by default
    for t in 1..=5 {
        let mut snap = SystemSnapshot::new(10.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(300), "spotify.exe", 0.5));
        let plan = engine.evaluate_tick(&snap);

        for m in &plan.mutations {
            if let PolicyMutation::SetEcoQos { pid, new, .. } = m {
                if *pid == Pid(300) {
                    assert_ne!(
                        *new,
                        EcoQosState::Enabled,
                        "Excluded music/stream processes must never be placed in EcoQoS!"
                    );
                }
            }
        }
    }
}

#[test]
fn test_ecoqos_disabled_when_config_switched_off() {
    let mut config = GovernorPolicyConfig::default();
    config.eco_qos_enabled = false;
    let mut engine = GovernorEngine::new(config);

    for t in 1..=5 {
        let mut snap = SystemSnapshot::new(10.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(400), "regular.exe", 0.5));
        let plan = engine.evaluate_tick(&snap);
        assert!(plan.is_empty());
    }
}
