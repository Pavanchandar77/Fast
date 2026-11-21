//! Decides which signature effect a window lifecycle event should play.
//!
//! Platform-free so the rules are tested on every OS; the Windows bridge
//! feeds it WinEvents and executes the plans (capture, hide, animate).

use std::collections::{HashMap, HashSet};

/// Window lifecycle events, keyed by a stable window id (the HWND value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeEvent {
    Foreground(u64),
    Shown(u64),
    Hidden(u64),
    Destroyed(u64),
    MinimizeStart(u64),
    MinimizeEnd(u64),
    DragStart(u64),
    DragEnd(u64),
}

/// What the bridge should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Refresh the cached snapshot (used later by close and minimize).
    Capture(u64),
    Open(u64),
    Close(u64),
    Minimize(u64),
    Restore(u64),
    Ripple(u64),
    DragBegin(u64),
    DragEnd(u64),
    Forget(u64),
}

#[derive(Debug, Default)]
pub struct MotionPlanner {
    visible: HashSet<u64>,
    minimized: HashSet<u64>,
    cached: HashSet<u64>,
    /// Most recent foreground first, for the switcher.
    recency: Vec<u64>,
    dragging: Option<u64>,
    /// Windows seen at startup are adopted without an open animation.
    adopted: HashMap<u64, ()>,
}

impl MotionPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// A window that was already on screen when Nexus started.
    pub fn adopt(&mut self, id: u64) {
        self.visible.insert(id);
        self.adopted.insert(id, ());
    }

    pub fn mark_cached(&mut self, id: u64) {
        self.cached.insert(id);
    }

    pub fn is_cached(&self, id: u64) -> bool {
        self.cached.contains(&id)
    }

    /// Visible windows, most recently focused first.
    pub fn switcher_order(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self
            .recency
            .iter()
            .copied()
            .filter(|id| self.visible.contains(id) || self.minimized.contains(id))
            .collect();
        let mut rest: Vec<u64> = self
            .visible
            .iter()
            .copied()
            .filter(|id| !out.contains(id))
            .collect();
        rest.sort_unstable();
        out.extend(rest);
        out
    }

    /// Visible, non-minimized windows other than `except`.
    pub fn others(&self, except: u64) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .visible
            .iter()
            .copied()
            .filter(|id| *id != except && !self.minimized.contains(id))
            .collect();
        v.sort_unstable();
        v
    }

    pub fn on(&mut self, ev: LifeEvent) -> Vec<Plan> {
        use LifeEvent::*;
        match ev {
            Foreground(id) => {
                self.recency.retain(|x| *x != id);
                self.recency.insert(0, id);
                self.recency.truncate(32);
                if self.visible.contains(&id) && !self.minimized.contains(&id) {
                    vec![Plan::Capture(id), Plan::Ripple(id)]
                } else {
                    vec![]
                }
            }
            Shown(id) => {
                if self.minimized.contains(&id) || !self.visible.insert(id) {
                    return vec![];
                }
                vec![Plan::Open(id)]
            }
            Hidden(id) | Destroyed(id) => {
                let was_visible = self.visible.remove(&id);
                let was_min = self.minimized.remove(&id);
                let cached = if matches!(ev, Destroyed(_)) {
                    self.recency.retain(|x| *x != id);
                    self.adopted.remove(&id);
                    self.cached.remove(&id)
                } else {
                    self.cached.contains(&id)
                };
                let mut out = Vec::new();
                if was_visible && !was_min && cached {
                    out.push(Plan::Close(id));
                }
                if matches!(ev, Destroyed(_)) {
                    out.push(Plan::Forget(id));
                }
                out
            }
            MinimizeStart(id) => {
                if self.visible.contains(&id)
                    && self.minimized.insert(id)
                    && self.cached.contains(&id)
                {
                    vec![Plan::Minimize(id)]
                } else {
                    vec![]
                }
            }
            MinimizeEnd(id) => {
                if self.minimized.remove(&id) {
                    self.visible.insert(id);
                    vec![Plan::Restore(id)]
                } else {
                    vec![]
                }
            }
            DragStart(id) => {
                if self.dragging.replace(id).is_some() {
                    vec![]
                } else {
                    vec![Plan::DragBegin(id)]
                }
            }
            DragEnd(id) => {
                if self.dragging == Some(id) {
                    self.dragging = None;
                    vec![Plan::Capture(id), Plan::DragEnd(id)]
                } else {
                    vec![]
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LifeEvent::*;

    #[test]
    fn new_window_blooms_once() {
        let mut p = MotionPlanner::new();
        assert_eq!(p.on(Shown(1)), vec![Plan::Open(1)]);
        assert!(
            p.on(Shown(1)).is_empty(),
            "repeat show is not a second open"
        );
    }

    #[test]
    fn close_needs_a_cached_snapshot() {
        let mut p = MotionPlanner::new();
        p.on(Shown(1));
        assert_eq!(
            p.on(Destroyed(1)),
            vec![Plan::Forget(1)],
            "no pixels, no dust"
        );
        p.on(Shown(2));
        p.mark_cached(2);
        assert_eq!(p.on(Destroyed(2)), vec![Plan::Close(2), Plan::Forget(2)]);
    }

    #[test]
    fn hide_then_destroy_dusts_once() {
        let mut p = MotionPlanner::new();
        p.adopt(3);
        p.mark_cached(3);
        assert_eq!(p.on(Hidden(3)), vec![Plan::Close(3)]);
        assert_eq!(p.on(Destroyed(3)), vec![Plan::Forget(3)]);
    }

    #[test]
    fn minimize_restore_round_trip() {
        let mut p = MotionPlanner::new();
        p.adopt(4);
        p.mark_cached(4);
        assert_eq!(p.on(MinimizeStart(4)), vec![Plan::Minimize(4)]);
        assert!(p.on(MinimizeStart(4)).is_empty());
        assert!(p.on(Shown(4)).is_empty(), "restore is not an open");
        assert_eq!(p.on(MinimizeEnd(4)), vec![Plan::Restore(4)]);
        assert!(p.on(MinimizeEnd(4)).is_empty());
    }

    #[test]
    fn closing_a_minimized_window_does_not_dust() {
        let mut p = MotionPlanner::new();
        p.adopt(5);
        p.mark_cached(5);
        p.on(MinimizeStart(5));
        assert_eq!(p.on(Destroyed(5)), vec![Plan::Forget(5)]);
    }

    #[test]
    fn focus_ripples_and_orders_switcher() {
        let mut p = MotionPlanner::new();
        for id in [1, 2, 3] {
            p.adopt(id);
        }
        assert_eq!(p.on(Foreground(2)), vec![Plan::Capture(2), Plan::Ripple(2)]);
        p.on(Foreground(3));
        assert_eq!(p.switcher_order(), vec![3, 2, 1]);
        assert_eq!(p.others(3), vec![1, 2]);
        assert!(p.on(Foreground(99)).is_empty(), "unknown window");
    }

    #[test]
    fn drag_pairs_start_and_end() {
        let mut p = MotionPlanner::new();
        assert_eq!(p.on(DragStart(7)), vec![Plan::DragBegin(7)]);
        assert!(p.on(DragEnd(8)).is_empty());
        assert_eq!(p.on(DragEnd(7)), vec![Plan::Capture(7), Plan::DragEnd(7)]);
    }
}
