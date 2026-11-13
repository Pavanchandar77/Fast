//! nexus-watchdog: Zero-overhead fail-safe shell watchdog.
//! Prevents the "Black Screen of Death" by monitoring nexus-os.exe health
//! and automatically falling back to C:\Windows\explorer.exe upon repeated crashes or config errors.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, MB_SYSTEMMODAL};
use os_config::schema::SystemConfig;

const MAX_CRASHES_IN_WINDOW: usize = 2;
const CRASH_WINDOW_DURATION: Duration = Duration::from_secs(10);
const EXPLORER_PATH: &str = r"C:\Windows\explorer.exe";

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    println!("[nexus-watchdog] Shell watchdog initialized.");

    // 1. Pre-flight check: validate configuration before launching shell
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

    let daemon_exe = match find_daemon_executable() {
        Some(path) => path,
        None => {
            show_alert(
                "Nexus-OS Binary Not Found",
                "nexus-os.exe could not be located.\nFalling back to standard Windows Explorer.",
            );
            launch_explorer_fallback();
            return;
        }
    };

    let mut crash_timestamps: Vec<Instant> = Vec::new();

    loop {
        println!("[nexus-watchdog] Spawning {:?}", daemon_exe);
        let start_time = Instant::now();

        let mut child = match Command::new(&daemon_exe).spawn() {
            Ok(c) => c,
            Err(e) => {
                show_alert(
                    "Nexus-OS Launch Failure",
                    &format!("Failed to execute {:?}: {}\nFalling back to Windows Explorer.", daemon_exe, e),
                );
                launch_explorer_fallback();
                return;
            }
        };

        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                println!("[nexus-watchdog] Error waiting on child: {}", e);
                break;
            }
        };

        let uptime = start_time.elapsed();
        println!(
            "[nexus-watchdog] nexus-os exited with status: {:?} (uptime: {:.2?})",
            status, uptime
        );

        // Clean exit (e.g. user pressed Ctrl+Alt+Shift+Esc panic key)
        if status.success() {
            println!("[nexus-watchdog] Clean exit requested. Launching Windows Explorer...");
            launch_explorer_fallback();
            return;
        }

        // Abnormal crash - record timestamp and prune window
        let now = Instant::now();
        crash_timestamps.push(now);
        crash_timestamps.retain(|&t| now.duration_since(t) <= CRASH_WINDOW_DURATION);

        if crash_timestamps.len() >= MAX_CRASHES_IN_WINDOW {
            show_alert(
                "Nexus-OS Crash Limit Reached",
                "nexus-os.exe crashed multiple times in under 10 seconds.\n\
                 Emergency fail-safe active: Restoring standard Windows Explorer shell.",
            );
            launch_explorer_fallback();
            return;
        }

        // Brief delay before restart to prevent tight loop spin
        std::thread::sleep(Duration::from_millis(500));
    }

    launch_explorer_fallback();
}

fn validate_config(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(()); // Will generate default
    }
    SystemConfig::load_from_file(path)?;
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

fn find_daemon_executable() -> Option<PathBuf> {
    if let Ok(mut current_exe) = std::env::current_exe() {
        current_exe.set_file_name("nexus-os.exe");
        if current_exe.exists() {
            return Some(current_exe);
        }
    }

    let candidates = [
        PathBuf::from("nexus-os.exe"),
        PathBuf::from("target/release/nexus-os.exe"),
        PathBuf::from("target/debug/nexus-os.exe"),
    ];

    for c in candidates {
        if c.exists() {
            return Some(c);
        }
    }

    None
}

fn launch_explorer_fallback() {
    println!("[nexus-watchdog] Launching fallback shell: {}", EXPLORER_PATH);
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
