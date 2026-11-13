use std::collections::HashMap;
use std::ffi::c_void;
use parking_lot::RwLock;
use tracing::{debug, info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JobObjectCpuRateControlInformation, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOB_OBJECT_CPU_RATE_CONTROL,
};

#[derive(Clone, Copy)]
pub struct SendHandle(pub HANDLE);
unsafe impl Send for SendHandle {}
unsafe impl Sync for SendHandle {}

// Rate control flags: Enable rate control (0x1) + Hard Cap (0x4)
const RATE_CONTROL_ENABLE: u32 = 0x1;
const RATE_CONTROL_HARD_CAP: u32 = 0x4;

pub struct JobThrottleManager {
    // Permanent job mapping per PID until process terminates
    jobs: RwLock<HashMap<u32, SendHandle>>,
}

impl JobThrottleManager {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
        }
    }

    /// Throttles a background process to rate_percent (default 1 = 1% CPU rate).
    /// If a Job Object already exists for this PID, it modifies the rate control flags in-place.
    /// This prevents handle bloat and avoids nested Job Object assignment restrictions.
    pub fn throttle_process(&self, pid: u32, process_handle: HANDLE, rate_percent: u32) -> anyhow::Result<()> {
        let rate_val = rate_percent.clamp(1, 90) * 100;

        let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        info.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL(RATE_CONTROL_ENABLE | RATE_CONTROL_HARD_CAP);
        info.Anonymous.CpuRate = rate_val;

        let guard = self.jobs.read();
        if let Some(&send_handle) = guard.get(&pid) {
            // In-place re-enable: process is already assigned to this Job Object
            unsafe {
                SetInformationJobObject(
                    send_handle.0,
                    JobObjectCpuRateControlInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                )?;
            }
            debug!("Job Object Throttler: Re-enabled in-place {}% CPU cap for PID {}", rate_percent, pid);
            return Ok(());
        }
        drop(guard);

        // First-time assignment: create Job Object and assign
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
        unsafe {
            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            ) {
                let _ = CloseHandle(job);
                return Err(e.into());
            }

            if let Err(e) = AssignProcessToJobObject(job, process_handle) {
                let _ = CloseHandle(job);
                return Err(e.into());
            }
        }

        self.jobs.write().insert(pid, SendHandle(job));
        info!("⚡ Job Object Throttler: Assigned PID {} to Job Object with {}% CPU cap", pid, rate_percent);
        Ok(())
    }

    /// Un-throttles a process by setting ControlFlags = 0 in-place.
    /// DOES NOT close the Job handle, keeping the process attachment intact and leak-free.
    pub fn unthrottle_process(&self, pid: u32) {
        let guard = self.jobs.read();
        if let Some(&send_handle) = guard.get(&pid) {
            let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
            // Disabling rate control flags entirely in-place
            info.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL(0);
            info.Anonymous.CpuRate = 0;

            unsafe {
                let res = SetInformationJobObject(
                    send_handle.0,
                    JobObjectCpuRateControlInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                );
                if let Err(e) = res {
                    warn!("Failed to disable CPU rate control in-place for PID {}: {:?}", pid, e);
                } else {
                    debug!("⚡ Job Object Throttler: Un-throttled PID {} in-place (0% cap / full speed)", pid);
                }
            }
        }
    }

    /// Cleans up and closes the Job Object handle once the process exits
    pub fn on_process_exit(&self, pid: u32) {
        if let Some(send_handle) = self.jobs.write().remove(&pid) {
            unsafe {
                let _ = CloseHandle(send_handle.0);
            }
            debug!("Job Object Throttler: Closed Job handle on exit for PID {}", pid);
        }
    }
}

impl Drop for JobThrottleManager {
    fn drop(&mut self) {
        let mut map = self.jobs.write();
        for (_, send_handle) in map.drain() {
            unsafe {
                let _ = CloseHandle(send_handle.0);
            }
        }
    }
}
