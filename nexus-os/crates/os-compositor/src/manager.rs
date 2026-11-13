use std::collections::HashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, info};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetWindowPos, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SWP_NOZORDER,
};
use os_core::window::WindowHandle;
use os_config::schema::{CompositorConfig, SpringConfigDto};
use crate::hooks::WindowEvent;
use crate::hotkeys::HotkeyAction;
use crate::spring::{SpringConfig, SpringRect};

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
}

impl WindowManager {
    pub fn new(config: Arc<RwLock<CompositorConfig>>) -> Self {
        Self {
            config,
            windows: HashMap::new(),
            active_workspace: 1,
            active_window: None,
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

    pub fn register_window(&mut self, hwnd: WindowHandle) {
        if !hwnd.is_manageable() {
            return;
        }

        if self.windows.contains_key(&hwnd) {
            return;
        }

        // 1. Enforce DWM animation suppression for instant interruptible spring physics
        if let Err(e) = hwnd.disable_dwm_transitions() {
            debug!("Could not disable DWM transitions for {:?}: {:?}", hwnd.0, e);
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

        self.retile_workspace(self.active_workspace);
    }

    pub fn unregister_window(&mut self, hwnd: WindowHandle) {
        if self.windows.remove(&hwnd).is_some() {
            info!("Window unregistered from WM: {:?}", hwnd.0);
            if self.active_window == Some(hwnd) {
                self.active_window = None;
            }
            self.retile_workspace(self.active_workspace);
        }
    }

    pub fn set_foreground(&mut self, hwnd: WindowHandle) {
        if !self.windows.contains_key(&hwnd) && hwnd.is_manageable() {
            self.register_window(hwnd);
        }
        self.active_window = Some(hwnd);
    }

    pub fn retile_workspace(&mut self, workspace_id: u8) {
        let (screen_w, screen_h) = unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        };

        let (top_bar_height, inner_gap, outer_gap, master_ratio, layout) = {
            let cfg = self.config.read();
            (
                if cfg.top_bar.enabled { cfg.top_bar.height } else { 0 },
                cfg.inner_gap,
                cfg.outer_gap,
                cfg.master_ratio,
                cfg.layout.clone(),
            )
        };

        let usable_x = outer_gap;
        let usable_y = top_bar_height + outer_gap;
        let usable_w = screen_w - (outer_gap * 2);
        let usable_h = screen_h - top_bar_height - (outer_gap * 2);

        let workspace_windows: Vec<WindowHandle> = self
            .windows
            .iter()
            .filter(|(_, win)| {
                win.workspace == workspace_id
                    && !win.is_floating
                    && !win.is_minimized
                    && !win.handle.is_cloaked()
            })
            .map(|(h, _)| *h)
            .collect();

        let count = workspace_windows.len();
        if count == 0 {
            return;
        }

        if layout == "floating" {
            return;
        }

        if count == 1 {
            let hwnd = workspace_windows[0];
            if let Some(win) = self.windows.get_mut(&hwnd) {
                win.target_rect = (usable_x, usable_y, usable_w, usable_h);
                win.spring.set_target(
                    usable_x as f32,
                    usable_y as f32,
                    usable_w as f32,
                    usable_h as f32,
                );
            }
            return;
        }

        // Master-Stack & Dwindle tiling calculation
        let master_w = ((usable_w as f32 * master_ratio) - (inner_gap as f32 / 2.0)) as i32;
        let stack_w = usable_w - master_w - inner_gap;
        let stack_count = count - 1;
        let stack_slot_h = (usable_h - (inner_gap * (stack_count as i32 - 1))) / stack_count as i32;

        // Master pane (window index 0)
        let master_hwnd = workspace_windows[0];
        if let Some(win) = self.windows.get_mut(&master_hwnd) {
            win.target_rect = (usable_x, usable_y, master_w, usable_h);
            win.spring.set_target(
                usable_x as f32,
                usable_y as f32,
                master_w as f32,
                usable_h as f32,
            );
        }

        // Stack panes (window indices 1..count)
        let stack_x = usable_x + master_w + inner_gap;
        for (i, hwnd) in workspace_windows.iter().skip(1).enumerate() {
            let stack_y = usable_y + (i as i32 * (stack_slot_h + inner_gap));
            let h = if i == stack_count - 1 {
                // Fill any rounding difference
                usable_y + usable_h - stack_y
            } else {
                stack_slot_h
            };

            if let Some(win) = self.windows.get_mut(hwnd) {
                win.target_rect = (stack_x, stack_y, stack_w, h);
                win.spring.set_target(
                    stack_x as f32,
                    stack_y as f32,
                    stack_w as f32,
                    h as f32,
                );
            }
        }
    }

    /// Advances physics simulation and applies smooth window coordinates at 120Hz
    pub fn update_animations(&mut self, dt: f32) {
        for (_, win) in self.windows.iter_mut() {
            if !win.spring.is_at_rest() {
                win.spring.step(dt);
                let (x, y, w, h) = win.spring.current_rect();
                unsafe {
                    let _ = SetWindowPos(
                        win.handle.0,
                        HWND::default(),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
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
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = false;
                }
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowHide(hwnd) => {
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = true;
                }
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowCloaked(hwnd) => {
                // UWP / Windows 11 cloaked frame: immediately exclude from tiling layout
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = true;
                }
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::WindowUncloaked(hwnd) => {
                // UWP app uncloaked: register if new, un-minimize and retile
                if !self.windows.contains_key(&hwnd) && hwnd.is_manageable() {
                    self.register_window(hwnd);
                } else if let Some(win) = self.windows.get_mut(&hwnd) {
                    win.is_minimized = false;
                }
                self.retile_workspace(self.active_workspace);
            }
            WindowEvent::MoveSizeEnd(hwnd) => {
                // If the user manually repositioned a floating window, sync target
                if let Some(win) = self.windows.get_mut(&hwnd) {
                    if let Ok(rect) = hwnd.rect() {
                        let w = rect.right - rect.left;
                        let h = rect.bottom - rect.top;
                        win.target_rect = (rect.left, rect.top, w, h);
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
                self.retile_workspace(ws);
            }
            HotkeyAction::MoveToWorkspace(ws) => {
                if let Some(active) = self.active_window {
                    if let Some(win) = self.windows.get_mut(&active) {
                        win.workspace = ws;
                    }
                    self.retile_workspace(self.active_workspace);
                }
            }
            HotkeyAction::ToggleFloating => {
                if let Some(active) = self.active_window {
                    if let Some(win) = self.windows.get_mut(&active) {
                        win.is_floating = !win.is_floating;
                    }
                    self.retile_workspace(self.active_workspace);
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
}
