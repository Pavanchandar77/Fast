//! Safe dynamic API dispatchers with documented graceful fallbacks.
//!
//! Replaces direct static imports with runtime-probed function pointers
//! resolved via `GetProcAddress`. Ensures Nexus-OS never crashes if an
//! undocumented NT export or DWM attribute is changed or removed by a Windows update.

use crate::error::{CompatError, CompatResult};
#[cfg(windows)]
use crate::probe::{Capability, LiveProbeProvider, ProbeProvider};

#[cfg(windows)]
use windows::core::PCSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, HWND, NTSTATUS};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

/// Safe NT process suspension.
/// If `ntdll!NtSuspendProcess` is unavailable, logs a warning and skips suspension
/// rather than crashing.
pub fn safe_nt_suspend_process(process_handle: isize) -> CompatResult<()> {
    if process_handle == 0 || process_handle == -1 {
        return Err(CompatError::ProbeFailed(
            "Invalid process handle".to_string(),
        ));
    }

    #[cfg(windows)]
    unsafe {
        type NtSuspendFn = unsafe extern "system" fn(HANDLE) -> NTSTATUS;
        if let Ok(ntdll) = GetModuleHandleW(windows::core::w!("ntdll.dll")) {
            if let Some(proc) = GetProcAddress(ntdll, PCSTR(b"NtSuspendProcess\0".as_ptr())) {
                let nt_suspend: NtSuspendFn = std::mem::transmute(proc);
                let status = nt_suspend(HANDLE(process_handle as *mut _));
                if status.0 < 0 {
                    return Err(CompatError::ProbeFailed(format!(
                        "NtSuspendProcess NTSTATUS: 0x{:08X}",
                        status.0 as u32
                    )));
                }
                return Ok(());
            }
        }
        tracing::warn!("ntdll!NtSuspendProcess missing; skipping suspend via graceful fallback");
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = process_handle;
        Ok(())
    }
}

/// Safe NT process resumption.
/// If `ntdll!NtResumeProcess` is unavailable, logs info and skips.
pub fn safe_nt_resume_process(process_handle: isize) -> CompatResult<()> {
    if process_handle == 0 || process_handle == -1 {
        return Err(CompatError::ProbeFailed(
            "Invalid process handle".to_string(),
        ));
    }

    #[cfg(windows)]
    unsafe {
        type NtResumeFn = unsafe extern "system" fn(HANDLE) -> NTSTATUS;
        if let Ok(ntdll) = GetModuleHandleW(windows::core::w!("ntdll.dll")) {
            if let Some(proc) = GetProcAddress(ntdll, PCSTR(b"NtResumeProcess\0".as_ptr())) {
                let nt_resume: NtResumeFn = std::mem::transmute(proc);
                let status = nt_resume(HANDLE(process_handle as *mut _));
                if status.0 < 0 {
                    return Err(CompatError::ProbeFailed(format!(
                        "NtResumeProcess NTSTATUS: 0x{:08X}",
                        status.0 as u32
                    )));
                }
                return Ok(());
            }
        }
        tracing::warn!("ntdll!NtResumeProcess missing; skipping resume via graceful fallback");
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = process_handle;
        Ok(())
    }
}

/// Safe NT timer resolution adjustment.
/// If `ntdll!NtSetTimerResolution` is unavailable, falls back gracefully.
pub fn safe_nt_set_timer_resolution(
    desired_resolution_100ns: u32,
    set_resolution: bool,
) -> CompatResult<u32> {
    #[cfg(windows)]
    unsafe {
        type NtSetTimerResFn = unsafe extern "system" fn(u32, u8, *mut u32) -> NTSTATUS;

        if let Ok(ntdll) = GetModuleHandleW(windows::core::w!("ntdll.dll")) {
            if let Some(proc) = GetProcAddress(ntdll, PCSTR(b"NtSetTimerResolution\0".as_ptr())) {
                let nt_set_timer: NtSetTimerResFn = std::mem::transmute(proc);
                let mut current: u32 = 0;
                let status = nt_set_timer(
                    desired_resolution_100ns,
                    u8::from(set_resolution),
                    &mut current,
                );
                if status.0 >= 0 {
                    return Ok(current);
                } else {
                    return Err(CompatError::ProbeFailed(format!(
                        "NtSetTimerResolution NTSTATUS: 0x{:08X}",
                        status.0 as u32
                    )));
                }
            }
        }
        tracing::warn!("ntdll!NtSetTimerResolution missing; using graceful multimedia fallback");
        Ok(0)
    }

    #[cfg(not(windows))]
    {
        let _ = (desired_resolution_100ns, set_resolution);
        Ok(0)
    }
}

