//! End-to-end tests of the Explorer state machine.

use crate::action::{Action, CursorMove, DirDiff, Effect, Msg, ViewKind};
use crate::explorer::{Explorer, Focus};
use crate::keymap::{Chord, Key, Mods};
use crate::pane::Location;
use nx_fs::index::SearchHit;
use nx_fs::ops::{ConflictPolicy, OpKind, OpRequest};
use nx_fs::preview::Priority;
use nx_fs::{DirEntry, EntryKind};
use proptest::prelude::*;
use std::path::PathBuf;

fn e(name: &str) -> DirEntry {
    let kind = if name.contains('.') {
        EntryKind::File
    } else {
        EntryKind::Dir
    };
    DirEntry::new(name.into(), kind, 10)
}

fn last_list(fx: &[Effect]) -> u64 {
    fx.iter()
        .rev()
        .find_map(|f| match f {
            Effect::ListDir { request, .. } | Effect::Search { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap()
}

/// Feed a listing for the most recent ListDir in `fx`.
fn feed(ex: &mut Explorer, fx: &[Effect], names: &[&str]) {
    let request = last_list(fx);
    ex.update(Msg::ListingBatch {
        request,
        entries: names.iter().map(|n| e(n)).collect(),
    });
    ex.update(Msg::ListingDone { request });
}

fn setup(names: &[&str]) -> Explorer {
    let (mut ex, fx) = Explorer::new("/home/me".into());
    feed(&mut ex, &fx, names);
    ex
}

fn act(ex: &mut Explorer, a: Action) -> Vec<Effect> {
    ex.update(Msg::Action(a))
}

fn key(ex: &mut Explorer, k: Key, mods: Mods) -> Vec<Effect> {
    ex.update(Msg::Key {
        chord: Chord::new(k, mods),
        now_ms: 0,
    })
}

fn typed(ex: &mut Explorer, s: &str) -> Vec<Effect> {
    ex.update(Msg::Text {
        text: s.into(),
        now_ms: 0,
    })
}

fn cursor_name(ex: &Explorer) -> Option<String> {
    ex.pane()
        .and_then(|p| p.cursor_entry())
        .map(|e| e.display_name.clone())
}

fn dir(ex: &Explorer) -> PathBuf {
    ex.pane()
        .and_then(|p| p.location.dir().map(|d| d.to_path_buf()))
        .unwrap()
}

#[test]
fn initial_listing_and_open_folder() {
    let mut ex = setup(&["docs", "a.txt"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("docs"));
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::ListDir { path, .. }] if path == &PathBuf::from("/home/me/docs"))
    );
    assert_eq!(dir(&ex), PathBuf::from("/home/me/docs"));
    assert_eq!(ex.sidebar.recents()[0], PathBuf::from("/home/me/docs"));
}

#[test]
fn opening_a_file_emits_open_and_frecency() {
    let mut ex = setup(&["a.txt"]);
    let fx = act(&mut ex, Action::OpenSelection);
    assert_eq!(
        fx,
        vec![
            Effect::Open("/home/me/a.txt".into()),
            Effect::RecordOpen("/home/me/a.txt".into())
        ]
    );
}

#[test]
fn up_and_back_reselect_the_folder_we_left() {
    let (mut ex, fx) = Explorer::new("/home/me/docs".into());
    feed(&mut ex, &fx, &["x.txt"]);
    let fx = act(&mut ex, Action::GoUp);
    feed(&mut ex, &fx, &["a", "docs", "z"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("docs"));
    let fx = act(&mut ex, Action::GoBack);
    feed(&mut ex, &fx, &["x.txt"]);
    assert_eq!(dir(&ex), PathBuf::from("/home/me/docs"));
    let fx = act(&mut ex, Action::GoForward);
    assert_eq!(dir(&ex), PathBuf::from("/home/me"));
    feed(&mut ex, &fx, &["a", "docs", "z"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("docs"));
}

#[test]
fn stale_listing_batches_are_ignored() {
    let (mut ex, fx0) = Explorer::new("/a".into());
    let old = last_list(&fx0);
    let fx1 = act(&mut ex, Action::Navigate("/b".into()));
    assert!(fx1.contains(&Effect::CancelListing { request: old }));
    ex.update(Msg::ListingBatch {
        request: old,
        entries: vec![e("stale.txt")],
    });
    assert_eq!(ex.pane().map(|p| p.len()), Some(0));
    feed(&mut ex, &fx1, &["fresh.txt"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("fresh.txt"));
}

#[test]
fn listing_failure_sets_status() {
    let (mut ex, fx) = Explorer::new("/root".into());
    ex.update(Msg::ListingFailed {
        request: last_list(&fx),
        error: "Access denied".into(),
    });
    assert_eq!(ex.status.as_deref(), Some("Access denied"));
    assert!(matches!(
        ex.pane().map(|p| &p.load),
        Some(crate::pane::LoadState::Failed { .. })
    ));
}

#[test]
fn tabs_open_close_reopen() {
    let mut ex = setup(&[]);
    act(&mut ex, Action::NewTab);
    act(&mut ex, Action::Navigate("/tmp".into()));
    assert_eq!(ex.tabs().len(), 2);
    assert_eq!(ex.tab().map(|t| t.title()), Some("tmp".into()));
    act(&mut ex, Action::CloseTab);
    assert_eq!(ex.tabs().len(), 1);
    let fx = act(&mut ex, Action::ReopenClosedTab);
    assert!(matches!(&fx[..], [Effect::ListDir { path, .. }] if path == &PathBuf::from("/tmp")));
    act(&mut ex, Action::NextTab);
    assert_eq!(ex.active_tab(), 0);
    act(&mut ex, Action::PrevTab);
    assert_eq!(ex.active_tab(), 1);
    act(&mut ex, Action::CloseTab);
    assert_eq!(act(&mut ex, Action::CloseTab), vec![Effect::CloseWindow]);
    assert_eq!(
        ex.tabs().len(),
        1,
        "last tab stays; the host closes the window"
    );
}

#[test]
fn split_pane_copy_and_move_to_other() {
    let mut ex = setup(&["a.txt"]);
    assert!(act(&mut ex, Action::CopyToOtherPane).is_empty());
    assert!(ex.status.is_some());
    let fx = act(&mut ex, Action::ToggleSplit);
    assert!(ex.tab().is_some_and(|t| t.is_split()));
    act(&mut ex, Action::Navigate("/backup".into()));
    feed(&mut ex, &fx, &[]);
    act(&mut ex, Action::FocusOtherPane);
    let fx = act(&mut ex, Action::CopyToOtherPane);
    assert_eq!(
        fx,
        vec![Effect::StartOp(OpRequest::copy(
            vec!["/home/me/a.txt".into()],
            "/backup"
        ))]
    );
    let fx = key(&mut ex, Key::F(6), Mods::SHIFT);
    assert_eq!(
        fx,
        vec![Effect::StartOp(OpRequest::move_to(
            vec!["/home/me/a.txt".into()],
            "/backup"
        ))]
    );
    act(&mut ex, Action::ToggleSplit);
    assert!(!ex.tab().is_some_and(|t| t.is_split()));
    assert_eq!(
        dir(&ex),
        PathBuf::from("/home/me"),
        "the focused pane is kept"
    );
}

#[test]
fn clipboard_copy_cut_paste() {
    let mut ex = setup(&["a.txt", "b.txt"]);
    act(&mut ex, Action::SelectAll);
    let fx = key(&mut ex, Key::Char('X'), Mods::CTRL);
    assert_eq!(
        fx,
        vec![Effect::SetClipboard {
            paths: vec!["/home/me/a.txt".into(), "/home/me/b.txt".into()],
            cut: true
        }]
    );
    assert!(
        act(&mut ex, Action::Paste).is_empty(),
        "cut+paste into the same folder is a no-op"
    );
    let fx = act(&mut ex, Action::Navigate("/dst".into()));
    feed(&mut ex, &fx, &[]);
    let fx = act(&mut ex, Action::Paste);
    assert!(
        matches!(&fx[..], [Effect::StartOp(OpRequest { kind: OpKind::Move { dest }, .. })] if dest == &PathBuf::from("/dst"))
    );
    assert!(ex.clipboard.is_none(), "cut is consumed");
}

#[test]
fn duplicate_keeps_both() {
    let mut ex = setup(&["a.txt"]);
    let fx = key(&mut ex, Key::Char('D'), Mods::CTRL);
    assert_eq!(
        fx,
        vec![Effect::StartOp(
            OpRequest::copy(vec!["/home/me/a.txt".into()], "/home/me")
                .with_conflict(ConflictPolicy::KeepBoth)
        )]
    );
}

#[test]
fn delete_to_trash_and_permanent_needs_confirmation() {
    let mut ex = setup(&["a.txt"]);
    let fx = key(&mut ex, Key::Delete, Mods::NONE);
    assert_eq!(
        fx,
        vec![Effect::StartOp(OpRequest::delete(
            vec!["/home/me/a.txt".into()],
            true
        ))]
    );
    assert!(key(&mut ex, Key::Delete, Mods::SHIFT).is_empty());
    assert!(ex
        .prompt
        .as_ref()
        .is_some_and(|p| p.message.contains("a.txt")));
    // Other keys are swallowed while the prompt is up.
    assert!(key(&mut ex, Key::Char('A'), Mods::CTRL).is_empty());
    assert!(key(&mut ex, Key::Escape, Mods::NONE).is_empty());
    assert!(ex.prompt.is_none());
    key(&mut ex, Key::Delete, Mods::SHIFT);
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert_eq!(
        fx,
        vec![Effect::StartOp(OpRequest::delete(
            vec!["/home/me/a.txt".into()],
            false
        ))]
    );
}

#[test]
fn rename_selects_stem_validates_and_reselects() {
    let mut ex = setup(&["report.final.pdf", "z.txt"]);
    key(&mut ex, Key::F(2), Mods::NONE);
    assert_eq!(ex.focus, Focus::Rename);
    assert_eq!(ex.input.selection(), Some((0, 12)));
    typed(&mut ex, "bad|name");
    assert!(key(&mut ex, Key::Enter, Mods::NONE).is_empty());
    assert!(ex
        .status
        .as_deref()
        .is_some_and(|s| s.starts_with("Invalid name")));
    assert_eq!(
        ex.focus,
        Focus::Rename,
        "keeps editing after an invalid name"
    );
    ex.input = crate::input::TextInput::new("summary.pdf");
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert_eq!(
        fx,
        vec![Effect::StartOp(OpRequest {
            kind: OpKind::Rename {
                new_name: "summary.pdf".into()
            },
            sources: vec!["/home/me/report.final.pdf".into()],
            conflict: ConflictPolicy::Ask
        })]
    );
    assert_eq!(ex.focus, Focus::Pane);
    let fx = ex.update(Msg::DirChanged {
        dir: "/home/me".into(),
    });
    feed(&mut ex, &fx, &["summary.pdf", "z.txt"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("summary.pdf"));
}

#[test]
fn rename_to_same_name_is_noop() {
    let mut ex = setup(&["a.txt"]);
    act(&mut ex, Action::BeginRename);
    assert!(key(&mut ex, Key::Enter, Mods::NONE).is_empty());
    assert_eq!(ex.focus, Focus::Pane);
}

#[test]
fn watcher_diff_application_without_reload() {
    let mut ex = setup(&["a.txt", "b.txt", "c.txt"]);
    assert_eq!(ex.pane().unwrap().len(), 3);

    // 1. Create a new file d.txt via DirDiff
    let d_entry = e("d.txt");
    let fx = ex.update(Msg::DirDiff {
        dir: "/home/me".into(),
        diffs: vec![DirDiff::Created(d_entry.clone())],
    });
    // Applying diffs produces NO reload effect!
    assert!(
        fx.is_empty(),
        "diff application must not emit reload effects"
    );
    assert_eq!(ex.pane().unwrap().len(), 4);
    assert!(ex.pane().unwrap().row_of(&"d.txt".into()).is_some());

    // 2. Rename b.txt -> b_renamed.txt
    let b_renamed = e("b_renamed.txt");
    ex.update(Msg::DirDiff {
        dir: "/home/me".into(),
        diffs: vec![DirDiff::Renamed {
            from: "b.txt".into(),
            to: b_renamed,
        }],
    });
    assert_eq!(ex.pane().unwrap().len(), 4);
    assert!(ex.pane().unwrap().row_of(&"b.txt".into()).is_none());
    assert!(ex.pane().unwrap().row_of(&"b_renamed.txt".into()).is_some());

    // 3. Remove a.txt
    ex.update(Msg::DirDiff {
        dir: "/home/me".into(),
        diffs: vec![DirDiff::Removed("a.txt".into())],
    });
    assert_eq!(ex.pane().unwrap().len(), 3);
    assert!(ex.pane().unwrap().row_of(&"a.txt".into()).is_none());
}

#[test]
fn new_folder_picks_unique_name() {
    let mut ex = setup(&["New folder", "a.txt"]);
    key(&mut ex, Key::Char('N'), Mods::CTRL_SHIFT);
    assert_eq!(ex.focus, Focus::NewFolder);
    assert_eq!(ex.input.text(), "New folder (2)");
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::StartOp(OpRequest { kind: OpKind::NewFolder { name, parent }, .. })] if name == "New folder (2)" && parent == &PathBuf::from("/home/me"))
    );
}

#[test]
fn address_bar_and_escape() {
    let mut ex = setup(&[]);
    key(&mut ex, Key::Char('L'), Mods::CTRL);
    assert_eq!(ex.focus, Focus::AddressBar);
    assert_eq!(ex.input.text(), "/home/me");
    typed(&mut ex, "\"/var/log\"");
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::ListDir { path, .. }] if path == &PathBuf::from("/var/log"))
    );
    key(&mut ex, Key::F(4), Mods::NONE);
    typed(&mut ex, "/nowhere");
    assert!(key(&mut ex, Key::Escape, Mods::NONE).is_empty());
    assert_eq!(dir(&ex), PathBuf::from("/var/log"));
}

