use nx_governor_policy::{
    EcoQosState, GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, PriorityClass,
    ProcessSample, SystemSnapshot,
};

#[test]
fn test_process_exit_drops_state_and_emits_release_job() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Track a process
    let mut snap = SystemSnapshot::new(20.0, 1000);
    snap.add_process(ProcessSample::new(Pid(333), "app.exe", 5.0));
    let _ = engine.evaluate_tick(&snap);

    assert!(engine.get_tracked_process(Pid(333)).is_some());

    // Process exits
    let plan = engine.on_process_exit(Pid(333), 2000);
    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan.mutations[0],
        PolicyMutation::ReleaseJob { pid: Pid(333) }
    );

    // Tracked state must be completely dropped
    assert!(engine.get_tracked_process(Pid(333)).is_none());
    assert_eq!(engine.tracked_process_count(), 0);
}

#[test]
fn test_process_exit_restores_demoted_and_boosted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Boost PID 400
    let _ = engine.on_foreground_changed(Some(Pid(400)), 1000);
    assert!(engine.get_tracked_process(Pid(400)).unwrap().is_boosted);

    // PID 400 terminates while foreground
    let plan = engine.on_process_exit(Pid(400), 2000);

    // Must emit release job and restore priority
    let released = plan
        .mutations
        .contains(&PolicyMutation::ReleaseJob { pid: Pid(400) });
    let restored = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid,
                previous: PriorityClass::AboveNormal,
                new: PriorityClass::Normal
            } if *pid == Pid(400)
        )
    });

    assert!(released);
    assert!(restored);
    assert_eq!(engine.current_foreground_pid(), None);
}

#[test]
fn test_pid_reuse_cannot_inherit_stale_caps() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // PID 555 is demoted by ProBalance
    for t in 1..=2 {
        let mut snap = SystemSnapshot::new(90.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(555), "old_hog.exe", 85.0));
        let _ = engine.evaluate_tick(&snap);
    }
    assert!(engine.get_tracked_process(Pid(555)).unwrap().is_demoted);

    // Old process terminates
    let exit_plan = engine.on_process_exit(Pid(555), 3000);
    assert!(exit_plan
        .mutations
        .contains(&PolicyMutation::ReleaseJob { pid: Pid(555) }));
    assert!(engine.get_tracked_process(Pid(555)).is_none());

    // OS immediately reuses PID 555 for a brand new process "new_app.exe"
    let mut snap_new = SystemSnapshot::new(20.0, 4000);
    snap_new.add_process(ProcessSample::new(Pid(555), "new_app.exe", 2.0));
    let _ = engine.evaluate_tick(&snap_new);

    let new_proc = engine
        .get_tracked_process(Pid(555))
        .expect("New process tracked");
    assert_eq!(new_proc.name, "new_app.exe");
    assert_eq!(new_proc.current_priority, PriorityClass::Normal);
    assert!(
        !new_proc.is_demoted,
        "Reused PID must NOT inherit demotion!"
    );
    assert!(!new_proc.is_boosted, "Reused PID must NOT inherit boost!");
    assert_eq!(
        new_proc.eco_qos,
        EcoQosState::Disabled,
        "Reused PID must NOT inherit EcoQoS!"
    );
}

#[test]
fn test_reset_all_clean_shutdown() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Boost PID 10, demote PID 20
    let _ = engine.on_foreground_changed(Some(Pid(10)), 1000);

    for t in 1..=2 {
        let mut snap = SystemSnapshot::new(90.0, t * 1000).with_foreground(Some(Pid(10)));
        snap.add_process(ProcessSample::new(Pid(10), "active.exe", 10.0).with_foreground(true));
        snap.add_process(ProcessSample::new(Pid(20), "hog.exe", 90.0));
        let _ = engine.evaluate_tick(&snap);
    }

    // Call reset_all (e.g. Nexus shutdown / fail-safe panic)
    let plan = engine.reset_all(5000);

    // Both PID 10 and PID 20 must be restored to Normal and released
    let pid10_restored = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid,
                new: PriorityClass::Normal,
                ..
            } if *pid == Pid(10)
        )
    });

    let pid20_restored = plan.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid,
                new: PriorityClass::Normal,
                ..
            } if *pid == Pid(20)
        )
    });

    assert!(pid10_restored, "PID 10 must be restored on reset_all");
    assert!(pid20_restored, "PID 20 must be restored on reset_all");
    assert_eq!(engine.current_foreground_pid(), None);
}
