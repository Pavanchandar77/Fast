#![allow(clippy::field_reassign_with_default)]

use nx_governor_policy::{
    GovernorEngine, GovernorPolicyConfig, Pid, PolicyMutation, PriorityClass, ProcessSample,
    SystemSnapshot,
};
use std::time::Instant;

#[test]
fn test_safe_defaults_invariants() {
    let config = GovernorPolicyConfig::default();

    // Invariant 1: never suspend processes by default
    assert!(
        !config.allow_suspend,
        "Safe default: allow_suspend must be false"
    );

    // Invariant 2: no below normal background priority by default
    assert!(
        !config.allow_below_normal_default,
        "Safe default: allow_below_normal_default must be false"
    );

    // Invariant 3: never boost to High unless whitelisted
    assert_eq!(
        config.max_boost_class,
        PriorityClass::AboveNormal,
        "Safe default: max_boost_class must be AboveNormal, not High"
    );

    // Invariant 4: dry-run default is false
    assert!(!config.dry_run);

    // Invariant 5: game mode auto detect is on
    assert!(config.game_mode_auto_detect);

    // Invariant 6: default exclusions exist
    assert!(!config.exclude_rules.is_empty());
}

#[test]
fn test_dry_run_mode_emits_dry_run_plans() {
    let mut config = GovernorPolicyConfig::default();
    config.dry_run = true;
    config.boost_foreground = true;
    let mut engine = GovernorEngine::new(config);

    let plan = engine.on_foreground_changed(Some(Pid(100)), 1000);
    assert_eq!(plan.len(), 1);
    assert!(
        plan.is_dry_run,
        "Plan emitted in dry-run mode must have is_dry_run = true"
    );

    assert_eq!(
        plan.mutations[0],
        PolicyMutation::SetPriorityClass {
            pid: Pid(100),
            previous: PriorityClass::Normal,
            new: PriorityClass::AboveNormal,
        }
    );
}

#[test]
fn test_policy_evaluation_timing_budget_sub_100_microseconds() {
    let config = GovernorPolicyConfig::default();
    let mut engine = GovernorEngine::new(config);

    // Construct realistic 100 process snapshot
    let mut snapshot = SystemSnapshot::new(75.0, 1000).with_foreground(Some(Pid(50)));
    for i in 1..=100 {
        let pid = Pid(i as u32);
        let name = format!("process_{}.exe", i);
        let cpu = if i % 15 == 0 { 60.0 } else { 1.5 };
        let mut sample = ProcessSample::new(pid, name, cpu);
        if pid == Pid(50) {
            sample = sample.with_foreground(true);
        }
        snapshot.add_process(sample);
    }

    // Warm-up
    for _ in 0..10 {
        let _ = engine.evaluate_tick(&snapshot);
    }

    // Measure 100 iterations
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = engine.evaluate_tick(&snapshot);
    }
    let elapsed = start.elapsed();
    let per_tick = elapsed / iterations;

    // Must be cheap: easily under 500 µs (typically < 30 µs on modern CPUs)
    println!(
        "GovernorEngine 100-process decision latency: {:.2} µs/tick (debug_assertions={})",
        per_tick.as_secs_f64() * 1_000_000.0,
        cfg!(debug_assertions)
    );
    let budget = if cfg!(debug_assertions) {
        std::time::Duration::from_millis(5)
    } else {
        std::time::Duration::from_micros(100)
    };
    assert!(
        per_tick < budget,
        "Policy evaluation tick took {:?}, exceeding budget {:?}",
        per_tick,
        budget
    );
}
