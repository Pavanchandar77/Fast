use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID, NTSTATUS};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, SetPriorityClass,
    BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_SUSPEND_RESUME,
};
use crate::error::{CoreError, CoreResult};
use crate::ntdll::{NtResumeProcess, NtSuspendProcess, NtTerminateProcess};

pub struct ProcessHandle {
    handle: HANDLE,
    pid: u32,
}

impl ProcessHandle {
    pub fn open(pid: u32, desired_access: u32) -> CoreResult<Self> {
        let handle = unsafe {
            OpenProcess(
                windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS(desired_access),
                false,
                pid,
            )?
        };
        Ok(Self { handle, pid })
    }

    pub fn open_for_governance(pid: u32) -> CoreResult<Self> {
        let access = PROCESS_SET_INFORMATION.0
            | PROCESS_QUERY_LIMITED_INFORMATION.0
            | PROCESS_SUSPEND_RESUME.0
            | PROCESS_SET_QUOTA.0;

        match Self::open(pid, access) {
            Ok(h) => Ok(h),
            Err(_) => {
                let fallback = PROCESS_SET_INFORMATION.0 | PROCESS_QUERY_LIMITED_INFORMATION.0;
                Self::open(pid, fallback)
            }
        }
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn empty_working_set(&self) -> CoreResult<()> {
        unsafe {
            K32EmptyWorkingSet(self.handle).ok()?;
        }
        Ok(())
    }

    pub fn set_priority(&self, priority: PROCESS_CREATION_FLAGS) -> CoreResult<()> {
        unsafe {
            SetPriorityClass(self.handle, priority)?;
        }
        Ok(())
    }

    pub fn boost_to_high(&self) -> CoreResult<()> {
        self.set_priority(HIGH_PRIORITY_CLASS)
    }

    pub fn demote_to_below_normal(&self) -> CoreResult<()> {
        self.set_priority(BELOW_NORMAL_PRIORITY_CLASS)
    }

    pub fn set_normal(&self) -> CoreResult<()> {
        self.set_priority(NORMAL_PRIORITY_CLASS)
    }

    pub fn set_idle(&self) -> CoreResult<()> {
        self.set_priority(IDLE_PRIORITY_CLASS)
    }

    pub fn suspend(&self) -> CoreResult<()> {
        let status = unsafe { NtSuspendProcess(self.handle) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }

    pub fn resume(&self) -> CoreResult<()> {
        let status = unsafe { NtResumeProcess(self.handle) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }

    pub fn terminate(&self, exit_code: u32) -> CoreResult<()> {
        let status = unsafe { NtTerminateProcess(self.handle, NTSTATUS(exit_code as i32)) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

pub fn enable_debug_privilege() -> CoreResult<()> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )?;

        let mut luid = LUID::default();
        LookupPrivilegeValueW(PCWSTR::null(), w!("SeDebugPrivilege"), &mut luid)?;

        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        AdjustTokenPrivileges(
            token,
            false,
            Some(&tp),
            0,
            None,
            None,
        )?;

        let _ = CloseHandle(token);
        Ok(())
    }
}
