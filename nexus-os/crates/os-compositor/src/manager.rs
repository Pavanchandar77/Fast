use crate::hooks::WindowEvent;
use crate::hotkeys::HotkeyAction;
use crate::spring::{SpringConfig, SpringRect};
use os_config::schema::{CompositorConfig, SpringConfigDto};
use os_core::window::WindowHandle;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[derive(Debug, Clone)]
pub struct ManagedWindow {
    pub handle: WindowHandle,
    pub spring: SpringRect,
    pub is_floating: bool,
    pub is_minimized: bool,
    pub workspace: u8,
    pub target_rect: (i32, i32, i32, i32),
}

pub struct WindowManager {
    config: Arc<RwLock<CompositorConfig>>,
    windows: HashMap<WindowHandle, ManagedWindow>,
    active_workspace: u8,
    active_window: Option<WindowHandle>,
    model: nx_wm::WindowManagerModel,
}

impl WindowManager {
    pub fn new(config: Arc<RwLock<CompositorConfig>>) -> Self {
        let (screen_w, screen_h) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        let top_bar_height = {
            let cfg = config.read();
            if cfg.top_bar.enabled {
                cfg.top_bar.height
            } else {
                0
            }
        };

        let bounds = nx_wm::Rect::new(0, 0, screen_w.max(1) as u32, screen_h.max(1) as u32);
        let work_area = nx_wm::Rect::new(
            0,
            top_bar_height,
            screen_w.max(1) as u32,
            (screen_h - top_bar_height).max(1) as u32,
        );

        let model = nx_wm::WindowManagerModel::new(bounds, work_area);

        Self {
            config,
            windows: HashMap::new(),
            active_workspace: 1,
            active_window: None,
            model,
        }
    }

    pub fn to_spring_config(dto: &SpringConfigDto) -> SpringConfig {
        SpringConfig {
            stiffness: dto.stiffness,
            damping: dto.damping,
            mass: dto.mass,
            rest_velocity_threshold: dto.rest_velocity_threshold,
            rest_displacement_threshold: dto.rest_displacement_threshold,
        }
    }

    pub fn model(&self) -> &nx_wm::WindowManagerModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut nx_wm::WindowManagerModel {
        &mut self.model
    }

