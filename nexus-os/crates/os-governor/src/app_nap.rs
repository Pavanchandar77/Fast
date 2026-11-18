use crate::job_object::JobThrottleManager;
use nexus_model::governor::{GovernorPolicy, GovernorPolicyConfig};
use os_config::schema::GovernorConfig;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

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
        #[cfg(windows)]
        let own_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        #[cfg(not(windows))]
        let own_pid = std::process::id();
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
        let process_name = match get_process_name(pid) {
            Ok(name) => name,
            Err(_) => return true,
        };
        let policy_cfg = self.policy_config();
        GovernorPolicy::is_exempt(pid, &process_name, &policy_cfg)
            || GovernorPolicy::is_user_excluded(&process_name, &policy_cfg)
    }

    fn policy_config(&self) -> GovernorPolicyConfig {
        let cfg = self.config.read();
        GovernorPolicyConfig {
            dynamic_priority_boost: cfg.dynamic_priority_boost,
            app_nap_enabled: cfg.app_nap_enabled,
            app_nap_idle_secs: cfg.app_nap_idle_secs,
            cpu_throttle_percent: cfg.cpu_throttle_percent,
            trim_on_minimize: cfg.trim_on_minimize,
            trim_on_unfocused: cfg.trim_on_unfocused,
            user_exclusions: cfg.exclude_from_nap.clone(),
            own_pid: self.own_pid,
        }
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

                let policy_cfg = {
                    let cfg = config.read();
                    GovernorPolicyConfig {
                        dynamic_priority_boost: cfg.dynamic_priority_boost,
                        app_nap_enabled: enabled,
                        app_nap_idle_secs: idle_secs,
                        cpu_throttle_percent: throttle_percent,
                        trim_on_minimize: cfg.trim_on_minimize,
                        trim_on_unfocused: cfg.trim_on_unfocused,
                        user_exclusions: cfg.exclude_from_nap.clone(),
                        own_pid,
                    }
                };
                if GovernorPolicy::is_exempt(pid, &process_name, &policy_cfg)
                    || GovernorPolicy::is_user_excluded(&process_name, &policy_cfg)
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

#[cfg(windows)]
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

#[cfg(not(windows))]
pub fn get_process_name(pid: u32) -> anyhow::Result<String> {
    Ok(format!("process-{pid}"))
}
