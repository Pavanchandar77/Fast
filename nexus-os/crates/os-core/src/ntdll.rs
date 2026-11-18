//! Direct Import Address Table (IAT) static linkage to ntdll.
//! Eliminates runtime GetProcAddress heuristics that trigger Windows Defender false positives.

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, NTSTATUS};

#[cfg(windows)]
pub const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);

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

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    pub fn NtSuspendProcess(process_handle: HANDLE) -> NTSTATUS;
    pub fn NtResumeProcess(process_handle: HANDLE) -> NTSTATUS;
    pub fn NtSetTimerResolution(
        desired_resolution: u32,
        set_resolution: u8,
        current_resolution: *mut u32,
    ) -> NTSTATUS;
    pub fn NtTerminateProcess(process_handle: HANDLE, exit_status: NTSTATUS) -> NTSTATUS;
    pub fn RtlGetVersion(lp_version_information: *mut OSVERSIONINFOW) -> NTSTATUS;
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NTSTATUS(pub i32);

#[cfg(not(windows))]
#[allow(non_snake_case)]
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
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
    };

    let status = unsafe { RtlGetVersion(&mut info) };
    if status.0 >= 0 {
        WindowsVersion {
            major: info.dwMajorVersion,
            minor: info.dwMinorVersion,
            build: info.dwBuildNumber,
        }
    } else {
        WindowsVersion {
            major: 10,
            minor: 0,
            build: 22000,
        }
    }
}

#[cfg(not(windows))]
pub fn get_windows_version() -> WindowsVersion {
    WindowsVersion {
        major: 10,
        minor: 0,
        build: 22631,
    }
}