#[test]
fn search_flow_with_results() {
    let mut ex = setup(&["a.txt"]);
    key(&mut ex, Key::Char('F'), Mods::CTRL);
    typed(&mut ex, "budget");
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::Search { query, scope: Some(s), .. }] if query == "budget" && s == &PathBuf::from("/home/me"))
    );
    let request = last_list(&fx);
    let hits = vec![
        SearchHit {
            id: 1,
            score: 900,
            path: "/home/me/work/budget.xlsx".into(),
            is_dir: false,
        },
        SearchHit {
            id: 2,
            score: 500,
            path: "/home/me/budgets".into(),
            is_dir: true,
        },
    ];
    ex.update(Msg::SearchResults { request, hits });
    assert_eq!(ex.tab().map(|t| t.title()), Some("Search: budget".into()));
    assert_eq!(
        cursor_name(&ex).as_deref(),
        Some("budget.xlsx"),
        "rank order is kept"
    );
    let fx = act(&mut ex, Action::OpenSelection);
    assert_eq!(fx[0], Effect::Open("/home/me/work/budget.xlsx".into()));
    act(&mut ex, Action::BeginRename);
    assert_eq!(ex.focus, Focus::Pane, "no rename inside search results");
    act(&mut ex, Action::GoUp);
    assert_eq!(
        ex.pane().map(|p| p.location.clone()),
        Some(Location::Dir("/home/me".into()))
    );
}

