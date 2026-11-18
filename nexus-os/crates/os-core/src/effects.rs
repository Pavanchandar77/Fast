//! Win32 [`SystemMutator`]. Dead processes and destroyed windows revert as
//! success so one stale entry cannot stop the rest of the restore.

use std::ffi::c_void;

use nexus_journal::{JournalError, Mutation, SystemMutator};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::System::Threading::{
    ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABM_SETPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, ShowWindow, SW_HIDE, SW_SHOW};

use crate::hollow_shell::{show_taskbar_raw, HollowShellManager};
use crate::jobs;
use crate::ntdll::NtSetTimerResolution;
use crate::process::ProcessHandle;
use crate::window::{hwnd_from_bits, WindowHandle};
use crate::Rect;

pub(crate) struct Win32Mutator;

impl SystemMutator for Win32Mutator {
    fn apply(&mut self, mutation: &Mutation) -> Result<(), JournalError> {
        match mutation {
            Mutation::ProcessPriority { pid, new, .. } => {
                open_live(*pid)?.set_priority_class(*new).map_err(apply_err)
            }
            Mutation::JobCap {
                pid, new_percent, ..
            } => jobs::set_cap(*pid, *new_percent).map_err(apply_err),
            Mutation::Suspend { pid } => {
                open_live(*pid)?.suspend_if_allowed(true).map_err(apply_err)
            }
            Mutation::TaskbarHide => HollowShellManager::hide_taskbar_raw().map_err(apply_err),
            Mutation::AppBar { hwnd, edge, rect } => app_bar(*hwnd, *edge, *rect, true),
            Mutation::TimerResolution { new_100ns, .. } => set_timer(*new_100ns, true),
            Mutation::WindowCloak { hwnd } => set_cloak(*hwnd, true),
            Mutation::WindowHide { hwnd } => set_hidden(*hwnd, true),
            Mutation::WindowPosition { hwnd, new, .. } => place(*hwnd, *new),
            Mutation::EcoQos { pid, enabled, .. } => set_eco_qos(*pid, *enabled),
        }
    }

    fn revert(&mut self, mutation: &Mutation) -> Result<(), JournalError> {
        match mutation {
            Mutation::ProcessPriority { pid, previous, .. } => {
                let Ok(handle) = ProcessHandle::open_for_governance(*pid) else {
                    return Ok(());
                };
                handle.set_priority_class(*previous).map_err(apply_err)
            }
            Mutation::JobCap {
                pid,
                previous_percent,
                ..
            } => jobs::set_cap(*pid, previous_percent.unwrap_or(0)).map_err(apply_err),
            Mutation::Suspend { pid } => {
                let Ok(handle) = ProcessHandle::open_for_governance(*pid) else {
                    return Ok(());
                };
                handle.resume().map_err(apply_err)
            }
            Mutation::TaskbarHide => {
                show_taskbar_raw();
                Ok(())
            }
            Mutation::AppBar { hwnd, edge, rect } => app_bar(*hwnd, *edge, *rect, false),
            Mutation::TimerResolution { previous_100ns, .. } => {
                if *previous_100ns == 0 {
                    set_timer(0, false)
                } else {
                    set_timer(*previous_100ns, true)
                }
            }
            Mutation::WindowCloak { hwnd } => set_cloak(*hwnd, false),
            Mutation::WindowHide { hwnd } => set_hidden(*hwnd, false),
            Mutation::WindowPosition { hwnd, previous, .. } => place(*hwnd, *previous),
            Mutation::EcoQos {
                pid,
                previous_enabled,
                ..
            } => set_eco_qos(*pid, *previous_enabled),
        }
    }
}

fn apply_err(err: impl std::fmt::Display) -> JournalError {
    JournalError::Apply(err.to_string())
}

fn open_live(pid: u32) -> Result<ProcessHandle, JournalError> {
    ProcessHandle::open_for_governance(pid).map_err(apply_err)
}

/// Efficiency mode. A dead pid is success: there is nothing left to restore.
fn set_eco_qos(pid: u32, enabled: bool) -> Result<(), JournalError> {
    let Ok(handle) = ProcessHandle::open_for_governance(pid) else {
        return Ok(());
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if enabled {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    };
    unsafe {
        SetProcessInformation(
            handle.handle(),
            ProcessPowerThrottling,
            &state as *const _ as *const c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
        .map_err(apply_err)?;
    }
    Ok(())
}

fn set_timer(hundred_ns: u32, set_resolution: bool) -> Result<(), JournalError> {
    let mut current = 0u32;
    let status =
        unsafe { NtSetTimerResolution(hundred_ns, u8::from(set_resolution), &mut current) };
    if status.0 < 0 {
        Err(JournalError::Apply(format!(
            "NtSetTimerResolution NTSTATUS 0x{:08X}",
            status.0 as u32
        )))
    } else {
        Ok(())
    }
}

fn hwnd_live(bits: u64) -> Option<HWND> {
    let hwnd = hwnd_from_bits(bits);
    if unsafe { IsWindow(hwnd).as_bool() } {
        Some(hwnd)
    } else {
        None
    }
}

fn set_cloak(bits: u64, cloak: bool) -> Result<(), JournalError> {
    let Some(hwnd) = hwnd_live(bits) else {
        return Ok(());
    };
    let value = BOOL(i32::from(cloak));
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &value as *const _ as *const c_void,
            std::mem::size_of::<BOOL>() as u32,
        )
        .map_err(apply_err)?;
    }
    Ok(())
}

fn set_hidden(bits: u64, hidden: bool) -> Result<(), JournalError> {
    let Some(hwnd) = hwnd_live(bits) else {
        return Ok(());
    };
    let cmd = if hidden { SW_HIDE } else { SW_SHOW };
    let _ = unsafe { ShowWindow(hwnd, cmd) };
    Ok(())
}

fn place(bits: u64, rect: Rect) -> Result<(), JournalError> {
    let Some(hwnd) = hwnd_live(bits) else {
        return Ok(());
    };
    WindowHandle::new(hwnd)
        .place_async(RECT {
            left: rect.x,
            top: rect.y,
            right: rect.x.saturating_add(rect.width),
            bottom: rect.y.saturating_add(rect.height),
        })
        .map_err(apply_err)
}

fn app_bar(bits: u64, edge: u32, rect: Rect, register: bool) -> Result<(), JournalError> {
    let Some(hwnd) = hwnd_live(bits) else {
        return Ok(());
    };
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: 0,
        uEdge: edge,
        rc: RECT {
            left: rect.x,
            top: rect.y,
            right: rect.x.saturating_add(rect.width),
            bottom: rect.y.saturating_add(rect.height),
        },
        lParam: LPARAM(0),
    };
    unsafe {
        if register {
            SHAppBarMessage(ABM_NEW, &mut data);
            SHAppBarMessage(ABM_SETPOS, &mut data);
        } else {
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
    }
    Ok(())
}
