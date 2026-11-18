//! nexus-watchdog: Zero-overhead fail-safe shell watchdog.
//! Prevents the "Black Screen of Death" by monitoring nexus-os.exe health
//! and automatically falling back to C:\Windows\explorer.exe upon repeated crashes or config errors.

#[cfg(windows)]
mod imp {
    use os_config::schema::SystemConfig;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SYSTEMMODAL,
    };

    const MAX_CRASHES_IN_WINDOW: usize = 2;
    const CRASH_WINDOW_DURATION: Duration = Duration::from_secs(10);
    const EXPLORER_PATH: &str = r"C:\Windows\explorer.exe";

    pub fn run() {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        println!("[nexus-watchdog] Shell watchdog initialized.");

        let config_path = find_config_path();
        if let Err(err) = validate_config(&config_path) {
            show_alert(
                "Nexus-OS Config Validation Error",
                &format!(
                    "Failed to parse {:?}:\n{}\n\nFalling back to standard Windows Explorer.",
                    config_path, err
                ),
            );
            launch_explorer_fallback();
            return;
        }

        let binary_path = match find_nexus_binary() {
            Some(p) => p,
            None => {
                show_alert(
                    "Nexus-OS Missing Binary",
                    "Could not locate nexus-os.exe binary.\nFalling back to standard Windows Explorer.",
                );
                launch_explorer_fallback();
                return;
            }
        };

        println!(
            "[nexus-watchdog] Managing child lifecycle: {:?}",
            binary_path
        );

        let mut crash_timestamps: Vec<Instant> = Vec::new();

        loop {
            let start_time = Instant::now();

            let mut child = match Command::new(&binary_path).spawn() {
                Ok(c) => c,
                Err(err) => {
                    show_alert(
                        "Nexus-OS Launch Failure",
                        &format!(
                            "Failed to execute {:?}: {}\n\nFalling back to Explorer.",
                            binary_path, err
                        ),
                    );
                    launch_explorer_fallback();
                    return;
                }
            };

            let status = match child.wait() {
                Ok(s) => s,
                Err(err) => {
                    println!("[nexus-watchdog] Error waiting on child: {}", err);
                    break;
                }
            };

            if status.success() {
                println!(
                    "[nexus-watchdog] Clean exit requested by Nexus-OS. Restoring explorer.exe..."
                );
                launch_explorer_fallback();
                break;
            }

            let lifetime = start_time.elapsed();
            println!(
                "[nexus-watchdog] Child exited abnormally with {:?} after {:?}",
                status.code(),
                lifetime
            );

            let now = Instant::now();
            crash_timestamps.retain(|&t| now.duration_since(t) < CRASH_WINDOW_DURATION);
            crash_timestamps.push(now);

            if crash_timestamps.len() >= MAX_CRASHES_IN_WINDOW {
                show_alert(
                    "Nexus-OS Crash Loop Detected",
                    &format!(
                        "Nexus-OS crashed {} times within {:?}.\nFalling back to C:\\Windows\\explorer.exe to preserve system access.",
                        crash_timestamps.len(),
                        CRASH_WINDOW_DURATION
                    ),
                );
                launch_explorer_fallback();
                break;
            }

            println!("[nexus-watchdog] Restarting Nexus-OS child immediately...");
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn validate_config(path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        SystemConfig::parse(&content).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn find_config_path() -> PathBuf {
        let local = PathBuf::from("config/system.toml");
        if local.exists() {
            return local;
        }
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            let user_cfg = PathBuf::from(user_profile)
                .join(".config")
                .join("nexus-os")
                .join("system.toml");
            if user_cfg.exists() {
                return user_cfg;
            }
        }
        local
    }

    fn find_nexus_binary() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from("nexus-os.exe"),
            PathBuf::from("target/release/nexus-os.exe"),
            PathBuf::from("target/debug/nexus-os.exe"),
        ];

        candidates.into_iter().find(|c| c.exists())
    }

    fn launch_explorer_fallback() {
        println!(
            "[nexus-watchdog] Launching fallback shell: {}",
            EXPLORER_PATH
        );
        let _ = Command::new(EXPLORER_PATH).spawn();
    }

    fn show_alert(title: &str, message: &str) {
        let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            MessageBoxW(
                windows::Win32::Foundation::HWND::default(),
                PCWSTR(wide_message.as_ptr()),
                PCWSTR(wide_title.as_ptr()),
                MB_ICONERROR | MB_OK | MB_SYSTEMMODAL,
            );
        }
    }
}

#[cfg(windows)]
fn main() {
    imp::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("nexus-watchdog requires Windows NT");
}