#[test]
fn palette_runs_commands() {
    let mut ex = setup(&["a.txt"]);
    ex.update(Msg::Favorites(vec!["/srv/media".into()]));
    key(&mut ex, Key::Char('P'), Mods::CTRL);
    assert_eq!(ex.focus, Focus::Palette);
    typed(&mut ex, "media");
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::ListDir { path, .. }] if path == &PathBuf::from("/srv/media"))
    );
    assert!(ex.palette.is_none());
    key(&mut ex, Key::Char('P'), Mods::CTRL);
    typed(&mut ex, "view det");
    key(&mut ex, Key::Enter, Mods::NONE);
    assert_eq!(ex.pane().map(|p| p.view), Some(ViewKind::Details));
}

#[test]
fn type_to_select_from_keys() {
    let mut ex = setup(&["apple.txt", "banana.txt", "cherry.txt"]);
    ex.update(Msg::Key {
        chord: Chord::plain(Key::Char('C')),
        now_ms: 10,
    });
    assert_eq!(cursor_name(&ex).as_deref(), Some("cherry.txt"));
    ex.update(Msg::Text {
        text: "b".into(),
        now_ms: 5000,
    });
    assert_eq!(cursor_name(&ex).as_deref(), Some("banana.txt"));
}

#[test]
fn quick_look_follows_cursor() {
    let mut ex = setup(&["a.png", "b.png"]);
    let fx = key(&mut ex, Key::Space, Mods::NONE);
    assert_eq!(
        fx,
        vec![Effect::RequestPreview {
            path: "/home/me/a.png".into(),
            priority: Priority::Focused
        }]
    );
    let fx = key(&mut ex, Key::Down, Mods::NONE);
    assert_eq!(
        fx,
        vec![Effect::RequestPreview {
            path: "/home/me/b.png".into(),
            priority: Priority::Focused
        }]
    );
    key(&mut ex, Key::Escape, Mods::NONE);
    assert!(!ex.quick_look);
    assert!(key(&mut ex, Key::Up, Mods::NONE).is_empty());
}

