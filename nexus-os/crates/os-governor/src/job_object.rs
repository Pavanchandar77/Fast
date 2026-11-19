//! Journaled CPU caps. The Win32 job object lives in `os-core`; this type
//! only remembers which pids we have already recorded so a 5s worker cannot
//! append a new journal entry for the same process.

use os_core::Mutation;
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct JobThrottleManager {
    caps: RwLock<HashMap<u32, u32>>,
}

impl JobThrottleManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a cap before applying it. A second call for the same pid is a no-op.
    pub fn throttle_process(&self, pid: u32, rate_percent: u32) -> anyhow::Result<()> {
        if self.caps.read().contains_key(&pid) {
            return Ok(());
        }
        let percent = rate_percent.clamp(1, 90);
        os_core::recovery::commit(&Mutation::JobCap {
            pid,
            previous_percent: None,
            new_percent: percent,
        })
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        self.caps.write().insert(pid, percent);
        Ok(())
    }

    /// Reverts the journaled cap. The process returns to uncapped.
    pub fn unthrottle_process(&self, pid: u32) {
        let _ = os_core::recovery::undo_last(
            |mutation| matches!(mutation, Mutation::JobCap { pid: found, .. } if *found == pid),
        );
        self.caps.write().remove(&pid);
    }

    pub fn on_process_exit(&self, pid: u32) {
        self.unthrottle_process(pid);
    }
}
