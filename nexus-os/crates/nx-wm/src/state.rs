use crate::animation::{AnimationReason, LayoutTransition};
use crate::focus::{find_directional_candidate, find_window_at_point};
use crate::geometry::{Direction, Point, Rect};
use crate::keybinding::{Action, Key, KeybindingMap, Modifiers};
use crate::layout::{compute_layout, LayoutKind, LayoutParams};
use crate::monitor::Monitor;
use crate::plan::{MutationPlan, WindowMutation, WindowSnapshot};
use crate::rules::{RuleAction, RuleEngine};
use crate::snapping::{compute_snap_rect, SnapZone};
use crate::types::{MonitorId, WindowId, WindowInfo, WindowState, WorkspaceId};
use std::collections::HashMap;
use tracing::{debug, info};

/// Complete pure window manager model for Nexus-OS.
/// Tracks multi-monitor state, per-monitor workspaces, ordered window trees,
/// window rules, keybinding dispatch, focus transitions, and produces
/// dry-run-able, reversible mutation plans.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowManagerModel {
    monitors: Vec<Monitor>,
    workspaces: HashMap<WorkspaceId, crate::workspace::Workspace>,
    windows: HashMap<WindowId, WindowState>,
    rules: RuleEngine,
    keybindings: KeybindingMap,
    active_monitor_id: MonitorId,
    follow_mouse: bool,
    default_layout: LayoutKind,
    default_layout_params: LayoutParams,
}

impl WindowManagerModel {
    pub fn new(primary_bounds: Rect, primary_work_area: Rect) -> Self {
        let primary_mon_id = MonitorId(0);
        let primary_monitor = Monitor::primary(primary_bounds, primary_work_area);

        let mut workspaces = HashMap::new();
        // Initialize workspaces 1 through 9
        for i in 1..=9 {
            let ws_id = WorkspaceId(i);
            let mon_id = if i == 1 { primary_mon_id } else { primary_mon_id };
            let ws = crate::workspace::Workspace::new(ws_id, mon_id, LayoutKind::Dwindle);
            workspaces.insert(ws_id, ws);
        }

        Self {
            monitors: vec![primary_monitor],
            workspaces,
            windows: HashMap::new(),
            rules: RuleEngine::with_defaults(),
            keybindings: KeybindingMap::default(),
            active_monitor_id: primary_mon_id,
            follow_mouse: false,
            default_layout: LayoutKind::Dwindle,
            default_layout_params: LayoutParams::default(),
        }
    }

    pub fn with_keybindings(mut self, map: KeybindingMap) -> Self {
        self.keybindings = map;
        self
    }

    pub fn with_rules(mut self, rules: RuleEngine) -> Self {
        self.rules = rules;
        self
    }

    pub fn set_follow_mouse(&mut self, enabled: bool) {
        self.follow_mouse = enabled;
    }

    pub fn set_default_layout(&mut self, layout: LayoutKind) {
        self.default_layout = layout;
    }

    pub fn set_default_layout_params(&mut self, params: LayoutParams) {
        self.default_layout_params = params;
    }

