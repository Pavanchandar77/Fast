use nx_power::snapping::{SnapGeometry, SnapStore, SnapZone, ZoneSet};
use nx_power::types::{Rect, RectF, WindowId};
use std::collections::HashMap;

#[test]
fn test_default_zone_sets() {
    let store = SnapStore::new();

    let split = store.get_zone_set("split-50-50").unwrap();
    assert_eq!(split.zones.len(), 2);

    let three_col = store.get_zone_set("three-column").unwrap();
    assert_eq!(three_col.zones.len(), 3);

    let grid = store.get_zone_set("grid-2x2").unwrap();
    assert_eq!(grid.zones.len(), 4);

    let priority = store.get_zone_set("priority-grid").unwrap();
    assert_eq!(priority.zones.len(), 3);

    let focus = store.get_zone_set("focus-sidecars").unwrap();
    assert_eq!(focus.zones.len(), 3);
}

#[test]
fn test_zone_geometry_projection() {
    let work_area = Rect::new(0, 0, 1920, 1080);
    let zone_left = SnapZone::new(1, "Left", RectF::new(0.0, 0.0, 0.5, 1.0));
    let zone_right = SnapZone::new(2, "Right", RectF::new(0.5, 0.0, 0.5, 1.0));

    let rect_left = zone_left.calculate_rect(&work_area, 10, 8);
    let rect_right = zone_right.calculate_rect(&work_area, 10, 8);

    // Margin shrinks usable width: 1920 - 20 = 1900
    // Usable height: 1080 - 20 = 1060
    assert!(rect_left.x >= 10);
    assert!(rect_right.x > rect_left.right());
    assert_eq!(rect_left.height, rect_right.height);
}

#[test]
fn test_nearest_zone_solver() {
    let mut store = SnapStore::new();
    store.set_active_zone_set("split-50-50").unwrap();

    let work_area = Rect::new(0, 0, 1920, 1080);

    // Cursor at (200, 500) is inside Left Zone
    let left_zone = store
        .find_nearest_zone(200, 500, &work_area, 0, 0, 100)
        .unwrap();
    assert_eq!(left_zone.id, 1);

    // Cursor at (1700, 500) is inside Right Zone
    let right_zone = store
        .find_nearest_zone(1700, 500, &work_area, 0, 0, 100)
        .unwrap();
    assert_eq!(right_zone.id, 2);
}

#[test]
fn test_snap_plan_apply_and_unsnap_identity() {
    let mut store = SnapStore::new();
    let win = WindowId::new(42);
    let orig_bounds = Rect::new(100, 100, 800, 600);
    let geo = SnapGeometry::new(Rect::new(0, 0, 1920, 1080), 8, 4);

    let mut plan = store
        .create_snap_plan(win, "split-50-50", 1, Some(orig_bounds), &geo)
        .unwrap();

    // Dry run
    let dry_run = plan.dry_run();
    assert_eq!(dry_run.window_id, win);
    assert_eq!(dry_run.previous_rect, Some(orig_bounds));

    // Apply snap
    let unsnap = plan.apply(&mut store).unwrap();
    assert_eq!(unsnap.window_id, win);

    // Unsnap revert restores original window bounds
    let restored = unsnap.revert(&mut store).unwrap();
    assert_eq!(restored, Some(orig_bounds));
}

#[test]
fn test_custom_zone_set_registration() {
    let mut store = SnapStore::new();

    let custom_set = ZoneSet::new(
        "ultrawide-4-col",
        "4 Equal Columns",
        vec![
            SnapZone::new(1, "Col 1", RectF::new(0.0, 0.0, 0.25, 1.0)),
            SnapZone::new(2, "Col 2", RectF::new(0.25, 0.0, 0.25, 1.0)),
            SnapZone::new(3, "Col 3", RectF::new(0.5, 0.0, 0.25, 1.0)),
            SnapZone::new(4, "Col 4", RectF::new(0.75, 0.0, 0.25, 1.0)),
        ],
    );

    store.register_zone_set(custom_set);
    store.set_active_zone_set("ultrawide-4-col").unwrap();
    assert_eq!(store.active_zone_set().zones.len(), 4);
}

#[test]
fn test_snap_groups() {
    let mut store = SnapStore::new();

    let mut members = HashMap::new();
    members.insert(WindowId::new(101), 1);
    members.insert(WindowId::new(102), 2);

    let group_id = store.create_snap_group("Pair Split", "split-50-50", members);
    let group = store.get_snap_group(group_id).unwrap();

    assert_eq!(group.name, "Pair Split");
    assert_eq!(group.members.len(), 2);
    assert_eq!(group.members.get(&WindowId::new(101)), Some(&1));
    assert_eq!(group.members.get(&WindowId::new(102)), Some(&2));
}
