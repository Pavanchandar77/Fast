use crate::error::PowerResult;
use crate::types::{PlanStatus, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Requested action for pinning or unpinning a window to topmost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlwaysOnTopAction {
    Pin,
    Unpin,
    Toggle,
}

/// Information report from dry-running an Always-on-top plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlwaysOnTopDryRun {
    pub window_id: WindowId,
    pub previous_pinned: bool,
    pub target_pinned: bool,
    pub is_no_op: bool,
}

/// Reversible mutation plan for always-on-top window pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlwaysOnTopPlan {
    pub window_id: WindowId,
    pub action: AlwaysOnTopAction,
    pub previous_pinned: bool,
    pub target_pinned: bool,
    pub status: PlanStatus,
}

impl AlwaysOnTopPlan {
    pub fn new(store: &AlwaysOnTopStore, window_id: WindowId, action: AlwaysOnTopAction) -> Self {
        let previous_pinned = store.is_pinned(window_id);
        let target_pinned = match action {
            AlwaysOnTopAction::Pin => true,
            AlwaysOnTopAction::Unpin => false,
            AlwaysOnTopAction::Toggle => !previous_pinned,
        };

        Self {
            window_id,
            action,
            previous_pinned,
            target_pinned,
            status: PlanStatus::DryRun,
        }
    }

    pub fn dry_run(&self) -> AlwaysOnTopDryRun {
        AlwaysOnTopDryRun {
            window_id: self.window_id,
            previous_pinned: self.previous_pinned,
            target_pinned: self.target_pinned,
            is_no_op: self.previous_pinned == self.target_pinned,
        }
    }

    /// Applies the plan to the store, returning an `AlwaysOnTopRevertPlan` for reversible undo.
    pub fn apply(&mut self, store: &mut AlwaysOnTopStore) -> PowerResult<AlwaysOnTopRevertPlan> {
        let revert_plan = AlwaysOnTopRevertPlan {
            window_id: self.window_id,
            revert_to_pinned: self.previous_pinned,
        };

        if self.target_pinned {
            store.pinned.insert(self.window_id);
        } else {
            store.pinned.remove(&self.window_id);
        }

        self.status = PlanStatus::Applied;
        Ok(revert_plan)
    }
}

/// Reversible undo plan that restores the window to its previous always-on-top pin state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlwaysOnTopRevertPlan {
    pub window_id: WindowId,
    pub revert_to_pinned: bool,
}

impl AlwaysOnTopRevertPlan {
    pub fn revert(&self, store: &mut AlwaysOnTopStore) -> PowerResult<()> {
        if self.revert_to_pinned {
            store.pinned.insert(self.window_id);
        } else {
            store.pinned.remove(&self.window_id);
        }
        Ok(())
    }
}

/// In-memory store of pinned always-on-top windows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlwaysOnTopStore {
    pinned: HashSet<WindowId>,
}

impl AlwaysOnTopStore {
    pub fn new() -> Self {
        Self {
            pinned: HashSet::new(),
        }
    }

    pub fn is_pinned(&self, window_id: WindowId) -> bool {
        self.pinned.contains(&window_id)
    }

    pub fn pinned_count(&self) -> usize {
        self.pinned.len()
    }

    pub fn list_pinned(&self) -> Vec<WindowId> {
        let mut list: Vec<WindowId> = self.pinned.iter().copied().collect();
        list.sort_unstable();
        list
    }

    pub fn clear(&mut self) {
        self.pinned.clear();
    }
}
