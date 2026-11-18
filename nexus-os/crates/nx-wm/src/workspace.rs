use crate::layout::{LayoutKind, LayoutParams};
use crate::tree::OrderedWindowTree;
use crate::types::{MonitorId, WindowId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// Represents a virtual workspace containing an ordered tree of managed windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub monitor_id: MonitorId,
    pub layout: LayoutKind,
    pub layout_params: LayoutParams,
    pub tree: OrderedWindowTree,
    pub last_focused: Option<WindowId>,
    pub is_visible: bool,
}

impl Workspace {
    pub fn new(id: WorkspaceId, monitor_id: MonitorId, layout: LayoutKind) -> Self {
        Self {
            id,
            name: format!("Workspace {}", id.0),
            monitor_id,
            layout,
            layout_params: LayoutParams::default(),
            tree: OrderedWindowTree::new(),
            last_focused: None,
            is_visible: true,
        }
    }

    pub fn with_params(mut self, params: LayoutParams) -> Self {
        self.layout_params = params;
        self
    }

    pub fn add_window(&mut self, id: WindowId) {
        self.tree.insert_after_active(id);
        self.last_focused = Some(id);
    }

    pub fn remove_window(&mut self, id: WindowId) -> Option<usize> {
        let removed = self.tree.remove(id);
        if self.last_focused == Some(id) {
            self.last_focused = self.tree.active_window();
        }
        removed
    }

    pub fn contains_window(&self, id: WindowId) -> bool {
        self.tree.contains(id)
    }

    pub fn active_window(&self) -> Option<WindowId> {
        self.tree.active_window()
    }

    pub fn set_active_window(&mut self, id: WindowId) {
        if self.tree.contains(id) {
            self.tree.set_active(id);
            self.last_focused = Some(id);
        }
    }

    pub fn cycle_window(&mut self, forward: bool) -> Option<WindowId> {
        let next = self.tree.cycle_active(forward);
        if let Some(id) = next {
            self.last_focused = Some(id);
        }
        next
    }
}
