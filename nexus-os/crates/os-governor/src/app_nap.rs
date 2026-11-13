use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{info, warn};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use os_core::process::ProcessHandle;
use os_config::schema::GovernorConfig;
use crate::job_object::JobThrottleManager;

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
            job_throttler: Arc::new(JobThrottleManager::new()),
            own_pid,
        }
    }

    pub fn mark_active(&self, pid: u32) {
        if pid == 0 || pid == self.own_pid {
            return;
        }

        // 1. Release in-place from documented Win32 Job Object rate cap (zero handle recreate)
        self.job_throttler.unthrottle_process(pid);

        // 2. If previously deep-frozen via NtSuspendProcess, wake immediately
        let was_suspended = self.suspended_pids.write().remove(&pid);
        if was_suspended {
            if let Ok(handle) = ProcessHandle::open_for_governance(pid) {
                if let Err(e) = handle.resume() {
                    warn!("Failed to resume App Nap process PID {}: {:?}", pid, e);
                } else {
                    info!("⚡ App Nap: Instantly resumed PID {} via NtResumeProcess", pid);
                }
            }
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
        if self.is_process_excluded(pid) {
            return Ok(());
        }

        let handle = ProcessHandle::open_for_governance(pid)?;
        let throttle_rate = { self.config.read().cpu_throttle_percent.clamp(1, 10) };

        // Prefer documented Win32 Job Objects (1% CPU rate limit cap) - zero update breakage
        if let Err(e) = self.job_throttler.throttle_process(pid, handle.handle(), throttle_rate) {
            warn!("Job Object throttling fallback to NtSuspendProcess for PID {}: {:?}", pid, e);
            handle.suspend()?;
            self.suspended_pids.write().insert(pid);
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
        let job_throttler = self.job_throttler.clone();
        let own_pid = self.own_pid;

        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));

                let (enabled, idle_secs, throttle_percent) = {
                    let cfg = config.read();
                    (cfg.app_nap_enabled, cfg.app_nap_idle_secs.max(15), cfg.cpu_throttle_percent.clamp(1, 10))
                };

                if !enabled {
                    continue;
                }

                let now = Instant::now();
                let idle_threshold = Duration::from_secs(idle_secs);

                let candidates: Vec<u32> = {
                    let active_map = last_active.read();
                    let suspended_set = suspended.read();

                    active_map
                        .iter()
                        .filter(|(&pid, &time)| {
                            pid != own_pid
                                && now.duration_since(time) >= idle_threshold
                                && !suspended_set.contains(&pid)
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
                    if user_exclusions.iter().any(|ex| process_name.contains(&ex.to_lowercase())) {
                        continue;
                    }

                    if let Ok(handle) = ProcessHandle::open_for_governance(pid) {
                        // Apply documented Win32 Job Object rate cap (1%)
                        if job_throttler.throttle_process(pid, handle.handle(), throttle_percent).is_ok() {
                            info!("⚡ App Nap: Throttled background PID {} ({}) to {}% CPU", pid, process_name, throttle_percent);
                        }
                    }
                }
            }
        })
    }
}

pub fn get_process_name(pid: u32) -> anyhow::Result<String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

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
