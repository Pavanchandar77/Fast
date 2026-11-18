//! Listing a 100k-entry folder: time to first batch and full listing.

use criterion::{criterion_group, criterion_main, Criterion};
use nx_fs::listing::{list_dir_with, spawn_listing, ListEvent, ListOptions};
use nx_fs::sort::{sort_entries, SortSpec};
use nx_fs::{CancelToken, ThreadPool};
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Duration;

const N: usize = 100_000;

fn make_dir() -> tempfile::TempDir {
    let t = tempfile::Builder::new()
        .prefix("nx-bench-100k")
        .tempdir()
        .expect("tempdir");
    for i in 0..N {
        let name = format!(
            "{} file {i}.{}",
            ["report", "photo", "notes", "budget"][i % 4],
            ["txt", "jpg", "md", "xlsx"][i % 4]
        );
        std::fs::File::create(t.path().join(name)).expect("create");
    }
    t
}

fn first_batch(dir: &Path, opts: &ListOptions) -> usize {
    let mut n = 0;
    let _ = list_dir_with(dir, opts, &CancelToken::new(), &mut |b| {
        n = b.len();
        ControlFlow::Break(())
    });
    n
}

fn full(dir: &Path, opts: &ListOptions) -> usize {
    let mut n = 0;
    list_dir_with(dir, opts, &CancelToken::new(), &mut |b| {
        n += b.len();
        ControlFlow::Continue(())
    })
    .expect("list");
    n
}

fn bench(c: &mut Criterion) {
    let t = make_dir();
    let dir = t.path().to_path_buf();
    let opts = ListOptions::default();
    let names_only = ListOptions {
        with_metadata: false,
        ..ListOptions::default()
    };
    assert_eq!(full(&dir, &opts), N);

    let mut g = c.benchmark_group("list_100k");
    g.sample_size(20).measurement_time(Duration::from_secs(8));
    g.bench_function("first_batch_sync", |b| b.iter(|| first_batch(&dir, &opts)));
    let pool = ThreadPool::new(4, "bench").expect("pool");
    g.bench_function("first_batch_async_pool", |b| {
        b.iter(|| {
            let h = spawn_listing(&pool, dir.clone(), opts, 1).expect("spawn");
            match h.events.recv().expect("event") {
                ListEvent::Batch { entries, .. } => entries.len(),
                other => panic!("{other:?}"),
            }
        })
    });
    g.bench_function("full_with_metadata", |b| b.iter(|| full(&dir, &opts)));
    g.bench_function("full_names_only", |b| b.iter(|| full(&dir, &names_only)));
    let mut entries = Vec::new();
    list_dir_with(&dir, &opts, &CancelToken::new(), &mut |b| {
        entries.extend(b);
        ControlFlow::Continue(())
    })
    .expect("list");
    g.bench_function("natural_sort_100k", |b| {
        b.iter_batched(
            || entries.clone(),
            |mut v| sort_entries(&mut v, SortSpec::default()),
            criterion::BatchSize::LargeInput,
        )
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
