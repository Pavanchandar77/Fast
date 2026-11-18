use nx_wm::geometry::{Direction, Point, Rect};
use nx_wm::keybinding::{
    parse_action, parse_key_combination, Action, Key, KeyCombination, KeybindingMap, ModKey,
    Modifiers,
};
use nx_wm::layout::{compute_layout, LayoutKind, LayoutParams};
use nx_wm::monitor::Monitor;
use nx_wm::plan::WindowMutation;
use nx_wm::rules::{RuleAction, RuleEngine, RuleMatcher, WindowRule};
use nx_wm::snapping::{compute_snap_rect, SnapZone};
use nx_wm::state::WindowManagerModel;
use nx_wm::tree::OrderedWindowTree;
use nx_wm::types::{MonitorId, WindowId, WindowInfo, WorkspaceId};
use proptest::prelude::*;

fn make_window_info(id: u64, title: &str, class: &str, is_visible: bool) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        title: title.to_string(),
        class_name: class.to_string(),
        process_name: "test.exe".to_string(),
        rect: Rect::new(100, 100, 800, 600),
        is_visible,
        is_minimized: false,
        is_cloaked: false,
        is_dialog_or_popup: false,
        style: 0x14CF0000,
        ex_style: 0x00000100,
    }
}

// ============================================================================
// 1. GEOMETRY TESTS
// ============================================================================

#[test]
fn test_rect_basic_properties() {
    let r = Rect::new(10, 20, 100, 200);
    assert_eq!(r.right(), 110);
    assert_eq!(r.bottom(), 220);
    assert_eq!(r.center(), Point::new(60, 120));
    assert!(r.contains_point(10, 20));
    assert!(r.contains_point(109, 219));
    assert!(!r.contains_point(110, 220));
    assert!(!r.contains_point(9, 20));
}

#[test]
fn test_rect_intersection() {
    let r1 = Rect::new(0, 0, 100, 100);
    let r2 = Rect::new(50, 50, 100, 100);
    assert!(r1.intersects(&r2));

    let inter = r1.intersection(&r2).unwrap();
    assert_eq!(inter, Rect::new(50, 50, 50, 50));

    let r3 = Rect::new(200, 200, 50, 50);
    assert!(!r1.intersects(&r3));
    assert_eq!(r1.intersection(&r3), None);
}

#[test]
fn test_rect_shrink_and_gap_safety() {
    let r = Rect::new(10, 10, 100, 100);
    let shrunk = r.shrink(5);
    assert_eq!(shrunk, Rect::new(15, 15, 90, 90));

    // Shrinking by gap larger than rect dimensions returns original without underflow
    let tiny = Rect::new(0, 0, 10, 10);
    let safe_shrunk = tiny.shrink(10);
    assert_eq!(safe_shrunk, tiny);
}

#[test]
fn test_directional_distance_cardinal() {
    let center = Rect::new(100, 100, 100, 100);
    let left = Rect::new(0, 100, 80, 100);
    let right = Rect::new(220, 100, 80, 100);
    let up = Rect::new(100, 0, 100, 80);
    let down = Rect::new(100, 220, 100, 80);

    assert!(center.directional_distance(&left, Direction::Left).is_some());
    assert!(center.directional_distance(&left, Direction::Right).is_none());
    assert!(center.directional_distance(&right, Direction::Right).is_some());
    assert!(center.directional_distance(&up, Direction::Up).is_some());
    assert!(center.directional_distance(&down, Direction::Down).is_some());
}

// ============================================================================
// 2. LAYOUT ENGINE TESTS
// ============================================================================

#[test]
fn test_layout_dwindle_single_window() {
    let work_area = Rect::new(0, 30, 1920, 1050);
    let params = LayoutParams::default();
    let windows = vec![WindowId(1)];

    let res = compute_layout(LayoutKind::Dwindle, &windows, work_area, &params);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].1, work_area.shrink(params.outer_gap));
}

#[test]
fn test_layout_dwindle_two_windows_horizontal_split() {
    let work_area = Rect::new(0, 0, 1000, 600);
    let params = LayoutParams {
        inner_gap: 10,
        outer_gap: 0,
        master_ratio: 0.5,
        ..Default::default()
    };
    let windows = vec![WindowId(1), WindowId(2)];

    let res = compute_layout(LayoutKind::Dwindle, &windows, work_area, &params);
    assert_eq!(res.len(), 2);
    let r1 = res[0].1;
    let r2 = res[1].1;

    // Both inside work area
    assert!(work_area.contains_rect(&r1));
    assert!(work_area.contains_rect(&r2));
    // No intersection
    assert!(!r1.intersects(&r2));
    // Gap between them equals inner_gap
    assert_eq!(r2.x - r1.right(), 10);
    assert_eq!(r1.height, work_area.height);
    assert_eq!(r2.height, work_area.height);
}