    pub fn apply_plan(&mut self, plan: nx_wm::MutationPlan) {
        for m in plan.mutations {
            match m {
                nx_wm::WindowMutation::SetPosition {
                    window_id, rect, ..
                } => {
                    let hwnd = hwnd_from_id(window_id);
                    if let Some(win) = self.windows.get_mut(&hwnd) {
                        win.target_rect = (rect.x, rect.y, rect.width as i32, rect.height as i32);
                        win.spring.set_target(
                            rect.x as f32,
                            rect.y as f32,
                            rect.width as f32,
                            rect.height as f32,
                        );
                    }
                }
                nx_wm::WindowMutation::SetCloak { window_id, cloaked } => {
                    let hwnd = hwnd_from_id(window_id);
                    if let Some(win) = self.windows.get_mut(&hwnd) {
                        win.is_minimized = cloaked;
                    }
                    journal_cloak(window_id.0, cloaked);
                }
                nx_wm::WindowMutation::SetVisibility { window_id, visible } => {
                    let hwnd = hwnd_from_id(window_id);
                    if let Some(win) = self.windows.get_mut(&hwnd) {
                        win.is_minimized = !visible;
                    }
                    journal_visibility(window_id.0, visible);
                }
                nx_wm::WindowMutation::SetFocus { window_id } => {
                    self.active_window = Some(hwnd_from_id(window_id));
                }
                nx_wm::WindowMutation::SetFloating {
                    window_id,
                    floating,
                } => {
                    let hwnd = hwnd_from_id(window_id);
                    if let Some(win) = self.windows.get_mut(&hwnd) {
                        win.is_floating = floating;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn register_window(&mut self, hwnd: WindowHandle) {
        if !hwnd.is_manageable() {
            return;
        }

        if self.windows.contains_key(&hwnd) {
            return;
        }

        // 1. Enforce DWM animation suppression for instant interruptible spring physics
        if let Err(e) = hwnd.disable_dwm_transitions() {
            debug!(
                "Could not disable DWM transitions for {:?}: {:?}",
                hwnd.0, e
            );
        }

        let initial_rect = hwnd.rect().unwrap_or(RECT {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        });

        let x = initial_rect.left;
        let y = initial_rect.top;
        let w = (initial_rect.right - initial_rect.left).max(50);
        let h = (initial_rect.bottom - initial_rect.top).max(50);

        let spring_cfg = {
            let cfg = self.config.read();
            Self::to_spring_config(&cfg.spring)
        };

        let mut spring = SpringRect::new(x as f32, y as f32, w as f32, h as f32, spring_cfg);
        spring.set_target(x as f32, y as f32, w as f32, h as f32);

        let managed = ManagedWindow {
            handle: hwnd,
            spring,
            is_floating: false,
            is_minimized: hwnd.is_minimized(),
            workspace: self.active_workspace,
            target_rect: (x, y, w, h),
        };

        self.windows.insert(hwnd, managed);
        info!("Window registered to WM: {:?} ('{}')", hwnd.0, hwnd.title());

        let info = nx_wm::WindowInfo {
            id: nx_wm::WindowId(hwnd.0 .0 as u64),
            title: hwnd.title(),
            class_name: hwnd.class_name(),
            process_name: hwnd.pid().to_string(),
            rect: nx_wm::Rect::new(x, y, w as u32, h as u32),
            is_visible: hwnd.is_visible(),
            is_minimized: hwnd.is_minimized(),
            is_cloaked: hwnd.is_cloaked(),
            is_dialog_or_popup: false,
            style: 0,
            ex_style: 0,
        };

        let (plan, _) = self.model.on_window_show(info);
        self.apply_plan(plan);
        self.retile_workspace(self.active_workspace);
    }

    pub fn unregister_window(&mut self, hwnd: WindowHandle) {
        if self.windows.remove(&hwnd).is_some() {
            info!("Window unregistered from WM: {:?}", hwnd.0);
            if self.active_window == Some(hwnd) {
                self.active_window = None;
            }
            let (plan, _) = self
                .model
                .on_window_destroyed(nx_wm::WindowId(hwnd.0 .0 as u64));
            self.apply_plan(plan);
            self.retile_workspace(self.active_workspace);
        }
    }

    pub fn set_foreground(&mut self, hwnd: WindowHandle) {
        if !self.windows.contains_key(&hwnd) && hwnd.is_manageable() {
            self.register_window(hwnd);
        }
        self.active_window = Some(hwnd);
        let (plan, _) = self.model.focus_window(nx_wm::WindowId(hwnd.0 .0 as u64));
        self.apply_plan(plan);
    }

    pub fn retile_workspace(&mut self, workspace_id: u8) {
        let (screen_w, screen_h) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

        let (top_bar_height, inner_gap, outer_gap, master_ratio, layout_str) = {
            let cfg = self.config.read();
            (
                if cfg.top_bar.enabled {
                    cfg.top_bar.height
                } else {
                    0
                },
                cfg.inner_gap.max(0) as u32,
                cfg.outer_gap.max(0) as u32,
                cfg.master_ratio,
                cfg.layout.clone(),
            )
        };

        let bounds = nx_wm::Rect::new(0, 0, screen_w.max(1) as u32, screen_h.max(1) as u32);
        let work_area = nx_wm::Rect::new(
            0,
            top_bar_height,
            screen_w.max(1) as u32,
            (screen_h - top_bar_height).max(1) as u32,
        );

        let layout_kind = layout_str
            .parse::<nx_wm::LayoutKind>()
            .unwrap_or(nx_wm::LayoutKind::Dwindle);
        let layout_params = nx_wm::LayoutParams {
            inner_gap,
            outer_gap,
            master_ratio,
            master_count: 1,
            scroll_column_width: 0,
            scroll_active_index: 0,
        };

        if let Some(ws) = self.model.workspace_mut(nx_wm::WorkspaceId(workspace_id)) {
            ws.layout = layout_kind;
            ws.layout_params = layout_params;
        }

        let _ = self
            .model
            .reposition_monitor(nx_wm::MonitorId(0), bounds, work_area, 1.0);
        let (plan, _) = self.model.retile_workspace(
            nx_wm::WorkspaceId(workspace_id),
            nx_wm::AnimationReason::Retile,
        );
        self.apply_plan(plan);
    }

    /// Advances physics simulation and applies smooth window coordinates at 120Hz
    /// Uses SWP_ASYNCWINDOWPOS to strictly prevent hung applications from blocking compositor loop.
    pub fn update_animations(&mut self, dt: f32) {
        for win in self.windows.values_mut() {
            if !win.spring.is_at_rest() {
                win.spring.step(dt);
                let (x, y, w, h) = win.spring.current_rect();
                // `set_rect` journals the original rectangle once and moves
                // with `SWP_ASYNCWINDOWPOS`, so a hung window cannot stall the loop.
                let _ = win.handle.set_rect(RECT {
                    left: x,
                    top: y,
                    right: x.saturating_add(w),
                    bottom: y.saturating_add(h),
                });
            }
        }
    }

    pub fn handle_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::ForegroundChanged(hwnd) => {
                self.set_foreground(hwnd);
            }
            WindowEvent::WindowCreated(hwnd) => {
                self.register_window(hwnd);
            }
            WindowEvent::WindowDestroyed(hwnd) => {
                self.unregister_window(hwnd);
            }
            WindowEvent::WindowShow(hwnd) => {
                if !self.windows.contains_key(&hwnd) && hwnd.is_manageable() {
                    self.register_window(hwnd);
                } else if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = false;
                }
                let initial_rect = hwnd.rect().unwrap_or(RECT {
                    left: 100,
                    top: 100,
                    right: 900,
                    bottom: 700,
                });
                let info = nx_wm::WindowInfo {
                    id: nx_wm::WindowId(hwnd.0 .0 as u64),
                    title: hwnd.title(),
                    class_name: hwnd.class_name(),
                    process_name: hwnd.pid().to_string(),
                    rect: nx_wm::Rect::new(
                        initial_rect.left,
                        initial_rect.top,
                        (initial_rect.right - initial_rect.left).max(10) as u32,
                        (initial_rect.bottom - initial_rect.top).max(10) as u32,
                    ),
                    is_visible: hwnd.is_visible(),
                    is_minimized: hwnd.is_minimized(),
                    is_cloaked: hwnd.is_cloaked(),
                    is_dialog_or_popup: false,
                    style: 0,
                    ex_style: 0,
                };
                let (plan, _) = self.model.on_window_show(info);
                self.apply_plan(plan);
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowHide(hwnd) => {
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = true;
                }
                let (plan, _) = self.model.on_window_hide(nx_wm::WindowId(hwnd.0 .0 as u64));
                self.apply_plan(plan);
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowCloaked(hwnd) => {
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = true;
                }
                let (plan, _) = self
                    .model
                    .on_window_cloaked(nx_wm::WindowId(hwnd.0 .0 as u64));
                self.apply_plan(plan);
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowUncloaked(hwnd) => {
                if !self.windows.contains_key(&hwnd) && hwnd.is_manageable() {
                    self.register_window(hwnd);
                } else if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = false;
                }
                let (plan, _) = self
                    .model
                    .on_window_uncloaked(nx_wm::WindowId(hwnd.0 .0 as u64));
                self.apply_plan(plan);
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::MoveSizeEnd(hwnd) => {
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    if let Ok(rect) = hwnd.rect() {
                        let w = rect.right - rect.left;
                        let h = rect.bottom - rect.top;
                        win.target_rect = (rect.left, rect.top, w, h);
                        self.model.on_manual_move_resize(
                            nx_wm::WindowId(hwnd.0 .0 as u64),
                            nx_wm::Rect::new(
                                rect.left,
                                rect.top,
                                w.max(10) as u32,
                                h.max(10) as u32,
                            ),
                        );
                    }
                }
            }
        }
    }

    pub fn handle_hotkey(&mut self, action: HotkeyAction) {
        match action {
            HotkeyAction::FocusWorkspace(ws) => {
                info!("Switching to workspace {}", ws);
                self.active_workspace = ws;
                let (plan, _) = self.model.switch_workspace(nx_wm::WorkspaceId(ws));
                self.apply_plan(plan);
            }
            HotkeyAction::MoveToWorkspace(ws) => {
                if let Some(active) = self.active_window {
                    let (plan, _) = self.model.move_window_to_workspace(
                        nx_wm::WindowId(active.0 .0 as u64),
                        nx_wm::WorkspaceId(ws),
                    );
                    self.apply_plan(plan);
                }
            }
            HotkeyAction::ToggleFloating => {
                if let Some(active) = self.active_window {
                    let (plan, _) = self
                        .model
                        .toggle_floating(nx_wm::WindowId(active.0 .0 as u64));
                    self.apply_plan(plan);
                }
            }
            HotkeyAction::FocusDirection(dir) => {
                let nx_dir = match dir {
                    nexus_model::hotkeys::Direction::Left => nx_wm::Direction::Left,
                    nexus_model::hotkeys::Direction::Right => nx_wm::Direction::Right,
                    nexus_model::hotkeys::Direction::Up => nx_wm::Direction::Up,
                    nexus_model::hotkeys::Direction::Down => nx_wm::Direction::Down,
                };
                let (plan, _) = self.model.focus_direction(nx_dir);
                self.apply_plan(plan);
            }
            HotkeyAction::ToggleFullscreen => {
                if let Some(active) = self.active_window {
                    let (plan, _) = self
                        .model
                        .toggle_fullscreen(nx_wm::WindowId(active.0 .0 as u64));
                    self.apply_plan(plan);
                }
            }
            HotkeyAction::CloseWindow => {
                if let Some(active) = self.active_window {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            active.0,
                            windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                            windows::Win32::Foundation::WPARAM(0),
                            windows::Win32::Foundation::LPARAM(0),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    pub fn restore_all_on_exit(&mut self) {
        let plan = self.model.create_exit_plan();
        self.apply_plan(plan);
    }

    pub fn active_workspace(&self) -> u8 {
        self.active_workspace
    }

    pub fn active_window(&self) -> Option<WindowHandle> {
        self.active_window
    }

    pub fn active_window_rect(&self) -> Option<[f32; 4]> {
        if let Some(hwnd) = self.active_window {
            if let Some(win) = self.windows.get(&hwnd) {
                let (x, y, w, h) = win.spring.current_rect();
                return Some([x as f32, y as f32, w as f32, h as f32]);
            }
            if let Ok(rect) = hwnd.rect() {
                let w = (rect.right - rect.left).max(10);
                let h = (rect.bottom - rect.top).max(10);
                return Some([rect.left as f32, rect.top as f32, w as f32, h as f32]);
            }
        }
        None
    }

    pub fn active_window_title(&self) -> String {
        self.active_window.map(|h| h.title()).unwrap_or_default()
    }
}

fn hwnd_from_id(window_id: nx_wm::WindowId) -> WindowHandle {
    WindowHandle(windows::Win32::Foundation::HWND(window_id.0 as *mut _))
}

/// Cloak is a system mutation. Record it before DWM applies it, and undo the
/// same entry when the workspace comes back.
fn journal_cloak(hwnd: u64, cloaked: bool) {
    if os_core::recovery::is_dry_run() {
        info!("dry-run: cloak hwnd {hwnd:#x} -> {cloaked}");
        return;
    }
    if cloaked {
        let mutation = os_core::Mutation::WindowCloak { hwnd };
        if let Err(err) = os_core::recovery::commit(&mutation) {
            debug!("cloak journal failed for hwnd {hwnd:#x}: {err}");
        }
    } else {
        let _ = os_core::recovery::undo_last(
            |mutation| matches!(mutation, os_core::Mutation::WindowCloak { hwnd: found } if *found == hwnd),
        );
    }
}

/// Hide and show go through the journal so a crash cannot leave a window invisible.
fn journal_visibility(hwnd: u64, visible: bool) {
    if os_core::recovery::is_dry_run() {
        info!("dry-run: visible hwnd {hwnd:#x} -> {visible}");
        return;
    }
    if visible {
        let _ = os_core::recovery::undo_last(
            |mutation| matches!(mutation, os_core::Mutation::WindowHide { hwnd: found } if *found == hwnd),
        );
    } else {
        let mutation = os_core::Mutation::WindowHide { hwnd };
        if let Err(err) = os_core::recovery::commit(&mutation) {
            debug!("hide journal failed for hwnd {hwnd:#x}: {err}");
        }
    }
}
