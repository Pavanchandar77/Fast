//! Win32 platform implementation for Nexus-OS.
//! Executes real system mutations (window manipulation, job object rate caps,
//! timer resolution, taskbar suppression, appbars, and hotkeys) via Win32 NT APIs.

use nexus_model::hotkeys::{Key, Modifiers};
use nexus_model::layout::{Rect, WindowId};
use nexus_model::platform::{
    AppBarEdge, Platform, PlatformError, PowerProfile, PrivacyToggle, ProcessPriority,
    RegistryValueData, ServiceRunState,
};

#[cfg(windows)]
use crate::hollow_shell::HollowShellManager;
#[cfg(windows)]
use crate::ntdll::NtSetTimerResolution;
#[cfg(windows)]
use crate::process::ProcessHandle;
#[cfg(windows)]
use crate::window::WindowHandle;
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED,
};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
    SetInformationJobObject, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, REALTIME_PRIORITY_CLASS,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_BACK, VK_DELETE,
    VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6,
    VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
    VK_SPACE, VK_TAB, VK_UP,
};
#[cfg(windows)]
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_REMOVE, APPBARDATA,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CLOSE,
};

#[derive(Default, Clone, Copy, Debug)]
pub struct Win32Platform;

impl Win32Platform {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Platform for Win32Platform {
    fn move_resize_window(&self, id: WindowId, rect: Rect) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            SetWindowPos(
                hwnd,
                HWND::default(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|e| PlatformError::OperationFailed(format!("SetWindowPos failed: {}", e)))
        }
    }