#[test]
fn test_layout_dwindle_four_windows_spiral() {
    let work_area = Rect::new(0, 0, 1200, 800);
    let params = LayoutParams {
        inner_gap: 8,
        outer_gap: 12,
        master_ratio: 0.5,
        ..Default::default()
    };
    let windows = vec![WindowId(1), WindowId(2), WindowId(3), WindowId(4)];

    let res = compute_layout(LayoutKind::Dwindle, &windows, work_area, &params);
    assert_eq!(res.len(), 4);

    let usable = work_area.shrink(params.outer_gap);
    for i in 0..4 {
        assert!(usable.contains_rect(&res[i].1));
        for j in (i + 1)..4 {
            assert!(!res[i].1.intersects(&res[j].1));
        }
    }
}

#[test]
fn test_layout_master_stack_distribution() {
    let work_area = Rect::new(0, 0, 1000, 600);
    let params = LayoutParams {
        inner_gap: 10,
        outer_gap: 0,
        master_ratio: 0.6,
        master_count: 1,
        ..Default::default()
    };
    let windows = vec![WindowId(1), WindowId(2), WindowId(3)];

    let res = compute_layout(LayoutKind::MasterStack, &windows, work_area, &params);
    assert_eq!(res.len(), 3);

    let master = res[0].1;
    let stack1 = res[1].1;
    let stack2 = res[2].1;

    assert!(work_area.contains_rect(&master));
    assert!(work_area.contains_rect(&stack1));
    assert!(work_area.contains_rect(&stack2));

    // Master on the left, stack on the right
    assert_eq!(master.x, 0);
    assert!(stack1.x > master.right());
    assert_eq!(stack1.x - master.right(), 10);
    assert_eq!(stack1.x, stack2.x);

    // Stack1 above stack2
    assert_eq!(stack2.y - stack1.bottom(), 10);
    // Total stack height fills work area
    assert_eq!(stack2.bottom(), work_area.height as i32);
}

#[test]
fn test_layout_scrolling_ribbon() {
    let work_area = Rect::new(0, 0, 1920, 1080);
    let params = LayoutParams {
        inner_gap: 16,
        outer_gap: 20,
        master_ratio: 0.5,
        master_count: 1,
        scroll_column_width: 800,
        scroll_active_index: 1, // Window 2 is active and centered
    };
    let windows = vec![WindowId(1), WindowId(2), WindowId(3)];

    let res = compute_layout(LayoutKind::Scrolling, &windows, work_area, &params);
    assert_eq!(res.len(), 3);

    let usable = work_area.shrink(params.outer_gap);
    let active_rect = res[1].1;
    let expected_center_x = usable.x + (usable.width as i32 - 800) / 2;
    assert_eq!(active_rect.x, expected_center_x);

    // Window 1 is to the left of window 2
    let win1_rect = res[0].1;
    assert_eq!(active_rect.x - win1_rect.right(), 16);

    // Window 3 is to the right of window 2
    let win3_rect = res[2].1;
    assert_eq!(win3_rect.x - active_rect.right(), 16);
}

#[test]
fn test_layout_monocle() {
    let work_area = Rect::new(0, 0, 1920, 1080);
    let params = LayoutParams::default();
    let windows = vec![WindowId(1), WindowId(2), WindowId(3)];

    let res = compute_layout(LayoutKind::Monocle, &windows, work_area, &params);
    assert_eq!(res.len(), 3);
    let usable = work_area.shrink(params.outer_gap);
    for (_, rect) in res {
        assert_eq!(rect, usable);
    }
}

// ============================================================================
// 3. ORDERED WINDOW TREE TESTS
// ============================================================================

#[test]
fn test_tree_stable_insertion_order() {
    let mut tree = OrderedWindowTree::new();
    tree.push(WindowId(10));
    tree.push(WindowId(20));
    tree.push(WindowId(30));

    assert_eq!(tree.as_slice(), &[WindowId(10), WindowId(20), WindowId(30)]);
}

#[test]
fn test_tree_mru_focus_history() {
    let mut tree = OrderedWindowTree::new();
    tree.push(WindowId(1));
    tree.push(WindowId(2));
    tree.push(WindowId(3));

    assert_eq!(tree.active_window(), Some(WindowId(3)));
    tree.set_active(WindowId(1));
    assert_eq!(tree.active_window(), Some(WindowId(1)));

    // Removing active window falls back to previous MRU window (3)
    tree.remove(WindowId(1));
    assert_eq!(tree.active_window(), Some(WindowId(3)));
}

