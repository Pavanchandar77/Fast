use crate::error::{PowerError, PowerResult};
use crate::types::WindowId;
use serde::{Deserialize, Serialize};

/// Lightweight placeholder thumbnail metadata when DWM live thumbnails are inactive or running on non-Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderThumbnail {
    pub width: u32,
    pub height: u32,
    pub dominant_color: (u8, u8, u8),
    pub label: String,
}

impl Default for PlaceholderThumbnail {
    fn default() -> Self {
        Self {
            width: 320,
            height: 180,
            dominant_color: (40, 44, 52),
            label: "Window Preview".to_string(),
        }
    }
}

/// An entry in the Alt-Tab switcher window list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltTabWindow {
    pub id: WindowId,
    pub title: String,
    pub app_name: String,
    pub is_minimized: bool,
    pub thumbnail: PlaceholderThumbnail,
}

impl AltTabWindow {
    pub fn new(id: WindowId, title: impl Into<String>, app_name: impl Into<String>) -> Self {
        let t = title.into();
        let a = app_name.into();
        Self {
            id,
            title: t.clone(),
            app_name: a,
            is_minimized: false,
            thumbnail: PlaceholderThumbnail {
                width: 320,
                height: 180,
                dominant_color: (50, 50, 60),
                label: t,
            },
        }
    }
}

/// Status of the Alt-Tab switcher overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AltTabState {
    #[default]
    Idle,
    Active {
        selected_index: usize,
    },
}

/// State machine driving the Alt-Tab switcher with MRU z-order tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AltTabSwitcher {
    windows: Vec<AltTabWindow>,
    state: AltTabState,
}

impl AltTabSwitcher {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            state: AltTabState::Idle,
        }
    }

    pub fn state(&self) -> AltTabState {
        self.state
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, AltTabState::Active { .. })
    }

    pub fn windows(&self) -> &[AltTabWindow] {
        &self.windows
    }

    pub fn selected_index(&self) -> Option<usize> {
        match self.state {
            AltTabState::Active { selected_index } => Some(selected_index),
            AltTabState::Idle => None,
        }
    }

    pub fn selected_window(&self) -> Option<&AltTabWindow> {
        self.selected_index().and_then(|idx| self.windows.get(idx))
    }

    /// Primary trigger when Alt+Tab is pressed:
    /// - If idle: activates overlay and highlights the second window (MRU index 1).
    /// - If active: cycles to the next window in MRU order.
    pub fn press_alt_tab(&mut self) {
        if self.windows.is_empty() {
            return;
        }

        match self.state {
            AltTabState::Idle => {
                // When opened, select the previous active window (index 1 if available, otherwise 0)
                let initial_idx = if self.windows.len() > 1 { 1 } else { 0 };
                self.state = AltTabState::Active {
                    selected_index: initial_idx,
                };
            }
            AltTabState::Active { selected_index } => {
                let next_idx = (selected_index + 1) % self.windows.len();
                self.state = AltTabState::Active {
                    selected_index: next_idx,
                };
            }
        }
    }

    /// Moves selection backwards (Shift + Tab).
    pub fn step_prev(&mut self) {
        if self.windows.is_empty() {
            return;
        }

        match self.state {
            AltTabState::Idle => {
                let initial_idx = self.windows.len().saturating_sub(1);
                self.state = AltTabState::Active {
                    selected_index: initial_idx,
                };
            }
            AltTabState::Active { selected_index } => {
                let prev_idx = if selected_index == 0 {
                    self.windows.len().saturating_sub(1)
                } else {
                    selected_index - 1
                };
                self.state = AltTabState::Active {
                    selected_index: prev_idx,
                };
            }
        }
    }

    /// Selects a specific index directly (e.g. via mouse hover/click).
    pub fn select_index(&mut self, index: usize) -> PowerResult<()> {
        if index < self.windows.len() {
            self.state = AltTabState::Active {
                selected_index: index,
            };
            Ok(())
        } else {
            Err(PowerError::AltTabError(format!(
                "Index {} exceeds window count {}",
                index,
                self.windows.len()
            )))
        }
    }

    /// Alt key released: commits selection, moves chosen window to MRU top (index 0), and returns `WindowId`.
    pub fn release_alt(&mut self) -> Option<WindowId> {
        let chosen_idx = match self.state {
            AltTabState::Active { selected_index } => selected_index,
            AltTabState::Idle => return None,
        };

        self.state = AltTabState::Idle;

        if chosen_idx < self.windows.len() {
            let chosen = self.windows.remove(chosen_idx);
            let id = chosen.id;
            self.windows.insert(0, chosen);
            Some(id)
        } else {
            None
        }
    }

    /// Esc or cancel: dismisses overlay without changing focus or MRU order.
    pub fn cancel(&mut self) {
        self.state = AltTabState::Idle;
    }

    /// Inserts or updates a window in the switcher list.
    pub fn upsert_window(&mut self, window: AltTabWindow) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == window.id) {
            self.windows[pos] = window;
        } else {
            self.windows.push(window);
        }
    }

    /// Removes a closed window.
    pub fn remove_window(&mut self, id: WindowId) -> Option<AltTabWindow> {
        let removed = if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let item = self.windows.remove(pos);
            if let AltTabState::Active { selected_index } = self.state {
                if self.windows.is_empty() {
                    self.state = AltTabState::Idle;
                } else if selected_index >= self.windows.len() {
                    self.state = AltTabState::Active {
                        selected_index: self.windows.len().saturating_sub(1),
                    };
                }
            }
            Some(item)
        } else {
            None
        };
        removed
    }

    /// Moves a window to the top of the MRU list (e.g. when foreign window is activated).
    pub fn bring_to_front(&mut self, id: WindowId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            if pos > 0 {
                let win = self.windows.remove(pos);
                self.windows.insert(0, win);
            }
        }
    }
}
