use nx_governor_policy::{
    GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, PriorityClass, ProcessSample,
    SystemSnapshot,
};

#[test]
fn test_probalance_sustained_hog_demoted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Tick 1: High CPU for hog (75%), high system CPU (80%)
    let mut snap1 = SystemSnapshot::new(80.0, 1000);
    snap1.add_process(ProcessSample::new(Pid(500), "ffmpeg.exe", 75.0));
    let plan1 = engine.evaluate_tick(&snap1);
    // 1st tick is not yet sustained (sustained_ticks = 2)
    assert!(plan1.is_empty());

    // Tick 2: Second consecutive high CPU tick
    let mut snap2 = SystemSnapshot::new(82.0, 2000);
    snap2.add_process(ProcessSample::new(Pid(500), "ffmpeg.exe", 76.0));
    let plan2 = engine.evaluate_tick(&snap2);

    assert_eq!(plan2.len(), 1);
    assert_eq!(
        plan2.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(500),
            previous: PriorityClass::Normal,
            new: PriorityClass::BelowNormal,
        }
    );

    let proc = engine.get_tracked_process(Pid(500)).expect("Tracked");
    assert!(proc.is_demoted);
    assert_eq!(proc.current_priority, PriorityClass::BelowNormal);
}

#[test]
fn test_probalance_transient_spike_no_demotion() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Tick 1: Spike (80%)
    let mut snap1 = SystemSnapshot::new(80.0, 1000);
    snap1.add_process(ProcessSample::new(Pid(501), "compiler.exe", 80.0));
    let plan1 = engine.evaluate_tick(&snap1);
    assert!(plan1.is_empty());

    // Tick 2: Spike drops back to 5%
    let mut snap2 = SystemSnapshot::new(20.0, 2000);
    snap2.add_process(ProcessSample::new(Pid(501), "compiler.exe", 5.0));
    let plan2 = engine.evaluate_tick(&snap2);
    assert!(plan2.is_empty());

    let proc = engine.get_tracked_process(Pid(501)).expect("Tracked");
    assert!(!proc.is_demoted);
    assert_eq!(proc.current_priority, PriorityClass::Normal);
}

#[test]
fn test_probalance_automatic_restore_after_spike_ends() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Ticks 1 & 2: Demote ffmpeg
    for t in 1..=2 {
        let mut snap = SystemSnapshot::new(85.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(502), "ffmpeg.exe", 75.0));
        let _ = engine.evaluate_tick(&snap);
    }
    assert!(engine.get_tracked_process(Pid(502)).unwrap().is_demoted);

    // Tick 3: CPU normal (5%), recovery tick 1
    let mut snap3 = SystemSnapshot::new(30.0, 3000);
    snap3.add_process(ProcessSample::new(Pid(502), "ffmpeg.exe", 5.0));
    let plan3 = engine.evaluate_tick(&snap3);
    assert!(plan3.is_empty()); // Needs 2 restore ticks

    // Tick 4: CPU normal (5%), recovery tick 2 -> AUTOMATIC RESTORE!
    let mut snap4 = SystemSnapshot::new(30.0, 4000);
    snap4.add_process(ProcessSample::new(Pid(502), "ffmpeg.exe", 5.0));
    let plan4 = engine.evaluate_tick(&snap4);

    let restored = plan4.mutations.iter().any(|m| {
        matches!(
            m,
            PolicyMutation::SetPriorityClass {
                pid: p,
                previous: PriorityClass::BelowNormal,
                new: PriorityClass::Normal,
            } if *p == Pid(502)
        )
    });
    assert!(restored, "Must restore priority class to Normal");

    let proc = engine.get_tracked_process(Pid(502)).expect("Tracked");
    assert!(!proc.is_demoted);
    assert_eq!(proc.current_priority, PriorityClass::Normal);
}

#[test]
fn test_no_critical_process_is_ever_demoted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // dwm.exe at 100% CPU on 100% system CPU for 10 ticks
    for t in 1..=10 {
        let mut snap = SystemSnapshot::new(100.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(99), "dwm.exe", 100.0));
        let plan = engine.evaluate_tick(&snap);

        // MUST NEVER demote dwm.exe
        for m in &plan.mutations {
            if let PolicyMutation::SetPriorityClass { pid, new, .. } = m {
                if *pid == Pid(99) {
                    panic!("CRITICAL PROCESS DEMOTED: {:?}", new);
                }
            }
        }
    }
}

