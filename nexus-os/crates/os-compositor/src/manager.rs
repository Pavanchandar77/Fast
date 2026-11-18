use crate::hooks::WindowEvent;
use crate::hotkeys::HotkeyAction;
use crate::spring::{SpringConfig, SpringRect};
use nexus_model::layout::{LayoutEngine, LayoutKind, LayoutOptions, Rect, WindowId};
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
        let (screen_w, screen_h) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

        let (top_bar_height, inner_gap, outer_gap, master_ratio, layout) = {
            let cfg = self.config.read();
            (
                if cfg.top_bar.enabled {
                    cfg.top_bar.height
                } else {
                    0
                },
                cfg.inner_gap,
                cfg.outer_gap,
                cfg.master_ratio,
                cfg.layout.clone(),
            )
        };

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

        if workspace_windows.is_empty() {
            return;
        }

        let layout_kind: LayoutKind = layout.parse().unwrap_or(LayoutKind::Dwindle);
        if layout_kind == LayoutKind::Floating {
            return;
        }

        let screen_rect = Rect::new(0, 0, screen_w, screen_h);
        let layout_opts = LayoutOptions {
            inner_gap,
            outer_gap,
            master_ratio,
            top_bar_height,
        };

        let window_ids: Vec<WindowId> = workspace_windows
            .iter()
            .map(|h| WindowId::new(h.0 .0 as u64))
            .collect();

        let placements =
            LayoutEngine::compute_layout(layout_kind, screen_rect, &window_ids, &layout_opts);

        for (hwnd, win_id) in workspace_windows.iter().zip(window_ids) {
            if let Some(rect) = placements.get(&win_id) {
                if let Some(win) = self.windows.get_mut(hwnd) {
                    win.target_rect = (rect.x, rect.y, rect.width, rect.height);
                    win.spring.set_target(
                        rect.x as f32,
                        rect.y as f32,
                        rect.width as f32,
                        rect.height as f32,
                    );
                }
            }
        }
    }

    /// Advances physics simulation and applies smooth window coordinates at 120Hz
    pub fn update_animations(&mut self, dt: f32) {
        for win in self.windows.values_mut() {
            if !win.spring.is_at_rest() {
                win.spring.step(dt);
                let (x, y, w, h) = win.spring.current_rect();
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