#[test]
fn test_tree_cycle_active() {
    let mut tree = OrderedWindowTree::new();
    tree.push(WindowId(1));
    tree.push(WindowId(2));
    tree.push(WindowId(3));
    tree.set_active(WindowId(1));

    assert_eq!(tree.cycle_active(true), Some(WindowId(2)));
    assert_eq!(tree.cycle_active(true), Some(WindowId(3)));
    assert_eq!(tree.cycle_active(true), Some(WindowId(1))); // wrap forward
    assert_eq!(tree.cycle_active(false), Some(WindowId(3))); // wrap backward
}

#[test]
fn test_tree_swap_and_move() {
    let mut tree = OrderedWindowTree::new();
    tree.push(WindowId(1));
    tree.push(WindowId(2));
    tree.push(WindowId(3));

    tree.swap_windows(WindowId(1), WindowId(3));
    assert_eq!(tree.as_slice(), &[WindowId(3), WindowId(2), WindowId(1)]);

    tree.move_window(WindowId(2), 0);
    assert_eq!(tree.as_slice(), &[WindowId(2), WindowId(3), WindowId(1)]);
}

// ============================================================================
// 4. WORKSPACE HIDE / SHOW & ISOLATION
// ============================================================================

#[test]
fn test_workspace_switch_cloaking_and_restoration() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    // Register win 1 on workspace 1
    let (p1, _) = wm.on_window_show(make_window_info(1, "Editor", "Code", true));
    assert!(p1.cloaked_targets().get(&WindowId(1)) == Some(&false));

    // Switch to workspace 2
    let (p_switch, _) = wm.switch_workspace(WorkspaceId(2));
    assert_eq!(wm.active_workspace_id(), WorkspaceId(2));
    // Window 1 on workspace 1 must be cloaked & hidden
    assert_eq!(p_switch.cloaked_targets().get(&WindowId(1)), Some(&true));
    assert_eq!(p_switch.visibility_targets().get(&WindowId(1)), Some(&false));
    assert!(wm.window(WindowId(1)).unwrap().is_cloaked);

    // Register win 2 on workspace 2
    let (p2, _) = wm.on_window_show(make_window_info(2, "Browser", "Firefox", true));
    assert_eq!(p2.cloaked_targets().get(&WindowId(2)), Some(&false));

    // Switch back to workspace 1
    let (p_back, _) = wm.switch_workspace(WorkspaceId(1));
    // Window 2 must be cloaked, window 1 must be uncloaked & visible
    assert_eq!(p_back.cloaked_targets().get(&WindowId(2)), Some(&true));
    assert_eq!(p_back.cloaked_targets().get(&WindowId(1)), Some(&false));
    assert_eq!(p_back.visibility_targets().get(&WindowId(1)), Some(&true));
}

#[test]
fn test_workspace_switch_restores_last_focused() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    wm.on_window_show(make_window_info(1, "Doc1", "Editor", true));
    wm.on_window_show(make_window_info(2, "Doc2", "Editor", true));
    // Focus window 1 specifically
    wm.focus_window(WindowId(1));
    assert_eq!(wm.active_window(), Some(WindowId(1)));

    // Switch away to workspace 2 and back
    wm.switch_workspace(WorkspaceId(2));
    let (plan_back, _) = wm.switch_workspace(WorkspaceId(1));

    // Focus target must be window 1 (last focused)
    assert_eq!(plan_back.focus_target(), Some(WindowId(1)));
    assert_eq!(wm.active_window(), Some(WindowId(1)));
}

#[test]
fn test_move_window_to_inactive_workspace() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));
    wm.on_window_show(make_window_info(1, "Terminal", "Alacritty", true));

    let (plan, _) = wm.move_window_to_workspace(WindowId(1), WorkspaceId(3));
    assert_eq!(plan.cloaked_targets().get(&WindowId(1)), Some(&true));
    assert_eq!(plan.visibility_targets().get(&WindowId(1)), Some(&false));

    let win = wm.window(WindowId(1)).unwrap();
    assert_eq!(win.workspace_id, WorkspaceId(3));
    assert!(win.is_cloaked);
    assert!(!wm.workspace(WorkspaceId(1)).unwrap().contains_window(WindowId(1)));
    assert!(wm.workspace(WorkspaceId(3)).unwrap().contains_window(WindowId(1)));
}

// ============================================================================
// 5. WINDOW REGISTRATION & RULES (AUDIT BUGFIXES)
// ============================================================================