    // --- Query Accessors ---

    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    pub fn monitor(&self, id: MonitorId) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.id == id)
    }

    pub fn monitor_mut(&mut self, id: MonitorId) -> Option<&mut Monitor> {
        self.monitors.iter_mut().find(|m| m.id == id)
    }

    pub fn active_monitor(&self) -> &Monitor {
        self.monitor(self.active_monitor_id)
            .unwrap_or_else(|| &self.monitors[0])
    }

    pub fn active_workspace_id(&self) -> WorkspaceId {
        self.active_monitor().active_workspace
    }

    pub fn active_workspace(&self) -> &crate::workspace::Workspace {
        let ws_id = self.active_workspace_id();
        self.workspaces.get(&ws_id).expect("active workspace must exist")
    }

    pub fn active_workspace_mut(&mut self) -> &mut crate::workspace::Workspace {
        let ws_id = self.active_workspace_id();
        self.workspaces.get_mut(&ws_id).expect("active workspace must exist")
    }

    pub fn active_window(&self) -> Option<WindowId> {
        self.active_workspace().active_window()
    }

    pub fn active_window_rect(&self) -> Option<Rect> {
        let active = self.active_window()?;
        self.windows.get(&active).map(|w| w.target_rect)
    }

    pub fn window(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowState> {
        self.windows.get_mut(&id)
    }

    pub fn all_windows(&self) -> impl Iterator<Item = &WindowState> {
        self.windows.values()
    }

    pub fn workspace(&self, id: WorkspaceId) -> Option<&crate::workspace::Workspace> {
        self.workspaces.get(&id)
    }

    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut crate::workspace::Workspace> {
        self.workspaces.get_mut(&id)
    }

    // --- Window Registration & Show / Hide Handlers ---

    /// Handles WindowCreated event: if visible, registers and tiles immediately.
    pub fn on_window_created(&mut self, info: WindowInfo) -> (MutationPlan, Option<LayoutTransition>) {
        if info.is_visible {
            self.on_window_show(info)
        } else {
            // Window created invisible: record state without tiling yet
            let active_mon_id = self.active_monitor_id;
            let active_ws_id = self.active_workspace_id();
            let state = WindowState::new(info.clone(), active_ws_id, active_mon_id, false, false);
            self.windows.insert(info.id, state);
            (MutationPlan::new(), None)
        }
    }

    /// Handles WindowShow event: registered if new, un-minimized if existing, then retiled.
    /// This fixes the audit bug where new windows were never tiled on SHOW!
    pub fn on_window_show(&mut self, info: WindowInfo) -> (MutationPlan, Option<LayoutTransition>) {
        let id = info.id;
        let mut plan = MutationPlan::new();

        if let Some(win) = self.windows.get_mut(&id) {
            win.is_minimized = false;
            win.is_visible = true;
            win.is_cloaked = false;
            win.title = info.title;
            win.class_name = info.class_name;
            win.process_name = info.process_name;

            let ws_id = win.workspace_id;
            if let Some(ws) = self.workspaces.get_mut(&ws_id) {
                if !ws.contains_window(id) {
                    ws.add_window(id);
                }
            }

            if ws_id == self.active_workspace_id() {
                plan.push(WindowMutation::SetCloak { window_id: id, cloaked: false });
                plan.push(WindowMutation::SetVisibility { window_id: id, visible: true });
                plan.push(WindowMutation::SetFocus { window_id: id });
            }

            let (retile_plan, trans) = self.retile_workspace(ws_id, AnimationReason::Retile);
            plan.extend(retile_plan);
            return (plan, trans);
        }

        // New window! Evaluate rules
        let active_mon = self.active_monitor().clone();
        let active_ws_id = active_mon.active_workspace;

        let rule_actions = self.rules.evaluate(&info);
        let mut is_floating = info.is_dialog_or_popup;
        let mut target_ws_id = active_ws_id;
        let mut is_auto_excluded = RuleEngine::is_auto_excluded_game(&info, active_mon.bounds);

        for action in &rule_actions {
            match action {
                RuleAction::Float => is_floating = true,
                RuleAction::Tile => is_floating = false,
                RuleAction::AssignWorkspace(ws) => target_ws_id = WorkspaceId(*ws),
                RuleAction::AutoExclude => is_auto_excluded = true,
                RuleAction::PiP => is_floating = true,
                RuleAction::SetInitialSize { .. } => {
                    is_floating = true;
                }
            }
        }

        let target_mon_id = self
            .workspaces
            .get(&target_ws_id)
            .map(|ws| ws.monitor_id)
            .unwrap_or(active_mon.id);

        let mut state = WindowState::new(
            info.clone(),
            target_ws_id,
            target_mon_id,
            is_floating,
            is_auto_excluded,
        );

        for action in &rule_actions {
            if let RuleAction::SetInitialSize { width, height } = action {
                state.floating_rect.width = *width;
                state.floating_rect.height = *height;
            }
        }

        self.windows.insert(id, state);

        // Add to target workspace
        debug!("Window {} registered to workspace {}", id, target_ws_id);
        if let Some(ws) = self.workspaces.get_mut(&target_ws_id) {
            ws.add_window(id);
        }

        // If target workspace is currently active, ensure window is uncloaked, visible, and focused
        if target_ws_id == active_ws_id {
            plan.push(WindowMutation::SetCloak { window_id: id, cloaked: false });
            plan.push(WindowMutation::SetVisibility { window_id: id, visible: true });
            plan.push(WindowMutation::SetFocus { window_id: id });

            let (retile_plan, transition) = self.retile_workspace(target_ws_id, AnimationReason::WindowOpened);
            plan.extend(retile_plan);
            (plan, transition)
        } else {
            // Target workspace is inactive: cloak & hide the window
            if let Some(win) = self.windows.get_mut(&id) {
                win.is_cloaked = true;
                win.is_visible = false;
            }
            plan.push(WindowMutation::SetCloak { window_id: id, cloaked: true });
            plan.push(WindowMutation::SetVisibility { window_id: id, visible: false });
            (plan, None)
        }
    }

    pub fn on_window_hide(&mut self, id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.is_visible = false;
            win.is_minimized = true;
            let ws_id = win.workspace_id;
            self.retile_workspace(ws_id, AnimationReason::WindowClosed)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn on_window_destroyed(&mut self, id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        let ws_id = if let Some(win) = self.windows.remove(&id) {
            win.workspace_id
        } else {
            return (MutationPlan::new(), None);
        };

        if let Some(ws) = self.workspaces.get_mut(&ws_id) {
            ws.remove_window(id);
        }

        self.retile_workspace(ws_id, AnimationReason::WindowClosed)
    }

    pub fn on_window_cloaked(&mut self, id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.is_cloaked = true;
            let ws_id = win.workspace_id;
            self.retile_workspace(ws_id, AnimationReason::Retile)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn on_window_uncloaked(&mut self, id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.is_cloaked = false;
            win.is_minimized = false;
            let ws_id = win.workspace_id;
            self.retile_workspace(ws_id, AnimationReason::Retile)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn on_manual_move_resize(&mut self, id: WindowId, new_rect: Rect) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.current_rect = new_rect;
            win.target_rect = new_rect;
            if win.is_floating {
                win.floating_rect = new_rect;
            }
        }
    }

    // --- Workspace & Layout Retiling ---

    /// Pure function computing target positions for all tileable windows on `workspace_id`.
    /// Emits a MutationPlan with `SetPosition` and a `LayoutTransition` carrying from/to coordinates.
    pub fn retile_workspace(
        &mut self,
        workspace_id: WorkspaceId,
        reason: AnimationReason,
    ) -> (MutationPlan, Option<LayoutTransition>) {
        let ws = match self.workspaces.get(&workspace_id) {
            Some(w) => w,
            None => return (MutationPlan::new(), None),
        };

        let mon_id = ws.monitor_id;
        let mon = match self.monitor(mon_id) {
            Some(m) => m,
            None => return (MutationPlan::new(), None),
        };

        let work_area = mon.work_area;
        let layout_kind = ws.layout;
        let layout_params = ws.layout_params.clone();

        // Collect tileable window IDs in tree order
        let tileable_ids: Vec<WindowId> = ws
            .tree
            .iter()
            .copied()
            .filter(|id| {
                self.windows
                    .get(id)
                    .map(|w| w.is_tileable())
                    .unwrap_or(false)
            })
            .collect();

        let targets = compute_layout(layout_kind, &tileable_ids, work_area, &layout_params);

        let mut plan = MutationPlan::new();
        let mut transition = LayoutTransition::new(reason);

        for (win_id, target_rect) in targets {
            if let Some(win) = self.windows.get_mut(&win_id) {
                let from_rect = win.target_rect;
                win.target_rect = target_rect;

                plan.push(WindowMutation::SetPosition {
                    window_id: win_id,
                    rect: target_rect,
                    async_pos: true,
                });

                transition.add(win_id, from_rect, target_rect);
            }
        }

        let trans_opt = if transition.is_empty() { None } else { Some(transition) };
        (plan, trans_opt)
    }

    // --- Workspace Switching ---

    /// Switches active workspace on the active monitor.
    /// Purely generates mutations to:
    /// - Cloak and hide windows of the outgoing workspace
    /// - Uncloak, show, and position windows of the incoming workspace
    /// - Focus the last active window on the incoming workspace
    pub fn switch_workspace(&mut self, target_ws_id: WorkspaceId) -> (MutationPlan, Option<LayoutTransition>) {
        let current_ws_id = self.active_workspace_id();

        if current_ws_id == target_ws_id {
            return (MutationPlan::new(), None);
        }

        let target_mon_id = self
            .workspaces
            .get(&target_ws_id)
            .map(|ws| ws.monitor_id)
            .unwrap_or(self.active_monitor_id);
        self.active_monitor_id = target_mon_id;

        debug!("Switching active workspace to {} on monitor {}", target_ws_id, target_mon_id);
        let mut plan = MutationPlan::new();

        // 1. Cloak and hide windows on outgoing workspace
        if let Some(outgoing_ws) = self.workspaces.get_mut(&current_ws_id) {
            outgoing_ws.is_visible = false;
            for win_id in outgoing_ws.tree.iter().copied() {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    win.is_cloaked = true;
                    plan.push(WindowMutation::SetCloak { window_id: win_id, cloaked: true });
                    plan.push(WindowMutation::SetVisibility { window_id: win_id, visible: false });
                }
            }
        }

        // 2. Activate incoming workspace on monitor
        if let Some(mon) = self.monitor_mut(target_mon_id) {
            mon.assign_workspace(target_ws_id);
            mon.set_active_workspace(target_ws_id);
        }

        // 3. Uncloak and show windows on incoming workspace
        let mut focus_win = None;
        if let Some(incoming_ws) = self.workspaces.get_mut(&target_ws_id) {
            incoming_ws.is_visible = true;
            focus_win = incoming_ws.last_focused.or_else(|| incoming_ws.tree.active_window());

            for win_id in incoming_ws.tree.iter().copied() {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    win.is_cloaked = false;
                    win.is_visible = true;
                    win.monitor_id = target_mon_id;
                    plan.push(WindowMutation::SetCloak { window_id: win_id, cloaked: false });
                    plan.push(WindowMutation::SetVisibility { window_id: win_id, visible: true });
                }
            }
        }

        if let Some(focus_id) = focus_win {
            plan.push(WindowMutation::SetFocus { window_id: focus_id });
        }

        // 4. Retile incoming workspace
        let (retile_plan, transition) = self.retile_workspace(target_ws_id, AnimationReason::WorkspaceSwitch);
        plan.extend(retile_plan);

        (plan, transition)
    }

    /// Moves a window from its current workspace to `target_ws_id`.
    pub fn move_window_to_workspace(
        &mut self,
        win_id: WindowId,
        target_ws_id: WorkspaceId,
    ) -> (MutationPlan, Option<LayoutTransition>) {
        let (current_ws_id, mon_id) = match self.windows.get(&win_id) {
            Some(w) => (w.workspace_id, w.monitor_id),
            None => return (MutationPlan::new(), None),
        };

        if current_ws_id == target_ws_id {
            return (MutationPlan::new(), None);
        }

        debug!("Switching active workspace to {}", target_ws_id);
        let mut plan = MutationPlan::new();

        // Remove from old workspace
        if let Some(old_ws) = self.workspaces.get_mut(&current_ws_id) {
            old_ws.remove_window(win_id);
        }

        // Add to new workspace
        if let Some(new_ws) = self.workspaces.get_mut(&target_ws_id) {
            new_ws.add_window(win_id);
        }

        // Update window metadata
        let is_target_active = target_ws_id == self.active_workspace_id();
        if let Some(win) = self.windows.get_mut(&win_id) {
            win.workspace_id = target_ws_id;
            win.monitor_id = mon_id;

            if is_target_active {
                win.is_cloaked = false;
                win.is_visible = true;
                plan.push(WindowMutation::SetCloak { window_id: win_id, cloaked: false });
                plan.push(WindowMutation::SetVisibility { window_id: win_id, visible: true });
            } else {
                win.is_cloaked = true;
                win.is_visible = false;
                plan.push(WindowMutation::SetCloak { window_id: win_id, cloaked: true });
                plan.push(WindowMutation::SetVisibility { window_id: win_id, visible: false });
            }
        }

        // Retile old workspace
        let (old_retile, _) = self.retile_workspace(current_ws_id, AnimationReason::WindowMoved);
        plan.extend(old_retile);

        // Retile target workspace if active
        let mut transition = None;
        if is_target_active {
            let (new_retile, trans) = self.retile_workspace(target_ws_id, AnimationReason::WindowMoved);
            plan.extend(new_retile);
            transition = trans;
        }

        (plan, transition)
    }

    // --- Window Toggles (Floating, Fullscreen, Snapping) ---

    pub fn toggle_floating(&mut self, win_id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        let (ws_id, will_float) = match self.windows.get_mut(&win_id) {
            Some(w) => {
                w.is_floating = !w.is_floating;
                (w.workspace_id, w.is_floating)
            }
            None => return (MutationPlan::new(), None),
        };

        let mut plan = MutationPlan::new();
        plan.push(WindowMutation::SetFloating {
            window_id: win_id,
            floating: will_float,
        });

        if will_float {
            if let Some(win) = self.windows.get_mut(&win_id) {
                let rect = win.floating_rect;
                win.target_rect = rect;
                plan.push(WindowMutation::SetPosition {
                    window_id: win_id,
                    rect,
                    async_pos: true,
                });
            }
        }

        let (retile_plan, trans) = self.retile_workspace(ws_id, AnimationReason::FloatingToggle);
        plan.extend(retile_plan);

        (plan, trans)
    }

    pub fn toggle_fullscreen(&mut self, win_id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        let (ws_id, mon_id, was_fullscreen) = match self.windows.get(&win_id) {
            Some(w) => (w.workspace_id, w.monitor_id, w.is_fullscreen),
            None => return (MutationPlan::new(), None),
        };

        let mon_bounds = self.monitor(mon_id).map(|m| m.bounds).unwrap_or(Rect::new(0, 0, 1920, 1080));
        let mut plan = MutationPlan::new();
        let mut transition = LayoutTransition::new(AnimationReason::FullscreenToggle);

        if !was_fullscreen {
            // Enter fullscreen
            if let Some(win) = self.windows.get_mut(&win_id) {
                let from_rect = win.target_rect;
                win.saved_rect_before_fullscreen = Some(from_rect);
                win.is_fullscreen = true;
                win.target_rect = mon_bounds;

                plan.push(WindowMutation::SetFullscreen {
                    window_id: win_id,
                    fullscreen: true,
                    saved_rect: Some(from_rect),
                });
                plan.push(WindowMutation::SetPosition {
                    window_id: win_id,
                    rect: mon_bounds,
                    async_pos: true,
                });
                transition.add(win_id, from_rect, mon_bounds);
            }
        } else {
            // Exit fullscreen
            if let Some(win) = self.windows.get_mut(&win_id) {
                let from_rect = win.target_rect;
                let restored = win.saved_rect_before_fullscreen.unwrap_or(win.floating_rect);
                win.is_fullscreen = false;
                win.target_rect = restored;
                win.saved_rect_before_fullscreen = None;

                plan.push(WindowMutation::SetFullscreen {
                    window_id: win_id,
                    fullscreen: false,
                    saved_rect: None,
                });
                transition.add(win_id, from_rect, restored);
            }
        }

        let (retile_plan, _) = self.retile_workspace(ws_id, AnimationReason::FullscreenToggle);
        plan.extend(retile_plan);

        (plan, Some(transition))
    }

    pub fn snap_window(&mut self, win_id: WindowId, zone: SnapZone) -> (MutationPlan, Option<LayoutTransition>) {
        let mon = self.active_monitor().clone();
        let gap = self.active_workspace().layout_params.outer_gap;
        let snap_rect = compute_snap_rect(zone, mon.work_area, gap);

        let mut plan = MutationPlan::new();
        let mut transition = LayoutTransition::new(AnimationReason::Snap);

        if let Some(win) = self.windows.get_mut(&win_id) {
            let from_rect = win.target_rect;
            win.is_floating = true;
            win.floating_rect = snap_rect;
            win.target_rect = snap_rect;

            plan.push(WindowMutation::SetFloating {
                window_id: win_id,
                floating: true,
            });
            plan.push(WindowMutation::SetPosition {
                window_id: win_id,
                rect: snap_rect,
                async_pos: true,
            });
            transition.add(win_id, from_rect, snap_rect);
        }

        let ws_id = self.active_workspace_id();
        let (retile_plan, _) = self.retile_workspace(ws_id, AnimationReason::Snap);
        plan.extend(retile_plan);

        (plan, Some(transition))
    }

    // --- Focus & Navigation ---

    pub fn focus_window(&mut self, id: WindowId) -> (MutationPlan, Option<LayoutTransition>) {
        let ws_id = match self.windows.get(&id) {
            Some(w) => w.workspace_id,
            None => return (MutationPlan::new(), None),
        };

        if let Some(ws) = self.workspaces.get_mut(&ws_id) {
            ws.set_active_window(id);
        }

        let mut plan = MutationPlan::new();
        plan.push(WindowMutation::SetFocus { window_id: id });
        (plan, None)
    }

    pub fn focus_direction(&mut self, dir: Direction) -> (MutationPlan, Option<LayoutTransition>) {
        let active = match self.active_window() {
            Some(w) => w,
            None => return (MutationPlan::new(), None),
        };

        let rects: Vec<(WindowId, Rect)> = self
            .active_workspace()
            .tree
            .iter()
            .copied()
            .filter_map(|w_id| self.windows.get(&w_id).map(|w| (w_id, w.target_rect)))
            .collect();

        if let Some(cand) = find_directional_candidate(active, dir, &rects) {
            self.focus_window(cand)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn move_direction(&mut self, dir: Direction) -> (MutationPlan, Option<LayoutTransition>) {
        let active = match self.active_window() {
            Some(w) => w,
            None => return (MutationPlan::new(), None),
        };

        let rects: Vec<(WindowId, Rect)> = self
            .active_workspace()
            .tree
            .iter()
            .copied()
            .filter_map(|w_id| self.windows.get(&w_id).map(|w| (w_id, w.target_rect)))
            .collect();

        if let Some(neighbor) = find_directional_candidate(active, dir, &rects) {
            let ws_id = self.active_workspace_id();
            if let Some(ws) = self.workspaces.get_mut(&ws_id) {
                ws.tree.swap_windows(active, neighbor);
            }
            self.retile_workspace(ws_id, AnimationReason::WindowMoved)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn cycle_focus(&mut self, forward: bool) -> (MutationPlan, Option<LayoutTransition>) {
        let ws_id = self.active_workspace_id();
        let next = if let Some(ws) = self.workspaces.get_mut(&ws_id) {
            ws.cycle_window(forward)
        } else {
            None
        };

        if let Some(id) = next {
            let mut plan = MutationPlan::new();
            plan.push(WindowMutation::SetFocus { window_id: id });
            (plan, None)
        } else {
            (MutationPlan::new(), None)
        }
    }

    pub fn on_pointer_moved(&mut self, point: Point) -> (MutationPlan, Option<LayoutTransition>) {
        if !self.follow_mouse {
            return (MutationPlan::new(), None);
        }

        let rects: Vec<(WindowId, Rect)> = self
            .active_workspace()
            .tree
            .iter()
            .copied()
            .filter_map(|w_id| self.windows.get(&w_id).map(|w| (w_id, w.target_rect)))
            .collect();

        if let Some(hovered) = find_window_at_point(point, &rects) {
            if self.active_window() != Some(hovered) {
                return self.focus_window(hovered);
            }
        }

        (MutationPlan::new(), None)
    }

    // --- Multi-Monitor Management ---

    pub fn add_monitor(&mut self, monitor: Monitor) -> (MutationPlan, Option<LayoutTransition>) {
        let mon_id = monitor.id;
        if self.monitor(mon_id).is_none() {
            self.monitors.push(monitor);
        }
        (MutationPlan::new(), None)
    }

    /// Handles unplugging a monitor:
    /// Migrates all workspaces and windows from the unplugged monitor to the primary monitor.
    /// NEVER LOSES WINDOWS!
    pub fn remove_monitor(&mut self, monitor_id: MonitorId) -> (MutationPlan, Option<LayoutTransition>) {
        if self.monitors.len() <= 1 {
            // Cannot remove last remaining monitor
            return (MutationPlan::new(), None);
        }

        self.monitors.retain(|m| m.id != monitor_id);
        let primary_id = self.monitors[0].id;
        info!(
            "Monitor {} removed, migrating workspaces to monitor {}",
            monitor_id, primary_id
        );
        if self.active_monitor_id == monitor_id {
            self.active_monitor_id = primary_id;
        }

        let mut plan = MutationPlan::new();

        // Migrate all workspaces and windows from removed monitor
        for ws in self.workspaces.values_mut() {
            if ws.monitor_id == monitor_id {
                ws.monitor_id = primary_id;
                for win_id in ws.tree.iter().copied() {
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        win.monitor_id = primary_id;
                    }
                }
            }
        }

        // Add workspaces to surviving primary monitor
        let migrated_ws_ids: Vec<WorkspaceId> = self
            .workspaces
            .iter()
            .filter(|(_, ws)| ws.monitor_id == primary_id)
            .map(|(&ws_id, _)| ws_id)
            .collect();

        if let Some(prim_mon) = self.monitor_mut(primary_id) {
            for ws_id in &migrated_ws_ids {
                prim_mon.assign_workspace(*ws_id);
            }
        }

        let mut transition = None;
        for &ws_id in &migrated_ws_ids {
            let (ws_plan, trans) = self.retile_workspace(ws_id, AnimationReason::MonitorChanged);
            plan.extend(ws_plan);
            if trans.is_some() {
                transition = trans;
            }
        }

        (plan, transition)
    }

    pub fn reposition_monitor(
        &mut self,
        monitor_id: MonitorId,
        bounds: Rect,
        work_area: Rect,
        scale: f64,
    ) -> (MutationPlan, Option<LayoutTransition>) {
        if let Some(mon) = self.monitor_mut(monitor_id) {
            mon.bounds = bounds;
            mon.work_area = work_area;
            mon.scale_factor = scale;
        }

        let active_ws = self.active_workspace_id();
        self.retile_workspace(active_ws, AnimationReason::MonitorChanged)
    }

    // --- Keybindings Dispatch ---

    pub fn handle_key(
        &mut self,
        modifiers: Modifiers,
        key: Key,
    ) -> Option<(Action, MutationPlan, Option<LayoutTransition>)> {
        // If the shortcut is not in our bindings, check if it's a reserved Windows shortcut
        // and immediately pass through!
        let action = self.keybindings.lookup(modifiers, key)?.clone();

        let (mut plan, trans) = match &action {
            Action::CloseWindow => {
                let mut p = MutationPlan::new();
                if let Some(active) = self.active_window() {
                    // Plan window destruction / close
                    p.push(WindowMutation::SetVisibility {
                        window_id: active,
                        visible: false,
                    });
                }
                (p, None)
            }
            Action::ToggleFullscreen => {
                if let Some(active) = self.active_window() {
                    self.toggle_fullscreen(active)
                } else {
                    (MutationPlan::new(), None)
                }
            }
            Action::ToggleFloating => {
                if let Some(active) = self.active_window() {
                    self.toggle_floating(active)
                } else {
                    (MutationPlan::new(), None)
                }
            }
            Action::FocusDirection(dir) => self.focus_direction(*dir),
            Action::MoveDirection(dir) => self.move_direction(*dir),
            Action::CycleWindow(forward) => self.cycle_focus(*forward),
            Action::FocusWorkspace(ws) => self.switch_workspace(WorkspaceId(*ws)),
            Action::MoveToWorkspace(ws) => {
                if let Some(active) = self.active_window() {
                    self.move_window_to_workspace(active, WorkspaceId(*ws))
                } else {
                    (MutationPlan::new(), None)
                }
            }
            Action::SetLayout(l) => {
                let ws_id = self.active_workspace_id();
                if let Some(ws) = self.workspaces.get_mut(&ws_id) {
                    ws.layout = *l;
                }
                self.retile_workspace(ws_id, AnimationReason::LayoutChanged)
            }
            Action::CycleLayout => {
                let ws_id = self.active_workspace_id();
                if let Some(ws) = self.workspaces.get_mut(&ws_id) {
                    ws.layout = match ws.layout {
                        LayoutKind::Dwindle => LayoutKind::MasterStack,
                        LayoutKind::MasterStack => LayoutKind::Scrolling,
                        LayoutKind::Scrolling => LayoutKind::Monocle,
                        LayoutKind::Monocle => LayoutKind::Floating,
                        LayoutKind::Floating => LayoutKind::Dwindle,
                    };
                }
                self.retile_workspace(ws_id, AnimationReason::LayoutChanged)
            }
            Action::Snap(zone) => {
                if let Some(active) = self.active_window() {
                    self.snap_window(active, *zone)
                } else {
                    (MutationPlan::new(), None)
                }
            }
            _ => (MutationPlan::new(), None),
        };

        // When a Super-based shortcut is intercepted, plan Win-key-up masking
        // so the Windows Start Menu is not accidentally opened
        if modifiers.super_key {
            plan.push(WindowMutation::MaskWinKeyUp);
        }

        Some((action, plan, trans))
    }

    // --- Snapshots & Crash Recovery / Exit Plan ---

    /// Creates an in-memory snapshot of all window states for dry-run verification and reversibility.
    pub fn create_snapshots(&self) -> HashMap<WindowId, WindowSnapshot> {
        self.windows
            .iter()
            .map(|(&id, w)| {
                (
                    id,
                    WindowSnapshot {
                        window_id: id,
                        rect: w.target_rect,
                        visible: w.is_visible,
                        cloaked: w.is_cloaked,
                        is_floating: w.is_floating,
                        is_fullscreen: w.is_fullscreen,
                    },
                )
            })
            .collect()
    }

    /// Emits a comprehensive mutation plan to uncloak and show EVERY window
    /// across ALL workspaces before shutdown, ensuring Nexus leaves no ghost/invisible windows.
    pub fn create_exit_plan(&self) -> MutationPlan {
        let mut plan = MutationPlan::new();
        for (&id, win) in &self.windows {
            // Restore visibility
            plan.push(WindowMutation::SetCloak { window_id: id, cloaked: false });
            plan.push(WindowMutation::SetVisibility { window_id: id, visible: true });
            // Restore normal rect if fullscreen
            if win.is_fullscreen {
                let restored = win.saved_rect_before_fullscreen.unwrap_or(win.floating_rect);
                plan.push(WindowMutation::SetFullscreen {
                    window_id: id,
                    fullscreen: false,
                    saved_rect: None,
                });
                plan.push(WindowMutation::SetPosition {
                    window_id: id,
                    rect: restored,
                    async_pos: false,
                });
            }
        }
        plan
    }
}
