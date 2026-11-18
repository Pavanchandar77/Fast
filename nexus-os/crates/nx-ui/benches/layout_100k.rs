use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::layout::{row_index_at, virtual_window, PrefixHeights, VirtualSpec};
use nx_ui::widgets::VirtualTable;

fn bench_virtual_window_100k(c: &mut Criterion) {
    let spec = VirtualSpec {
        row_count: 100_000,
        row_height: 32.0,
        overscan: 4,
    };

    let mut group = c.benchmark_group("layout_100k_rows");

    group.bench_function("virtual_window_fixed_top", |b| {
        b.iter(|| virtual_window(black_box(&spec), black_box(0.0), black_box(800.0)))
    });

    group.bench_function("virtual_window_fixed_mid", |b| {
        b.iter(|| virtual_window(black_box(&spec), black_box(1_600_000.0), black_box(800.0)))
    });

    group.bench_function("hit_test_fixed_100k", |b| {
        b.iter(|| {
            row_index_at(
                black_box(&spec),
                black_box(1_600_000.0),
                black_box(800.0),
                black_box(412.0),
            )
        })
    });

    let heights: Vec<f32> = (0..100_000)
        .map(|i| match i % 4 {
            0 => 28.0,
            1 => 36.0,
            2 => 48.0,
            _ => 64.0,
        })
        .collect();
    let prefix = PrefixHeights::from_heights(&heights);

    group.bench_function("prefix_binary_search_window_100k", |b| {
        b.iter(|| prefix.window(black_box(1_600_000.0), black_box(800.0), black_box(4)))
    });

    group.bench_function("prefix_offset_lookup_100k", |b| {
        b.iter(|| prefix.offset(black_box(50_000)))
    });

    let mut table = VirtualTable::new(100_000);
    let input = Input::default();
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);

    group.bench_function("table_steady_update_100k", |b| {
        b.iter(|| {
            table.update(black_box(&input), black_box(bounds), black_box(0.016));
        })
    });

    group.finish();
}

criterion_group!(benches, bench_virtual_window_100k);
criterion_main!(benches);