#[test]
fn test_register_on_show_bugfix() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    // Bug in audit: WindowCreated when invisible was discarded and never tiled on show!
    let info_hidden = make_window_info(100, "App", "AppClass", false);
    let (p_create, trans_create) = wm.on_window_created(info_hidden.clone());
    assert!(p_create.is_empty());
    assert!(trans_create.is_none());

    // Window becomes visible -> WindowShow triggers
    let mut info_shown = info_hidden;
    info_shown.is_visible = true;
    let (p_show, trans_show) = wm.on_window_show(info_shown);

    assert!(!p_show.is_empty());
    assert!(trans_show.is_some());
    assert_eq!(p_show.visibility_targets().get(&WindowId(100)), Some(&true));
    assert_eq!(p_show.cloaked_targets().get(&WindowId(100)), Some(&false));
    assert!(wm.workspace(WorkspaceId(1)).unwrap().contains_window(WindowId(100)));
}

#[test]
fn test_window_rule_dialog_auto_float() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    let mut dialog_info = make_window_info(50, "Open File", "#32770", true);
    dialog_info.is_dialog_or_popup = true;

    wm.on_window_show(dialog_info);
    let win = wm.window(WindowId(50)).unwrap();
    assert!(win.is_floating);
    assert!(!win.is_tileable());
}

#[test]
fn test_window_rule_pip_compact_size() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    let pip_info = make_window_info(51, "YouTube - Picture in picture", "Chrome", true);
    wm.on_window_show(pip_info);

    let win = wm.window(WindowId(51)).unwrap();
    assert!(win.is_floating);
    assert_eq!(win.floating_rect.width, 480);
    assert_eq!(win.floating_rect.height, 270);
}

#[test]
fn test_window_rule_per_app_workspace() {
    let mut rules = RuleEngine::new();
    rules.add_rule(WindowRule::new(
        vec![RuleMatcher::ProcessEquals("slack.exe".into())],
        vec![RuleAction::AssignWorkspace(4)],
    ));

    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050))
        .with_rules(rules);

    let mut slack_info = make_window_info(77, "Slack", "SlackMain", true);
    slack_info.process_name = "slack.exe".into();

    let (plan, _) = wm.on_window_show(slack_info);
    let win = wm.window(WindowId(77)).unwrap();
    assert_eq!(win.workspace_id, WorkspaceId(4));
    // Since active workspace is 1, window on ws 4 must be cloaked & hidden
    assert_eq!(plan.cloaked_targets().get(&WindowId(77)), Some(&true));
    assert!(win.is_cloaked);
}

#[test]
fn test_auto_exclude_fullscreen_games() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));

    let mut game_info = make_window_info(99, "Cyberpunk 2077", "REDengine", true);
    game_info.process_name = "cyberpunk2077.exe".into();
    game_info.rect = Rect::new(0, 0, 1920, 1080);
    game_info.style = 0x80000000; // borderless fullscreen

    wm.on_window_show(game_info);
    let win = wm.window(WindowId(99)).unwrap();
    assert!(win.is_auto_excluded);
    assert!(!win.is_tileable());
}

// ============================================================================
// 6. FOCUS MODEL & NAVIGATION
// ============================================================================

#[test]
fn test_directional_focus_left_right() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.on_window_show(make_window_info(1, "Left", "T", true));
    wm.on_window_show(make_window_info(2, "Right", "T", true));

    // Initially window 2 is active
    wm.focus_window(WindowId(2));
    assert_eq!(wm.active_window(), Some(WindowId(2)));

    // Move focus left -> should focus window 1
    let (plan_left, _) = wm.focus_direction(Direction::Left);
    assert_eq!(plan_left.focus_target(), Some(WindowId(1)));
    assert_eq!(wm.active_window(), Some(WindowId(1)));

    // Move focus right -> should focus window 2
    let (plan_right, _) = wm.focus_direction(Direction::Right);
    assert_eq!(plan_right.focus_target(), Some(WindowId(2)));
    assert_eq!(wm.active_window(), Some(WindowId(2)));
}

#[test]
fn test_move_direction_swaps_windows() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.on_window_show(make_window_info(1, "Win1", "T", true));
    wm.on_window_show(make_window_info(2, "Win2", "T", true));

    wm.focus_window(WindowId(1));
    assert_eq!(wm.active_workspace().tree.as_slice(), &[WindowId(1), WindowId(2)]);

    // Move window 1 right -> swaps positions with window 2
    let (plan, trans) = wm.move_direction(Direction::Right);
    assert!(!plan.is_empty());
    assert!(trans.is_some());
    assert_eq!(wm.active_workspace().tree.as_slice(), &[WindowId(2), WindowId(1)]);
}

