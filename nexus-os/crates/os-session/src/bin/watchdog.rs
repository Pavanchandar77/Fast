//! nexus-watchdog: fail-safe shell watchdog.
//!
//! It watches nexus-os.exe with a named heartbeat event and a RegisterHotKey
//! chord. Either a hung main thread or the fail-safe hotkey kills the shell,
//! reverts the journal, and starts Explorer. The hotkey is delivered on this
//! thread, so it still works when the shell is not pumping messages.

#[cfg(windows)]
mod imp {
    use os_config::{parse_or_default, HotkeyChord};
    use os_core::heartbeat::{Heartbeat, STALE_AFTER};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitStatus};
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HWND, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
        MOD_SHIFT, MOD_WIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MessageBoxW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage,
        MB_ICONERROR, MB_OK, MB_SYSTEMMODAL, MSG, PM_REMOVE, QS_ALLINPUT, WM_HOTKEY,
    };

    const MAX_CRASHES_IN_WINDOW: usize = 2;
    const CRASH_WINDOW_DURATION: Duration = Duration::from_secs(10);
    const EXPLORER_PATH: &str = r"C:\Windows\explorer.exe";
    const HOTKEY_ID: i32 = 0x4E58;

    pub fn run() {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        println!("[nexus-watchdog] Shell watchdog initialized.");
        let _ = drain_failsafe();

        let dry_run = std::env::args().any(|arg| arg == "--dry-run");
        let config_path = find_config_path();
        let chord = match load_chord(&config_path) {
            Ok(chord) => chord,
            Err(err) => {
                show_alert(
                    "Nexus-OS Config Validation Error",
                    &format!(
                        "Failed to parse {:?}:\n{}\n\nFalling back to standard Windows Explorer.",
                        config_path, err
                    ),
                );
                restore_and_explorer();
                return;
            }
        };
        if let Err(err) = register_failsafe(chord) {
            println!("[nexus-watchdog] RegisterHotKey failed: {err}");
        }

        let daemon_exe = match find_daemon_executable() {
            Some(path) => path,
            None => {
                show_alert(
                "Nexus-OS Binary Not Found",
                "nexus-os.exe could not be located.\nFalling back to standard Windows Explorer.",
            );
                restore_and_explorer();
                unregister_failsafe();
                return;
            }
        };

        let mut crash_timestamps: Vec<Instant> = Vec::new();
        loop {
            println!("[nexus-watchdog] Spawning {:?}", daemon_exe);
            let mut command = Command::new(&daemon_exe);
            if dry_run {
                command.arg("--dry-run");
            }
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(err) => {
                    show_alert(
                        "Nexus-OS Launch Failure",
                        &format!(
                            "Failed to execute {:?}: {}\nFalling back to Windows Explorer.",
                            daemon_exe, err
                        ),
                    );
                    restore_and_explorer();
                    unregister_failsafe();
                    return;
                }
            };

            let process = open_process(child.id());
            let end = watch_child(&mut child, process);
            if let Some(process) = process {
                if !process.is_invalid() {
                    let _ = unsafe { CloseHandle(process) };
                }
            }

            match end {
                WatchEnd::Exited(status) => {
                    let uptime_note = format!("{status:?}");
                    println!("[nexus-watchdog] nexus-os exited ({uptime_note})");
                    if let Err(err) = os_core::recovery::restore_journal_file() {
                        println!("[nexus-watchdog] journal restore failed: {err}");
                    }
                    if status.success() {
                        println!(
                            "[nexus-watchdog] Clean exit requested. Launching Windows Explorer..."
                        );
                        launch_explorer_fallback();
                        unregister_failsafe();
                        return;
                    }
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
                        unregister_failsafe();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                WatchEnd::FailSafe => {
                    println!("[nexus-watchdog] Fail-safe hotkey. Killing the shell and restoring.");
                    terminate(&mut child);
                    restore_and_explorer();
                    unregister_failsafe();
                    return;
                }
                WatchEnd::Hang => {
                    println!(
                        "[nexus-watchdog] Heartbeat stopped. Killing the shell and restoring."
                    );
                    terminate(&mut child);
                    restore_and_explorer();
                    unregister_failsafe();
                    return;
                }
            }
        }
    }

    enum WatchEnd {
        Exited(ExitStatus),
        FailSafe,
        Hang,
    }

    struct BeatWatch {
        started: Instant,
        last_beat: Instant,
        saw_beat: bool,
        beat: Option<Heartbeat>,
    }

    impl BeatWatch {
        fn new() -> Self {
            let now = Instant::now();
            Self {
                started: now,
                last_beat: now,
                saw_beat: false,
                beat: None,
            }
        }

        fn poll_open(&mut self) {
            if self.beat.is_none() {
                if let Ok(beat) = Heartbeat::open_existing() {
                    self.beat = Some(beat);
                }
            }
        }

        fn on_pulse(&mut self) {
            if let Some(beat) = &self.beat {
                let _ = beat.reset();
            }
            self.last_beat = Instant::now();
            self.saw_beat = true;
        }

        fn is_stale(&self) -> bool {
            let elapsed = if self.saw_beat {
                self.last_beat.elapsed()
            } else {
                self.started.elapsed()
            };
            elapsed >= STALE_AFTER
        }
    }

    fn watch_child(child: &mut Child, process: Option<HANDLE>) -> WatchEnd {
        let Some(process) = process else {
            return watch_polled(child);
        };
        if process.is_invalid() {
            return watch_polled(child);
        }
        let mut beats = BeatWatch::new();
        loop {
            beats.poll_open();
            let mut handles = vec![process];
            if let Some(beat) = &beats.beat {
                handles.push(beat.raw());
            }
            let wait =
                unsafe { MsgWaitForMultipleObjects(Some(&handles), false, 1_000, QS_ALLINPUT) };
            let code = wait.0;
            if code == WAIT_TIMEOUT.0 {
                if beats.is_stale() {
                    return WatchEnd::Hang;
                }
                continue;
            }
            if code == WAIT_FAILED.0 {
                if let Ok(Some(status)) = child.try_wait() {
                    return WatchEnd::Exited(status);
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            if code == WAIT_OBJECT_0.0 {
                return match child.wait() {
                    Ok(status) => WatchEnd::Exited(status),
                    Err(_) => watch_polled(child),
                };
            }
            if beats.beat.is_some() && code == WAIT_OBJECT_0.0 + 1 {
                beats.on_pulse();
                continue;
            }
            if code == WAIT_OBJECT_0.0 + handles.len() as u32 && drain_failsafe() {
                return WatchEnd::FailSafe;
            }
            if beats.is_stale() {
                return WatchEnd::Hang;
            }
        }
    }

    fn watch_polled(child: &mut Child) -> WatchEnd {
        let mut beats = BeatWatch::new();
        loop {
            if drain_failsafe() {
                return WatchEnd::FailSafe;
            }
            beats.poll_open();
            if let Some(beat) = &beats.beat {
                // A manual-reset event that is already signaled is observed by a zero wait.
                let signaled = unsafe {
                    MsgWaitForMultipleObjects(Some(&[beat.raw()]), false, 0, QS_ALLINPUT).0
                        == WAIT_OBJECT_0.0
                };
                if signaled {
                    beats.on_pulse();
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => return WatchEnd::Exited(status),
                Ok(None) if beats.is_stale() => return WatchEnd::Hang,
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(_) => return WatchEnd::Hang,
            }
        }
    }

    fn terminate(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    fn open_process(pid: u32) -> Option<HANDLE> {
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
            .ok()
        }
    }

    fn drain_failsafe() -> bool {
        let mut saw = false;
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_HOTKEY {
                    saw = true;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        saw
    }

    fn register_failsafe(chord: HotkeyChord) -> windows::core::Result<()> {
        unsafe {
            RegisterHotKey(
                HWND::default(),
                HOTKEY_ID,
                modifiers(chord),
                u32::from(chord.vk),
            )
        }
    }

    fn unregister_failsafe() {
        let _ = unsafe { UnregisterHotKey(HWND::default(), HOTKEY_ID) };
    }

    fn modifiers(chord: HotkeyChord) -> HOT_KEY_MODIFIERS {
        let mut mods = MOD_NOREPEAT;
        if chord.ctrl {
            mods |= MOD_CONTROL;
        }
        if chord.alt {
            mods |= MOD_ALT;
        }
        if chord.shift {
            mods |= MOD_SHIFT;
        }
        if chord.win {
            mods |= MOD_WIN;
        }
        mods
    }

    fn load_chord(path: &Path) -> anyhow::Result<HotkeyChord> {
        if !path.exists() {
            return Ok(HotkeyChord::default_fail_safe());
        }
        let config = os_config::schema::SystemConfig::load_from_file(path)?;
        Ok(parse_or_default(&config.general.fail_safe_hotkey))
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

        candidates.into_iter().find(|candidate| candidate.exists())
    }

    fn restore_and_explorer() {
        if let Err(err) = os_core::recovery::restore_journal_file() {
            println!("[nexus-watchdog] journal restore failed: {err}");
        }
        launch_explorer_fallback();
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
                HWND::default(),
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
