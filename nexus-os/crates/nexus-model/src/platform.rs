//! Platform abstraction trait and mock/fake platform implementation.
//! Decouples core shell, governor, and window manager logic from raw Win32 APIs,
//! enabling 100% Linux compilation and automated testability.

use crate::hotkeys::{Key, Modifiers};
use crate::layout::{Rect, WindowId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessPriority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppBarEdge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    #[error("Window not found: {0:?}")]
    WindowNotFound(WindowId),
    #[error("Process not found: PID {0}")]
    ProcessNotFound(u32),
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Platform operation unsupported")]
    Unsupported,
}

/// Abstract contract for all OS mutations performed by Nexus-OS.
pub trait Platform: Send + Sync {
    // Window management mutations
    fn move_resize_window(&self, id: WindowId, rect: Rect) -> Result<(), PlatformError>;
    fn show_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn hide_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn set_foreground_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn close_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn set_dwm_transitions(&self, id: WindowId, enabled: bool) -> Result<(), PlatformError>;
    fn get_window_rect(&self, id: WindowId) -> Result<Rect, PlatformError>;
    fn is_window_visible(&self, id: WindowId) -> bool;
    fn is_window_cloaked(&self, id: WindowId) -> bool;

    // Process and governor mutations
    fn set_process_priority(
        &self,
        pid: u32,
        priority: ProcessPriority,
    ) -> Result<(), PlatformError>;
    fn set_job_cpu_rate_cap(&self, pid: u32, rate_percent: u32) -> Result<(), PlatformError>;
    fn remove_job_cpu_rate_cap(&self, pid: u32) -> Result<(), PlatformError>;
    fn suspend_process(&self, pid: u32) -> Result<(), PlatformError>;
    fn resume_process(&self, pid: u32) -> Result<(), PlatformError>;
    fn empty_process_working_set(&self, pid: u32) -> Result<(), PlatformError>;

    // Taskbar and AppBar mutations
    fn set_taskbar_visible(&self, visible: bool) -> Result<(), PlatformError>;
    fn register_appbar(
        &self,
        id: WindowId,
        edge: AppBarEdge,
        size: u32,
    ) -> Result<(), PlatformError>;
    fn unregister_appbar(&self, id: WindowId) -> Result<(), PlatformError>;

    // System timer and hotkey mutations
    fn set_timer_resolution_ns(&self, resolution_100ns: u32) -> Result<(), PlatformError>;
    fn register_hotkey(&self, id: u32, modifiers: Modifiers, key: Key)
        -> Result<(), PlatformError>;
    fn unregister_hotkey(&self, id: u32) -> Result<(), PlatformError>;
}

/// Historical record of a mutation invoked on the platform
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformCall {
    MoveResizeWindow {
        id: WindowId,
        rect: Rect,
    },
    ShowWindow {
        id: WindowId,
    },
    HideWindow {
        id: WindowId,
    },
    SetForegroundWindow {
        id: WindowId,
    },
    CloseWindow {
        id: WindowId,
    },
    SetDwmTransitions {
        id: WindowId,
        enabled: bool,
    },
    SetProcessPriority {
        pid: u32,
        priority: ProcessPriority,
    },
    SetJobCpuRateCap {
        pid: u32,
        rate_percent: u32,
    },
    RemoveJobCpuRateCap {
        pid: u32,
    },
    SuspendProcess {
        pid: u32,
    },
    ResumeProcess {
        pid: u32,
    },
    EmptyProcessWorkingSet {
        pid: u32,
    },
    SetTaskbarVisible {
        visible: bool,
    },
    RegisterAppBar {
        id: WindowId,
        edge: AppBarEdge,
        size: u32,
    },
    UnregisterAppBar {
        id: WindowId,
    },
    SetTimerResolution {
        resolution_100ns: u32,
    },
    RegisterHotkey {
        id: u32,
        modifiers: Modifiers,
        key: Key,
    },
    UnregisterHotkey {
        id: u32,
    },
}

#[derive(Debug, Clone)]
pub struct FakeWindowState {
    pub rect: Rect,
    pub visible: bool,
    pub cloaked: bool,
    pub dwm_transitions: bool,
}

