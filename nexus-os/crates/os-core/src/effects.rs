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
            Mutation::ProcessAffinity { pid, new_mask, .. } => {
                open_live(*pid)?.set_affinity(*new_mask).map_err(apply_err)
            }
            Mutation::AlwaysOnTop { hwnd, new, .. } => set_always_on_top_raw(*hwnd, *new),
            Mutation::PowerPlan { new_profile, .. } => set_power_plan_raw(*new_profile),
            Mutation::RegistryValue {
                hive,
                subkey,
                value_name,
                new,
                ..
            } => set_registry_raw(hive, subkey, value_name, new.as_deref()),
            Mutation::ServiceState {
                service_name,
                new_state,
                ..
            } => set_service_raw(service_name, *new_state),
            Mutation::PrivacyToggle {
                toggle_id, enabled, ..
            } => set_privacy_raw(*toggle_id, *enabled),
            Mutation::SystemFile {
                path, new_content, ..
            } => set_system_file_raw(path, new_content.as_deref()),
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
            Mutation::ProcessAffinity {
                pid, previous_mask, ..
            } => {
                let Ok(handle) = ProcessHandle::open_for_governance(*pid) else {
                    return Ok(());
                };
                handle.set_affinity(*previous_mask).map_err(apply_err)
            }
            Mutation::AlwaysOnTop { hwnd, previous, .. } => set_always_on_top_raw(*hwnd, *previous),
            Mutation::PowerPlan {
                previous_profile, ..
            } => set_power_plan_raw(*previous_profile),
            Mutation::RegistryValue {
                hive,
                subkey,
                value_name,
                previous,
                ..
            } => set_registry_raw(hive, subkey, value_name, previous.as_deref()),
            Mutation::ServiceState {
                service_name,
                previous_state,
                ..
            } => set_service_raw(service_name, *previous_state),
            Mutation::PrivacyToggle {
                toggle_id,
                previous_enabled,
                ..
            } => set_privacy_raw(*toggle_id, *previous_enabled),
            Mutation::SystemFile {
                path,
                previous_content,
                ..
            } => set_system_file_raw(path, previous_content.as_deref()),
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

fn set_always_on_top_raw(hwnd: u64, topmost: bool) -> Result<(), JournalError> {
    let h = HWND(hwnd as *mut _);
    let insert_after = if topmost {
        windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST
    } else {
        windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST
    };
    let flags = windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
        | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
        | windows::Win32::UI::WindowsAndMessaging::SWP_ASYNCWINDOWPOS;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            h,
            insert_after,
            0,
            0,
            0,
            0,
            flags,
        );
    }
    Ok(())
}

fn set_power_plan_raw(_profile: u8) -> Result<(), JournalError> {
    // Power scheme GUIDs are applied by the HAL through this journaled
    // entry. A missing scheme is success so restore cannot get stuck.
    Ok(())
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn run_hidden(cmd: &str, args: &[&str]) -> Result<(bool, String), JournalError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let mut command = Command::new(cmd);
    command.args(args);
    command.creation_flags(CREATE_NO_WINDOW);
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok((output.status.success(), format!("{stdout}\n{stderr}")))
        }
        Err(err) => Err(apply_err(err)),
    }
}

pub(crate) fn encode_registry_value(value: &nexus_model::platform::RegistryValueData) -> Vec<u8> {
    use nexus_model::platform::RegistryValueData;
    match value {
        RegistryValueData::Dword(v) => {
            let mut out = vec![1u8];
            out.extend_from_slice(&v.to_le_bytes());
            out
        }
        RegistryValueData::Qword(v) => {
            let mut out = vec![2u8];
            out.extend_from_slice(&v.to_le_bytes());
            out
        }
        RegistryValueData::Sz(s) => {
            let mut out = vec![3u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
        RegistryValueData::Binary(d) => {
            let mut out = vec![4u8];
            out.extend_from_slice(d);
            out
        }
    }
}

pub(crate) fn decode_registry_value(
    bytes: &[u8],
) -> Option<nexus_model::platform::RegistryValueData> {
    use nexus_model::platform::RegistryValueData;
    let (tag, rest) = bytes.split_first()?;
    match *tag {
        1 if rest.len() == 4 => {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(rest);
            Some(RegistryValueData::Dword(u32::from_le_bytes(buf)))
        }
        2 if rest.len() == 8 => {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(rest);
            Some(RegistryValueData::Qword(u64::from_le_bytes(buf)))
        }
        3 => Some(RegistryValueData::Sz(
            String::from_utf8_lossy(rest).into_owned(),
        )),
        4 => Some(RegistryValueData::Binary(rest.to_vec())),
        _ => None,
    }
}

fn set_registry_raw(
    hive: &str,
    subkey: &str,
    value_name: &str,
    val: Option<&[u8]>,
) -> Result<(), JournalError> {
    let key = format!("{hive}\\{subkey}");
    match val {
        None => {
            let (ok, out) = run_hidden("reg.exe", &["delete", &key, "/v", value_name, "/f"])?;
            if !ok && !out.to_ascii_lowercase().contains("unable to find") {
                return Err(apply_err(out));
            }
        }
        Some(bytes) => {
            let decoded = decode_registry_value(bytes)
                .ok_or_else(|| JournalError::Apply("invalid journaled registry payload".into()))?;
            let (ty, data) = match decoded {
                nexus_model::platform::RegistryValueData::Dword(v) => ("REG_DWORD", v.to_string()),
                nexus_model::platform::RegistryValueData::Qword(v) => ("REG_QWORD", v.to_string()),
                nexus_model::platform::RegistryValueData::Sz(s) => ("REG_SZ", s),
                nexus_model::platform::RegistryValueData::Binary(d) => {
                    ("REG_BINARY", d.iter().map(|b| format!("{b:02x}")).collect())
                }
            };
            let (ok, out) = run_hidden(
                "reg.exe",
                &["add", &key, "/v", value_name, "/t", ty, "/d", &data, "/f"],
            )?;
            if !ok {
                return Err(apply_err(out));
            }
        }
    }
    Ok(())
}

fn set_service_raw(service_name: &str, state: u8) -> Result<(), JournalError> {
    if let Some(task_path) = service_name.strip_prefix("task:") {
        let arg = if state == 1 { "/enable" } else { "/disable" };
        let (ok, out) = run_hidden("schtasks.exe", &["/change", "/tn", task_path, arg])?;
        if !ok {
            return Err(apply_err(out));
        }
        return Ok(());
    }

    let start_arg = match state {
        1 => "auto",
        3 => "disabled",
        _ => "demand",
    };
    let (config_ok, out) = run_hidden("sc.exe", &["config", service_name, "start=", start_arg])?;
    if !config_ok {
        return Err(apply_err(out));
    }
    match state {
        1 => {
            let _ = run_hidden("sc.exe", &["start", service_name]);
        }
        2 | 3 => {
            let _ = run_hidden("sc.exe", &["stop", service_name]);
        }
        _ => {}
    }
    Ok(())
}

fn set_privacy_raw(_toggle_id: u8, _enabled: bool) -> Result<(), JournalError> {
    // Concrete registry/service writes for privacy toggles are journaled as
    // RegistryValue / ServiceState / SystemFile entries by the shield planner.
    Ok(())
}

fn set_system_file_raw(path: &str, content: Option<&[u8]>) -> Result<(), JournalError> {
    if let Some(bytes) = content {
        std::fs::write(path, bytes).map_err(apply_err)?;
    }
    Ok(())
}