#[test]
fn test_no_foreground_process_is_ever_demoted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Active foreground game/app consuming 100% CPU
    for t in 1..=10 {
        let mut snap = SystemSnapshot::new(100.0, t * 1000).with_foreground(Some(Pid(444)));
        let sample = ProcessSample::new(Pid(444), "heavy_app.exe", 100.0).with_foreground(true);
        snap.add_process(sample);

        let plan = engine.evaluate_tick(&snap);
        for m in &plan.mutations {
            if let PolicyMutation::SetPriorityClass { pid, new, .. } = m {
                if *pid == Pid(444) {
                    assert!(
                        *new >= PriorityClass::Normal,
                        "FOREGROUND PROCESS DEMOTED: {:?}",
                        new
                    );
                }
            }
        }
    }
}

#[test]
fn test_no_excluded_process_is_ever_demoted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // cargo.exe and rustc.exe are in default exclude list
    for t in 1..=5 {
        let mut snap = SystemSnapshot::new(95.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(601), "cargo.exe", 95.0));
        snap.add_process(ProcessSample::new(Pid(602), "rustc.exe", 95.0));
        let plan = engine.evaluate_tick(&snap);

        for m in &plan.mutations {
            if let PolicyMutation::SetPriorityClass { pid, new, .. } = m {
                if *pid == Pid(601) || *pid == Pid(602) {
                    panic!("EXCLUDED PROCESS DEMOTED: PID {:?} -> {:?}", pid, new);
                }
            }
        }
    }
}

#[test]
fn test_no_game_process_is_ever_demoted() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    for t in 1..=5 {
        let mut snap = SystemSnapshot::new(95.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(777), "cs2.exe", 95.0));
        let plan = engine.evaluate_tick(&snap);

        for m in &plan.mutations {
            if let PolicyMutation::SetPriorityClass { pid, new, .. } = m {
                if *pid == Pid(777) {
                    panic!("GAME PROCESS DEMOTED: {:?}", new);
                }
            }
        }
    }
}

#[test]
fn test_every_demotion_has_matching_restore_invariant() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Demote 3 hogs
    for t in 1..=2 {
        let mut snap = SystemSnapshot::new(90.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(10), "hog1.exe", 70.0));
        snap.add_process(ProcessSample::new(Pid(20), "hog2.exe", 70.0));
        snap.add_process(ProcessSample::new(Pid(30), "hog3.exe", 70.0));
        let _ = engine.evaluate_tick(&snap);
    }

    assert!(engine.get_tracked_process(Pid(10)).unwrap().is_demoted);
    assert!(engine.get_tracked_process(Pid(20)).unwrap().is_demoted);
    assert!(engine.get_tracked_process(Pid(30)).unwrap().is_demoted);

    // Spike stops
    let mut all_restores = Vec::new();
    for t in 3..=5 {
        let mut snap = SystemSnapshot::new(20.0, t * 1000);
        snap.add_process(ProcessSample::new(Pid(10), "hog1.exe", 5.0));
        snap.add_process(ProcessSample::new(Pid(20), "hog2.exe", 5.0));
        snap.add_process(ProcessSample::new(Pid(30), "hog3.exe", 5.0));
        let plan = engine.evaluate_tick(&snap);
        for m in plan.mutations {
            all_restores.push(m);
        }
    }

    // Must have restored all 3
    let restored_pids: Vec<Pid> = all_restores
        .iter()
        .filter_map(|m| {
            if let PolicyMutation::SetPriorityClass { pid, new, .. } = m {
                if *new == PriorityClass::Normal {
                    return Some(*pid);
                }
            }
            None
        })
        .collect();

    assert!(restored_pids.contains(&Pid(10)));
    assert!(restored_pids.contains(&Pid(20)));
    assert!(restored_pids.contains(&Pid(30)));
}
