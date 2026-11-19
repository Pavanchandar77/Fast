//! Direct and probed linkage to ntdll routed through `nx-compat`.
//! Eliminates fragile static linkage while preserving capability probing and graceful fallbacks.

#![allow(clippy::missing_safety_doc)]

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, NTSTATUS};

#[cfg(windows)]
pub const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);

#[cfg(windows)]
#[allow(non_snake_case)]
pub unsafe fn NtSuspendProcess(process_handle: HANDLE) -> NTSTATUS {
    match nx_compat::dispatch::safe_nt_suspend_process(process_handle.0 as isize) {
        Ok(()) => STATUS_SUCCESS,
        Err(_) => NTSTATUS(-1),
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
pub unsafe fn NtResumeProcess(process_handle: HANDLE) -> NTSTATUS {
    match nx_compat::dispatch::safe_nt_resume_process(process_handle.0 as isize) {
        Ok(()) => STATUS_SUCCESS,
        Err(_) => NTSTATUS(-1),
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
pub unsafe fn NtSetTimerResolution(
    desired_resolution: u32,
    set_resolution: u8,
    current_resolution: *mut u32,
) -> NTSTATUS {
    match nx_compat::dispatch::safe_nt_set_timer_resolution(desired_resolution, set_resolution != 0)
    {
        Ok(curr) => {
            if !current_resolution.is_null() {
                *current_resolution = curr;
            }
            STATUS_SUCCESS
        }
        Err(_) => NTSTATUS(-1),
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
pub unsafe fn NtTerminateProcess(process_handle: HANDLE, exit_status: NTSTATUS) -> NTSTATUS {
    match nx_compat::dispatch::safe_nt_terminate_process(process_handle.0 as isize, exit_status.0) {
        Ok(()) => STATUS_SUCCESS,
        Err(_) => NTSTATUS(-1),
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
pub struct OSVERSIONINFOW {
    pub dwOSVersionInfoSize: u32,
    pub dwMajorVersion: u32,
    pub dwMinorVersion: u32,
    pub dwBuildNumber: u32,
    pub dwPlatformId: u32,
    pub szCSDVersion: [u16; 128],
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NTSTATUS(pub i32);

#[cfg(not(windows))]
#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn NtSetTimerResolution(
    _desired_resolution: u32,
    _set_resolution: u8,
    _current_resolution: *mut u32,
) -> NTSTATUS {
    NTSTATUS(0)
}

#[derive(Debug, Clone, Copy)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

#[cfg(windows)]
pub fn get_windows_version() -> WindowsVersion {
    let build_info = nx_compat::detect_build();
    WindowsVersion {
        major: build_info.major,
        minor: build_info.minor,
        build: build_info.build,
    }
}

#[cfg(not(windows))]
pub fn get_windows_version() -> WindowsVersion {
    let build_info = nx_compat::detect_build();
    WindowsVersion {
        major: build_info.major,
        minor: build_info.minor,
        build: build_info.build,
    }
}
