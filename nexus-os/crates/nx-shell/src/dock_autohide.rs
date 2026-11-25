//! Dock auto-hide state machine (RetroBar-style: hide after the pointer
//! leaves, show when it hits the screen edge or a menu is open).
//!
//! The timings and the "don't hide while a menu is open" rule follow
//! RetroBar's `ShouldAllowAutoHide` (Apache-2.0, dremin/RetroBar). This
//! module is the policy only; the view applies the resulting show/hide.

use std::time::{Duration, Instant};

/// How long the pointer must stay off the dock before it hides.
pub const HIDE_DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Hide,
    Show,
}

#[derive(Clone, Debug)]
pub struct AutoHide {
    pub enabled: bool,
    pub hidden: bool,
    popup_open: bool,
    pointer_over: bool,
    edge_hot: bool,
    left_at: Option<Instant>,
}

impl AutoHide {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            hidden: false,
            popup_open: false,
            pointer_over: false,
            edge_hot: false,
            left_at: None,
        }
    }

    /// Pointer moved: `over_dock` is over the dock itself, `edge_hot` is in
    /// the thin reveal strip at the screen edge the dock is anchored to.
    pub fn pointer(&mut self, over_dock: bool, edge_hot: bool, now: Instant) -> Action {
        let leaving_safe = (self.pointer_over && !over_dock && !edge_hot)
            || (self.edge_hot && !edge_hot && !over_dock);
        self.pointer_over = over_dock;
        self.edge_hot = edge_hot;
        if over_dock || edge_hot {
            self.left_at = None;
        } else if leaving_safe || self.left_at.is_none() {
            // Restart the delay whenever the dock becomes leaveable, so
            // leaving the reveal strip or a just-closed menu does not hide
            // instantly.
            self.left_at = Some(now);
        }
        self.step(now)
    }

    /// A dock menu or jump list is open — never hide while it is.
    pub fn set_popup(&mut self, open: bool, now: Instant) -> Action {
        self.popup_open = open;
        if open {
            self.left_at = None;
        } else if !self.pointer_over && !self.edge_hot {
            self.left_at = Some(now);
        }
        self.step(now)
    }

    /// Advance timers. Call on each frame while auto-hide is enabled.
    pub fn tick(&mut self, now: Instant) -> Action {
        self.step(now)
    }

    fn can_hide(&self) -> bool {
        self.enabled && !self.popup_open && !self.pointer_over && !self.edge_hot
    }

    fn step(&mut self, now: Instant) -> Action {
        if !self.enabled {
            if self.hidden {
                self.hidden = false;
                return Action::Show;
            }
            return Action::None;
        }
        if self.popup_open || self.pointer_over || self.edge_hot {
            if self.hidden {
                self.hidden = false;
                return Action::Show;
            }
            return Action::None;
        }
        if !self.hidden {
            if let Some(left) = self.left_at {
                if now.duration_since(left) >= HIDE_DELAY && self.can_hide() {
                    self.hidden = true;
                    return Action::Hide;
                }
            }
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn disabled_never_hides() {
        let mut h = AutoHide::new(false);
        let t = t0();
        assert_eq!(h.pointer(false, false, t), Action::None);
        assert_eq!(h.tick(t + HIDE_DELAY * 2), Action::None);
        assert!(!h.hidden);
    }

    #[test]
    fn hides_after_delay_when_pointer_leaves() {
        let mut h = AutoHide::new(true);
        let t = t0();
        assert_eq!(h.pointer(true, false, t), Action::None);
        assert_eq!(h.pointer(false, false, t), Action::None);
        assert_eq!(h.tick(t + Duration::from_millis(200)), Action::None);
        assert_eq!(h.tick(t + HIDE_DELAY), Action::Hide);
        assert!(h.hidden);
        assert_eq!(h.tick(t + HIDE_DELAY * 2), Action::None);
    }

    #[test]
    fn shows_when_pointer_returns_or_hits_edge() {
        let mut h = AutoHide::new(true);
        let t = t0();
        let _ = h.pointer(false, false, t);
        assert_eq!(h.tick(t + HIDE_DELAY), Action::Hide);
        assert_eq!(h.pointer(false, true, t + HIDE_DELAY), Action::Show);
        assert!(!h.hidden);
        let _ = h.pointer(false, false, t + HIDE_DELAY);
        assert_eq!(h.tick(t + HIDE_DELAY * 2), Action::Hide);
        assert_eq!(h.pointer(true, false, t + HIDE_DELAY * 2), Action::Show);
    }

    #[test]
    fn popup_blocks_hide_and_reveals() {
        let mut h = AutoHide::new(true);
        let t = t0();
        let _ = h.pointer(false, false, t);
        assert_eq!(h.set_popup(true, t + HIDE_DELAY), Action::None);
        assert_eq!(h.tick(t + HIDE_DELAY * 2), Action::None);
        assert!(!h.hidden);
        let _ = h.set_popup(false, t + HIDE_DELAY * 2);
        // pointer still away: starts the timer, does not hide immediately
        assert_eq!(h.tick(t + HIDE_DELAY * 2), Action::None);
        assert_eq!(h.tick(t + HIDE_DELAY * 3), Action::Hide);
        assert_eq!(h.set_popup(true, t + HIDE_DELAY * 3), Action::Show);
    }
}
