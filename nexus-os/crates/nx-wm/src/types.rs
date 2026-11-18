use crate::geometry::Rect;
use serde::{Deserialize, Serialize};

/// Unique identifier for a managed window (wraps raw HWND as u64 or synthetic ID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Win(0x{:X})", self.0)
    }
}

/// Unique identifier for a monitor display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MonitorId(pub u32);

impl std::fmt::Display for MonitorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mon({})", self.0)
    }
}

/// Unique identifier for a virtual workspace (typically 1..=9 or beyond).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(pub u8);

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ws({})", self.0)
    }
}

/// Initial or updated window metadata provided by OS hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    pub class_name: String,
    pub process_name: String,
    pub rect: Rect,
    pub is_visible: bool,
    pub is_minimized: bool,
    pub is_cloaked: bool,
    pub is_dialog_or_popup: bool,
    pub style: u32,
    pub ex_style: u32,
}

/// In-memory state of a window managed by `nx-wm`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub id: WindowId,
    pub title: String,
    pub class_name: String,
    pub process_name: String,
    pub is_floating: bool,
    pub is_fullscreen: bool,
    pub is_minimized: bool,
    pub is_cloaked: bool,
    pub is_visible: bool,
    pub workspace_id: WorkspaceId,
    pub monitor_id: MonitorId,
    pub current_rect: Rect,
    pub target_rect: Rect,
    pub floating_rect: Rect,
    pub saved_rect_before_fullscreen: Option<Rect>,
    pub is_auto_excluded: bool,
}

impl WindowState {
    pub fn new(
        info: WindowInfo,
        workspace_id: WorkspaceId,
        monitor_id: MonitorId,
        is_floating: bool,
        is_auto_excluded: bool,
    ) -> Self {
        let current_rect = info.rect;
        let floating_rect = if current_rect.is_empty() {
            Rect::new(100, 100, 800, 600)
        } else {
            current_rect
        };

        Self {
            id: info.id,
            title: info.title,
            class_name: info.class_name,
            process_name: info.process_name,
            is_floating,
            is_fullscreen: false,
            is_minimized: info.is_minimized,
            is_cloaked: info.is_cloaked,
            is_visible: info.is_visible,
            workspace_id,
            monitor_id,
            current_rect,
            target_rect: current_rect,
            floating_rect,
            saved_rect_before_fullscreen: None,
            is_auto_excluded,
        }
    }

    /// Whether this window should participate in tiling calculations on its workspace.
    pub fn is_tileable(&self) -> bool {
        !self.is_floating
            && !self.is_fullscreen
            && !self.is_minimized
            && !self.is_cloaked
            && !self.is_auto_excluded
    }
}
