//! Foreground priority. Boost only, and only through the journal.
//!
//! Leaving the foreground reverts that one entry, which puts back whatever
//! class the process had. Nexus does not demote anyone to below-normal.

use crate::app_nap::get_process_name;
use os_config::schema::GovernorConfig;
use os_core::process::ProcessHandle;
use os_core::{clamp_priority, parse_priority, Mutation, PriorityRole};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tracing::{debug, info};

pub struct ForegroundBooster {
    config: Arc<RwLock<GovernorConfig>>,
    current_foreground_pid: AtomicU32,
}

impl ForegroundBooster {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        Self {
            config,
            current_foreground_pid: AtomicU32::new(0),
        }
    }

    /// Request a 0.5ms timer. Revert clears this process's request.
    pub fn set_timer_resolution_0_5ms() -> anyhow::Result<()> {
        os_core::recovery::commit(&Mutation::TimerResolution {
            previous_100ns: 0,
            new_100ns: 5_000,
        })
        .map_err(|err| anyhow::anyhow!(err.to_string()))
    }

    pub fn on_foreground_changed(&self, new_pid: u32) {
        if new_pid == 0 {
            return;
        }
        let old_pid = self.current_foreground_pid.swap(new_pid, Ordering::SeqCst);
        if old_pid == new_pid {
            return;
        }
        if old_pid != 0 {
            let _ = os_core::recovery::undo_last(|mutation| {
                matches!(
                    mutation,
                    Mutation::ProcessPriority { pid, .. } if *pid == old_pid
                )
            });
        }

        let (requested_text, whitelist) = {
            let cfg = self.config.read();
            (
                cfg.foreground_priority.clone(),
                cfg.priority_whitelist.clone(),
            )
        };
        let name = get_process_name(new_pid).unwrap_or_default();
        let requested = parse_priority(&requested_text);
        let desired = clamp_priority(requested, PriorityRole::Foreground, &name, &whitelist);
        let Ok(handle) = ProcessHandle::open_for_governance(new_pid) else {
            return;
        };
        let Ok(current) = handle.priority() else {
            return;
        };
        if desired.rank() <= current.rank() {
            debug!(
                "foreground PID {new_pid} already at {current:?}; not lowering it to {desired:?}"
            );
            return;
        }
        let mutation = Mutation::ProcessPriority {
            pid: new_pid,
            previous: current,
            new: desired,
        };
        match os_core::recovery::commit(&mutation) {
            Ok(()) => info!("foreground PID {new_pid} ({name}) boosted {current:?} -> {desired:?}"),
            Err(err) => debug!("could not boost PID {new_pid}: {err}"),
        }
    }
}

impl Default for ForegroundBooster {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(GovernorConfig::default())))
    }
}
