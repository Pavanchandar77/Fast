//! Pure Window and Workspace State Machine for Nexus-OS.
//! Solves non-deterministic HashMap window ordering and off-workspace window visibility issues.
//! Pure, fully deterministic, zero OS dependencies.

use crate::layout::{LayoutEngine, LayoutKind, LayoutOptions, Rect, WindowId};
use crate::spring::{SpringConfig, SpringRect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedWindowState {
    pub id: WindowId,
    pub title: String,
    pub workspace: u8,
    pub is_floating: bool,
    pub is_minimized: bool,
    pub is_fullscreen: bool,
    pub is_cloaked: bool,
    pub target_rect: Rect,
    #[serde(skip)]
    pub spring: SpringRect,
}

impl ManagedWindowState {
    pub fn new(
        id: WindowId,
        title: String,
        workspace: u8,
        initial_rect: Rect,
        spring_cfg: SpringConfig,
    ) -> Self {
        let spring = SpringRect::new(
            initial_rect.x as f32,
            initial_rect.y as f32,
            initial_rect.width as f32,
            initial_rect.height as f32,
            spring_cfg,
        );

        Self {
            id,
            title,
            workspace,
            is_floating: false,
            is_minimized: false,
            is_fullscreen: false,
            is_cloaked: false,
            target_rect: initial_rect,
            spring,
        }
    }
}

/// Commands produced by state transitions for the shell platform to execute
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowMutation {
    MoveResize { id: WindowId, rect: Rect },
    Show { id: WindowId },
    Hide { id: WindowId },
    Focus { id: WindowId },
    Close { id: WindowId },
}

/// High-level events processed by the window manager state machine
#[derive(Debug, Clone, PartialEq)]
pub enum StateEvent {
    WindowCreated {
        id: WindowId,
        title: String,
        initial_rect: Rect,
    },
    WindowDestroyed {
        id: WindowId,
    },
    WindowFocused {
        id: WindowId,
    },
    WindowMinimized {
        id: WindowId,
    },
    WindowRestored {
        id: WindowId,
    },
    WindowCloaked {
        id: WindowId,
    },
    WindowUncloaked {
        id: WindowId,
    },
    SwitchWorkspace {
        workspace: u8,
    },
    MoveWindowToWorkspace {
        id: WindowId,
        workspace: u8,
    },
    ToggleFloating {
        id: WindowId,
    },
    ToggleFullscreen {
        id: WindowId,
    },
    ScreenResized {
        screen: Rect,
    },
    TickAnimations {
        dt: f32,
    },
}

pub struct WindowManagerStateMachine {
    windows: HashMap<WindowId, ManagedWindowState>,
    // Deterministic ordered list of window IDs per workspace
    workspace_windows: HashMap<u8, VecDeque<WindowId>>,
    active_workspace: u8,
    focused_window: Option<WindowId>,
    screen: Rect,
    layout_kind: LayoutKind,
    layout_opts: LayoutOptions,
    spring_cfg: SpringConfig,
}

impl WindowManagerStateMachine {
    pub fn new(
        screen: Rect,
        layout_kind: LayoutKind,
        layout_opts: LayoutOptions,
        spring_cfg: SpringConfig,
    ) -> Self {
        Self {
            windows: HashMap::new(),
            workspace_windows: HashMap::new(),
            active_workspace: 1,
            focused_window: None,
            screen,
            layout_kind,
            layout_opts,
            spring_cfg,
        }
    }

    pub fn active_workspace(&self) -> u8 {
        self.active_workspace
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused_window
    }

    pub fn get_window(&self, id: WindowId) -> Option<&ManagedWindowState> {
        self.windows.get(&id)
    }

    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut ManagedWindowState> {
        self.windows.get_mut(&id)
    }

    pub fn all_windows(&self) -> impl Iterator<Item = &ManagedWindowState> {
        self.windows.values()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn set_screen(&mut self, screen: Rect) {
        self.screen = screen;
    }

    pub fn set_layout_kind(&mut self, kind: LayoutKind) {
        self.layout_kind = kind;
    }

    pub fn set_layout_options(&mut self, opts: LayoutOptions) {
        self.layout_opts = opts;
    }

    /// Primary event handling dispatch. Mutates internal state deterministically
    /// and returns the list of platform mutations to perform.
    pub fn handle_event(&mut self, event: StateEvent) -> Vec<WindowMutation> {
        let mut mutations = Vec::new();

        match event {
            StateEvent::WindowCreated {
                id,
                title,
                initial_rect,
            } => {
                if self.windows.contains_key(&id) {
                    return mutations;
                }

                let managed = ManagedWindowState::new(
                    id,
                    title,
                    self.active_workspace,
                    initial_rect,
                    self.spring_cfg,
                );
                self.windows.insert(id, managed);
                self.workspace_windows
                    .entry(self.active_workspace)
                    .or_default()
                    .push_back(id);

                self.focused_window = Some(id);
                self.retile_workspace(self.active_workspace, &mut mutations);
            }

            StateEvent::WindowDestroyed { id } => {
                if let Some(win) = self.windows.remove(&id) {
                    if let Some(queue) = self.workspace_windows.get_mut(&win.workspace) {
                        queue.retain(|&w| w != id);
                    }

                    if self.focused_window == Some(id) {
                        self.focused_window = self
                            .workspace_windows
                            .get(&self.active_workspace)
                            .and_then(|q| q.front().copied());
                        if let Some(new_focus) = self.focused_window {
                            mutations.push(WindowMutation::Focus { id: new_focus });
                        }
                    }

                    self.retile_workspace(win.workspace, &mut mutations);
                }
            }

            StateEvent::WindowFocused { id } => {
                if !self.windows.contains_key(&id) {
                    return mutations;
                }

                self.focused_window = Some(id);
                // Move focused window to front of workspace queue for master pane promotion if desired
                if let Some(win) = self.windows.get(&id) {
                    let ws = win.workspace;
                    if let Some(queue) = self.workspace_windows.get_mut(&ws) {
                        if let Some(idx) = queue.iter().position(|&w| w == id) {
                            if let Some(item) = queue.remove(idx) {
                                queue.push_front(item);
                            }
                        }
                    }
                }
            }

            StateEvent::WindowMinimized { id } => {
                let ws = self.windows.get_mut(&id).map(|win| {
                    win.is_minimized = true;
                    win.workspace
                });
                if let Some(ws) = ws {
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::WindowRestored { id } => {
                let ws = self.windows.get_mut(&id).map(|win| {
                    win.is_minimized = false;
                    win.is_cloaked = false;
                    win.workspace
                });
                if let Some(ws) = ws {
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::WindowCloaked { id } => {
                let ws = self.windows.get_mut(&id).map(|win| {
                    win.is_cloaked = true;
                    win.workspace
                });
                if let Some(ws) = ws {
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::WindowUncloaked { id } => {
                let ws = self.windows.get_mut(&id).map(|win| {
                    win.is_cloaked = false;
                    win.workspace
                });
                if let Some(ws) = ws {
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::SwitchWorkspace { workspace } => {
                if workspace == self.active_workspace || workspace == 0 {
                    return mutations;
                }

                let old_ws = self.active_workspace;
                self.active_workspace = workspace;

                // 1. Hide windows belonging to the old workspace
                for win in self.windows.values() {
                    if win.workspace == old_ws && !win.is_minimized && !win.is_cloaked {
                        mutations.push(WindowMutation::Hide { id: win.id });
                    }
                }

                // 2. Show windows belonging to the new active workspace
                for win in self.windows.values() {
                    if win.workspace == workspace && !win.is_minimized && !win.is_cloaked {
                        mutations.push(WindowMutation::Show { id: win.id });
                    }
                }

                // 3. Update focus
                self.focused_window = self
                    .workspace_windows
                    .get(&workspace)
                    .and_then(|q| q.front().copied());
                if let Some(f) = self.focused_window {
                    mutations.push(WindowMutation::Focus { id: f });
                }

                // 4. Retile the new active workspace
                self.retile_workspace(workspace, &mut mutations);
            }

            StateEvent::MoveWindowToWorkspace { id, workspace } => {
                if workspace == 0 {
                    return mutations;
                }

                let current_ws = match self.windows.get(&id) {
                    Some(w) => w.workspace,
                    None => return mutations,
                };

                if current_ws == workspace {
                    return mutations;
                }

                if let Some(old_queue) = self.workspace_windows.get_mut(&current_ws) {
                    old_queue.retain(|&w| w != id);
                }
                self.workspace_windows
                    .entry(workspace)
                    .or_default()
                    .push_back(id);

                if let Some(win) = self.windows.get_mut(&id) {
                    win.workspace = workspace;
                }

                // If moved away from active workspace, hide it
                if current_ws == self.active_workspace && workspace != self.active_workspace {
                    mutations.push(WindowMutation::Hide { id });
                } else if current_ws != self.active_workspace && workspace == self.active_workspace
                {
                    mutations.push(WindowMutation::Show { id });
                }

                self.retile_workspace(current_ws, &mut mutations);
                if workspace != current_ws {
                    self.retile_workspace(workspace, &mut mutations);
                }
            }

            StateEvent::ToggleFloating { id } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.is_floating = !win.is_floating;
                    let ws = win.workspace;
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::ToggleFullscreen { id } => {
                if let Some(win) = self.windows.get_mut(&id) {
                    win.is_fullscreen = !win.is_fullscreen;
                    let ws = win.workspace;
                    self.retile_workspace(ws, &mut mutations);
                }
            }

            StateEvent::ScreenResized { screen } => {
                self.screen = screen;
                self.retile_workspace(self.active_workspace, &mut mutations);
            }

            StateEvent::TickAnimations { dt } => {
                for win in self.windows.values_mut() {
                    if !win.spring.is_at_rest() {
                        win.spring.step(dt);
                        let (x, y, w, h) = win.spring.current_rect();
                        mutations.push(WindowMutation::MoveResize {
                            id: win.id,
                            rect: Rect::new(x, y, w, h),
                        });
                    }
                }
            }
        }

        mutations
    }

    /// Recomputes layout for a given workspace and synchronizes spring targets
    pub fn retile_workspace(&mut self, workspace_id: u8, mutations: &mut Vec<WindowMutation>) {
        let windows: Vec<WindowId> = self
            .workspace_windows
            .get(&workspace_id)
            .map(|q| {
                q.iter()
                    .copied()
                    .filter(|w| {
                        if let Some(win) = self.windows.get(w) {
                            !win.is_floating && !win.is_minimized && !win.is_cloaked
                        } else {
                            false
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if windows.is_empty() {
            return;
        }

        let computed = LayoutEngine::compute_layout(
            self.layout_kind,
            self.screen,
            &windows,
            &self.layout_opts,
        );

        for (id, rect) in computed {
            if let Some(win) = self.windows.get_mut(&id) {
                win.target_rect = rect;
                win.spring.set_target(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                );

                if workspace_id == self.active_workspace {
                    mutations.push(WindowMutation::MoveResize { id, rect });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_sm() -> WindowManagerStateMachine {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let spring = SpringConfig::default();
        WindowManagerStateMachine::new(screen, LayoutKind::MasterStack, opts, spring)
    }

    #[test]
    fn test_register_and_retile_window() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(100);
        let muts = sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "Test App".into(),
            initial_rect: Rect::new(50, 50, 400, 300),
        });

        assert_eq!(sm.window_count(), 1);
        assert_eq!(sm.focused_window(), Some(w1));
        assert!(muts
            .iter()
            .any(|m| matches!(m, WindowMutation::MoveResize { id, .. } if *id == w1)));
    }

    #[test]
    fn test_switch_workspace_hides_and_shows() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(100);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App WS 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        // Switch to workspace 2
        let muts = sm.handle_event(StateEvent::SwitchWorkspace { workspace: 2 });
        assert_eq!(sm.active_workspace(), 2);
        // Window on WS 1 must be hidden!
        assert!(muts.contains(&WindowMutation::Hide { id: w1 }));

        // Create window on WS 2
        let w2 = WindowId::new(200);
        sm.handle_event(StateEvent::WindowCreated {
            id: w2,
            title: "App WS 2".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        // Switch back to workspace 1
        let muts_back = sm.handle_event(StateEvent::SwitchWorkspace { workspace: 1 });
        assert_eq!(sm.active_workspace(), 1);
        // Window on WS 2 must be hidden, window on WS 1 shown!
        assert!(muts_back.contains(&WindowMutation::Hide { id: w2 }));
        assert!(muts_back.contains(&WindowMutation::Show { id: w1 }));
    }

    #[test]
    fn test_move_window_to_workspace() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(100);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        let muts = sm.handle_event(StateEvent::MoveWindowToWorkspace {
            id: w1,
            workspace: 3,
        });
        assert_eq!(sm.get_window(w1).unwrap().workspace, 3);
        assert!(muts.contains(&WindowMutation::Hide { id: w1 }));
    }

    #[test]
    fn test_destroy_window_updates_focus() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        let w2 = WindowId::new(2);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });
        sm.handle_event(StateEvent::WindowCreated {
            id: w2,
            title: "App 2".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        let muts = sm.handle_event(StateEvent::WindowDestroyed { id: w2 });
        assert_eq!(sm.window_count(), 1);
        assert_eq!(sm.focused_window(), Some(w1));
        assert!(muts.contains(&WindowMutation::Focus { id: w1 }));
    }

    #[test]
    fn test_toggle_floating() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        assert!(!sm.get_window(w1).unwrap().is_floating);
        sm.handle_event(StateEvent::ToggleFloating { id: w1 });
        assert!(sm.get_window(w1).unwrap().is_floating);
        sm.handle_event(StateEvent::ToggleFloating { id: w1 });
        assert!(!sm.get_window(w1).unwrap().is_floating);
    }

    #[test]
    fn test_toggle_fullscreen() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        assert!(!sm.get_window(w1).unwrap().is_fullscreen);
        sm.handle_event(StateEvent::ToggleFullscreen { id: w1 });
        assert!(sm.get_window(w1).unwrap().is_fullscreen);
    }

    #[test]
    fn test_cloaked_and_uncloaked_window() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        sm.handle_event(StateEvent::WindowCloaked { id: w1 });
        assert!(sm.get_window(w1).unwrap().is_cloaked);

        sm.handle_event(StateEvent::WindowUncloaked { id: w1 });
        assert!(!sm.get_window(w1).unwrap().is_cloaked);
    }

    #[test]
    fn test_minimized_and_restored_window() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        sm.handle_event(StateEvent::WindowMinimized { id: w1 });
        assert!(sm.get_window(w1).unwrap().is_minimized);

        sm.handle_event(StateEvent::WindowRestored { id: w1 });
        assert!(!sm.get_window(w1).unwrap().is_minimized);
    }

    #[test]
    fn test_screen_resize() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        let new_screen = Rect::new(0, 0, 2560, 1440);
        let muts = sm.handle_event(StateEvent::ScreenResized { screen: new_screen });
        assert!(muts
            .iter()
            .any(|m| matches!(m, WindowMutation::MoveResize { id, .. } if *id == w1)));
    }

    #[test]
    fn test_tick_animations() {
        let mut sm = setup_sm();
        let w1 = WindowId::new(1);
        sm.handle_event(StateEvent::WindowCreated {
            id: w1,
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });

        let muts = sm.handle_event(StateEvent::TickAnimations { dt: 1.0 / 60.0 });
        // Animation tick produced MoveResize mutations
        assert!(!muts.is_empty());
    }

    #[test]
    fn test_all_windows_iterator() {
        let mut sm = setup_sm();
        sm.handle_event(StateEvent::WindowCreated {
            id: WindowId::new(1),
            title: "App 1".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });
        sm.handle_event(StateEvent::WindowCreated {
            id: WindowId::new(2),
            title: "App 2".into(),
            initial_rect: Rect::new(0, 0, 800, 600),
        });
        let titles: Vec<String> = sm.all_windows().map(|w| w.title.clone()).collect();
        assert_eq!(titles.len(), 2);
    }
}