#[test]
fn test_follow_mouse_focus() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.set_follow_mouse(true);

    wm.on_window_show(make_window_info(1, "Win1", "T", true));
    wm.on_window_show(make_window_info(2, "Win2", "T", true));
    wm.focus_window(WindowId(1));

    let win2_rect = wm.window(WindowId(2)).unwrap().target_rect;
    let (plan, _) = wm.on_pointer_moved(win2_rect.center());
    assert_eq!(plan.focus_target(), Some(WindowId(2)));
    assert_eq!(wm.active_window(), Some(WindowId(2)));
}

// ============================================================================
// 7. MULTI-MONITOR ADD / REPOSITION / UNPLUG (MIGRATION)
// ============================================================================

#[test]
fn test_multi_monitor_add_and_workspaces() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080));
    let sec_mon = Monitor::new(
        MonitorId(1),
        "HDMI-1",
        Rect::new(1920, 0, 1920, 1080),
        Rect::new(1920, 0, 1920, 1080),
        1.0,
        WorkspaceId(2),
    );

    wm.add_monitor(sec_mon);
    assert_eq!(wm.monitors().len(), 2);
    assert_eq!(wm.monitor(MonitorId(1)).unwrap().name, "HDMI-1");
}

#[test]
fn test_multi_monitor_unplug_never_loses_windows() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080));
    let sec_mon = Monitor::new(
        MonitorId(1),
        "HDMI-1",
        Rect::new(1920, 0, 1920, 1080),
        Rect::new(1920, 0, 1920, 1080),
        1.0,
        WorkspaceId(2),
    );
    wm.add_monitor(sec_mon);

    // Put window 10 on workspace 2 (on monitor 1)
    if let Some(ws2) = wm.workspace_mut(WorkspaceId(2)) {
        ws2.monitor_id = MonitorId(1);
    }
    wm.switch_workspace(WorkspaceId(2));
    wm.on_window_show(make_window_info(10, "SecMonApp", "Class", true));
    assert_eq!(wm.window(WindowId(10)).unwrap().monitor_id, MonitorId(1));

    // Unplug monitor 1!
    let (plan, trans) = wm.remove_monitor(MonitorId(1));
    assert_eq!(wm.monitors().len(), 1);

    // Window 10 must still exist and be migrated to monitor 0
    let win10 = wm.window(WindowId(10)).unwrap();
    assert_eq!(win10.monitor_id, MonitorId(0));
    assert!(wm.workspace(WorkspaceId(2)).unwrap().contains_window(WindowId(10)));
    assert!(!plan.is_empty());
    assert!(trans.is_some());
}

// ============================================================================
// 8. KEYBINDINGS, WINDOWS PASSTHROUGH & WIN-KEY-UP MASKING
// ============================================================================

#[test]
fn test_keybinding_parsing() {
    let combo = parse_key_combination("Return", ModKey::Super).unwrap();
    assert_eq!(combo.modifiers, Modifiers::NONE.with_super());
    assert_eq!(combo.key, Key::Return);

    let combo2 = parse_key_combination("Ctrl+Alt+Shift+Esc", ModKey::Super).unwrap();
    assert_eq!(combo2.modifiers, Modifiers::NONE.with_ctrl().with_alt().with_shift());
    assert_eq!(combo2.key, Key::Escape);

    let act = parse_action("window:close").unwrap();
    assert_eq!(act, Action::CloseWindow);

    let act2 = parse_action("workspace:5").unwrap();
    assert_eq!(act2, Action::FocusWorkspace(5));
}

#[test]
fn test_windows_reserved_shortcuts_protection() {
    // Audit problem: previous code swallowed Win+Space, Win+Q, Win+1..9 blindly
    assert!(KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_super(),
        Key::Space
    ));
    assert!(KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_super(),
        Key::Char('Q')
    ));
    assert!(KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_super(),
        Key::Char('1')
    ));
    assert!(KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_alt(),
        Key::Tab
    ));
    assert!(KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_super(),
        Key::Char('Z')
    ));

    // Super+V is NOT reserved by Windows shell defaults for taskbar apps (it's clipboard history, but distinct from Win+1..9)
    assert!(!KeybindingMap::is_windows_reserved_shortcut(
        Modifiers::NONE.with_super(),
        Key::Char('V')
    ));
}

#[test]
fn test_win_key_up_masking_planned() {
    let mut map = KeybindingMap::new(ModKey::Super);
    map.bind(
        KeyCombination::new(Modifiers::NONE.with_super(), Key::Char('Q')),
        Action::CloseWindow,
    );

    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080))
        .with_keybindings(map);

    let res = wm.handle_key(Modifiers::NONE.with_super(), Key::Char('Q'));
    assert!(res.is_some());
    let (_, plan, _) = res.unwrap();

    // Must emit MaskWinKeyUp mutation to prevent Windows Start Menu activation
    assert!(plan.has_mask_win_key_up());
}

