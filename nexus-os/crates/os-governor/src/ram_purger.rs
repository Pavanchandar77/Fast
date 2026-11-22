use crate::app_nap::get_process_name;
use nexus_model::governor::CRITICAL_PROCESSES;
use nexus_model::platform::Platform;
use os_config::schema::GovernorConfig;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

const CRITICAL_SYSTEM_PROCESSES: &[&str] = CRITICAL_PROCESSES;

/// Extra names that must keep their working set. Trimming these makes the
/// desktop hitch or takes down a system process. Every other idle process
/// is eligible, not a short list of apps.
const KEEP_RESIDENT: &[&str] = &[
    "secure system",
    "memory compression",
    "idle",
    "registry",
    "system",
];

/// True when an idle process should give its working set back.
/// Foreground, this process, and Windows' own critical processes stay put.
/// Every other process is a candidate, including ones that were never focused.
pub fn should_trim(pid: u32, name: &str, foreground: u32, own_pid: u32) -> bool {
    if pid == 0 || pid == 4 || pid == own_pid || pid == foreground {
        return false;
    }
    let lower = name.to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if KEEP_RESIDENT.iter().any(|keep| lower == *keep) {
        return false;
    }
    if CRITICAL_SYSTEM_PROCESSES.iter().any(|crit| lower == *crit) {
        return false;
    }
    if lower.starts_with("nexus-") || lower == "nexus-os.exe" {
        return false;
    }
    true
}

pub struct RamPurger {
    config: Arc<RwLock<GovernorConfig>>,
    process_last_active: Arc<RwLock<HashMap<u32, Instant>>>,
    purged_processes: Arc<RwLock<HashMap<u32, Instant>>>,
    own_pid: u32,
    platform: Arc<dyn Platform>,
}

impl RamPurger {
    pub fn new(config: Arc<RwLock<GovernorConfig>>, platform: Arc<dyn Platform>) -> Self {
        #[cfg(windows)]
        let own_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        #[cfg(not(windows))]
        let own_pid = std::process::id();
        Self {
            config,
            process_last_active: Arc::new(RwLock::new(HashMap::new())),
            purged_processes: Arc::new(RwLock::new(HashMap::new())),
            own_pid,
            platform,
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
                debug!(
                    "Skipping working set trim for PID {} - active within cooldown (anti-thrash)",
                    pid
                );
                return Ok(());
            }
        }

        self.platform
            .empty_process_working_set(pid)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.purged_processes.write().insert(pid, Instant::now());
        info!("🧹 Flushed working set for PID {} via Platform trait", pid);
        Ok(())
    }

    pub fn start_worker(&self) -> std::thread::JoinHandle<()> {
        let config = self.config.clone();
        let last_active = self.process_last_active.clone();
        let purged = self.purged_processes.clone();
        let own_pid = self.own_pid;
        let platform = self.platform.clone();

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

                let foreground = foreground_pid();
                let processes = list_processes();
                let candidates: Vec<u32> = {
                    let active_map = last_active.read();
                    let purged_map = purged.read();
                    processes
                        .iter()
                        .filter(|(pid, name)| {
                            if purged_map.contains_key(pid) {
                                return false;
                            }
                            if let Some(seen) = active_map.get(pid) {
                                if now.duration_since(*seen) < threshold {
                                    return false;
                                }
                            }
                            should_trim(*pid, name, foreground, own_pid)
                        })
                        .map(|(pid, _)| *pid)
                        .collect()
                };

                for pid in candidates {
                    if platform.empty_process_working_set(pid).is_ok() {
                        purged.write().insert(pid, now);
                        info!("flushed idle working set for pid {pid}");
                    }
                }
            }
        })
    }
}

fn foreground_pid() -> u32 {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return 0;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// One snapshot of (pid, exe name). Scanning once keeps the governor light.
fn list_processes() -> Vec<(u32, String)> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut out = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if entry.th32ProcessID != 0 && !name.is_empty() {
                    out.push((entry.th32ProcessID, name));
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::should_trim;

    #[test]
    fn every_idle_app_is_trimmed_except_the_system_and_the_foreground() {
        assert!(should_trim(100, "chrome.exe", 50, 1));
        assert!(should_trim(100, "Code.exe", 50, 1));
        assert!(should_trim(100, "spotify.exe", 50, 1));
        assert!(!should_trim(50, "chrome.exe", 50, 1), "foreground stays");
        assert!(!should_trim(1, "nexus-os.exe", 50, 1), "our own process stays");
        assert!(!should_trim(8, "explorer.exe", 50, 1), "the desktop shell stays");
        assert!(!should_trim(9, "dwm.exe", 50, 1));
        assert!(!should_trim(10, "svchost.exe", 50, 1));
        assert!(!should_trim(11, "csrss.exe", 50, 1));
        assert!(!should_trim(12, "Memory Compression", 50, 1));
        assert!(!should_trim(13, "nexus-governor.exe", 50, 1));
        assert!(!should_trim(0, "chrome.exe", 50, 1));
        assert!(!should_trim(4, "System", 50, 1));
    }
}
