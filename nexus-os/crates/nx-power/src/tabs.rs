use crate::error::{PowerError, PowerResult};
use crate::types::{PlanStatus, Rect, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strongly typed identifier for a window tab group container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabGroupId(pub u64);

impl std::fmt::Display for TabGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TabGroup({})", self.0)
    }
}

/// A tab representing an individual foreign or native window in a tabbed group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowTab {
    pub window_id: WindowId,
    pub title: String,
    pub icon_hint: Option<String>,
}

impl WindowTab {
    pub fn new(window_id: WindowId, title: impl Into<String>) -> Self {
        Self {
            window_id,
            title: title.into(),
            icon_hint: None,
        }
    }
}

/// A tabbed container grouping multiple window IDs into a single tab strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: TabGroupId,
    pub name: String,
    pub tabs: Vec<WindowTab>,
    pub active_index: usize,
    pub frame_bounds: Rect,
}

impl TabGroup {
    pub fn active_tab(&self) -> Option<&WindowTab> {
        self.tabs.get(self.active_index)
    }

    pub fn active_window(&self) -> Option<WindowId> {
        self.active_tab().map(|t| t.window_id)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }
}

/// Dry-run report for a tab activation switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSwitchDryRun {
    pub group_id: TabGroupId,
    pub activate_window: WindowId,
    pub cloak_windows: Vec<WindowId>,
    pub sync_bounds: Rect,
}

/// Reversible mutation plan that syncs window visibility and geometry on tab selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSwitchPlan {
    pub group_id: TabGroupId,
    pub previous_index: usize,
    pub target_index: usize,
    pub activate_window: WindowId,
    pub cloak_windows: Vec<WindowId>,
    pub sync_bounds: Rect,
    pub status: PlanStatus,
}

impl TabSwitchPlan {
    pub fn dry_run(&self) -> TabSwitchDryRun {
        TabSwitchDryRun {
            group_id: self.group_id,
            activate_window: self.activate_window,
            cloak_windows: self.cloak_windows.clone(),
            sync_bounds: self.sync_bounds,
        }
    }

    pub fn apply(&mut self, manager: &mut TabGroupManager) -> PowerResult<TabSwitchRevertPlan> {
        let revert = TabSwitchRevertPlan {
            group_id: self.group_id,
            restore_index: self.previous_index,
        };

        let group = manager
            .groups
            .get_mut(&self.group_id)
            .ok_or(PowerError::TabGroupNotFound(self.group_id.0))?;

        if self.target_index >= group.tabs.len() {
            return Err(PowerError::TabGroupNotFound(self.group_id.0));
        }

        group.active_index = self.target_index;
        self.status = PlanStatus::Applied;
        Ok(revert)
    }
}

/// Reversible undo plan that rolls back tab selection to the previous tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSwitchRevertPlan {
    pub group_id: TabGroupId,
    pub restore_index: usize,
}

impl TabSwitchRevertPlan {
    pub fn revert(&self, manager: &mut TabGroupManager) -> PowerResult<()> {
        let group = manager
            .groups
            .get_mut(&self.group_id)
            .ok_or(PowerError::TabGroupNotFound(self.group_id.0))?;

        if self.restore_index < group.tabs.len() {
            group.active_index = self.restore_index;
            Ok(())
        } else {
            Err(PowerError::TabGroupNotFound(self.group_id.0))
        }
    }
}

/// State machine managing universal window tab groups (grouping arbitrary HWNDs into tabs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TabGroupManager {
    groups: HashMap<TabGroupId, TabGroup>,
    window_map: HashMap<WindowId, TabGroupId>,
    next_id: u64,
}

impl TabGroupManager {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            window_map: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn get_group(&self, group_id: TabGroupId) -> Option<&TabGroup> {
        self.groups.get(&group_id)
    }

    pub fn group_for_window(&self, window_id: WindowId) -> Option<TabGroupId> {
        self.window_map.get(&window_id).copied()
    }

    /// Creates a new tab group with an initial window tab and container bounds.
    pub fn create_group(
        &mut self,
        name: impl Into<String>,
        initial_tab: WindowTab,
        frame_bounds: Rect,
    ) -> TabGroupId {
        let id = TabGroupId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        self.window_map.insert(initial_tab.window_id, id);

        let group = TabGroup {
            id,
            name: name.into(),
            tabs: vec![initial_tab],
            active_index: 0,
            frame_bounds,
        };

        self.groups.insert(id, group);
        id
    }