// ============================================================================
// 9. SNAPPING & FULLSCREEN
// ============================================================================

#[test]
fn test_snap_zones_computation() {
    let work_area = Rect::new(0, 0, 1920, 1080);
    let left_half = compute_snap_rect(SnapZone::LeftHalf, work_area, 0);
    assert_eq!(left_half, Rect::new(0, 0, 960, 1080));

    let right_half = compute_snap_rect(SnapZone::RightHalf, work_area, 0);
    assert_eq!(right_half, Rect::new(960, 0, 960, 1080));

    let top_half = compute_snap_rect(SnapZone::TopHalf, work_area, 0);
    assert_eq!(top_half, Rect::new(0, 0, 1920, 540));
}

#[test]
fn test_fullscreen_toggle_state_machine() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));
    wm.on_window_show(make_window_info(1, "Video", "Player", true));

    let original_rect = wm.window(WindowId(1)).unwrap().target_rect;

    // Enter fullscreen
    let (p_enter, _) = wm.toggle_fullscreen(WindowId(1));
    assert!(wm.window(WindowId(1)).unwrap().is_fullscreen);
    assert_eq!(
        p_enter.position_targets().get(&WindowId(1)),
        Some(&Rect::new(0, 0, 1920, 1080))
    );

    // Exit fullscreen
    let (p_exit, _) = wm.toggle_fullscreen(WindowId(1));
    assert!(!wm.window(WindowId(1)).unwrap().is_fullscreen);
    assert_eq!(
        p_exit.position_targets().get(&WindowId(1)),
        Some(&original_rect)
    );
}

// ============================================================================
// 10. DRY-RUN APPLY / REVERT IDENTITY & EXIT RESTORATION
// ============================================================================

#[test]
fn test_dry_run_apply_revert_identity() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));
    wm.on_window_show(make_window_info(1, "W1", "C", true));
    wm.on_window_show(make_window_info(2, "W2", "C", true));

    // Capture initial snapshot
    let initial_snapshots = wm.create_snapshots();

    // Execute state mutation: switch to workspace 2
    let (plan, _) = wm.switch_workspace(WorkspaceId(2));

    // Compute inverse plan using snapshots
    let inverse_plan = plan.compute_inverse(&initial_snapshots);

    // Apply inverse plan on in-memory window states
    for m in &inverse_plan.mutations {
        match m {
            WindowMutation::SetPosition { window_id, rect, .. } => {
                if let Some(win) = wm.window_mut(*window_id) {
                    win.target_rect = *rect;
                }
            }
            WindowMutation::SetVisibility { window_id, visible } => {
                if let Some(win) = wm.window_mut(*window_id) {
                    win.is_visible = *visible;
                }
            }
            WindowMutation::SetCloak { window_id, cloaked } => {
                if let Some(win) = wm.window_mut(*window_id) {
                    win.is_cloaked = *cloaked;
                }
            }
            _ => {}
        }
    }

    // Verify identity: all windows have returned to their exact initial attributes
    for (&id, initial) in &initial_snapshots {
        let win = wm.window(id).unwrap();
        assert_eq!(win.target_rect, initial.rect);
        assert_eq!(win.is_visible, initial.visible);
        assert_eq!(win.is_cloaked, initial.cloaked);
    }
}

#[test]
fn test_exit_plan_restores_all_cloaked_windows() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 30, 1920, 1050));
    wm.on_window_show(make_window_info(1, "W1", "C", true));
    wm.switch_workspace(WorkspaceId(2));
    wm.on_window_show(make_window_info(2, "W2", "C", true));

    // Win 1 is on workspace 1 (cloaked)
    assert!(wm.window(WindowId(1)).unwrap().is_cloaked);

    // When exiting Nexus, create_exit_plan must uncloak and show EVERY window
    let exit_plan = wm.create_exit_plan();
    assert_eq!(exit_plan.cloaked_targets().get(&WindowId(1)), Some(&false));
    assert_eq!(exit_plan.cloaked_targets().get(&WindowId(2)), Some(&false));
    assert_eq!(exit_plan.visibility_targets().get(&WindowId(1)), Some(&true));
    assert_eq!(exit_plan.visibility_targets().get(&WindowId(2)), Some(&true));
}

// ============================================================================
// 11. PROPTESTS FOR LAYOUT INVARIANTS (NO OVERLAPS, STRICTLY INSIDE BOUNDS)
// ============================================================================

