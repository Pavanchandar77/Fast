use nx_explorer::action::{Action, Effect, Msg};
use nx_explorer::input::TextInput;
use nx_explorer::keymap::{Chord, Key};
use nx_explorer::Focus;
use nx_explorer_ui::rubber_band::RubberBand;
use nx_explorer_ui::view_model::ExplorerWindow;
use nx_fs::entry::{DirEntry, EntryKind};
use nx_fs::index::engine::SearchEngine;
use nx_fs::index::frecency::FrecencyStore;
use nx_fs::index::matcher::Query;
use nx_fs::index::PathIndex;
use nx_fs::ops::OpKind;
use nx_fs::sort::SortKey;
use nx_ui::geom::Point;
use nx_ui::input::Input;
use proptest::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn make_test_entries(n: usize) -> Vec<DirEntry> {
    (0..n)
        .map(|i| {
            let name = format!("file_{i:03}.txt");
            let kind = if i % 4 == 0 {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            let size = ((i + 1) * 512) as u64;
            let mut e = DirEntry::new(OsString::from(name), kind, size);
            e.modified = Some(
                SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + (i * 3600) as u64),
            );
            e
        })
        .collect()
}

fn setup_window_with_entries(entries: Vec<DirEntry>) -> ExplorerWindow {
    let (mut win, initial_fx) = ExplorerWindow::new(PathBuf::from("/workspace/test"));
    let req = initial_fx
        .iter()
        .find_map(|f| match f {
            Effect::ListDir { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap_or(1);

    win.explorer.update(Msg::ListingBatch {
        request: req,
        entries,
    });
    win.explorer.update(Msg::ListingDone { request: req });
    win.sync_status();
    win.sync_tabs();
    win
}

#[test]
fn test_view_model_selection_workflow() {
    let entries = make_test_entries(10);
    let mut win = setup_window_with_entries(entries);

    // Initial state: cursor is 0, nothing selected
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.len(), 10);
    assert_eq!(pane.selected_count(), 0);

    // 1. Single click row 2 -> selects row 2
    win.handle_msg(Msg::Action(Action::ClickRow {
        row: 2,
        ctrl: false,
        shift: false,
    }));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.cursor(), Some(2));
    assert_eq!(pane.selected_count(), 1);
    assert!(pane.is_selected(2));
    assert!(!pane.is_selected(3));

    // 2. Shift-click row 5 -> range selection 2..=5
    win.handle_msg(Msg::Action(Action::ClickRow {
        row: 5,
        ctrl: false,
        shift: true,
    }));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.cursor(), Some(5));
    assert_eq!(pane.selected_count(), 4);
    for r in 2..=5 {
        assert!(pane.is_selected(r), "row {r} should be selected");
    }
    assert!(!pane.is_selected(1));
    assert!(!pane.is_selected(6));

    // 3. Ctrl-click row 8 -> toggles row 8 into selection
    win.handle_msg(Msg::Action(Action::ClickRow {
        row: 8,
        ctrl: true,
        shift: false,
    }));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.cursor(), Some(8));
    assert_eq!(pane.selected_count(), 5);
    assert!(pane.is_selected(8));

    // 4. SelectAll (Ctrl+A) -> selects all 10 rows
    win.handle_msg(Msg::Action(Action::SelectAll));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.selected_count(), 10);

    // 5. ClearSelection -> 0 selected
    win.handle_msg(Msg::Action(Action::ClearSelection));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.selected_count(), 0);
}

#[test]
fn test_view_model_sorting_headers() {
    let entries = vec![
        DirEntry::new(OsString::from("zebra.txt"), EntryKind::File, 5000),
        DirEntry::new(OsString::from("alpha.txt"), EntryKind::File, 100),
        DirEntry::new(OsString::from("middle.txt"), EntryKind::File, 2500),
    ];
    let mut win = setup_window_with_entries(entries);

    // Default sort is Name ascending
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(
        pane.row(0).map(|e| e.name.to_str().unwrap()),
        Some("alpha.txt")
    );
    assert_eq!(
        pane.row(1).map(|e| e.name.to_str().unwrap()),
        Some("middle.txt")
    );
    assert_eq!(
        pane.row(2).map(|e| e.name.to_str().unwrap()),
        Some("zebra.txt")
    );

    // Click Name header -> flips to descending
    win.handle_msg(Msg::Action(Action::SortBy(SortKey::Name)));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(
        pane.row(0).map(|e| e.name.to_str().unwrap()),
        Some("zebra.txt")
    );
    assert_eq!(
        pane.row(1).map(|e| e.name.to_str().unwrap()),
        Some("middle.txt")
    );
    assert_eq!(
        pane.row(2).map(|e| e.name.to_str().unwrap()),
        Some("alpha.txt")
    );

    // Click Size header -> sorts by Size descending (largest first)
    win.handle_msg(Msg::Action(Action::SortBy(SortKey::Size)));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.row(0).map(|e| e.size), Some(5000));
    assert_eq!(pane.row(1).map(|e| e.size), Some(2500));
    assert_eq!(pane.row(2).map(|e| e.size), Some(100));

    // Click Size header again -> sorts by Size ascending
    win.handle_msg(Msg::Action(Action::SortBy(SortKey::Size)));
    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.row(0).map(|e| e.size), Some(100));
    assert_eq!(pane.row(1).map(|e| e.size), Some(2500));
    assert_eq!(pane.row(2).map(|e| e.size), Some(5000));
}

