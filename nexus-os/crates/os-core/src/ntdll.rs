//! Direct Import Address Table (IAT) static linkage to ntdll.
//! Eliminates runtime GetProcAddress heuristics that trigger Windows Defender false positives.

use windows::Win32::Foundation::{HANDLE, NTSTATUS};

pub const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);

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

#[derive(Debug, Clone, Copy)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

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