#[derive(Debug, Clone)]
pub struct FakeProcessState {
    pub priority: ProcessPriority,
    pub cpu_cap: Option<u32>,
    pub suspended: bool,
    pub working_set_purged: bool,
}

/// In-memory mock platform for unit and integration testing without Win32.
#[derive(Default)]
pub struct FakePlatform {
    pub windows: RwLock<HashMap<WindowId, FakeWindowState>>,
    pub processes: RwLock<HashMap<u32, FakeProcessState>>,
    pub taskbar_visible: RwLock<bool>,
    pub appbars: RwLock<HashMap<WindowId, (AppBarEdge, u32)>>,
    pub timer_resolution: RwLock<Option<u32>>,
    pub hotkeys: RwLock<HashMap<u32, (Modifiers, Key)>>,
    pub foreground_window: RwLock<Option<WindowId>>,
    pub calls: RwLock<Vec<PlatformCall>>,
}

impl FakePlatform {
    pub fn new() -> Self {
        Self {
            windows: RwLock::new(HashMap::new()),
            processes: RwLock::new(HashMap::new()),
            taskbar_visible: RwLock::new(true),
            appbars: RwLock::new(HashMap::new()),
            timer_resolution: RwLock::new(None),
            hotkeys: RwLock::new(HashMap::new()),
            foreground_window: RwLock::new(None),
            calls: RwLock::new(Vec::new()),
        }
    }

    pub fn insert_window(&self, id: WindowId, rect: Rect, visible: bool) {
        self.windows.write().insert(
            id,
            FakeWindowState {
                rect,
                visible,
                cloaked: false,
                dwm_transitions: true,
            },
        );
    }

    pub fn insert_process(&self, pid: u32) {
        self.processes.write().insert(
            pid,
            FakeProcessState {
                priority: ProcessPriority::Normal,
                cpu_cap: None,
                suspended: false,
                working_set_purged: false,
            },
        );
    }

    pub fn calls(&self) -> Vec<PlatformCall> {
        self.calls.read().clone()
    }

    pub fn clear_calls(&self) {
        self.calls.write().clear();
    }

    pub fn has_call(&self, call: &PlatformCall) -> bool {
        self.calls.read().contains(call)
    }

    pub fn is_taskbar_visible(&self) -> bool {
        *self.taskbar_visible.read()
    }

    pub fn get_process_priority(&self, pid: u32) -> Option<ProcessPriority> {
        self.processes.read().get(&pid).map(|p| p.priority)
    }

    pub fn is_process_suspended(&self, pid: u32) -> bool {
        self.processes
            .read()
            .get(&pid)
            .map(|p| p.suspended)
            .unwrap_or(false)
    }

    pub fn get_job_cpu_cap(&self, pid: u32) -> Option<u32> {
        self.processes.read().get(&pid).and_then(|p| p.cpu_cap)
    }
}