#[test]
fn test_view_model_rename_inline() {
    let entries = vec![
        DirEntry::new(
            OsString::from("quarterly_report.pdf"),
            EntryKind::File,
            4096,
        ),
        DirEntry::new(OsString::from("notes.txt"), EntryKind::File, 1024),
    ];
    let mut win = setup_window_with_entries(entries);

    // Select row 1 ("quarterly_report.pdf")
    win.handle_msg(Msg::Action(Action::ClickRow {
        row: 1,
        ctrl: false,
        shift: false,
    }));
    assert_eq!(win.explorer.focus, Focus::Pane);

    // 1. Begin Rename (e.g. F2 or context menu)
    win.handle_msg(Msg::Action(Action::BeginRename));
    assert_eq!(win.explorer.focus, Focus::Rename);
    // The editor text is populated with the entry name
    assert_eq!(win.explorer.input.text(), "quarterly_report.pdf");

    // 2. Cancel rename via Escape -> returns to Focus::Pane
    win.handle_chord(Chord::plain(Key::Escape), 100);
    assert_eq!(win.explorer.focus, Focus::Pane);

    // 3. Begin Rename again and commit new name via Enter
    win.handle_msg(Msg::Action(Action::BeginRename));
    assert_eq!(win.explorer.focus, Focus::Rename);
    // Replace input with "annual_report.pdf"
    win.explorer.input = TextInput::new("annual_report.pdf");
    let fx = win.handle_chord(Chord::plain(Key::Enter), 200);

    // Verify it generated an OpRequest for Rename
    let rename_effect = fx.iter().find_map(|f| match f {
        Effect::StartOp(op) => match &op.kind {
            OpKind::Rename { new_name } => Some((op.sources.clone(), new_name.clone())),
            _ => None,
        },
        _ => None,
    });
    assert!(rename_effect.is_some(), "must emit rename op effect");
    let (sources, new_name) = rename_effect.unwrap();
    assert_eq!(
        sources,
        vec![PathBuf::from("/workspace/test/quarterly_report.pdf")]
    );
    assert_eq!(new_name, "annual_report.pdf");
}

#[test]
fn test_view_model_watcher_diff_application() {
    let entries = vec![
        DirEntry::new(OsString::from("alpha.txt"), EntryKind::File, 100),
        DirEntry::new(OsString::from("beta.txt"), EntryKind::File, 200),
        DirEntry::new(OsString::from("gamma.txt"), EntryKind::File, 300),
    ];
    let mut win = setup_window_with_entries(entries);

    // Select beta.txt (index 1)
    win.handle_msg(Msg::Action(Action::ClickRow {
        row: 1,
        ctrl: false,
        shift: false,
    }));
    let pane = win.explorer.pane().expect("pane");
    assert!(pane.is_selected(1));
    assert_eq!(pane.len(), 3);

    let current_dir = win.current_dir().expect("current dir");

    // 1. Apply Created diff (delta.txt)
    let new_entry = DirEntry::new(OsString::from("delta.txt"), EntryKind::File, 400);
    win.handle_msg(Msg::DirDiff {
        dir: current_dir.clone(),
        diffs: vec![nx_explorer::action::DirDiff::Created(new_entry)],
    });

    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.len(), 4);
    // beta.txt should still remain selected!
    let beta_idx = (0..pane.len())
        .find(|&i| pane.row(i).map(|e| e.name.to_str().unwrap()) == Some("beta.txt"))
        .expect("beta present");
    assert!(
        pane.is_selected(beta_idx),
        "selection must persist across incremental diffs"
    );

    // 2. Apply Removed diff (alpha.txt)
    win.handle_msg(Msg::DirDiff {
        dir: current_dir.clone(),
        diffs: vec![nx_explorer::action::DirDiff::Removed(OsString::from(
            "alpha.txt",
        ))],
    });

    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.len(), 3);
    assert!(
        (0..pane.len()).all(|i| pane.row(i).map(|e| e.name.to_str().unwrap()) != Some("alpha.txt"))
    );

    // 3. Apply Renamed diff (gamma.txt -> zeta.txt)
    let renamed_entry = DirEntry::new(OsString::from("zeta.txt"), EntryKind::File, 300);
    win.handle_msg(Msg::DirDiff {
        dir: current_dir,
        diffs: vec![nx_explorer::action::DirDiff::Renamed {
            from: OsString::from("gamma.txt"),
            to: renamed_entry,
        }],
    });

    let pane = win.explorer.pane().expect("pane");
    assert_eq!(pane.len(), 3);
    assert!(
        (0..pane.len()).any(|i| pane.row(i).map(|e| e.name.to_str().unwrap()) == Some("zeta.txt"))
    );
}

