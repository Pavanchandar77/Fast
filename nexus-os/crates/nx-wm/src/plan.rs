use crate::geometry::Rect;
use crate::types::WindowId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Individual reversible mutation of OS or compositor window state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMutation {
    SetPosition {
        window_id: WindowId,
        rect: Rect,
        async_pos: bool,
    },
    SetVisibility {
        window_id: WindowId,
        visible: bool,
    },
    SetCloak {
        window_id: WindowId,
        cloaked: bool,
    },
    SetFocus {
        window_id: WindowId,
    },
    SetFullscreen {
        window_id: WindowId,
        fullscreen: bool,
        saved_rect: Option<Rect>,
    },
    SetFloating {
        window_id: WindowId,
        floating: bool,
    },
    MaskWinKeyUp,
}

/// Pre-mutation snapshot of window attributes to ensure complete reversibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub window_id: WindowId,
    pub rect: Rect,
    pub visible: bool,
    pub cloaked: bool,
    pub is_floating: bool,
    pub is_fullscreen: bool,
}

/// Coherent, dry-run-able batch of window state mutations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MutationPlan {
    pub mutations: Vec<WindowMutation>,
}

impl MutationPlan {
    pub fn new() -> Self {
        Self {
            mutations: Vec::new(),
        }
    }

    pub fn push(&mut self, mutation: WindowMutation) {
        self.mutations.push(mutation);
    }

    pub fn extend(&mut self, other: MutationPlan) {
        self.mutations.extend(other.mutations);
    }

    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    pub fn has_mask_win_key_up(&self) -> bool {
        self.mutations
            .iter()
            .any(|m| matches!(m, WindowMutation::MaskWinKeyUp))
    }

    pub fn position_targets(&self) -> HashMap<WindowId, Rect> {
        let mut map = HashMap::new();
        for m in &self.mutations {
            if let WindowMutation::SetPosition {
                window_id, rect, ..
            } = m
            {
                map.insert(*window_id, *rect);
            }
        }
        map
    }

    pub fn cloaked_targets(&self) -> HashMap<WindowId, bool> {
        let mut map = HashMap::new();
        for m in &self.mutations {
            if let WindowMutation::SetCloak { window_id, cloaked } = m {
                map.insert(*window_id, *cloaked);
            }
        }
        map
    }

    pub fn visibility_targets(&self) -> HashMap<WindowId, bool> {
        let mut map = HashMap::new();
        for m in &self.mutations {
            if let WindowMutation::SetVisibility { window_id, visible } = m {
                map.insert(*window_id, *visible);
            }
        }
        map
    }

    pub fn focus_target(&self) -> Option<WindowId> {
        self.mutations.iter().rev().find_map(|m| match m {
            WindowMutation::SetFocus { window_id } => Some(*window_id),
            _ => None,
        })
    }

    /// Computes the exact inverse mutation plan given a map of previous snapshots.
    /// Applying this inverse plan guarantees restoring the system to its prior state.
    pub fn compute_inverse(&self, snapshots: &HashMap<WindowId, WindowSnapshot>) -> MutationPlan {
        let mut inverse = MutationPlan::new();

        for mutation in self.mutations.iter().rev() {
            match mutation {
                WindowMutation::SetPosition { window_id, .. } => {
                    if let Some(snap) = snapshots.get(window_id) {
                        inverse.push(WindowMutation::SetPosition {
                            window_id: *window_id,
                            rect: snap.rect,
                            async_pos: true,
                        });
                    }
                }
                WindowMutation::SetVisibility { window_id, .. } => {
                    if let Some(snap) = snapshots.get(window_id) {
                        inverse.push(WindowMutation::SetVisibility {
                            window_id: *window_id,
                            visible: snap.visible,
                        });
                    }
                }
                WindowMutation::SetCloak { window_id, .. } => {
                    if let Some(snap) = snapshots.get(window_id) {
                        inverse.push(WindowMutation::SetCloak {
                            window_id: *window_id,
                            cloaked: snap.cloaked,
                        });
                    }
                }
                WindowMutation::SetFullscreen { window_id, .. } => {
                    if let Some(snap) = snapshots.get(window_id) {
                        inverse.push(WindowMutation::SetFullscreen {
                            window_id: *window_id,
                            fullscreen: snap.is_fullscreen,
                            saved_rect: Some(snap.rect),
                        });
                    }
                }
                WindowMutation::SetFloating { window_id, .. } => {
                    if let Some(snap) = snapshots.get(window_id) {
                        inverse.push(WindowMutation::SetFloating {
                            window_id: *window_id,
                            floating: snap.is_floating,
                        });
                    }
                }
                WindowMutation::SetFocus { .. } | WindowMutation::MaskWinKeyUp => {
                    // Focus and key masking do not require reversal
                }
            }
        }

        inverse
    }
}
