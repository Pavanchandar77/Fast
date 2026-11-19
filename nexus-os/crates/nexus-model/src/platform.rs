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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerProfile {
    EcoSilent,
    Balanced,
    MaxCompute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryValueData {
    Dword(u32),
    Qword(u64),
    Sz(String),
    Binary(Vec<u8>),
}

impl RegistryValueData {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Dword(v) => {
                let mut out = Vec::with_capacity(5);
                out.push(1);
                out.extend_from_slice(&v.to_le_bytes());
                out
            }
            Self::Qword(v) => {
                let mut out = Vec::with_capacity(9);
                out.push(2);
                out.extend_from_slice(&v.to_le_bytes());
                out
            }
            Self::Sz(s) => {
                let mut out = Vec::with_capacity(1 + s.len());
                out.push(3);
                out.extend_from_slice(s.as_bytes());
                out
            }
            Self::Binary(b) => {
                let mut out = Vec::with_capacity(1 + b.len());
                out.push(4);
                out.extend_from_slice(b);
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (tag, rest) = bytes.split_first()?;
        match *tag {
            1 if rest.len() == 4 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(rest);
                Some(Self::Dword(u32::from_le_bytes(buf)))
            }
            2 if rest.len() == 8 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(rest);
                Some(Self::Qword(u64::from_le_bytes(buf)))
            }
            3 => Some(Self::Sz(String::from_utf8_lossy(rest).into_owned())),
            4 => Some(Self::Binary(rest.to_vec())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceRunState {
    Running,
    Stopped,
    Disabled,
}

impl ServiceRunState {
    pub fn to_code(self) -> u8 {
        match self {
            ServiceRunState::Running => 1,
            ServiceRunState::Stopped => 2,
            ServiceRunState::Disabled => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(ServiceRunState::Running),
            2 => Some(ServiceRunState::Stopped),
            3 => Some(ServiceRunState::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrivacyToggle {
    Telemetry,
    Location,
    Camera,
    Microphone,
    AdvertisingId,
    ActivityHistory,
    Copilot,
    Recall,
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
    fn set_window_cloaked(&self, id: WindowId, cloaked: bool) -> Result<(), PlatformError>;
    fn set_foreground_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn close_window(&self, id: WindowId) -> Result<(), PlatformError>;
    fn set_dwm_transitions(&self, id: WindowId, enabled: bool) -> Result<(), PlatformError>;
    fn get_window_rect(&self, id: WindowId) -> Result<Rect, PlatformError>;
    fn is_window_visible(&self, id: WindowId) -> bool;
    fn is_window_cloaked(&self, id: WindowId) -> bool;
    fn set_always_on_top(&self, id: WindowId, topmost: bool) -> Result<(), PlatformError>;
    fn is_always_on_top(&self, id: WindowId) -> bool;

    // Process and governor mutations
    fn set_process_priority(
        &self,
        pid: u32,
        priority: ProcessPriority,
    ) -> Result<(), PlatformError>;
    fn set_process_affinity(&self, pid: u32, affinity_mask: u64) -> Result<(), PlatformError>;
    fn get_process_affinity(&self, pid: u32) -> Result<u64, PlatformError>;
    fn set_job_cpu_rate_cap(&self, pid: u32, rate_percent: u32) -> Result<(), PlatformError>;
    fn remove_job_cpu_rate_cap(&self, pid: u32) -> Result<(), PlatformError>;
    fn suspend_process(&self, pid: u32) -> Result<(), PlatformError>;
    fn resume_process(&self, pid: u32) -> Result<(), PlatformError>;
    fn empty_process_working_set(&self, pid: u32) -> Result<(), PlatformError>;

    // Power, registry, service, privacy, and file system mutations
    fn set_power_profile(&self, profile: PowerProfile) -> Result<(), PlatformError>;
    fn get_power_profile(&self) -> Result<PowerProfile, PlatformError>;
    fn set_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
        value: RegistryValueData,
    ) -> Result<(), PlatformError>;
    fn get_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValueData>, PlatformError>;
    fn delete_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), PlatformError>;
    fn set_service_state(
        &self,
        service_name: &str,
        state: ServiceRunState,
    ) -> Result<(), PlatformError>;
    fn get_service_state(
        &self,
        service_name: &str,
    ) -> Result<Option<ServiceRunState>, PlatformError>;
    fn set_privacy_toggle(&self, toggle: PrivacyToggle, enabled: bool)
        -> Result<(), PlatformError>;
    fn get_privacy_toggle(&self, toggle: PrivacyToggle) -> Result<bool, PlatformError>;
    fn write_system_file(&self, path: &str, content: &[u8]) -> Result<(), PlatformError>;
    fn read_system_file(&self, path: &str) -> Result<Vec<u8>, PlatformError>;

    // Taskbar and AppBar mutations
    fn set_taskbar_visible(&self, visible: bool) -> Result<(), PlatformError>;
    fn is_taskbar_visible(&self) -> bool {
        true
    }
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

    // Real process sampling and telemetry
    fn get_system_snapshot(&self) -> Result<nx_governor_policy::SystemSnapshot, PlatformError>;
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
    SetWindowCloaked {
        id: WindowId,
        cloaked: bool,
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
    SetAlwaysOnTop {
        id: WindowId,
        topmost: bool,
    },
    SetProcessAffinity {
        pid: u32,
        affinity_mask: u64,
    },
    SetPowerProfile {
        profile: PowerProfile,
    },
    SetRegistryValue {
        hive: String,
        subkey: String,
        value_name: String,
        value: RegistryValueData,
    },
    DeleteRegistryValue {
        hive: String,
        subkey: String,
        value_name: String,
    },
    SetServiceState {
        service_name: String,
        state: ServiceRunState,
    },
    SetPrivacyToggle {
        toggle: PrivacyToggle,
        enabled: bool,
    },
    WriteSystemFile {
        path: String,
        content: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct FakeWindowState {
    pub rect: Rect,
    pub visible: bool,
    pub cloaked: bool,
    pub dwm_transitions: bool,
    pub always_on_top: bool,
}

#[derive(Debug, Clone)]
pub struct FakeProcessState {
    pub name: String,
    pub priority: ProcessPriority,
    pub cpu_cap: Option<u32>,
    pub suspended: bool,
    pub working_set_purged: bool,
    pub affinity: Option<u64>,
    pub cpu_usage_pct: f32,
    pub is_fullscreen: bool,
}

/// In-memory mock platform for unit and integration testing without Win32.
pub struct FakePlatform {
    pub windows: RwLock<HashMap<WindowId, FakeWindowState>>,
    pub processes: RwLock<HashMap<u32, FakeProcessState>>,
    pub taskbar_visible: RwLock<bool>,
    pub appbars: RwLock<HashMap<WindowId, (AppBarEdge, u32)>>,
    pub timer_resolution: RwLock<Option<u32>>,
    pub hotkeys: RwLock<HashMap<u32, (Modifiers, Key)>>,
    pub foreground_window: RwLock<Option<WindowId>>,
    pub power_profile: RwLock<PowerProfile>,
    pub registry: RwLock<HashMap<(String, String, String), RegistryValueData>>,
    pub services: RwLock<HashMap<String, ServiceRunState>>,
    pub privacy_toggles: RwLock<HashMap<PrivacyToggle, bool>>,
    pub system_files: RwLock<HashMap<String, Vec<u8>>>,
    pub system_snapshot: RwLock<Option<nx_governor_policy::SystemSnapshot>>,
    pub calls: RwLock<Vec<PlatformCall>>,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self::new()
    }
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
            power_profile: RwLock::new(PowerProfile::Balanced),
            registry: RwLock::new(HashMap::new()),
            services: RwLock::new(HashMap::new()),
            privacy_toggles: RwLock::new(HashMap::new()),
            system_files: RwLock::new(HashMap::new()),
            system_snapshot: RwLock::new(None),
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
                always_on_top: false,
            },
        );
    }

    pub fn insert_process(&self, pid: u32) {
        self.processes.write().insert(
            pid,
            FakeProcessState {
                name: format!("process_{pid}.exe"),
                priority: ProcessPriority::Normal,
                cpu_cap: None,
                suspended: false,
                working_set_purged: false,
                affinity: None,
                cpu_usage_pct: 0.0,
                is_fullscreen: false,
            },
        );
    }

    pub fn insert_process_detailed(
        &self,
        pid: u32,
        name: impl Into<String>,
        cpu_usage_pct: f32,
        is_fullscreen: bool,
    ) {
        self.processes.write().insert(
            pid,
            FakeProcessState {
                name: name.into(),
                priority: ProcessPriority::Normal,
                cpu_cap: None,
                suspended: false,
                working_set_purged: false,
                affinity: None,
                cpu_usage_pct,
                is_fullscreen,
            },
        );
    }

    pub fn set_system_snapshot(&self, snapshot: nx_governor_policy::SystemSnapshot) {
        *self.system_snapshot.write() = Some(snapshot);
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
            always_on_top: false,
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
            always_on_top: false,
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

    fn set_window_cloaked(&self, id: WindowId, cloaked: bool) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetWindowCloaked { id, cloaked });
        let mut guard = self.windows.write();
        let win = guard.entry(id).or_insert(FakeWindowState {
            rect: Rect::new(0, 0, 800, 600),
            visible: true,
            cloaked: false,
            dwm_transitions: true,
            always_on_top: false,
        });
        win.cloaked = cloaked;
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
            name: format!("process_{pid}.exe"),
            priority,
            cpu_cap: None,
            suspended: false,
            working_set_purged: false,
            affinity: None,
            cpu_usage_pct: 0.0,
            is_fullscreen: false,
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
            name: format!("process_{pid}.exe"),
            priority: ProcessPriority::Normal,
            cpu_cap: Some(rate_percent),
            suspended: false,
            working_set_purged: false,
            affinity: None,
            cpu_usage_pct: 0.0,
            is_fullscreen: false,
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
            name: format!("process_{pid}.exe"),
            priority: ProcessPriority::Normal,
            cpu_cap: None,
            suspended: true,
            working_set_purged: false,
            affinity: None,
            cpu_usage_pct: 0.0,
            is_fullscreen: false,
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

    fn is_taskbar_visible(&self) -> bool {
        *self.taskbar_visible.read()
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

    fn set_always_on_top(&self, id: WindowId, topmost: bool) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetAlwaysOnTop { id, topmost });
        let mut guard = self.windows.write();
        let win = guard.entry(id).or_insert(FakeWindowState {
            rect: Rect::new(0, 0, 800, 600),
            visible: true,
            cloaked: false,
            dwm_transitions: true,
            always_on_top: false,
        });
        win.always_on_top = topmost;
        Ok(())
    }

    fn is_always_on_top(&self, id: WindowId) -> bool {
        self.windows
            .read()
            .get(&id)
            .map(|w| w.always_on_top)
            .unwrap_or(false)
    }

    fn set_process_affinity(&self, pid: u32, affinity_mask: u64) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetProcessAffinity { pid, affinity_mask });
        let mut guard = self.processes.write();
        let proc = guard.entry(pid).or_insert(FakeProcessState {
            name: format!("process_{pid}.exe"),
            priority: ProcessPriority::Normal,
            cpu_cap: None,
            suspended: false,
            working_set_purged: false,
            affinity: Some(affinity_mask),
            cpu_usage_pct: 0.0,
            is_fullscreen: false,
        });
        proc.affinity = Some(affinity_mask);
        Ok(())
    }

    fn get_process_affinity(&self, pid: u32) -> Result<u64, PlatformError> {
        self.processes
            .read()
            .get(&pid)
            .and_then(|p| p.affinity)
            .ok_or(PlatformError::ProcessNotFound(pid))
    }

    fn set_power_profile(&self, profile: PowerProfile) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetPowerProfile { profile });
        *self.power_profile.write() = profile;
        Ok(())
    }

    fn get_power_profile(&self) -> Result<PowerProfile, PlatformError> {
        Ok(*self.power_profile.read())
    }

    fn set_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
        value: RegistryValueData,
    ) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::SetRegistryValue {
            hive: hive.to_string(),
            subkey: subkey.to_string(),
            value_name: value_name.to_string(),
            value: value.clone(),
        });
        self.registry.write().insert(
            (hive.to_string(), subkey.to_string(), value_name.to_string()),
            value,
        );
        Ok(())
    }

    fn get_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValueData>, PlatformError> {
        Ok(self
            .registry
            .read()
            .get(&(hive.to_string(), subkey.to_string(), value_name.to_string()))
            .cloned())
    }

    fn delete_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::DeleteRegistryValue {
            hive: hive.to_string(),
            subkey: subkey.to_string(),
            value_name: value_name.to_string(),
        });
        self.registry.write().remove(&(
            hive.to_string(),
            subkey.to_string(),
            value_name.to_string(),
        ));
        Ok(())
    }

    fn set_service_state(
        &self,
        service_name: &str,
        state: ServiceRunState,
    ) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::SetServiceState {
            service_name: service_name.to_string(),
            state,
        });
        self.services
            .write()
            .insert(service_name.to_string(), state);
        Ok(())
    }

    fn get_service_state(
        &self,
        service_name: &str,
    ) -> Result<Option<ServiceRunState>, PlatformError> {
        Ok(self.services.read().get(service_name).copied())
    }

    fn set_privacy_toggle(
        &self,
        toggle: PrivacyToggle,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        self.calls
            .write()
            .push(PlatformCall::SetPrivacyToggle { toggle, enabled });
        self.privacy_toggles.write().insert(toggle, enabled);
        Ok(())
    }

    fn get_privacy_toggle(&self, toggle: PrivacyToggle) -> Result<bool, PlatformError> {
        Ok(self
            .privacy_toggles
            .read()
            .get(&toggle)
            .copied()
            .unwrap_or(false))
    }

    fn write_system_file(&self, path: &str, content: &[u8]) -> Result<(), PlatformError> {
        self.calls.write().push(PlatformCall::WriteSystemFile {
            path: path.to_string(),
            content: content.to_vec(),
        });
        self.system_files
            .write()
            .insert(path.to_string(), content.to_vec());
        Ok(())
    }

    fn read_system_file(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        self.system_files
            .read()
            .get(path)
            .cloned()
            .ok_or_else(|| PlatformError::OperationFailed(format!("File not found: {path}")))
    }

    fn get_system_snapshot(&self) -> Result<nx_governor_policy::SystemSnapshot, PlatformError> {
        if let Some(s) = self.system_snapshot.read().clone() {
            return Ok(s);
        }
        let procs = self.processes.read();
        let mut samples = Vec::new();
        let mut total_cpu = 0.0f32;
        let fg_id = *self.foreground_window.read();
        let fg_pid = fg_id.map(|w| w.0 as u32);
        let mut fullscreen_pid = None;

        for (&pid, proc) in procs.iter() {
            let is_fg = fg_pid == Some(pid);
            let is_fs = proc.is_fullscreen;
            if is_fs {
                fullscreen_pid = Some(nx_governor_policy::Pid(pid));
            }
            total_cpu += proc.cpu_usage_pct;
            let sample = nx_governor_policy::ProcessSample::new(
                nx_governor_policy::Pid(pid),
                proc.name.clone(),
                proc.cpu_usage_pct,
            )
            .with_foreground(is_fg)
            .with_fullscreen(is_fs);
            samples.push(sample);
        }
        let mut snap = nx_governor_policy::SystemSnapshot::new(
            total_cpu.clamp(0.0, 100.0),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        snap.foreground_pid = fg_pid.map(nx_governor_policy::Pid);
        snap.fullscreen_pid = fullscreen_pid;
        snap.processes = samples;
        Ok(snap)
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

        assert!(platform.set_window_cloaked(w, true).is_ok());
        assert!(platform.is_window_cloaked(w));
        assert!(platform.has_call(&PlatformCall::SetWindowCloaked {
            id: w,
            cloaked: true
        }));
        assert!(platform.set_window_cloaked(w, false).is_ok());
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

    #[test]
    fn test_fake_platform_extended_mutations() {
        let platform = FakePlatform::new();
        let w = WindowId::new(77);
        let pid = 4321;
        platform.insert_window(w, Rect::new(0, 0, 800, 600), true);
        platform.insert_process(pid);

        // Always on top
        assert!(!platform.is_always_on_top(w));
        assert!(platform.set_always_on_top(w, true).is_ok());
        assert!(platform.is_always_on_top(w));
        assert!(platform.has_call(&PlatformCall::SetAlwaysOnTop {
            id: w,
            topmost: true
        }));

        // Affinity
        assert!(platform.set_process_affinity(pid, 0x05).is_ok());
        assert_eq!(platform.get_process_affinity(pid).unwrap(), 0x05);
        assert!(platform.has_call(&PlatformCall::SetProcessAffinity {
            pid,
            affinity_mask: 0x05
        }));

        // Power Profile
        assert_eq!(
            platform.get_power_profile().unwrap(),
            PowerProfile::Balanced
        );
        assert!(platform.set_power_profile(PowerProfile::MaxCompute).is_ok());
        assert_eq!(
            platform.get_power_profile().unwrap(),
            PowerProfile::MaxCompute
        );
        assert!(platform.has_call(&PlatformCall::SetPowerProfile {
            profile: PowerProfile::MaxCompute
        }));

        // Registry
        assert!(platform
            .set_registry_value(
                "HKLM",
                r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                "AllowTelemetry",
                RegistryValueData::Dword(0)
            )
            .is_ok());
        assert_eq!(
            platform
                .get_registry_value(
                    "HKLM",
                    r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                    "AllowTelemetry"
                )
                .unwrap(),
            Some(RegistryValueData::Dword(0))
        );
        assert!(platform
            .delete_registry_value(
                "HKLM",
                r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                "AllowTelemetry"
            )
            .is_ok());
        assert_eq!(
            platform
                .get_registry_value(
                    "HKLM",
                    r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                    "AllowTelemetry"
                )
                .unwrap(),
            None
        );

        // Services
        assert!(platform
            .set_service_state("DiagTrack", ServiceRunState::Disabled)
            .is_ok());
        assert_eq!(
            platform.get_service_state("DiagTrack").unwrap(),
            Some(ServiceRunState::Disabled)
        );

        // Privacy toggles
        assert!(!platform
            .get_privacy_toggle(PrivacyToggle::Location)
            .unwrap());
        assert!(platform
            .set_privacy_toggle(PrivacyToggle::Location, true)
            .is_ok());
        assert!(platform
            .get_privacy_toggle(PrivacyToggle::Location)
            .unwrap());

        // System files
        assert!(platform
            .write_system_file(r"C:\Windows\System32\drivers\etc\hosts", b"127.0.0.1")
            .is_ok());
        assert_eq!(
            platform
                .read_system_file(r"C:\Windows\System32\drivers\etc\hosts")
                .unwrap(),
            b"127.0.0.1"
        );
    }

    #[test]
    fn test_fake_platform_system_snapshot() {
        let platform = FakePlatform::new();
        platform.insert_process_detailed(100, "game.exe", 45.0, true);
        platform.insert_process_detailed(200, "hog.exe", 85.0, false);
        platform.set_foreground_window(WindowId::new(100)).unwrap();

        let snap = platform.get_system_snapshot().unwrap();
        assert_eq!(snap.foreground_pid, Some(nx_governor_policy::Pid(100)));
        assert_eq!(snap.fullscreen_pid, Some(nx_governor_policy::Pid(100)));
        assert_eq!(snap.processes.len(), 2);
        assert!(snap.total_cpu_pct >= 99.0);
    }
}