fn sample_index() -> PathIndex {
    let mut idx = PathIndex::new();
    let root = idx.add_root("C:").expect("root");
    let docs = idx.insert(root, "Documents", true).expect("docs");
    idx.insert(docs, "Financials.xlsx", false).expect("sheet");
    idx.insert(docs, "Strategy.pdf", false).expect("pdf");
    idx
}

#[test]
fn test_index_round_trip_and_corruption_recovery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("index.nxix");

    // Build synthetic index
    let original = sample_index();

    // Save index
    original.save_file(&path).expect("save file");
    assert!(path.exists());

    // Round-trip load
    let loaded = PathIndex::load_file(&path).expect("load file");
    assert_eq!(loaded.len(), original.len());

    let engine = SearchEngine::new(loaded, FrecencyStore::default(), 0);
    let q = Query::parse("Financials");
    let hits = engine.search(&q, &Default::default());
    assert_eq!(hits.len(), 1);
    assert!(hits[0].path.contains("Financials.xlsx"));

    // Test CRC32 bit flip corruption detection
    let mut raw_bytes = std::fs::read(&path).expect("read");
    // Corrupt one byte in payload
    let len = raw_bytes.len();
    raw_bytes[len - 5] ^= 0xFF;
    std::fs::write(&path, &raw_bytes).expect("write corrupt");

    let corrupt_result = PathIndex::load_file(&path);
    assert!(
        corrupt_result.is_err(),
        "CRC32 mismatch must fail load_file"
    );

    // Recover via PersistentIndexManager
    let mut rebuild_invoked = false;
    let manager =
        nx_explorer_ui::index_cache::PersistentIndexManager::load_or_rebuild(path.clone(), || {
            rebuild_invoked = true;
            sample_index()
        });
    assert!(rebuild_invoked, "Corruption must trigger rebuild callback");
    let recovered_hits = manager.search("Strategy", 10);
    assert_eq!(recovered_hits.len(), 1);
    assert!(recovered_hits[0].path.contains("Strategy.pdf"));
}

#[test]
fn test_steady_frame_zero_allocations() {
    let entries = make_test_entries(10_000);
    let mut win = setup_window_with_entries(entries);

    // Steady scrolling: verify model update on 50 frames
    for _ in 0..50 {
        let input = Input {
            scroll_y: 28.0,
            ..Default::default()
        };
        win.update(&input, 0.016);
        assert!(win.explorer.pane().is_some());
    }
}

proptest! {
    #[test]
    fn proptest_rubber_band_geometry(
        x1 in 0.0f32..1000.0f32,
        y1 in 0.0f32..1000.0f32,
        x2 in 0.0f32..1000.0f32,
        y2 in 0.0f32..1000.0f32,
    ) {
        let mut rb = RubberBand::default();
        rb.start(Point::new(x1, y1));
        rb.update(Point::new(x2, y2));

        let rect = rb.rect();
        prop_assert!(rect.w >= 0.0);
        prop_assert!(rect.h >= 0.0);
        prop_assert_eq!(rect.x, x1.min(x2));
        prop_assert_eq!(rect.y, y1.min(y2));
        prop_assert_eq!(rect.w, (x1 - x2).abs());
        prop_assert_eq!(rect.h, (y1 - y2).abs());

        let test_point = Point::new((x1 + x2) * 0.5, (y1 + y2) * 0.5);
        prop_assert!(rect.contains(test_point));
    }
}
