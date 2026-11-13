use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{debug, info};
use windows::Win32::System::Threading::GetCurrentProcessId;
use os_core::process::ProcessHandle;
use os_config::schema::GovernorConfig;
use crate::app_nap::get_process_name;

const CRITICAL_SYSTEM_PROCESSES: &[&str] = &[
    "csrss.exe",
    "dwm.exe",
    "lsass.exe",
    "smss.exe",
    "winlogon.exe",
    "services.exe",
    "svchost.exe",
    "audiodg.exe",
    "system",
    "registry",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
    "conhost.exe",
    "explorer.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "nexus-os.exe",
    "nexus-compositor.exe",
    "nexus-governor.exe",
    "nexus-vfs.exe",
    "nexus-watchdog.exe",
    "spoolsv.exe",
    "securityhealthservice.exe",
];

pub struct RamPurger {
    config: Arc<RwLock<GovernorConfig>>,
    process_last_active: Arc<RwLock<HashMap<u32, Instant>>>,
    purged_processes: Arc<RwLock<HashMap<u32, Instant>>>,
    own_pid: u32,
}

impl RamPurger {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        let own_pid = unsafe { GetCurrentProcessId() };
        Self {
            config,
            process_last_active: Arc::new(RwLock::new(HashMap::new())),
            purged_processes: Arc::new(RwLock::new(HashMap::new())),
            own_pid,
        }
    }

    pub fn mark_active(&self, pid: u32) {
        if pid != 0 && pid != self.own_pid {
            self.process_last_active.write().insert(pid, Instant::now());
            // Clear purged status so when it goes idle again after sustained cooldown, it can be trimmed
            self.purged_processes.write().remove(&pid);
        }
    }

    /// Purges working set for a specific PID, subject to safety guards against thrashing
    pub fn purge_pid(&self, pid: u32) -> anyhow::Result<()> {
        if pid == 0 || pid == self.own_pid || pid == 4 {
            return Ok(());
        }

        // Check if critical system process
        if let Ok(name) = get_process_name(pid) {
            let lower = name.to_lowercase();
            if CRITICAL_SYSTEM_PROCESSES.iter().any(|&c| lower == c) {
                return Ok(());
            }
        }

        // Guard against rapid Alt-Tab thrashing: ensure sustained idle time before purge
        let cooldown_secs = {
            let cfg = self.config.read();
            cfg.ram_purge_delay_secs.max(20) // Enforce minimum 20s cooldown to eliminate hard page-fault thrashing
        };

        if let Some(&last_active) = self.process_last_active.read().get(&pid) {
            if last_active.elapsed() < Duration::from_secs(cooldown_secs) {
                debug!("Skipping working set trim for PID {} - active within cooldown (anti-thrash)", pid);
                return Ok(());
            }
        }

        let handle = ProcessHandle::open_for_governance(pid)?;
        handle.empty_working_set()?;
        self.purged_processes.write().insert(pid, Instant::now());
        info!("🧹 Flushed working set for PID {} via K32EmptyWorkingSet", pid);
        Ok(())
    }

    pub fn start_worker(&self) -> std::thread::JoinHandle<()> {
        let config = self.config.clone();
        let last_active = self.process_last_active.clone();
        let purged = self.purged_processes.clone();
        let own_pid = self.own_pid;

        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));

                let (enabled, delay_secs) = {
                    let cfg = config.read();
                    // Enforce minimum 30s cooldown to guard against SSD hard-fault thrashing
                    (cfg.ram_purge_enabled, cfg.ram_purge_delay_secs.max(30))
                };

                if !enabled {
                    continue;
                }

                let now = Instant::now();
                let threshold = Duration::from_secs(delay_secs);

                let candidates: Vec<u32> = {
                    let active_map = last_active.read();
                    let purged_map = purged.read();

                    active_map
                        .iter()
                        .filter(|(&pid, &time)| {
                            pid != own_pid
                                && pid != 4
                                && now.duration_since(time) >= threshold
                                && !purged_map.contains_key(&pid)
                        })
                        .map(|(&pid, _)| pid)
                        .collect()
                };

                for pid in candidates {
                    if let Ok(name) = get_process_name(pid) {
                        let lower = name.to_lowercase();
                        if CRITICAL_SYSTEM_PROCESSES.iter().any(|&c| lower == c) {
                            continue;
                        }
                    }

                    if let Ok(handle) = ProcessHandle::open_for_governance(pid) {
                        if handle.empty_working_set().is_ok() {
                            purged.write().insert(pid, now);
                            info!("🧹 Active background RAM purge completed for PID {}", pid);
                        }
                    }
                }
            }
        })
    }
}
