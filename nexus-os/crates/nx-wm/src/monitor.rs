use crate::geometry::{Point, Rect};
use crate::types::{MonitorId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// Represents a physical display monitor with virtual work area and DPI scale factor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub id: MonitorId,
    pub name: String,
    pub bounds: Rect,
    pub work_area: Rect,
    pub scale_factor: f64,
    pub workspaces: Vec<WorkspaceId>,
    pub active_workspace: WorkspaceId,
}

impl Monitor {
    pub fn new(
        id: MonitorId,
        name: impl Into<String>,
        bounds: Rect,
        work_area: Rect,
        scale_factor: f64,
        initial_workspace: WorkspaceId,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            bounds,
            work_area,
            scale_factor: if scale_factor > 0.0 { scale_factor } else { 1.0 },
            workspaces: vec![initial_workspace],
            active_workspace: initial_workspace,
        }
    }

    pub fn primary(bounds: Rect, work_area: Rect) -> Self {
        Self::new(MonitorId(0), "Primary", bounds, work_area, 1.0, WorkspaceId(1))
    }

    pub fn contains_point(&self, p: Point) -> bool {
        self.bounds.contains_point(p.x, p.y)
    }

    pub fn assign_workspace(&mut self, ws: WorkspaceId) {
        if !self.workspaces.contains(&ws) {
            self.workspaces.push(ws);
        }
    }

    pub fn remove_workspace(&mut self, ws: WorkspaceId) -> bool {
        let prev_len = self.workspaces.len();
        self.workspaces.retain(|&w| w != ws);
        if self.active_workspace == ws {
            if let Some(&first) = self.workspaces.first() {
                self.active_workspace = first;
            }
        }
        self.workspaces.len() != prev_len
    }

    pub fn set_active_workspace(&mut self, ws: WorkspaceId) {
        if self.workspaces.contains(&ws) {
            self.active_workspace = ws;
        }
    }
}
