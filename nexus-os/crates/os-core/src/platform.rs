//! Win32 platform implementation for Nexus-OS.
//! Executes real system mutations (window manipulation, job object rate caps,
//! timer resolution, taskbar suppression, appbars, and hotkeys) via Win32 NT APIs.

use nexus_model::hotkeys::{Key, Modifiers};
use nexus_model::layout::{Rect, WindowId};
use nexus_model::platform::{AppBarEdge, Platform, PlatformError, ProcessPriority};

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
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
    SetInformationJobObject, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    REALTIME_PRIORITY_CLASS,
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
    IsWindowVisible, PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CLOSE,
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
                SWP_NOACTIVATE | SWP_NOZORDER,
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
            ProcessPriority::AboveNormal => NORMAL_PRIORITY_CLASS,
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
        proc.suspend()
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
    fn set_process_priority(
        &self,
        _pid: u32,
        _priority: ProcessPriority,
    ) -> Result<(), PlatformError> {
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
}
