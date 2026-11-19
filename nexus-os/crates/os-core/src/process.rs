#[cfg(windows)]
use crate::error::{CoreError, CoreResult};
#[cfg(windows)]
use nexus_journal::PriorityClass;
#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID, NTSTATUS};
#[cfg(windows)]
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
#[cfg(windows)]
use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, OpenProcess, OpenProcessToken, SetPriorityClass,
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_SUSPEND_RESUME, REALTIME_PRIORITY_CLASS,
};

#[cfg(windows)]
pub struct ProcessHandle {
    handle: HANDLE,
    pid: u32,
}

#[cfg(windows)]
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

    pub fn priority(&self) -> CoreResult<PriorityClass> {
        let raw = unsafe { GetPriorityClass(self.handle) };
        if raw == 0 {
            return Err(CoreError::Generic("GetPriorityClass failed".to_string()));
        }
        Ok(priority_from_raw(raw))
    }

    /// Raw priority write used by [`crate::platform::Win32Platform`].
    /// The governor journals a clamped class and calls [`Self::set_priority_class`].
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

    pub fn affinity(&self) -> CoreResult<u64> {
        let mut proc_mask: usize = 0;
        let mut sys_mask: usize = 0;
        unsafe {
            windows::Win32::System::Threading::GetProcessAffinityMask(
                self.handle,
                &mut proc_mask,
                &mut sys_mask,
            )
            .map_err(|e| CoreError::Generic(format!("GetProcessAffinityMask failed: {e}")))?;
        }
        Ok(proc_mask as u64)
    }

    pub fn set_affinity(&self, mask: u64) -> CoreResult<()> {
        unsafe {
            windows::Win32::System::Threading::SetProcessAffinityMask(self.handle, mask as usize)
                .map_err(|e| CoreError::Generic(format!("SetProcessAffinityMask failed: {e}")))?;
        }
        Ok(())
    }

    /// Low-level priority write. Callers must journal the previous class
    /// and run it through [`crate::clamp_priority`] before raising a process.
    pub(crate) fn set_priority_class(&self, class: PriorityClass) -> CoreResult<()> {
        self.set_priority(priority_to_flags(class))
    }

    /// Suspend only when `allow_nt_suspend` is true.
    /// The default configuration passes false, and this returns without
    /// calling `NtSuspendProcess`.
    pub fn suspend_if_allowed(&self, allow_nt_suspend: bool) -> CoreResult<()> {
        if !allow_nt_suspend {
            return Ok(());
        }
        self.suspend()
    }

    fn suspend(&self) -> CoreResult<()> {
        let status = unsafe { crate::ntdll::NtSuspendProcess(self.handle) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }

    pub fn resume(&self) -> CoreResult<()> {
        let status = unsafe { crate::ntdll::NtResumeProcess(self.handle) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }

    pub fn terminate(&self, exit_code: u32) -> CoreResult<()> {
        let status =
            unsafe { crate::ntdll::NtTerminateProcess(self.handle, NTSTATUS(exit_code as i32)) };
        if status.0 < 0 {
            Err(CoreError::NtStatus(status.0 as u32))
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(windows)]
pub fn priority_from_raw(raw: u32) -> PriorityClass {
    if raw == IDLE_PRIORITY_CLASS.0 {
        PriorityClass::Idle
    } else if raw == BELOW_NORMAL_PRIORITY_CLASS.0 {
        PriorityClass::BelowNormal
    } else if raw == ABOVE_NORMAL_PRIORITY_CLASS.0 {
        PriorityClass::AboveNormal
    } else if raw == HIGH_PRIORITY_CLASS.0 {
        PriorityClass::High
    } else if raw == REALTIME_PRIORITY_CLASS.0 {
        PriorityClass::Realtime
    } else {
        PriorityClass::Normal
    }
}

#[cfg(windows)]
pub fn priority_to_flags(class: PriorityClass) -> PROCESS_CREATION_FLAGS {
    match class {
        PriorityClass::Idle => IDLE_PRIORITY_CLASS,
        PriorityClass::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        PriorityClass::Normal => NORMAL_PRIORITY_CLASS,
        PriorityClass::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
        PriorityClass::High => HIGH_PRIORITY_CLASS,
        PriorityClass::Realtime => REALTIME_PRIORITY_CLASS,
    }
}

#[cfg(windows)]
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

        AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)?;

        let _ = CloseHandle(token);
        Ok(())
    }
}

#[cfg(not(windows))]
pub struct ProcessHandle {
    pid: u32,
}

#[cfg(not(windows))]
impl ProcessHandle {
    pub fn open(pid: u32, _desired_access: u32) -> crate::error::CoreResult<Self> {
        Ok(Self { pid })
    }

    pub fn open_for_governance(pid: u32) -> crate::error::CoreResult<Self> {
        Ok(Self { pid })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn handle(&self) -> usize {
        0
    }

    pub fn priority(&self) -> crate::error::CoreResult<nexus_journal::PriorityClass> {
        Ok(nexus_journal::PriorityClass::Normal)
    }

    pub fn set_priority(&self, _priority: u32) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn set_priority_class(
        &self,
        _class: nexus_journal::PriorityClass,
    ) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn empty_working_set(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn boost_to_high(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn demote_to_below_normal(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn set_normal(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn set_idle(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn suspend_if_allowed(&self, allow_nt_suspend: bool) -> crate::error::CoreResult<()> {
        if !allow_nt_suspend {
            return Ok(());
        }
        self.suspend()
    }

    pub fn suspend(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn resume(&self) -> crate::error::CoreResult<()> {
        Ok(())
    }

    pub fn terminate(&self, _exit_code: u32) -> crate::error::CoreResult<()> {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn enable_debug_privilege() -> crate::error::CoreResult<()> {
    Ok(())
}
