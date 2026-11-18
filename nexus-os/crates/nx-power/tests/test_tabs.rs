use nx_power::tabs::{TabGroupManager, WindowTab};
use nx_power::types::{Rect, WindowId};

#[test]
fn test_tab_group_creation_and_tabs() {
    let mut manager = TabGroupManager::new();
    let bounds = Rect::new(50, 50, 1000, 700);

    let tab1 = WindowTab::new(WindowId::new(101), "Terminal");
    let group_id = manager.create_group("Dev Strip", tab1, bounds);

    assert_eq!(manager.len(), 1);
    let group = manager.get_group(group_id).unwrap();
    assert_eq!(group.name, "Dev Strip");
    assert_eq!(group.tab_count(), 1);
    assert_eq!(group.active_window(), Some(WindowId::new(101)));

    // Add 2nd tab
    let tab2 = WindowTab::new(WindowId::new(102), "Editor");
    manager.add_tab(group_id, tab2).unwrap();
    let group = manager.get_group(group_id).unwrap();
    assert_eq!(group.tab_count(), 2);
}

#[test]
fn test_tab_switch_plan_dry_run_and_revert() {
    let mut manager = TabGroupManager::new();
    let bounds = Rect::new(0, 0, 800, 600);

    let tab1 = WindowTab::new(WindowId::new(1), "Tab 1");
    let tab2 = WindowTab::new(WindowId::new(2), "Tab 2");
    let group_id = manager.create_group("Group", tab1, bounds);
    manager.add_tab(group_id, tab2).unwrap();

    let mut plan = manager.switch_tab_plan(group_id, 1).unwrap();

    // Dry run
    let dry_run = plan.dry_run();
    assert_eq!(dry_run.activate_window, WindowId::new(2));
    assert_eq!(dry_run.cloak_windows, vec![WindowId::new(1)]);
    assert_eq!(dry_run.sync_bounds, bounds);

    // Apply
    let revert = plan.apply(&mut manager).unwrap();
    assert_eq!(manager.get_group(group_id).unwrap().active_index, 1);

    // Revert
    revert.revert(&mut manager).unwrap();
    assert_eq!(manager.get_group(group_id).unwrap().active_index, 0);
}

#[test]
fn test_tab_reordering() {
    let mut manager = TabGroupManager::new();
    let bounds = Rect::new(0, 0, 800, 600);

    let tab1 = WindowTab::new(WindowId::new(1), "Tab 1");
    let tab2 = WindowTab::new(WindowId::new(2), "Tab 2");
    let tab3 = WindowTab::new(WindowId::new(3), "Tab 3");

    let group_id = manager.create_group("Group", tab1, bounds);
    manager.add_tab(group_id, tab2).unwrap();
    manager.add_tab(group_id, tab3).unwrap();

    // Initial order: [1, 2, 3]
    // Move tab at index 0 to index 2: [2, 3, 1]
    manager.reorder_tabs(group_id, 0, 2).unwrap();

    let group = manager.get_group(group_id).unwrap();
    assert_eq!(group.tabs[0].window_id, WindowId::new(2));
    assert_eq!(group.tabs[1].window_id, WindowId::new(3));
    assert_eq!(group.tabs[2].window_id, WindowId::new(1));
    assert_eq!(group.active_index, 2);
}

#[test]
fn test_tab_group_merge_and_close() {
    let mut manager = TabGroupManager::new();
    let bounds = Rect::new(0, 0, 800, 600);

    let g1 = manager.create_group("G1", WindowTab::new(WindowId::new(10), "G1-T1"), bounds);
    let g2 = manager.create_group("G2", WindowTab::new(WindowId::new(20), "G2-T1"), bounds);

    assert_eq!(manager.len(), 2);

    // Merge g2 into g1
    manager.merge_groups(g1, g2).unwrap();
    assert_eq!(manager.len(), 1);
    assert_eq!(manager.get_group(g1).unwrap().tab_count(), 2);
    assert_eq!(manager.group_for_window(WindowId::new(20)), Some(g1));

    // Close group
    let closed = manager.close_group(g1).unwrap();
    assert_eq!(closed.tab_count(), 2);
    assert_eq!(manager.len(), 0);
    assert_eq!(manager.group_for_window(WindowId::new(10)), None);
}