#[test]
fn columns_view_left_right_navigate() {
    let mut ex = setup(&["sub", "f.txt"]);
    act(&mut ex, Action::SetView(ViewKind::Columns));
    let fx = key(&mut ex, Key::Right, Mods::NONE);
    assert_eq!(dir(&ex), PathBuf::from("/home/me/sub"));
    feed(&mut ex, &fx, &[]);
    let fx = key(&mut ex, Key::Left, Mods::NONE);
    feed(&mut ex, &fx, &["f.txt", "sub"]);
    assert_eq!(cursor_name(&ex).as_deref(), Some("sub"));
}

#[test]
fn sidebar_favorites_and_drives() {
    let mut ex = setup(&["proj"]);
    let fx = key(&mut ex, Key::Char('B'), Mods::CTRL);
    assert_eq!(fx, vec![Effect::SaveFavorites(vec!["/home/me".into()])]);
    act(
        &mut ex,
        Action::ClickRow {
            row: 0,
            ctrl: false,
            shift: false,
        },
    );
    let fx = act(&mut ex, Action::AddFavorite);
    assert_eq!(
        fx,
        vec![Effect::SaveFavorites(vec![
            "/home/me".into(),
            "/home/me/proj".into()
        ])]
    );
    act(&mut ex, Action::FocusSidebar);
    key(&mut ex, Key::Down, Mods::NONE);
    let fx = key(&mut ex, Key::Enter, Mods::NONE);
    assert!(
        matches!(&fx[..], [Effect::ListDir { path, .. }] if path == &PathBuf::from("/home/me/proj"))
    );
    assert_eq!(ex.focus, Focus::Pane);
    let fx = act(&mut ex, Action::RemoveFavorite("/home/me".into()));
    assert_eq!(
        fx,
        vec![Effect::SaveFavorites(vec!["/home/me/proj".into()])]
    );
}

