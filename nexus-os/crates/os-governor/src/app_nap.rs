use crate::job_object::JobThrottleManager;
use os_config::schema::GovernorConfig;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Hardcoded Windows NT critical processes that must NEVER be suspended or trimmed.
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

pub struct AppNapManager {
    config: Arc<RwLock<GovernorConfig>>,
    process_last_active: Arc<RwLock<HashMap<u32, Instant>>>,
    suspended_pids: Arc<RwLock<HashSet<u32>>>,
    napped: Arc<RwLock<HashSet<u32>>>,
    job_throttler: Arc<JobThrottleManager>,
    own_pid: u32,
}

impl AppNapManager {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        let own_pid = unsafe { GetCurrentProcessId() };
        Self {
            config,
            process_last_active: Arc::new(RwLock::new(HashMap::new())),
            suspended_pids: Arc::new(RwLock::new(HashSet::new())),
            napped: Arc::new(RwLock::new(HashSet::new())),
            job_throttler: Arc::new(JobThrottleManager::new()),
            own_pid,
        }
    }

    pub fn mark_active(&self, pid: u32) {
        if pid == 0 || pid == self.own_pid {
            return;
        }

        self.job_throttler.unthrottle_process(pid);
        self.napped.write().remove(&pid);
        if self.suspended_pids.write().remove(&pid) {
            let allow_nt_suspend = self.config.read().allow_nt_suspend;
            undo_suspend(pid, allow_nt_suspend);
        }

        self.process_last_active.write().insert(pid, Instant::now());
    }

    pub fn is_process_excluded(&self, pid: u32) -> bool {
        if pid == self.own_pid || pid == 0 || pid == 4 {
            return true;
        }

        let process_name = match get_process_name(pid) {
            Ok(name) => name.to_lowercase(),
            Err(_) => return true,
        };

        for &crit in CRITICAL_SYSTEM_PROCESSES {
            if process_name == crit {
                return true;
            }
        }

        let user_exclusions = {
            let cfg = self.config.read();
            cfg.exclude_from_nap.clone()
        };

        for excl in user_exclusions {
            if process_name.contains(&excl.to_lowercase()) {
                return true;
            }
        }

        false
    }

    pub fn throttle_pid(&self, pid: u32) -> anyhow::Result<()> {
        if self.is_process_excluded(pid) || self.napped.read().contains(&pid) {
            return Ok(());
        }

        let (throttle_rate, allow_nt_suspend) = {
            let cfg = self.config.read();
            (cfg.cpu_throttle_percent.clamp(1, 10), cfg.allow_nt_suspend)
        };

        if self
            .job_throttler
            .throttle_process(pid, throttle_rate)
            .is_ok()
        {
            self.napped.write().insert(pid);
            return Ok(());
        }

        journal_suspend(pid, allow_nt_suspend)?;
        if allow_nt_suspend {
            self.suspended_pids.write().insert(pid);
            self.napped.write().insert(pid);
        }
        Ok(())
    }

    pub fn suspend_pid(&self, pid: u32) -> anyhow::Result<()> {
        self.throttle_pid(pid)
    }

    pub fn start_worker(&self) -> std::thread::JoinHandle<()> {
        let config = self.config.clone();
        let last_active = self.process_last_active.clone();
        let suspended = self.suspended_pids.clone();
        let napped = self.napped.clone();
        let job_throttler = self.job_throttler.clone();
        let own_pid = self.own_pid;

        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));

            let (enabled, idle_secs, throttle_percent, allow_nt_suspend) = {
                let cfg = config.read();
                (
                    cfg.app_nap_enabled,
                    cfg.app_nap_idle_secs.max(15),
                    cfg.cpu_throttle_percent.clamp(1, 10),
                    cfg.allow_nt_suspend,
                )
            };

            if !enabled {
                let held: Vec<u32> = napped.read().iter().copied().collect();
                for pid in held {
                    job_throttler.unthrottle_process(pid);
                    napped.write().remove(&pid);
                    if suspended.write().remove(&pid) {
                        undo_suspend(pid, allow_nt_suspend);
                    }
                }
                continue;
            }

            let now = Instant::now();
            let idle_threshold = Duration::from_secs(idle_secs);

            let candidates: Vec<u32> = {
                let active_map = last_active.read();
                let already = napped.read();
                active_map
                    .iter()
                    .filter(|(&pid, &time)| {
                        pid != own_pid
                            && now.duration_since(time) >= idle_threshold
                            && !already.contains(&pid)
                    })
                    .map(|(&pid, _)| pid)
                    .collect()
            };

            for pid in candidates {
                let process_name = match get_process_name(pid) {
                    Ok(n) => n.to_lowercase(),
                    Err(_) => continue,
                };

                if CRITICAL_SYSTEM_PROCESSES.iter().any(|&c| process_name == c) {
                    continue;
                }

                let user_exclusions = {
                    let cfg = config.read();
                    cfg.exclude_from_nap.clone()
                };
                if user_exclusions
                    .iter()
                    .any(|ex| process_name.contains(&ex.to_lowercase()))
                {
                    continue;
                }

                if job_throttler
                    .throttle_process(pid, throttle_percent)
                    .is_ok()
                {
                    napped.write().insert(pid);
                    info!(
                        "App Nap: capped background PID {} ({}) at {}% CPU",
                        pid, process_name, throttle_percent
                    );
                    continue;
                }
                if journal_suspend(pid, allow_nt_suspend).is_ok() && allow_nt_suspend {
                    suspended.write().insert(pid);
                    napped.write().insert(pid);
                    info!(
                        "App Nap: suspended background PID {} ({}) because the job cap failed",
                        pid, process_name
                    );
                }
            }
        })
    }
}

/// Records a suspend only when the operator opted in.
fn journal_suspend(pid: u32, allow_nt_suspend: bool) -> anyhow::Result<()> {
    if !allow_nt_suspend {
        warn!("App Nap: job cap failed for PID {pid}; leaving the process running");
        anyhow::bail!("suspend is disabled");
    }
    os_core::recovery::commit(&os_core::Mutation::Suspend { pid })
        .map_err(|err| anyhow::anyhow!(err.to_string()))
}

/// Resumes a process that was suspended through the journal.
fn undo_suspend(pid: u32, allow_nt_suspend: bool) {
    // Revert even when the flag is now false: the entry was recorded while
    // `allow_nt_suspend` was on, and the process must not stay frozen.
    let _ = allow_nt_suspend;
    let _ = os_core::recovery::undo_last(
        |mutation| matches!(mutation, os_core::Mutation::Suspend { pid: found } if *found == pid),
    );
}

pub fn get_process_name(pid: u32) -> anyhow::Result<String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    let _ = windows::Win32::Foundation::CloseHandle(snapshot);
                    return Ok(name);
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        anyhow::bail!("PID not found")
    }
}
