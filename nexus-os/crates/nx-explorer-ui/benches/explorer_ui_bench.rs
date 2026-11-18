use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nx_explorer::action::Msg;
use nx_explorer_ui::view_model::ExplorerWindow;
use nx_fs::entry::{DirEntry, EntryKind};
use nx_fs::index::engine::{SearchEngine, SearchOptions};
use nx_fs::index::frecency::FrecencyStore;
use nx_fs::index::matcher::Query;
use nx_fs::index::synthetic::build_index;
use nx_ui::atlas::Atlas;
use nx_ui::icon::IconCache;
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::text::TextEngine;
use nx_ui::widgets::Frame;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn make_100k_entries() -> Vec<DirEntry> {
    let mut entries = Vec::with_capacity(100_000);
    for i in 0..100_000 {
        let name = format!("document_{i:06}.pdf");
        let kind = if i % 10 == 0 {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        let size = (i * 1024) as u64;
        let mut e = DirEntry::new(OsString::from(name), kind, size);
        e.modified =
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1700000000 + (i % 86400) as u64));
        entries.push(e);
    }
    entries
}

fn bench_first_frame_100k(c: &mut Criterion) {
    let entries = make_100k_entries();
    let (mut win, initial_fx) = ExplorerWindow::new(PathBuf::from("/workspace"));
    let req = initial_fx
        .iter()
        .find_map(|f| match f {
            nx_explorer::action::Effect::ListDir { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap_or(1);

    win.explorer.update(Msg::ListingBatch {
        request: req,
        entries: entries.clone(),
    });
    win.explorer.update(Msg::ListingDone { request: req });

    c.bench_function("first_frame_100k_model_work", |b| {
        let input = Input::default();
        b.iter(|| {
            let fx = win.update(black_box(&input), 0.016);
            black_box((win.status_bar.item_count, fx.len()))
        });
    });

    let first_batch = entries[..1024].to_vec();
    c.bench_function("first_batch_streaming_model_work", |b| {
        b.iter_batched(
            || {
                let (w, ifx) = ExplorerWindow::new(PathBuf::from("/workspace"));
                let r = ifx
                    .iter()
                    .find_map(|f| match f {
                        nx_explorer::action::Effect::ListDir { request, .. } => Some(*request),
                        _ => None,
                    })
                    .unwrap_or(1);
                (w, r, first_batch.clone())
            },
            |(mut w, r, batch)| {
                w.explorer.update(Msg::ListingBatch {
                    request: r,
                    entries: batch,
                });
                let input = Input::default();
                w.update(&input, 0.016);
                black_box(w.status_bar.item_count)
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_scroll_frame_building_100k(c: &mut Criterion) {
    let entries = make_100k_entries();
    let (mut win, initial_fx) = ExplorerWindow::new(PathBuf::from("/workspace"));
    let req = initial_fx
        .iter()
        .find_map(|f| match f {
            nx_explorer::action::Effect::ListDir { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap_or(1);

    win.explorer.update(Msg::ListingBatch {
        request: req,
        entries,
    });
    win.explorer.update(Msg::ListingDone { request: req });

    let mut text = TextEngine::new().expect("text engine");
    let mut icons = IconCache::new();
    let mut atlas = Atlas::new(1024);
    let mut scene = Scene::with_capacity(4096, 16384);

    // Warm-up pass
    {
        let input = Input::default();
        win.update(&input, 0.016);
        let mut frame = Frame {
            scene: &mut scene,
            text: &mut text,
            icons: &mut icons,
            atlas: &mut atlas,
            theme: win.theme,
        };
        win.paint(&mut frame);
        let _ = scene.flatten(|_| &[]);
    }

    let mut offset = 0.0f32;

    c.bench_function("scroll_frame_building_100k", |b| {
        b.iter(|| {
            offset = (offset + 28.0) % 2_000_000.0;
            let input = Input {
                scroll_y: 28.0,
                ..Default::default()
            };
            win.update(&input, 0.016);

            scene.begin_frame();
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme: win.theme,
            };
            win.paint(&mut frame);
            let draw_list = scene.flatten(|_| &[]);
            black_box(draw_list.background.len())
        });
    });
}

fn bench_search_1m_names(c: &mut Criterion) {
    let index = build_index(1_000_000, 42).expect("1M tree");

    let engine = SearchEngine::new(index, FrecencyStore::default(), 0);
    let opts = SearchOptions {
        limit: 100,
        ..Default::default()
    };
    let q = Query::parse("budget report");

    c.bench_function("search_1m_names", |b| {
        b.iter(|| {
            let hits = engine.search(&q, &opts);
            black_box(hits.len())
        });
    });
}

criterion_group!(
    benches,
    bench_first_frame_100k,
    bench_scroll_frame_building_100k,
    bench_search_1m_names
);
criterion_main!(benches);