    /// Appends a window tab to an existing group.
    pub fn add_tab(&mut self, group_id: TabGroupId, tab: WindowTab) -> PowerResult<()> {
        // If window already belongs to another group, remove it first
        if let Some(existing_group) = self.window_map.get(&tab.window_id).copied() {
            if existing_group == group_id {
                return Ok(()); // Already in this group
            }
            let _ = self.remove_tab(tab.window_id);
        }

        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(PowerError::TabGroupNotFound(group_id.0))?;

        self.window_map.insert(tab.window_id, group_id);
        group.tabs.push(tab);
        Ok(())
    }

    /// Removes a window tab from its group. If the group becomes empty, it is deleted.
    pub fn remove_tab(&mut self, window_id: WindowId) -> PowerResult<Option<WindowTab>> {
        let group_id = match self.window_map.remove(&window_id) {
            Some(g) => g,
            None => return Ok(None),
        };

        let group = match self.groups.get_mut(&group_id) {
            Some(g) => g,
            None => return Ok(None),
        };

        let removed = if let Some(idx) = group.tabs.iter().position(|t| t.window_id == window_id) {
            let item = group.tabs.remove(idx);
            if group.active_index >= group.tabs.len() && !group.tabs.is_empty() {
                group.active_index = group.tabs.len().saturating_sub(1);
            }
            Some(item)
        } else {
            None
        };

        if group.tabs.is_empty() {
            self.groups.remove(&group_id);
        }

        Ok(removed)
    }

    /// Creates a plan to switch the active tab in a group.
    pub fn switch_tab_plan(
        &self,
        group_id: TabGroupId,
        target_index: usize,
    ) -> PowerResult<TabSwitchPlan> {
        let group = self
            .groups
            .get(&group_id)
            .ok_or(PowerError::TabGroupNotFound(group_id.0))?;

        if target_index >= group.tabs.len() {
            return Err(PowerError::TabGroupNotFound(group_id.0));
        }

        let activate_window = group.tabs[target_index].window_id;
        let cloak_windows: Vec<WindowId> = group
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != target_index)
            .map(|(_, t)| t.window_id)
            .collect();

        Ok(TabSwitchPlan {
            group_id,
            previous_index: group.active_index,
            target_index,
            activate_window,
            cloak_windows,
            sync_bounds: group.frame_bounds,
            status: PlanStatus::DryRun,
        })
    }

    /// Reorders tabs within a group (e.g. via drag-and-drop on the tab strip).
    pub fn reorder_tabs(
        &mut self,
        group_id: TabGroupId,
        from_idx: usize,
        to_idx: usize,
    ) -> PowerResult<()> {
        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(PowerError::TabGroupNotFound(group_id.0))?;

        if from_idx >= group.tabs.len() || to_idx >= group.tabs.len() {
            return Err(PowerError::TabGroupNotFound(group_id.0));
        }

        if from_idx == to_idx {
            return Ok(());
        }

        let item = group.tabs.remove(from_idx);
        group.tabs.insert(to_idx, item);

        // Adjust active_index
        if group.active_index == from_idx {
            group.active_index = to_idx;
        } else if from_idx < group.active_index && to_idx >= group.active_index {
            group.active_index = group.active_index.saturating_sub(1);
        } else if from_idx > group.active_index && to_idx <= group.active_index {
            group.active_index = group.active_index.saturating_add(1);
        }

        Ok(())
    }

    /// Merges all tabs from `source_id` into `target_id`, closing `source_id`.
    pub fn merge_groups(
        &mut self,
        target_id: TabGroupId,
        source_id: TabGroupId,
    ) -> PowerResult<()> {
        if target_id == source_id {
            return Ok(());
        }

        let source_tabs = {
            let src = self
                .groups
                .remove(&source_id)
                .ok_or(PowerError::TabGroupNotFound(source_id.0))?;
            src.tabs
        };

        for tab in source_tabs {
            self.window_map.insert(tab.window_id, target_id);
            if let Some(target) = self.groups.get_mut(&target_id) {
                target.tabs.push(tab);
            }
        }

        Ok(())
    }

    /// Closes and dissolves a tab group, unmapping all associated windows.
    pub fn close_group(&mut self, group_id: TabGroupId) -> PowerResult<TabGroup> {
        let group = self
            .groups
            .remove(&group_id)
            .ok_or(PowerError::TabGroupNotFound(group_id.0))?;

        for tab in &group.tabs {
            self.window_map.remove(&tab.window_id);
        }

        Ok(group)
    }
}