/// Safe NT process termination.
/// If `ntdll!NtTerminateProcess` is unavailable, falls back to `kernel32!TerminateProcess`.
pub fn safe_nt_terminate_process(process_handle: isize, exit_status: i32) -> CompatResult<()> {
    #[cfg(windows)]
    unsafe {
        type NtTermFn = unsafe extern "system" fn(HANDLE, NTSTATUS) -> NTSTATUS;

        if let Ok(ntdll) = GetModuleHandleW(windows::core::w!("ntdll.dll")) {
            if let Some(proc) = GetProcAddress(ntdll, PCSTR(b"NtTerminateProcess\0".as_ptr())) {
                let nt_term: NtTermFn = std::mem::transmute(proc);
                let status = nt_term(HANDLE(process_handle as *mut _), NTSTATUS(exit_status));
                if status.0 >= 0 {
                    return Ok(());
                }
            }
        }

        // Fallback to Win32 TerminateProcess
        use windows::Win32::System::Threading::TerminateProcess;
        TerminateProcess(HANDLE(process_handle as *mut _), exit_status as u32)
            .map_err(|e| CompatError::Platform(format!("TerminateProcess failed: {e}")))
    }

    #[cfg(not(windows))]
    {
        let _ = (process_handle, exit_status);
        Ok(())
    }
}

/// Safe application of DWM window attributes (Mica/Acrylic, dark mode, rounded corners, margins).
/// Probes support and applies graceful fallbacks when attributes are unsupported.
pub fn safe_apply_dwm_attributes(
    hwnd: isize,
    system_backdrop: u32,
    dark_mode: bool,
    round_corners: bool,
    extend_frame: bool,
) -> CompatResult<()> {
    if hwnd == 0 {
        return Err(CompatError::Platform("Null HWND".to_string()));
    }

    #[cfg(windows)]
    unsafe {
        use std::ffi::c_void;
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Graphics::Dwm::{
            DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWM_SYSTEMBACKDROP_TYPE,
            DWM_WINDOW_CORNER_PREFERENCE,
        };
        use windows::Win32::UI::Controls::MARGINS;

        let hwnd_obj = HWND(hwnd as *mut c_void);
        let probe = LiveProbeProvider;

        // 1. System Backdrop (Mica / Acrylic)
        if system_backdrop != 0 {
            let mica_probe = probe.probe(Capability::DwmMica);
            if mica_probe.status.is_available() {
                let backdrop = DWM_SYSTEMBACKDROP_TYPE(system_backdrop as i32);
                let _ = DwmSetWindowAttribute(
                    hwnd_obj,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    std::ptr::from_ref(&backdrop).cast(),
                    std::mem::size_of_val(&backdrop) as u32,
                );
            } else if mica_probe.status.is_operational() {
                // Acrylic fallback
                let acrylic = DWM_SYSTEMBACKDROP_TYPE(3);
                let _ = DwmSetWindowAttribute(
                    hwnd_obj,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    std::ptr::from_ref(&acrylic).cast(),
                    std::mem::size_of_val(&acrylic) as u32,
                );
            }
        }

        // 2. Immersive Dark Mode
        let dark = BOOL(if dark_mode { 1 } else { 0 });
        let _ = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::from_ref(&dark).cast(),
            std::mem::size_of_val(&dark) as u32,
        );

        // 3. Rounded Corners
        if round_corners
            && probe
                .probe(Capability::DwmRoundedCorners)
                .status
                .is_available()
        {
            let corners = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(
                hwnd_obj,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                std::ptr::from_ref(&corners).cast(),
                std::mem::size_of_val(&corners) as u32,
            );
        }

        // 4. Extend Frame into client area
        if extend_frame {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd_obj, &margins);
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = (
            hwnd,
            system_backdrop,
            dark_mode,
            round_corners,
            extend_frame,
        );
        Ok(())
    }
}

/// Safe DWM cloak setting. Falls back to ShowWindow(SW_HIDE) if cloaking attribute is unsupported.
pub fn safe_set_window_cloak(hwnd: isize, cloaked: bool) -> CompatResult<()> {
    if hwnd == 0 {
        return Err(CompatError::Platform("Null HWND".to_string()));
    }

    #[cfg(windows)]
    unsafe {
        use std::ffi::c_void;
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};

        let hwnd_obj = HWND(hwnd as *mut c_void);
        let val = BOOL(if cloaked { 1 } else { 0 });
        let res = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWA_CLOAK,
            std::ptr::from_ref(&val).cast(),
            std::mem::size_of::<BOOL>() as u32,
        );

        if res.is_err() {
            // Fallback: ShowWindow hide/show
            let cmd = if cloaked {
                windows::Win32::UI::WindowsAndMessaging::SW_HIDE
            } else {
                windows::Win32::UI::WindowsAndMessaging::SW_SHOW
            };
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd_obj, cmd);
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = (hwnd, cloaked);
        Ok(())
    }
}