impl Platform for FakePlatform {
    fn move_resize_window(&self, id: WindowId, rect: Rect) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::MoveResizeWindow { id, rect });
        let mut guard = self.windows.write();
        let win = guard.entry(id).or_insert(FakeWindowState {
            rect,
            visible: true,
            cloaked: false,
            dwm_transitions: true,
        });
        win.rect = rect;
        Ok(())
    }

    fn show_window(&self, id: WindowId) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::ShowWindow { id });
        let mut guard = self.windows.write();
        let win = guard.entry(id).or_insert(FakeWindowState {
            rect: Rect::new(0, 0, 800, 600),
            visible: true,
            cloaked: false,
            dwm_transitions: true,
        });
        win.visible = true;
        Ok(())
    }

    fn hide_window(&self, id: WindowId) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::HideWindow { id });
        if let Some(win) = self.windows.write().get_mut(&id) {
            win.visible = false;
        }
        Ok(())
    }

    fn set_foreground_window(&self, id: WindowId) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetForegroundWindow { id });
        *self.foreground_window.write() = Some(id);
        Ok(())
    }

    fn close_window(&self, id: WindowId) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::CloseWindow { id });
        self.windows.write().remove(&id);
        Ok(())
    }

    fn set_dwm_transitions(&self, id: WindowId, enabled: bool) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetDwmTransitions { id, enabled });
        if let Some(win) = self.windows.write().get_mut(&id) {
            win.dwm_transitions = enabled;
        }
        Ok(())
    }

    fn get_window_rect(&self, id: WindowId) -> Result<Rect, PlatformError> {
        self.windows
            .read()
            .get(&id)
            .map(|w| w.rect)
            .ok_or(PlatformError::WindowNotFound(id))
    }

    fn is_window_visible(&self, id: WindowId) -> bool {
        self.windows
            .read()
            .get(&id)
            .map(|w| w.visible)
            .unwrap_or(false)
    }

    fn is_window_cloaked(&self, id: WindowId) -> bool {
        self.windows
            .read()
            .get(&id)
            .map(|w| w.cloaked)
            .unwrap_or(false)
    }

    fn set_process_priority(
        &self,
        pid: u32,
        priority: ProcessPriority,
    ) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetProcessPriority { pid, priority });
        let mut guard = self.processes.write();
        let proc = guard.entry(pid).or_insert(FakeProcessState {
            priority,
            cpu_cap: None,
            suspended: false,
            working_set_purged: false,
        });
        proc.priority = priority;
        Ok(())
    }

    fn set_job_cpu_rate_cap(&self, pid: u32, rate_percent: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetJobCpuRateCap { pid, rate_percent });
        let mut guard = self.processes.write();
        let proc = guard.entry(pid).or_insert(FakeProcessState {
            priority: ProcessPriority::Normal,
            cpu_cap: Some(rate_percent),
            suspended: false,
            working_set_purged: false,
        });
        proc.cpu_cap = Some(rate_percent);
        Ok(())
    }

    fn remove_job_cpu_rate_cap(&self, pid: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::RemoveJobCpuRateCap { pid });
        if let Some(proc) = self.processes.write().get_mut(&pid) {
            proc.cpu_cap = None;
        }
        Ok(())
    }

    fn suspend_process(&self, pid: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SuspendProcess { pid });
        let mut guard = self.processes.write();
        let proc = guard.entry(pid).or_insert(FakeProcessState {
            priority: ProcessPriority::Normal,
            cpu_cap: None,
            suspended: true,
            working_set_purged: false,
        });
        proc.suspended = true;
        Ok(())
    }

    fn resume_process(&self, pid: u32) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::ResumeProcess { pid });
        if let Some(proc) = self.processes.write().get_mut(&pid) {
            proc.suspended = false;
        }
        Ok(())
    }

    fn empty_process_working_set(&self, pid: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::EmptyProcessWorkingSet { pid });
        if let Some(proc) = self.processes.write().get_mut(&pid) {
            proc.working_set_purged = true;
        }
        Ok(())
    }

    fn set_taskbar_visible(&self, visible: bool) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetTaskbarVisible { visible });
        *self.taskbar_visible.write() = visible;
        Ok(())
    }

    fn register_appbar(
        &self,
        id: WindowId,
        edge: AppBarEdge,
        size: u32,
    ) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::RegisterAppBar { id, edge, size });
        self.appbars.write().insert(id, (edge, size));
        Ok(())
    }

    fn unregister_appbar(&self, id: WindowId) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::UnregisterAppBar { id });
        self.appbars.write().remove(&id);
        Ok(())
    }

    fn set_timer_resolution_ns(&self, resolution_100ns: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetTimerResolution { resolution_100ns });
        *self.timer_resolution.write() = Some(resolution_100ns);
        Ok(())
    }

    fn register_hotkey(
        &self,
        id: u32,
        modifiers: Modifiers,
        key: Key,
    ) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::RegisterHotkey { id, modifiers, key });
        self.hotkeys.write().insert(id, (modifiers, key));
        Ok(())
    }

    fn unregister_hotkey(&self, id: u32) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::UnregisterHotkey { id });
        self.hotkeys.write().remove(&id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fake_platform_window_mutations() {
        let platform = FakePlatform::new();
        let w1 = WindowId::new(42);
        let rect = Rect::new(100, 100, 800, 600);

        assert!(platform.move_resize_window(w1, rect).is_ok());
        assert_eq!(platform.get_window_rect(w1).unwrap(), rect);
        assert!(platform.is_window_visible(w1));

        assert!(platform.hide_window(w1).is_ok());
        assert!(!platform.is_window_visible(w1));

        assert!(platform.show_window(w1).is_ok());
        assert!(platform.is_window_visible(w1));

        assert!(platform.close_window(w1).is_ok());
        assert!(!platform.is_window_visible(w1));
    }

    #[test]
    fn test_fake_platform_process_mutations() {
        let platform = FakePlatform::new();
        let pid = 1234;

        assert!(platform
            .set_process_priority(pid, ProcessPriority::High)
            .is_ok());
        assert_eq!(
            platform.get_process_priority(pid),
            Some(ProcessPriority::High)
        );

        assert!(platform.set_job_cpu_rate_cap(pid, 1).is_ok());
        assert_eq!(platform.get_job_cpu_cap(pid), Some(1));

        assert!(platform.suspend_process(pid).is_ok());
        assert!(platform.is_process_suspended(pid));

        assert!(platform.resume_process(pid).is_ok());
        assert!(!platform.is_process_suspended(pid));

        assert!(platform.remove_job_cpu_rate_cap(pid).is_ok());
        assert_eq!(platform.get_job_cpu_cap(pid), None);
    }

    #[test]
    fn test_fake_platform_call_log() {
        let platform = FakePlatform::new();
        assert!(platform.set_taskbar_visible(false).is_ok());
        assert!(!platform.is_taskbar_visible());

        assert!(platform.has_call(&PlatformCall::SetTaskbarVisible { visible: false }));
        assert_eq!(platform.calls().len(), 1);

        platform.clear_calls();
        assert_eq!(platform.calls().len(), 0);
    }

    #[test]
    fn test_fake_platform_timer_and_hotkeys() {
        let platform = FakePlatform::new();
        assert!(platform.set_timer_resolution_ns(5000).is_ok());
        assert_eq!(*platform.timer_resolution.read(), Some(5000));

        let mods = Modifiers::ctrl().with_alt().with_shift();
        assert!(platform.register_hotkey(1, mods, Key::Escape).is_ok());
        assert!(platform.unregister_hotkey(1).is_ok());
    }

    #[test]
    fn test_fake_platform_dwm_and_cloak() {
        let platform = FakePlatform::new();
        let w = WindowId::new(99);
        platform.insert_window(w, Rect::new(0, 0, 500, 500), true);

        assert!(platform.set_dwm_transitions(w, false).is_ok());
        assert!(platform.has_call(&PlatformCall::SetDwmTransitions {
            id: w,
            enabled: false
        }));
        assert!(!platform.is_window_cloaked(w));
    }

    #[test]
    fn test_fake_platform_appbar_operations() {
        let platform = FakePlatform::new();
        let w = WindowId::new(10);
        assert!(platform.register_appbar(w, AppBarEdge::Top, 28).is_ok());
        assert_eq!(
            platform.appbars.read().get(&w),
            Some(&(AppBarEdge::Top, 28))
        );
        assert!(platform.unregister_appbar(w).is_ok());
        assert_eq!(platform.appbars.read().get(&w), None);
    }

    #[test]
    fn test_fake_platform_working_set_and_missing_err() {
        let platform = FakePlatform::new();
        let pid = 5555;
        platform.insert_process(pid);
        assert!(platform.empty_process_working_set(pid).is_ok());
        assert!(
            platform
                .processes
                .read()
                .get(&pid)
                .unwrap()
                .working_set_purged
        );

        let missing = WindowId::new(99999);
        assert!(matches!(
            platform.get_window_rect(missing),
            Err(PlatformError::WindowNotFound(_))
        ));
    }

    #[test]
    fn test_platform_error_display() {
        let err1 = PlatformError::WindowNotFound(WindowId::new(1));
        assert!(err1.to_string().contains("Window not found"));

        let err2 = PlatformError::ProcessNotFound(42);
        assert!(err2.to_string().contains("PID 42"));

        let err3 = PlatformError::Unsupported;
        assert_eq!(err3.to_string(), "Platform operation unsupported");
    }
}