    fn show_window(&self, id: WindowId) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        Ok(())
    }

    fn hide_window(&self, id: WindowId) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        Ok(())
    }

    fn set_window_cloaked(&self, id: WindowId, cloaked: bool) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        let val: BOOL = BOOL(if cloaked { 1 } else { 0 });
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK,
                &val as *const _ as *const c_void,
                std::mem::size_of::<BOOL>() as u32,
            )
            .map_err(|e| {
                PlatformError::OperationFailed(format!("DwmSetWindowAttribute cloak failed: {e}"))
            })
        }
    }

    fn set_foreground_window(&self, id: WindowId) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    fn close_window(&self, id: WindowId) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }

    fn set_dwm_transitions(&self, id: WindowId, enabled: bool) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        let val: BOOL = BOOL(if enabled { 0 } else { 1 });
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &val as *const _ as *const c_void,
                std::mem::size_of::<BOOL>() as u32,
            )
            .map_err(|e| {
                PlatformError::OperationFailed(format!("DwmSetWindowAttribute failed: {}", e))
            })
        }
    }

    fn get_window_rect(&self, id: WindowId) -> Result<Rect, PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        let wh = WindowHandle::new(hwnd);
        let r = wh
            .rect()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
        Ok(Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    fn is_window_visible(&self, id: WindowId) -> bool {
        let hwnd = HWND(id.0 as *mut _);
        unsafe { IsWindowVisible(hwnd).as_bool() }
    }

    fn is_window_cloaked(&self, id: WindowId) -> bool {
        let hwnd = HWND(id.0 as *mut _);
        let wh = WindowHandle::new(hwnd);
        wh.is_cloaked()
    }

    fn set_process_priority(
        &self,
        pid: u32,
        priority: ProcessPriority,
    ) -> Result<(), PlatformError> {
        let win_pri = match priority {
            ProcessPriority::Idle => IDLE_PRIORITY_CLASS,
            ProcessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            ProcessPriority::Normal => NORMAL_PRIORITY_CLASS,
            ProcessPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            ProcessPriority::High => HIGH_PRIORITY_CLASS,
            ProcessPriority::Realtime => REALTIME_PRIORITY_CLASS,
        };

        let handle = ProcessHandle::open_for_governance(pid)
            .map_err(|_e| PlatformError::ProcessNotFound(pid))?;
        handle.set_priority(win_pri).map_err(|e| {
            PlatformError::OperationFailed(format!("SetPriorityClass failed: {:?}", e))
        })
    }

    fn set_job_cpu_rate_cap(&self, pid: u32, rate_percent: u32) -> Result<(), PlatformError> {
        let rate_val = rate_percent.clamp(1, 90) * 100;
        let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL(0x1 | 0x4), // ENABLE | HARD_CAP
            ..Default::default()
        };
        info.Anonymous.CpuRate = rate_val;

        let proc = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;

        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null())
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;

            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            ) {
                let _ = CloseHandle(job);
                return Err(PlatformError::OperationFailed(e.to_string()));
            }

            if let Err(e) = AssignProcessToJobObject(job, proc.handle()) {
                let _ = CloseHandle(job);
                return Err(PlatformError::OperationFailed(e.to_string()));
            }
        }
        Ok(())
    }

    fn remove_job_cpu_rate_cap(&self, _pid: u32) -> Result<(), PlatformError> {
        // Unthrottling handled via JobThrottleManager in-place or closing handle
        Ok(())
    }

    fn suspend_process(&self, pid: u32) -> Result<(), PlatformError> {
        let proc = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;
        // Explicit Platform call. The shell records a suspend only when
        // `allow_nt_suspend` is set; this path is not on the default governor.
        proc.suspend_if_allowed(true)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn resume_process(&self, pid: u32) -> Result<(), PlatformError> {
        let proc = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;
        proc.resume()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn empty_process_working_set(&self, pid: u32) -> Result<(), PlatformError> {
        let proc = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;
        proc.empty_working_set()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn set_taskbar_visible(&self, visible: bool) -> Result<(), PlatformError> {
        if visible {
            HollowShellManager::restore_taskbar();
            Ok(())
        } else {
            HollowShellManager::suppress_taskbar()
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))
        }
    }

    fn register_appbar(
        &self,
        id: WindowId,
        edge: AppBarEdge,
        size: u32,
    ) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        let u_edge = match edge {
            AppBarEdge::Top => ABE_TOP,
            AppBarEdge::Bottom => ABE_BOTTOM,
            AppBarEdge::Left => ABE_LEFT,
            AppBarEdge::Right => ABE_RIGHT,
        };

        unsafe {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: hwnd,
                uCallbackMessage: 0,
                uEdge: u_edge,
                rc: windows::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: size as i32,
                    bottom: size as i32,
                },
                lParam: LPARAM(0),
            };
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
        Ok(())
    }

    fn unregister_appbar(&self, id: WindowId) -> Result<(), PlatformError> {
        let hwnd = HWND(id.0 as *mut _);
        unsafe {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: hwnd,
                uCallbackMessage: 0,
                uEdge: ABE_TOP,
                rc: windows::Win32::Foundation::RECT::default(),
                lParam: LPARAM(0),
            };
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
        Ok(())
    }

    fn set_timer_resolution_ns(&self, resolution_100ns: u32) -> Result<(), PlatformError> {
        let mut current: u32 = 0;
        let status = unsafe { NtSetTimerResolution(resolution_100ns, 1, &mut current) };
        if status.0 < 0 {
            Err(PlatformError::OperationFailed(format!(
                "NtSetTimerResolution failed: 0x{:08X}",
                status.0 as u32
            )))
        } else {
            Ok(())
        }
    }

    fn register_hotkey(
        &self,
        id: u32,
        modifiers: Modifiers,
        key: Key,
    ) -> Result<(), PlatformError> {
        let mut fs_mods = windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS(0);
        if modifiers.ctrl {
            fs_mods |= MOD_CONTROL;
        }
        if modifiers.alt {
            fs_mods |= MOD_ALT;
        }
        if modifiers.shift {
            fs_mods |= MOD_SHIFT;
        }
        if modifiers.win {
            fs_mods |= MOD_WIN;
        }

        let vk = match key {
            Key::A => 'A' as u32,
            Key::B => 'B' as u32,
            Key::C => 'C' as u32,
            Key::D => 'D' as u32,
            Key::E => 'E' as u32,
            Key::F => 'F' as u32,
            Key::G => 'G' as u32,
            Key::H => 'H' as u32,
            Key::I => 'I' as u32,
            Key::J => 'J' as u32,
            Key::K => 'K' as u32,
            Key::L => 'L' as u32,
            Key::M => 'M' as u32,
            Key::N => 'N' as u32,
            Key::O => 'O' as u32,
            Key::P => 'P' as u32,
            Key::Q => 'Q' as u32,
            Key::R => 'R' as u32,
            Key::S => 'S' as u32,
            Key::T => 'T' as u32,
            Key::U => 'U' as u32,
            Key::V => 'V' as u32,
            Key::W => 'W' as u32,
            Key::X => 'X' as u32,
            Key::Y => 'Y' as u32,
            Key::Z => 'Z' as u32,
            Key::Num0 => '0' as u32,
            Key::Num1 => '1' as u32,
            Key::Num2 => '2' as u32,
            Key::Num3 => '3' as u32,
            Key::Num4 => '4' as u32,
            Key::Num5 => '5' as u32,
            Key::Num6 => '6' as u32,
            Key::Num7 => '7' as u32,
            Key::Num8 => '8' as u32,
            Key::Num9 => '9' as u32,
            Key::Return => VK_RETURN.0 as u32,
            Key::Space => VK_SPACE.0 as u32,
            Key::Escape => VK_ESCAPE.0 as u32,
            Key::Tab => VK_TAB.0 as u32,
            Key::Backspace => VK_BACK.0 as u32,
            Key::Delete => VK_DELETE.0 as u32,
            Key::Insert => VK_INSERT.0 as u32,
            Key::Home => VK_HOME.0 as u32,
            Key::End => VK_END.0 as u32,
            Key::PageUp => VK_PRIOR.0 as u32,
            Key::PageDown => VK_NEXT.0 as u32,
            Key::Up => VK_UP.0 as u32,
            Key::Down => VK_DOWN.0 as u32,
            Key::Left => VK_LEFT.0 as u32,
            Key::Right => VK_RIGHT.0 as u32,
            Key::F1 => VK_F1.0 as u32,
            Key::F2 => VK_F2.0 as u32,
            Key::F3 => VK_F3.0 as u32,
            Key::F4 => VK_F4.0 as u32,
            Key::F5 => VK_F5.0 as u32,
            Key::F6 => VK_F6.0 as u32,
            Key::F7 => VK_F7.0 as u32,
            Key::F8 => VK_F8.0 as u32,
            Key::F9 => VK_F9.0 as u32,
            Key::F10 => VK_F10.0 as u32,
            Key::F11 => VK_F11.0 as u32,
            Key::F12 => VK_F12.0 as u32,
        };

        unsafe {
            RegisterHotKey(HWND::default(), id as i32, fs_mods, vk).map_err(|e| {
                PlatformError::OperationFailed(format!("RegisterHotKey failed: {}", e))
            })
        }
    }

    fn unregister_hotkey(&self, id: u32) -> Result<(), PlatformError> {
        unsafe {
            UnregisterHotKey(HWND::default(), id as i32).map_err(|e| {
                PlatformError::OperationFailed(format!("UnregisterHotKey failed: {}", e))
            })
        }
    }

    fn set_always_on_top(&self, id: WindowId, topmost: bool) -> Result<(), PlatformError> {
        let previous = self.is_always_on_top(id);
        let mutation = nexus_journal::Mutation::AlwaysOnTop {
            hwnd: id.0,
            previous,
            new: topmost,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn is_always_on_top(&self, id: WindowId) -> bool {
        let hwnd = HWND(id.0 as *mut _);
        let ex_style = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE,
            )
        };
        ex_style as u32 & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST.0 != 0
    }

    fn set_process_affinity(&self, pid: u32, affinity_mask: u64) -> Result<(), PlatformError> {
        let handle = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;
        let prev = handle.affinity().unwrap_or(affinity_mask);
        let mutation = nexus_journal::Mutation::ProcessAffinity {
            pid,
            previous_mask: prev,
            new_mask: affinity_mask,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_process_affinity(&self, pid: u32) -> Result<u64, PlatformError> {
        let handle = ProcessHandle::open_for_governance(pid)
            .map_err(|_| PlatformError::ProcessNotFound(pid))?;
        handle
            .affinity()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn set_power_profile(&self, profile: PowerProfile) -> Result<(), PlatformError> {
        let code = match profile {
            PowerProfile::EcoSilent => 0,
            PowerProfile::Balanced => 1,
            PowerProfile::MaxCompute => 2,
        };
        let mutation = nexus_journal::Mutation::PowerPlan {
            previous_profile: 1,
            new_profile: code,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_power_profile(&self) -> Result<PowerProfile, PlatformError> {
        Ok(PowerProfile::Balanced)
    }

    fn set_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
        value: RegistryValueData,
    ) -> Result<(), PlatformError> {
        let previous = self
            .get_registry_value(hive, subkey, value_name)
            .ok()
            .flatten()
            .map(|prev| crate::effects::encode_registry_value(&prev));
        let mutation = nexus_journal::Mutation::RegistryValue {
            hive: hive.to_string(),
            subkey: subkey.to_string(),
            value_name: value_name.to_string(),
            previous,
            new: Some(crate::effects::encode_registry_value(&value)),
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValueData>, PlatformError> {
        let key = format!("{hive}\\{subkey}");
        let (ok, output) =
            crate::effects::run_hidden("reg.exe", &["query", &key, "/v", value_name])
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
        if !ok {
            return Ok(None);
        }
        Ok(parse_reg_query(&output, value_name))
    }

    fn delete_registry_value(
        &self,
        hive: &str,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), PlatformError> {
        let previous = self
            .get_registry_value(hive, subkey, value_name)
            .ok()
            .flatten()
            .map(|prev| crate::effects::encode_registry_value(&prev));
        let mutation = nexus_journal::Mutation::RegistryValue {
            hive: hive.to_string(),
            subkey: subkey.to_string(),
            value_name: value_name.to_string(),
            previous,
            new: None,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn set_service_state(
        &self,
        service_name: &str,
        state: ServiceRunState,
    ) -> Result<(), PlatformError> {
        let code = match state {
            ServiceRunState::Running => 1,
            ServiceRunState::Stopped => 2,
            ServiceRunState::Disabled => 3,
        };
        let mutation = nexus_journal::Mutation::ServiceState {
            service_name: service_name.to_string(),
            previous_state: 1,
            new_state: code,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_service_state(
        &self,
        service_name: &str,
    ) -> Result<Option<ServiceRunState>, PlatformError> {
        if let Some(task_path) = service_name.strip_prefix("task:") {
            let (ok, output) =
                crate::effects::run_hidden("schtasks.exe", &["/query", "/tn", task_path])
                    .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
            if !ok {
                return Ok(None);
            }
            if output.contains("Disabled") {
                return Ok(Some(ServiceRunState::Disabled));
            }
            return Ok(Some(ServiceRunState::Running));
        }
        let (ok, output) = crate::effects::run_hidden("sc.exe", &["query", service_name])
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
        if !ok {
            return Ok(None);
        }
        if output.contains("RUNNING") {
            Ok(Some(ServiceRunState::Running))
        } else {
            Ok(Some(ServiceRunState::Stopped))
        }
    }

    fn set_privacy_toggle(
        &self,
        toggle: PrivacyToggle,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        let id = match toggle {
            PrivacyToggle::Telemetry => 1,
            PrivacyToggle::Location => 2,
            PrivacyToggle::Camera => 3,
            PrivacyToggle::Microphone => 4,
            PrivacyToggle::AdvertisingId => 5,
            PrivacyToggle::ActivityHistory => 6,
            PrivacyToggle::Copilot => 7,
            PrivacyToggle::Recall => 8,
        };
        let mutation = nexus_journal::Mutation::PrivacyToggle {
            toggle_id: id,
            previous_enabled: !enabled,
            enabled,
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_privacy_toggle(&self, _toggle: PrivacyToggle) -> Result<bool, PlatformError> {
        Ok(false)
    }

    fn write_system_file(&self, path: &str, content: &[u8]) -> Result<(), PlatformError> {
        let previous_content = std::fs::read(path).ok();
        let mutation = nexus_journal::Mutation::SystemFile {
            path: path.to_string(),
            previous_content,
            new_content: Some(content.to_vec()),
        };
        crate::recovery::commit(&mutation)
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn read_system_file(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        std::fs::read(path).map_err(|e| PlatformError::OperationFailed(e.to_string()))
    }

    fn get_system_snapshot(&self) -> Result<nx_governor_policy::SystemSnapshot, PlatformError> {
        let fg_hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let mut fg_pid = None;
        let mut fullscreen_pid = None;

        if !fg_hwnd.0.is_null() {
            let mut pid = 0;
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                    fg_hwnd,
                    Some(&mut pid),
                );
            }
            if pid != 0 {
                fg_pid = Some(pid);
                let mut rect = windows::Win32::Foundation::RECT::default();
                unsafe {
                    let _ =
                        windows::Win32::UI::WindowsAndMessaging::GetWindowRect(fg_hwnd, &mut rect);
                    let screen_w = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                        windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
                    );
                    let screen_h = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                        windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
                    );
                    if rect.left <= 0
                        && rect.top <= 0
                        && rect.right >= screen_w
                        && rect.bottom >= screen_h
                    {
                        fullscreen_pid = Some(pid);
                    }
                }
            }
        }

        let mut samples = Vec::new();
        let mut is_game_active = false;
        unsafe {
            if let Ok(snap) =
                windows::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot(
                    windows::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPPROCESS,
                    0,
                )
            {
                let mut entry = windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<
                        windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W,
                    >() as u32,
                    ..Default::default()
                };

                if windows::Win32::System::Diagnostics::ToolHelp::Process32FirstW(snap, &mut entry)
                    .is_ok()
                {
                    loop {
                        let pid = entry.th32ProcessID;
                        let len = entry
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(entry.szExeFile.len());
                        let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                        let is_fg = fg_pid == Some(pid);
                        let is_fs = fullscreen_pid == Some(pid);

                        if (is_fg || is_fs)
                            && nx_governor_policy::KNOWN_GAME_EXECUTABLES
                                .iter()
                                .any(|g| name.eq_ignore_ascii_case(g))
                        {
                            is_game_active = true;
                        }

                        let sample = nx_governor_policy::ProcessSample::new(
                            nx_governor_policy::Pid(pid),
                            name,
                            0.0,
                        )
                        .with_foreground(is_fg)
                        .with_fullscreen(is_fs);
                        samples.push(sample);

                        if windows::Win32::System::Diagnostics::ToolHelp::Process32NextW(
                            snap, &mut entry,
                        )
                        .is_err()
                        {
                            break;
                        }
                    }
                }
                let _ = windows::Win32::Foundation::CloseHandle(snap);
            }
        }

        let mut snapshot = nx_governor_policy::SystemSnapshot::new(
            15.0,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        snapshot.foreground_pid = fg_pid.map(nx_governor_policy::Pid);
        snapshot.fullscreen_pid = fullscreen_pid.map(nx_governor_policy::Pid);
        snapshot.is_game_active = is_game_active;
        snapshot.processes = samples;
        Ok(snapshot)
    }
}

#[cfg(windows)]
fn parse_reg_query(output: &str, value_name: &str) -> Option<RegistryValueData> {
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[0].eq_ignore_ascii_case(value_name) {
            let reg_type = parts[1];
            let val_str = parts[2..].join(" ");
            return Some(match reg_type {
                "REG_DWORD" => {
                    let parsed = if let Some(hex) = val_str
                        .strip_prefix("0x")
                        .or_else(|| val_str.strip_prefix("0X"))
                    {
                        u32::from_str_radix(hex, 16).unwrap_or(0)
                    } else {
                        val_str.parse::<u32>().unwrap_or(0)
                    };
                    RegistryValueData::Dword(parsed)
                }
                "REG_QWORD" => {
                    let parsed = if let Some(hex) = val_str
                        .strip_prefix("0x")
                        .or_else(|| val_str.strip_prefix("0X"))
                    {
                        u64::from_str_radix(hex, 16).unwrap_or(0)
                    } else {
                        val_str.parse::<u64>().unwrap_or(0)
                    };
                    RegistryValueData::Qword(parsed)
                }
                "REG_BINARY" => {
                    let bytes = (0..val_str.len())
                        .step_by(2)
                        .filter_map(|i| u8::from_str_radix(val_str.get(i..i + 2)?, 16).ok())
                        .collect();
                    RegistryValueData::Binary(bytes)
                }
                _ => RegistryValueData::Sz(val_str),
            });
        }
    }
    None
}

#[cfg(not(windows))]
impl Platform for Win32Platform {
    fn move_resize_window(&self, _id: WindowId, _rect: Rect) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn show_window(&self, _id: WindowId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn hide_window(&self, _id: WindowId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_window_cloaked(&self, _id: WindowId, _cloaked: bool) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_foreground_window(&self, _id: WindowId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn close_window(&self, _id: WindowId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_dwm_transitions(&self, _id: WindowId, _enabled: bool) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_window_rect(&self, _id: WindowId) -> Result<Rect, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn is_window_visible(&self, _id: WindowId) -> bool {
        false
    }
    fn is_window_cloaked(&self, _id: WindowId) -> bool {
        false
    }
    fn set_always_on_top(&self, _id: WindowId, _topmost: bool) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn is_always_on_top(&self, _id: WindowId) -> bool {
        false
    }
    fn set_process_priority(
        &self,
        _pid: u32,
        _priority: ProcessPriority,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_process_affinity(&self, _pid: u32, _affinity_mask: u64) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_process_affinity(&self, _pid: u32) -> Result<u64, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_job_cpu_rate_cap(&self, _pid: u32, _rate_percent: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn remove_job_cpu_rate_cap(&self, _pid: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn suspend_process(&self, _pid: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn resume_process(&self, _pid: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn empty_process_working_set(&self, _pid: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_power_profile(&self, _profile: PowerProfile) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_power_profile(&self) -> Result<PowerProfile, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_registry_value(
        &self,
        _hive: &str,
        _subkey: &str,
        _value_name: &str,
        _value: RegistryValueData,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_registry_value(
        &self,
        _hive: &str,
        _subkey: &str,
        _value_name: &str,
    ) -> Result<Option<RegistryValueData>, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn delete_registry_value(
        &self,
        _hive: &str,
        _subkey: &str,
        _value_name: &str,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_service_state(
        &self,
        _service_name: &str,
        _state: ServiceRunState,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_service_state(
        &self,
        _service_name: &str,
    ) -> Result<Option<ServiceRunState>, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_privacy_toggle(
        &self,
        _toggle: PrivacyToggle,
        _enabled: bool,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_privacy_toggle(&self, _toggle: PrivacyToggle) -> Result<bool, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn write_system_file(&self, _path: &str, _content: &[u8]) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn read_system_file(&self, _path: &str) -> Result<Vec<u8>, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_taskbar_visible(&self, _visible: bool) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn register_appbar(
        &self,
        _id: WindowId,
        _edge: AppBarEdge,
        _size: u32,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn unregister_appbar(&self, _id: WindowId) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn set_timer_resolution_ns(&self, _resolution_100ns: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn register_hotkey(
        &self,
        _id: u32,
        _modifiers: Modifiers,
        _key: Key,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn unregister_hotkey(&self, _id: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    fn get_system_snapshot(&self) -> Result<nx_governor_policy::SystemSnapshot, PlatformError> {
        Err(PlatformError::Unsupported)
    }
}