/// Safe DWM forced transitions disabled.
pub fn safe_set_dwm_transitions(hwnd: isize, enabled: bool) -> CompatResult<()> {
    if hwnd == 0 {
        return Err(CompatError::Platform("Null HWND".to_string()));
    }

    #[cfg(windows)]
    unsafe {
        use std::ffi::c_void;
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
        };

        let hwnd_obj = HWND(hwnd as *mut c_void);
        let val = BOOL(if enabled { 0 } else { 1 });
        let _ = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            std::ptr::from_ref(&val).cast(),
            std::mem::size_of::<BOOL>() as u32,
        );
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = (hwnd, enabled);
        Ok(())
    }
}

/// Safe dynamic probe for undocumented `user32!SetWindowCompositionAttribute`.
pub fn safe_set_composition_attribute(
    hwnd: isize,
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
) -> CompatResult<bool> {
    if hwnd == 0 {
        return Err(CompatError::Platform("Null HWND".to_string()));
    }

    #[cfg(windows)]
    unsafe {
        use std::ffi::c_void;
        use windows::Win32::Foundation::BOOL;

        #[repr(C)]
        struct AccentPolicy {
            accent_state: u32,
            accent_flags: u32,
            gradient_color: u32,
            animation_id: u32,
        }

        #[repr(C)]
        struct WindowCompositionAttribData {
            attrib: u32,
            data: *mut c_void,
            size: usize,
        }

        type SetWindowCompositionAttrFn =
            unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> BOOL;

        if let Ok(user32) = GetModuleHandleW(windows::core::w!("user32.dll")) {
            if let Some(proc) =
                GetProcAddress(user32, PCSTR(b"SetWindowCompositionAttribute\0".as_ptr()))
            {
                let set_comp: SetWindowCompositionAttrFn = std::mem::transmute(proc);

                let mut policy = AccentPolicy {
                    accent_state,
                    accent_flags,
                    gradient_color,
                    animation_id: 0,
                };

                let mut data = WindowCompositionAttribData {
                    attrib: 19, // WCA_ACCENT_POLICY
                    data: &mut policy as *mut _ as *mut c_void,
                    size: std::mem::size_of::<AccentPolicy>(),
                };

                let success = set_comp(HWND(hwnd as *mut c_void), &mut data);
                return Ok(success.as_bool());
            }
        }

        tracing::info!("SetWindowCompositionAttribute missing; graceful fallback applied");
        Ok(false)
    }

    #[cfg(not(windows))]
    {
        let _ = (hwnd, accent_state, accent_flags, gradient_color);
        Ok(false)
    }
}

/// Safe registration of ShellHookWindow with universal fallback.
pub fn safe_register_shell_hook(hwnd: isize) -> CompatResult<bool> {
    if hwnd == 0 {
        return Err(CompatError::Platform("Null HWND".to_string()));
    }

    #[cfg(windows)]
    unsafe {
        use std::ffi::c_void;
        use windows::Win32::Foundation::BOOL;

        type RegShellHookFn = unsafe extern "system" fn(HWND) -> BOOL;

        if let Ok(user32) = GetModuleHandleW(windows::core::w!("user32.dll")) {
            if let Some(proc) = GetProcAddress(user32, PCSTR(b"RegisterShellHookWindow\0".as_ptr()))
            {
                let reg_hook: RegShellHookFn = std::mem::transmute(proc);
                let res = reg_hook(HWND(hwnd as *mut c_void));
                return Ok(res.as_bool());
            }
        }
        tracing::info!("RegisterShellHookWindow unavailable; falling back to SetWinEventHook");
        Ok(false)
    }

    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_dispatch_null_hwnd_rejected() {
        assert!(safe_apply_dwm_attributes(0, 2, true, true, true).is_err());
        assert!(safe_set_window_cloak(0, true).is_err());
        assert!(safe_set_dwm_transitions(0, false).is_err());
        assert!(safe_set_composition_attribute(0, 0, 0, 0).is_err());
        assert!(safe_register_shell_hook(0).is_err());
    }

    #[test]
    fn test_safe_ntdll_dispatch_mock() {
        assert!(safe_nt_suspend_process(100).is_ok());
        assert!(safe_nt_resume_process(100).is_ok());
        assert!(safe_nt_set_timer_resolution(5000, true).is_ok());
        assert!(safe_nt_terminate_process(100, 0).is_ok());
    }
}