proptest! {
    #[test]
    fn proptest_dwindle_invariants(
        num_windows in 1usize..15usize,
        w in 600u32..2560u32,
        h in 400u32..1440u32,
        gap in 0u32..16u32,
        master_ratio in 0.2f32..0.8f32,
    ) {
        let work_area = Rect::new(0, 0, w, h);
        let params = LayoutParams {
            inner_gap: gap,
            outer_gap: gap,
            master_ratio,
            ..Default::default()
        };
        let windows: Vec<WindowId> = (1..=num_windows).map(|i| WindowId(i as u64)).collect();
        let res = compute_layout(LayoutKind::Dwindle, &windows, work_area, &params);

        prop_assert_eq!(res.len(), num_windows);
        let usable = work_area.shrink(params.outer_gap);

        for (i, (_, r1)) in res.iter().enumerate() {
            prop_assert!(usable.contains_rect(r1), "Rect {:?} outside usable {:?}", r1, usable);
            prop_assert!(r1.width > 0);
            prop_assert!(r1.height > 0);

            for (_, r2) in res.iter().skip(i + 1) {
                prop_assert!(!r1.intersects(r2), "Rect {:?} intersects with {:?}", r1, r2);
            }
        }
    }

    #[test]
    fn proptest_master_stack_invariants(
        num_windows in 1usize..15usize,
        w in 600u32..2560u32,
        h in 400u32..1440u32,
        gap in 0u32..16u32,
        master_ratio in 0.2f32..0.8f32,
    ) {
        let work_area = Rect::new(0, 0, w, h);
        let params = LayoutParams {
            inner_gap: gap,
            outer_gap: gap,
            master_ratio,
            master_count: 1,
            ..Default::default()
        };
        let windows: Vec<WindowId> = (1..=num_windows).map(|i| WindowId(i as u64)).collect();
        let res = compute_layout(LayoutKind::MasterStack, &windows, work_area, &params);

        prop_assert_eq!(res.len(), num_windows);
        let usable = work_area.shrink(params.outer_gap);

        for (i, (_, r1)) in res.iter().enumerate() {
            prop_assert!(usable.contains_rect(r1), "Rect {:?} outside usable {:?}", r1, usable);
            prop_assert!(r1.width > 0);
            prop_assert!(r1.height > 0);

            for (_, r2) in res.iter().skip(i + 1) {
                prop_assert!(!r1.intersects(r2), "Rect {:?} intersects with {:?}", r1, r2);
            }
        }
    }

    #[test]
    fn proptest_scrolling_invariants(
        num_windows in 1usize..10usize,
        w in 800u32..2560u32,
        h in 600u32..1440u32,
        gap in 4u32..20u32,
        active_idx in 0usize..10usize,
    ) {
        let work_area = Rect::new(0, 0, w, h);
        let params = LayoutParams {
            inner_gap: gap,
            outer_gap: gap,
            master_ratio: 0.5,
            master_count: 1,
            scroll_column_width: 600,
            scroll_active_index: active_idx,
        };
        let windows: Vec<WindowId> = (1..=num_windows).map(|i| WindowId(i as u64)).collect();
        let res = compute_layout(LayoutKind::Scrolling, &windows, work_area, &params);

        prop_assert_eq!(res.len(), num_windows);
        let usable = work_area.shrink(params.outer_gap);

        for (_, r) in &res {
            prop_assert_eq!(r.y, usable.y);
            prop_assert_eq!(r.height, usable.height);
            prop_assert!(r.width > 0);
        }
    }
}

// ============================================================================
// 12. ADDITIONAL INTEGRATION & EDGE CASE SCENARIOS
// ============================================================================

#[test]
fn test_layout_empty_windows() {
    let work_area = Rect::new(0, 0, 1920, 1080);
    let params = LayoutParams::default();
    let res = compute_layout(LayoutKind::Dwindle, &[], work_area, &params);
    assert!(res.is_empty());
}

#[test]
fn test_cycle_layout_order() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080));
    assert_eq!(wm.active_workspace().layout, LayoutKind::Dwindle);

    wm.handle_key(
        Modifiers::NONE.with_super(),
        Key::Space,
    ); // not cycle

    let mut map = KeybindingMap::new(ModKey::Super);
    map.bind(
        KeyCombination::new(Modifiers::NONE.with_super(), Key::Char('L')),
        Action::CycleLayout,
    );
    let mut wm = wm.with_keybindings(map);

    wm.handle_key(Modifiers::NONE.with_super(), Key::Char('L'));
    assert_eq!(wm.active_workspace().layout, LayoutKind::MasterStack);

    wm.handle_key(Modifiers::NONE.with_super(), Key::Char('L'));
    assert_eq!(wm.active_workspace().layout, LayoutKind::Scrolling);

    wm.handle_key(Modifiers::NONE.with_super(), Key::Char('L'));
    assert_eq!(wm.active_workspace().layout, LayoutKind::Monocle);

    wm.handle_key(Modifiers::NONE.with_super(), Key::Char('L'));
    assert_eq!(wm.active_workspace().layout, LayoutKind::Floating);

    wm.handle_key(Modifiers::NONE.with_super(), Key::Char('L'));
    assert_eq!(wm.active_workspace().layout, LayoutKind::Dwindle);
}