#[test]
fn copy_path_and_properties() {
    let mut ex = setup(&["a.txt", "b.txt"]);
    act(&mut ex, Action::SelectAll);
    assert_eq!(
        act(&mut ex, Action::CopyPath),
        vec![Effect::CopyText("/home/me/a.txt\n/home/me/b.txt".into())]
    );
    act(&mut ex, Action::Undo);
    let fx = key(&mut ex, Key::Enter, Mods::ALT);
    assert!(matches!(&fx[..], [Effect::ShowProperties(p)] if p.len() == 2));
    assert_eq!(key(&mut ex, Key::Char('Z'), Mods::CTRL), vec![Effect::Undo]);
}

#[test]
fn viewport_drives_visible_previews() {
    let names: Vec<String> = (0..100).map(|i| format!("img{i:03}.png")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut ex = setup(&refs);
    let id = ex.pane().map(|p| p.id).unwrap();
    act(&mut ex, Action::SetView(ViewKind::Grid));
    let fx = ex.update(Msg::Viewport {
        pane: id,
        first_row: 2,
        rows: 3,
        grid_columns: 4,
    });
    let Some(Effect::SetVisiblePreviews(paths)) = fx.first() else {
        panic!("{fx:?}")
    };
    assert_eq!(paths.len(), 12);
    assert_eq!(paths[0], PathBuf::from("/home/me/img008.png"));
}

#[test]
fn multi_folder_open_opens_tabs() {
    let mut ex = setup(&["a", "b", "c.txt"]);
    act(
        &mut ex,
        Action::ClickRow {
            row: 0,
            ctrl: false,
            shift: false,
        },
    );
    act(
        &mut ex,
        Action::ClickRow {
            row: 1,
            ctrl: true,
            shift: false,
        },
    );
    let fx = act(&mut ex, Action::OpenSelection);
    assert_eq!(
        fx.iter()
            .filter(|f| matches!(f, Effect::ListDir { .. }))
            .count(),
        2
    );
    assert_eq!(ex.tabs().len(), 3);
}

#[test]
fn escape_clears_filter_then_selection() {
    let mut ex = setup(&["a.txt", "b.txt"]);
    act(&mut ex, Action::SetFilter("b".into()));
    act(&mut ex, Action::SelectAll);
    key(&mut ex, Key::Escape, Mods::NONE);
    assert_eq!(ex.pane().map(|p| p.len()), Some(2));
    key(&mut ex, Key::Escape, Mods::NONE);
    assert_eq!(ex.pane().map(|p| p.selected_count()), Some(0));
    act(
        &mut ex,
        Action::MoveCursor {
            by: CursorMove::End,
            extend: true,
        },
    );
    assert!(ex.pane().is_some_and(|p| p.selected_count() > 0));
}

fn arb_action() -> impl Strategy<Value = Action> {
    use Action as A;
    prop_oneof![
        Just(A::GoBack),
        Just(A::GoForward),
        Just(A::GoUp),
        Just(A::Refresh),
        Just(A::NewTab),
        Just(A::CloseTab),
        Just(A::ReopenClosedTab),
        Just(A::NextTab),
        Just(A::PrevTab),
        (0usize..5).prop_map(A::SelectTab),
        Just(A::ToggleSplit),
        Just(A::FocusOtherPane),
        Just(A::SetView(ViewKind::Grid)),
        Just(A::SetView(ViewKind::Columns)),
        Just(A::ToggleHidden),
        "[a-c]{0,2}".prop_map(A::SetFilter),
        (0u8..8, any::<bool>()).prop_map(|(m, extend)| A::MoveCursor {
            by: [
                CursorMove::Up,
                CursorMove::Down,
                CursorMove::Left,
                CursorMove::Right,
                CursorMove::PageUp,
                CursorMove::PageDown,
                CursorMove::Home,
                CursorMove::End
            ][m as usize],
            extend
        }),
        (0usize..8, any::<bool>(), any::<bool>()).prop_map(|(row, ctrl, shift)| A::ClickRow {
            row,
            ctrl,
            shift
        }),
        Just(A::SelectAll),
        Just(A::OpenSelection),
        Just(A::QuickLook),
        Just(A::Copy),
        Just(A::Cut),
        Just(A::Paste),
        Just(A::Delete { permanent: true }),
        any::<bool>().prop_map(A::Confirm),
        Just(A::BeginRename),
        Just(A::NewFolder),
        Just(A::OpenPalette),
        Just(A::FocusAddressBar),
        Just(A::FocusSearch),
        Just(A::AddFavorite),
        (0usize..4).prop_map(A::SidebarActivate),
        Just(A::FocusSidebar),
        Just(A::FocusPane),
    ]
}

#[derive(Debug, Clone)]
enum Step {
    Act(Action),
    Key(Key),
    Text(String),
    /// Complete the newest outstanding listing with n entries.
    Feed(usize),
}

fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => arb_action().prop_map(Step::Act),
        2 => prop_oneof![Just(Key::Enter), Just(Key::Escape), Just(Key::Backspace), Just(Key::Up), Just(Key::Down), Just(Key::Tab), Just(Key::Char('X'))].prop_map(Step::Key),
        1 => "[a-z.|]{1,3}".prop_map(Step::Text),
        3 => (0usize..8).prop_map(Step::Feed),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn random_sessions_never_panic_and_keep_invariants(steps in proptest::collection::vec(arb_step(), 0..120)) {
        let (mut ex, mut fx) = Explorer::new("/r/a/b".into());
        let mut outstanding: Vec<u64> = Vec::new();
        let track = |fx: &[Effect], out: &mut Vec<u64>| {
            for f in fx {
                match f {
                    Effect::ListDir { request, .. } | Effect::Search { request, .. } => out.push(*request),
                    Effect::CancelListing { request } => out.retain(|r| r != request),
                    _ => {}
                }
            }
        };
        track(&fx, &mut outstanding);
        for (i, step) in steps.into_iter().enumerate() {
            fx = match step {
                Step::Act(a) => ex.update(Msg::Action(a)),
                Step::Key(k) => ex.update(Msg::Key { chord: Chord::plain(k), now_ms: i as u64 * 300 }),
                Step::Text(t) => ex.update(Msg::Text { text: t, now_ms: i as u64 * 300 }),
                Step::Feed(n) => match outstanding.pop() {
                    Some(request) => {
                        let entries = (0..n).map(|k| e(&if k % 2 == 0 { format!("d{k}") } else { format!(".f{k}.txt") })).collect();
                        ex.update(Msg::ListingBatch { request, entries });
                        ex.update(Msg::ListingDone { request })
                    }
                    None => Vec::new(),
                },
            };
            track(&fx, &mut outstanding);
            prop_assert!(ex.check_invariants().is_ok(), "{:?}", ex.check_invariants());
        }
    }
}
