use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use nx_governor_policy::{
    GovernorEngine, GovernorPolicyConfig, Pid, ProcessSample, SystemSnapshot,
};

fn generate_snapshot(process_count: usize, high_system_cpu: bool) -> SystemSnapshot {
    let total_cpu = if high_system_cpu { 85.0 } else { 25.0 };
    let mut snapshot = SystemSnapshot::new(total_cpu, 1000).with_foreground(Some(Pid(100)));

    for i in 1..=process_count {
        let pid = Pid(i as u32);
        let name = format!("proc_{}.exe", i);
        // Introduce some CPU hogs
        let cpu = if i % 10 == 0 { 65.0 } else { 2.5 };
        let mut sample = ProcessSample::new(pid, name, cpu);
        if pid == Pid(100) {
            sample = sample.with_foreground(true);
        }
        snapshot.add_process(sample);
    }

    snapshot
}

fn bench_policy_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("GovernorEngine::evaluate_tick");

    for &size in &[100, 500, 1000] {
        let snapshot = generate_snapshot(size, true);

        group.bench_with_input(BenchmarkId::from_parameter(size), &snapshot, |b, snap| {
            let config = GovernorPolicyConfig::default();
            let mut engine = GovernorEngine::new(config);
            // Pre-seed engine with initial tick
            let _ = engine.evaluate_tick(snap);

            b.iter(|| {
                let plan = engine.evaluate_tick(black_box(snap));
                black_box(plan);
            });
        });
    }

    group.finish();
}

fn bench_foreground_switch(c: &mut Criterion) {
    let mut group = c.benchmark_group("GovernorEngine::on_foreground_changed");

    group.bench_function("fg_switch_single", |b| {
        let config = GovernorPolicyConfig::default();
        let mut engine = GovernorEngine::new(config);
        let mut toggle = false;

        b.iter(|| {
            let pid = if toggle {
                Some(Pid(200))
            } else {
                Some(Pid(300))
            };
            toggle = !toggle;
            let plan = engine.on_foreground_changed(black_box(pid), 2000);
            black_box(plan);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_policy_evaluation, bench_foreground_switch);
criterion_main!(benches);
