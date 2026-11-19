//! Named per-process job objects so a cap can be cleared after Nexus exits.
//!
//! The job is `Local\NexusOsJob-{pid}`. The handle stays open for the life of
//! the cap: closing it destroys the job, and a process can be assigned only once.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation, OpenJobObjectW,
    SetInformationJobObject, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL,
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
};
use windows::Win32::System::SystemServices::{
    JOB_OBJECT_ASSIGN_PROCESS, JOB_OBJECT_QUERY, JOB_OBJECT_SET_ATTRIBUTES, JOB_OBJECT_TERMINATE,
};
use windows::Win32::System::Threading::{PROCESS_SET_QUOTA, PROCESS_TERMINATE};

use crate::error::CoreResult;
use crate::process::ProcessHandle;

const ACCESS: u32 =
    JOB_OBJECT_ASSIGN_PROCESS | JOB_OBJECT_SET_ATTRIBUTES | JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE;

fn cache() -> &'static Mutex<HashMap<u32, isize>> {
    static CACHE: OnceLock<Mutex<HashMap<u32, isize>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_cache() -> std::sync::MutexGuard<'static, HashMap<u32, isize>> {
    match cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn job_name(pid: u32) -> Vec<u16> {
    format!("Local\\NexusOsJob-{pid}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn from_bits(bits: isize) -> HANDLE {
    HANDLE(bits as *mut c_void)
}

/// `percent == 0` clears the CPU rate. A missing job is already uncapped.
pub(crate) fn set_cap(pid: u32, percent: u32) -> CoreResult<()> {
    if percent == 0 {
        return clear_cap(pid);
    }
    let handle = open_or_create(pid)?;
    apply_rate(handle, percent)
}

pub(crate) fn clear_cap(pid: u32) -> CoreResult<()> {
    let name = job_name(pid);
    let handle = unsafe { OpenJobObjectW(ACCESS, false, PCWSTR(name.as_ptr())) };
    let handle = match handle {
        Ok(handle) => handle,
        Err(_) => return Ok(()),
    };
    let result = apply_rate(handle, 0);
    // The cached handle, when we have one, keeps the job alive. This extra
    // handle is only for the clear.
    if lock_cache().get(&pid).copied() != Some(handle.0 as isize) {
        let _ = unsafe { CloseHandle(handle) };
    }
    result
}

fn open_or_create(pid: u32) -> CoreResult<HANDLE> {
    if let Some(bits) = lock_cache().get(&pid).copied() {
        return Ok(from_bits(bits));
    }
    let name = job_name(pid);
    if let Ok(handle) = unsafe { OpenJobObjectW(ACCESS, false, PCWSTR(name.as_ptr())) } {
        lock_cache().insert(pid, handle.0 as isize);
        return Ok(handle);
    }
    let handle = unsafe { CreateJobObjectW(None, PCWSTR(name.as_ptr()))? };
    let created = unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
    if created {
        let process = ProcessHandle::open(pid, PROCESS_SET_QUOTA.0 | PROCESS_TERMINATE.0)?;
        if let Err(err) = unsafe { AssignProcessToJobObject(handle, process.handle()) } {
            let _ = unsafe { CloseHandle(handle) };
            return Err(err.into());
        }
    }
    lock_cache().insert(pid, handle.0 as isize);
    Ok(handle)
}

fn apply_rate(handle: HANDLE, percent: u32) -> CoreResult<()> {
    let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    unsafe {
        if percent == 0 {
            info.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL(0);
            info.Anonymous.CpuRate = 0;
        } else {
            info.ControlFlags =
                JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
            info.Anonymous.CpuRate = percent.saturating_mul(100);
        }
        SetInformationJobObject(
            handle,
            JobObjectCpuRateControlInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
        )?;
    }
    Ok(())
}
