//! Index build and query latency on 1M synthetic paths.

use criterion::{criterion_group, criterion_main, Criterion};
use nx_fs::index::engine::SearchOptions;
use nx_fs::index::frecency::FrecencyStore;
use nx_fs::index::mft::{MftTree, NTFS_ROOT_REF};
use nx_fs::index::synthetic::build_index;
use nx_fs::index::usn::{UsnRecord, ATTR_DIRECTORY};
use nx_fs::index::{Query, SearchEngine};
use std::time::Duration;

const N: usize = 1_000_000;

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("index_1m");
    g.sample_size(10).measurement_time(Duration::from_secs(20));
    g.bench_function("build_synthetic_1m", |b| {
        b.iter(|| build_index(N, 42).expect("build").len())
    });

    // MFT path: rebuild the same tree from USN-style records in MFT order.
    let ix = build_index(N, 42).expect("build");
    let records: Vec<UsnRecord> = ix
        .ids()
        .filter(|&i| ix.parent(i) != nx_fs::index::NO_PARENT)
        .map(|i| {
            let parent = ix.parent(i);
            UsnRecord {
                file_ref: u128::from(i) + 100,
                parent_ref: if ix.parent(parent) == nx_fs::index::NO_PARENT {
                    NTFS_ROOT_REF
                } else {
                    u128::from(parent) + 100
                },
                usn: i64::from(i),
                reason: 0,
                attributes: if ix.is_dir(i) { ATTR_DIRECTORY } else { 0x20 },
                name: ix.name(i).to_string(),
            }
        })
        .collect();
    g.bench_function("build_from_mft_records_1m", |b| {
        b.iter(|| {
            MftTree::build("C:", records.clone())
                .expect("mft")
                .index
                .len()
        })
    });
    g.finish();

    let mut engine = SearchEngine::new(ix, FrecencyStore::default(), 0);
    // Some frecency so ranking does real work.
    let some: Vec<String> = engine
        .index
        .ids()
        .step_by(997)
        .take(500)
        .map(|i| engine.index.full_path(i))
        .collect();
    for (k, p) in some.iter().enumerate() {
        engine.record_open(p, k as u64);
    }
    let queries = [
        ("prefix", "budget"),
        ("substring", "port_1"),
        ("fuzzy", "nxexpl"),
        ("multi_term", "summer photos"),
        ("path_segments", "src/report"),
        ("ext_filter", "ext:rs kernel"),
        ("single_char", "a"),
    ];
    let mut g = c.benchmark_group("query_1m");
    g.sample_size(50).measurement_time(Duration::from_secs(5));
    let par = SearchOptions::default();
    let single = SearchOptions {
        threads: 1,
        ..SearchOptions::default()
    };
    for (label, q) in queries {
        let query = Query::parse(q);
        let hits = engine.search(&query, &par);
        assert!(
            !hits.is_empty() || label == "path_segments",
            "{label} found nothing"
        );
        g.bench_function(format!("{label}_parallel"), |b| {
            b.iter(|| engine.search(&query, &par).len())
        });
        g.bench_function(format!("{label}_1thread"), |b| {
            b.iter(|| engine.search(&query, &single).len())
        });
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
