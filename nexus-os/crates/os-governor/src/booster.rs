use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{debug, info};
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS};
use os_core::ntdll::NtSetTimerResolution;
use os_core::process::ProcessHandle;

pub struct ForegroundBooster {
    current_foreground_pid: AtomicU32,
    previous_foreground_pid: AtomicU32,
}

impl ForegroundBooster {
    pub fn new() -> Self {
        Self {
            current_foreground_pid: AtomicU32::new(0),
            previous_foreground_pid: AtomicU32::new(0),
        }
    }

    pub fn set_timer_resolution_0_5ms() -> anyhow::Result<()> {
        let mut current: u32 = 0;
        // 5000 units of 100ns = 0.5ms resolution
        let status = unsafe { NtSetTimerResolution(5000, 1, &mut current) };
        if status.0 < 0 {
            anyhow::bail!("NtSetTimerResolution failed with NTSTATUS: 0x{:08X}", status.0 as u32);
        }
        info!("Kernel timer resolution locked to 0.5ms (current: {} units)", current);
        Ok(())
    }

    pub fn on_foreground_changed(&self, new_pid: u32) {
        if new_pid == 0 {
            return;
        }

        let old_pid = self.current_foreground_pid.swap(new_pid, Ordering::SeqCst);
        if old_pid == new_pid {
            return;
        }

        self.previous_foreground_pid.store(old_pid, Ordering::SeqCst);

        // 1. Demote old foreground process
        if old_pid != 0 {
            if let Ok(handle) = ProcessHandle::open_for_governance(old_pid) {
                if let Err(e) = handle.set_priority(BELOW_NORMAL_PRIORITY_CLASS) {
                    debug!("Could not demote PID {}: {:?}", old_pid, e);
                } else {
                    debug!("Demoted previous foreground PID {} to BELOW_NORMAL", old_pid);
                }
            }
        }

        // 2. Elevate new foreground process
        if let Ok(handle) = ProcessHandle::open_for_governance(new_pid) {
            if let Err(e) = handle.set_priority(HIGH_PRIORITY_CLASS) {
                debug!("Could not boost PID {}: {:?}", new_pid, e);
            } else {
                info!("⚡ Elevated active foreground PID {} to HIGH_PRIORITY_CLASS", new_pid);
            }
        }
    }
}
