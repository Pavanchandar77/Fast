use crate::types::WindowId;
use serde::{Deserialize, Serialize};

/// Deterministic, ordered tree/list of windows within a workspace.
/// Guarantees stable insertion order (no random HashMap reshuffling)
/// and tracks MRU focus history for seamless window destruction fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderedWindowTree {
    /// Ordered windows in tiling order (index 0 is master pane).
    windows: Vec<WindowId>,
    /// Focus history: most recently focused window is at index 0.
    focus_stack: Vec<WindowId>,
}

impl OrderedWindowTree {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            focus_stack: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn as_slice(&self) -> &[WindowId] {
        &self.windows
    }

    pub fn iter(&self) -> impl Iterator<Item = &WindowId> {
        self.windows.iter()
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.windows.contains(&id)
    }

    pub fn index_of(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| *w == id)
    }

    pub fn get(&self, index: usize) -> Option<WindowId> {
        self.windows.get(index).copied()
    }

    /// Returns the currently active window (top of focus stack).
    pub fn active_window(&self) -> Option<WindowId> {
        self.focus_stack.first().copied()
    }

    /// Sets the active window and updates the focus MRU stack.
    pub fn set_active(&mut self, id: WindowId) {
        if !self.windows.contains(&id) {
            return;
        }
        self.focus_stack.retain(|w| *w != id);
        self.focus_stack.insert(0, id);
    }

    /// Inserts a new window at the end of the tiling order.
    pub fn push(&mut self, id: WindowId) {
        if !self.contains(id) {
            self.windows.push(id);
            self.focus_stack.retain(|w| *w != id);
            self.focus_stack.insert(0, id);
        }
    }

    /// Inserts a new window adjacent to the currently active window,
    /// or at the end if no window is active.
    pub fn insert_after_active(&mut self, id: WindowId) {
        if self.contains(id) {
            return;
        }

        let insert_idx = match self.active_window() {
            Some(active) => self
                .index_of(active)
                .map(|idx| idx + 1)
                .unwrap_or(self.windows.len()),
            None => self.windows.len(),
        };

        if insert_idx >= self.windows.len() {
            self.windows.push(id);
        } else {
            self.windows.insert(insert_idx, id);
        }

        self.focus_stack.retain(|w| *w != id);
        self.focus_stack.insert(0, id);
    }

    /// Removes a window from both the tiling tree and the focus stack.
    /// Returns the previous tiling index if found.
    pub fn remove(&mut self, id: WindowId) -> Option<usize> {
        let idx = self.index_of(id)?;
        self.windows.remove(idx);
        self.focus_stack.retain(|w| *w != id);
        Some(idx)
    }

    /// Swaps two windows by their tiling indices.
    pub fn swap(&mut self, idx_a: usize, idx_b: usize) -> bool {
        if idx_a < self.windows.len() && idx_b < self.windows.len() {
            self.windows.swap(idx_a, idx_b);
            true
        } else {
            false
        }
    }

    /// Swaps two windows by their identifiers.
    pub fn swap_windows(&mut self, a: WindowId, b: WindowId) -> bool {
        if let (Some(idx_a), Some(idx_b)) = (self.index_of(a), self.index_of(b)) {
            self.swap(idx_a, idx_b)
        } else {
            false
        }
    }

    /// Moves a window to a target index in the tiling order.
    pub fn move_window(&mut self, id: WindowId, target_idx: usize) -> bool {
        if let Some(current_idx) = self.index_of(id) {
            let win = self.windows.remove(current_idx);
            let bounded_idx = target_idx.min(self.windows.len());
            self.windows.insert(bounded_idx, win);
            true
        } else {
            false
        }
    }

    /// Cycles the active window forward (next) or backward (prev) in tiling order.
    pub fn cycle_active(&mut self, forward: bool) -> Option<WindowId> {
        if self.windows.is_empty() {
            return None;
        }

        let current = self.active_window();
        let next_idx = match current.and_then(|id| self.index_of(id)) {
            Some(idx) => {
                if forward {
                    (idx + 1) % self.windows.len()
                } else {
                    if idx == 0 {
                        self.windows.len() - 1
                    } else {
                        idx - 1
                    }
                }
            }
            None => 0,
        };

        let next_id = self.windows[next_idx];
        self.set_active(next_id);
        Some(next_id)
    }

    /// Retains only windows matching the predicate.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&WindowId) -> bool,
    {
        self.windows.retain(&mut f);
        self.focus_stack.retain(&mut f);
    }
}
