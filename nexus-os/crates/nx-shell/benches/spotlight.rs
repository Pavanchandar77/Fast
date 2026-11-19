use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nx_shell::{engine_from_paths, Spotlight};

fn build_10k_spotlight() -> Spotlight {
    let mut spotlight = Spotlight::new();
    for i in 0..8_000 {
        spotlight.add_app(
            &format!("com.example.app_{i}"),
            &format!("Application {i} Professional Suite"),
            "Productivity Tools",
        );
    }
    for i in 0..2_000 {
        spotlight.add_setting(
            &format!("setting_{i}"),
            &format!("System Preference {i} Advanced Config"),
            "System Preferences",
        );
    }
    spotlight
}

fn bench_spotlight(c: &mut Criterion) {
    let spotlight_10k = build_10k_spotlight();

    let mut group = c.benchmark_group("spotlight_10k_items");

    // Benchmark query with common prefix across 10,000 items
    group.bench_function("query_prefix_app_42", |b| {
        b.iter(|| spotlight_10k.query_str(black_box("app 42")))
    });

    // Benchmark query with middle match
    group.bench_function("query_middle_suite_500", |b| {
        b.iter(|| spotlight_10k.query_str(black_box("suite 500")))
    });

    // Benchmark query that matches settings
    group.bench_function("query_setting_advanced", |b| {
        b.iter(|| spotlight_10k.query_str(black_box("preference 12")))
    });

    // Benchmark calculator query evaluation
    group.bench_function("calculator_complex_expr", |b| {
        b.iter(|| spotlight_10k.query_str(black_box("sqrt(16) * 2^4 + (10 - 2)%")))
    });

    // Benchmark empty query (recents)
    group.bench_function("empty_query_recents", |b| {
        b.iter(|| spotlight_10k.query_str(black_box("")))
    });

    // Benchmark with attached files
    let mut spotlight_with_files = Spotlight::with_builtins();
    let paths: Vec<String> = (0..1_000)
        .map(|i| format!("/Users/nexus/Documents/project_{}/file_{}.rs", i % 20, i))
        .collect();
    let path_slices: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    if let Ok(engine) = engine_from_paths(&path_slices) {
        spotlight_with_files.attach_files(engine);
    }

    group.bench_function("query_with_1k_files", |b| {
        b.iter(|| spotlight_with_files.query_str(black_box("file_42.rs")))
    });

    group.finish();
}

criterion_group!(benches, bench_spotlight);
criterion_main!(benches);
