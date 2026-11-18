use nx_power::always_on_top::{AlwaysOnTopAction, AlwaysOnTopPlan, AlwaysOnTopStore};
use nx_power::types::WindowId;

#[test]
fn test_always_on_top_dry_run() {
    let store = AlwaysOnTopStore::new();
    let win = WindowId::new(0x100);

    let plan = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Pin);
    let dry_run = plan.dry_run();

    assert_eq!(dry_run.window_id, win);
    assert!(!dry_run.previous_pinned);
    assert!(dry_run.target_pinned);
    assert!(!dry_run.is_no_op);

    // Verify store was not mutated during dry-run
    assert!(!store.is_pinned(win));
}

#[test]
fn test_always_on_top_apply_and_revert_identity() {
    let mut store = AlwaysOnTopStore::new();
    let win = WindowId::new(0x200);

    // 1. Pin
    let mut plan = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Pin);
    let revert_plan = plan.apply(&mut store).unwrap();

    assert!(store.is_pinned(win));
    assert_eq!(store.pinned_count(), 1);

    // 2. Revert (should restore to unpinned)
    revert_plan.revert(&mut store).unwrap();
    assert!(!store.is_pinned(win));
    assert_eq!(store.pinned_count(), 0);
}

#[test]
fn test_always_on_top_unpin_and_revert() {
    let mut store = AlwaysOnTopStore::new();
    let win = WindowId::new(0x300);

    // Initial state: pinned
    let mut initial_plan = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Pin);
    let _ = initial_plan.apply(&mut store).unwrap();
    assert!(store.is_pinned(win));

    // Unpin plan
    let mut unpin_plan = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Unpin);
    let revert_unpin = unpin_plan.apply(&mut store).unwrap();
    assert!(!store.is_pinned(win));

    // Reverting unpin restores pinned state
    revert_unpin.revert(&mut store).unwrap();
    assert!(store.is_pinned(win));
}

#[test]
fn test_always_on_top_toggle() {
    let mut store = AlwaysOnTopStore::new();
    let win = WindowId::new(0x400);

    // Toggle from false -> true
    let mut plan1 = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Toggle);
    assert!(plan1.target_pinned);
    let _ = plan1.apply(&mut store).unwrap();
    assert!(store.is_pinned(win));

    // Toggle from true -> false
    let mut plan2 = AlwaysOnTopPlan::new(&store, win, AlwaysOnTopAction::Toggle);
    assert!(!plan2.target_pinned);
    let _ = plan2.apply(&mut store).unwrap();
    assert!(!store.is_pinned(win));
}

#[test]
fn test_always_on_top_multi_window() {
    let mut store = AlwaysOnTopStore::new();
    let win1 = WindowId::new(10);
    let win2 = WindowId::new(20);
    let win3 = WindowId::new(30);

    let _ = AlwaysOnTopPlan::new(&store, win1, AlwaysOnTopAction::Pin).apply(&mut store);
    let _ = AlwaysOnTopPlan::new(&store, win2, AlwaysOnTopAction::Pin).apply(&mut store);

    assert!(store.is_pinned(win1));
    assert!(store.is_pinned(win2));
    assert!(!store.is_pinned(win3));

    let list = store.list_pinned();
    assert_eq!(list, vec![win1, win2]);
}
