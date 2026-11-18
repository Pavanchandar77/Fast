use crate::geometry::Rect;
use crate::types::WindowId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Reason for a window or layout coordinate transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnimationReason {
    WindowOpened,
    WindowClosed,
    WorkspaceSwitch,
    FocusChanged,
    Retile,
    LayoutChanged,
    MonitorChanged,
    WindowMoved,
    FullscreenToggle,
    FloatingToggle,
    Snap,
}

/// Emitted by the window manager whenever windows change target geometry.
/// Carries origin and destination rectangles so spring physics engines can smoothly interpolate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutTransition {
    pub from_rects: HashMap<WindowId, Rect>,
    pub to_rects: HashMap<WindowId, Rect>,
    pub reason: AnimationReason,
}

impl LayoutTransition {
    pub fn new(reason: AnimationReason) -> Self {
        Self {
            from_rects: HashMap::new(),
            to_rects: HashMap::new(),
            reason,
        }
    }

    pub fn add(&mut self, id: WindowId, from: Rect, to: Rect) {
        self.from_rects.insert(id, from);
        self.to_rects.insert(id, to);
    }

    pub fn is_empty(&self) -> bool {
        self.to_rects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.to_rects.len()
    }
}