#[test]
fn test_directional_focus_none_when_blocked() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.on_window_show(make_window_info(1, "Left", "T", true));
    wm.on_window_show(make_window_info(2, "Right", "T", true));

    // Focus left window
    wm.focus_window(WindowId(1));
    // Moving further left should find no candidate
    let (plan, _) = wm.focus_direction(Direction::Left);
    assert!(plan.is_empty());
    assert_eq!(wm.active_window(), Some(WindowId(1)));
}

#[test]
fn test_pointer_moved_disabled_follow_mouse() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.set_follow_mouse(false);

    wm.on_window_show(make_window_info(1, "W1", "T", true));
    wm.on_window_show(make_window_info(2, "W2", "T", true));
    wm.focus_window(WindowId(1));

    let win2_rect = wm.window(WindowId(2)).unwrap().target_rect;
    let (plan, _) = wm.on_pointer_moved(win2_rect.center());
    assert!(plan.is_empty());
    assert_eq!(wm.active_window(), Some(WindowId(1)));
}

#[test]
fn test_monitor_reposition_retiles_workspace() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080));
    wm.on_window_show(make_window_info(1, "W1", "T", true));

    // Shrink work area (e.g. 50px top bar enabled)
    let new_work_area = Rect::new(0, 50, 1920, 1030);
    let (plan, trans) = wm.reposition_monitor(MonitorId(0), Rect::new(0, 0, 1920, 1080), new_work_area, 1.0);

    assert!(!plan.is_empty());
    assert!(trans.is_some());
    let new_rect = wm.window(WindowId(1)).unwrap().target_rect;
    assert!(new_work_area.contains_rect(&new_rect));
}

#[test]
fn test_window_rule_conjunction_matching() {
    let mut rules = RuleEngine::new();
    rules.add_rule(WindowRule::new(
        vec![
            RuleMatcher::TitleContains("Debug".into()),
            RuleMatcher::ClassEquals("Console".into()),
        ],
        vec![RuleAction::Float],
    ));

    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080))
        .with_rules(rules);

    // Title matches, class doesn't -> doesn't float
    let info1 = make_window_info(1, "Debug Tool", "Other", true);
    wm.on_window_show(info1);
    assert!(!wm.window(WindowId(1)).unwrap().is_floating);

    // Both match -> floats!
    let info2 = make_window_info(2, "Debug Output", "Console", true);
    wm.on_window_show(info2);
    assert!(wm.window(WindowId(2)).unwrap().is_floating);
}

#[test]
fn test_snap_thirds_computation() {
    let work_area = Rect::new(0, 0, 1800, 900);
    let r_third1 = compute_snap_rect(SnapZone::LeftThird, work_area, 0);
    assert_eq!(r_third1, Rect::new(0, 0, 600, 900));

    let r_third2 = compute_snap_rect(SnapZone::CenterThird, work_area, 0);
    assert_eq!(r_third2, Rect::new(600, 0, 600, 900));

    let r_two_thirds = compute_snap_rect(SnapZone::LeftTwoThirds, work_area, 0);
    assert_eq!(r_two_thirds, Rect::new(0, 0, 1200, 900));
}

#[test]
fn test_window_destroyed_focus_reversion() {
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600));
    wm.on_window_show(make_window_info(1, "W1", "T", true));
    wm.on_window_show(make_window_info(2, "W2", "T", true));
    assert_eq!(wm.active_window(), Some(WindowId(2)));

    // Destroy active window 2 -> focus falls back to window 1
    let (plan, trans) = wm.on_window_destroyed(WindowId(2));
    assert_eq!(wm.active_window(), Some(WindowId(1)));
    assert!(!plan.is_empty());
    assert!(trans.is_some());
}

#[test]
fn test_close_window_action_emits_visibility_mutation() {
    let mut map = KeybindingMap::new(ModKey::Super);
    map.bind(
        KeyCombination::new(Modifiers::NONE.with_super(), Key::Char('Q')),
        Action::CloseWindow,
    );
    let mut wm = WindowManagerModel::new(Rect::new(0, 0, 1000, 600), Rect::new(0, 0, 1000, 600))
        .with_keybindings(map);

    wm.on_window_show(make_window_info(1, "W1", "T", true));
    let (_, plan, _) = wm.handle_key(Modifiers::NONE.with_super(), Key::Char('Q')).unwrap();

    assert_eq!(plan.visibility_targets().get(&WindowId(1)), Some(&false));
}

