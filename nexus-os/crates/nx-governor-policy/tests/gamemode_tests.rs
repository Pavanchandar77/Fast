#![allow(clippy::field_reassign_with_default)]

use nx_governor_policy::{
    EcoQosState, GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, PriorityClass,
    ProcessSample, SystemSnapshot,
};

#[test]
fn test_game_mode_auto_detect_from_hint() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    let snap = SystemSnapshot::new(30.0, 1000).with_game_active(true);
    let plan = engine.evaluate_tick(&snap);

    assert!(engine.is_game_mode_active());
    let _ = plan;
}

#[test]
fn test_game_mode_auto_detect_from_fullscreen_game() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    let mut snap = SystemSnapshot::new(40.0, 1000).with_fullscreen(Some(Pid(999)));
    let sample = ProcessSample::new(Pid(999), "eldenring.exe", 40.0).with_fullscreen(true);
    snap.add_process(sample);

    let _ = engine.evaluate_tick(&snap);
    assert!(engine.is_game_mode_active());
}

#[test]
fn test_game_mode_pauses_demotions_and_ecoqos() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Enter game mode
    let mut snap = SystemSnapshot::new(95.0, 1000).with_game_active(true);
    // Add background helper process running heavy CPU
    snap.add_process(ProcessSample::new(Pid(111), "obs64.exe", 50.0));
    snap.add_process(ProcessSample::new(Pid(222), "discord.exe", 20.0));
    snap.add_process(ProcessSample::new(Pid(333), "helper_service.exe", 90.0));

    for _ in 0..5 {
        let plan = engine.evaluate_tick(&snap);
        // Zero demotions and zero EcoQoS should be emitted
        for m in &plan.mutations {
            match m {
                PolicyMutation::SetPriorityClass { new, .. } => {
                    assert!(
                        *new >= PriorityClass::Normal,
                        "Paused governor must not demote during Game Mode"
                    );
                }
                PolicyMutation::SetEcoQos { new, .. } => {
                    assert_eq!(
                        *new,
                        EcoQosState::Disabled,
                        "Paused governor must not enable EcoQoS during Game Mode"
                    );
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_entering_game_mode_immediately_restores_demoted_processes() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // 1. Demote background hog
    for t in 1..=2 {
        let mut snap = SystemSnapshot::new(85.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(888), "converter.exe", 80.0));
        let _ = engine.evaluate_tick(&snap);
    }
    assert!(engine.get_tracked_process(Pid(888)).unwrap().is_demoted);

    // 2. Launch game -> Game Mode activates
    let mut game_snap = SystemSnapshot::new(85.0, 3000).with_game_active(true);
    game_snap.add_process(ProcessSample::new(Pid(888), "converter.exe", 80.0));
    game_snap.add_process(ProcessSample::new(Pid(777), "cs2.exe", 60.0));

    let plan = engine.evaluate_tick(&game_snap);

    // Must restore converter.exe so game doesn't stutter from priority inversions
    let restored = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid: p,
                previous: PriorityClass::BelowNormal,
                new: PriorityClass::Normal
            } if *p == Pid(888)
        )
    });
    assert!(
        restored,
        "Entering Game Mode must immediately restore demoted processes"
    );

    let proc = engine.get_tracked_process(Pid(888)).expect("Tracked");
    assert!(!proc.is_demoted);
    assert_eq!(proc.current_priority, PriorityClass::Normal);
}

#[test]
fn test_game_mode_exit_resumes_policy() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // 1. Active game
    let game_snap = SystemSnapshot::new(50.0, 1000).with_game_active(true);
    let _ = engine.evaluate_tick(&game_snap);
    assert!(engine.is_game_mode_active());

    // 2. Game closed
    let desktop_snap = SystemSnapshot::new(30.0, 2000).with_game_active(false);
    let _ = engine.evaluate_tick(&desktop_snap);
    assert!(!engine.is_game_mode_active());
}

#[test]
fn test_game_mode_explicit_override() {
    let mut config = GovernorPolicyConfig::default();
    config.game_mode_override = Some(false); // Force off
    let mut engine = GovernorEngine::new(config);

    let snap = SystemSnapshot::new(50.0, 1000).with_game_active(true);
    let _ = engine.evaluate_tick(&snap);
    assert!(
        !engine.is_game_mode_active(),
        "Override false must prevent game mode"
    );

    let mut config_on = GovernorPolicyConfig::default();
    config_on.game_mode_override = Some(true); // Force on
    let mut engine_on = GovernorEngine::new(config_on);
    let snap_idle = SystemSnapshot::new(10.0, 1000);
    let _ = engine_on.evaluate_tick(&snap_idle);
    assert!(
        engine_on.is_game_mode_active(),
        "Override true must activate game mode"
    );
}
