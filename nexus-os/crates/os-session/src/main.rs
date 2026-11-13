use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow,
};
mod hollow_shell;

use hollow_shell::HollowShellManager;
use os_compositor::hooks::WinEventListener;
use os_compositor::hotkeys::{GlobalKeyboardHook, HotkeyAction};
use os_compositor::manager::WindowManager;
use os_config::watcher::ConfigWatcher;
use os_core::window::WindowHandle;
use os_governor::Governor;

fn main() -> anyhow::Result<()> {
    // 0. Enforce Per-Monitor DPI V2 scaling
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    println!(
        r#"
  _  _ _____  ___   _ ___        ___  ___ 
 | \| | __\ \/ / | | / __| ___  / _ \/ __|
 | .` | _| >  <| |_| \__ \/ -_)| (_) \__ \
 |_|\_|___/_/\_\\___/|___/\___| \___/|___/
 Windows Subsidiary Operating Environment
 Active Kernel Daemon: Phase 1 Online
        "#
    );

    info!("Initializing Nexus-OS Subsystem Engine...");

    // 1. Locate and watch declarative configuration
    let config_path = find_config_path();
    info!("Loading system configuration from: {:?}", config_path);

    let (watcher, config_rx) = ConfigWatcher::new(&config_path)?;
    let initial_config = watcher.get();
    let config_arc = Arc::new(RwLock::new(initial_config));

    // 2. Initialize OS-Governor (Process Priority, RAM Purge, App Nap, Timer Res)
    let governor_config = Arc::new(RwLock::new(config_arc.read().governor.clone()));
    let governor = Arc::new(Governor::new(governor_config.clone()));
    governor.start()?;

    // 3. Initialize OS-Compositor & Window Manager
    let compositor_config = Arc::new(RwLock::new(config_arc.read().compositor.clone()));
    let wm = Arc::new(RwLock::new(WindowManager::new(compositor_config.clone())));

    // 4. Start WinEvent Hooks (Window lifecycle & foreground transitions)
    info!("Attaching NT WinEvent Hook subsystem...");
    let (_win_event_listener, win_event_rx) = WinEventListener::start()?;

    // 5. Start Low-Level Keyboard Hook (Global hotkeys + Panic Fail-Safe)
    info!("Attaching Low-Level Keyboard Hook (WH_KEYBOARD_LL)...");
    let (_keyboard_hook, hotkey_rx) = GlobalKeyboardHook::start()?;

    // 6. Enumerate and ingest all existing desktop windows into the compositor
    info!("Scanning initial desktop surface hierarchy...");
    enumerate_existing_windows(wm.clone());

    // Elevate the current foreground window immediately
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            let handle = WindowHandle::new(fg);
            governor.on_foreground_changed(handle);
            wm.write().set_foreground(handle);
        }
    }

    // 7. Enforce Hollowed Shell Trick: suppress explorer taskbar surfaces if configured
    if config_arc.read().compositor.suppress_native_taskbar {
        if let Err(e) = HollowShellManager::suppress_taskbar() {
            warn!("Could not suppress native taskbar: {:?}", e);
        }
    }

    info!("✅ Phase 1 Core Daemon active. Entering 120Hz Compositor & Event Loop.");

    let target_frame_time = Duration::from_micros(8333); // ~120 Hz
    let mut last_frame = Instant::now();

    loop {
        let frame_start = Instant::now();
        let dt = (frame_start - last_frame).as_secs_f32().min(0.05);
        last_frame = frame_start;

        // Drain incoming Window Events
        while let Ok(event) = win_event_rx.try_recv() {
            match &event {
                os_compositor::hooks::WindowEvent::ForegroundChanged(hwnd) => {
                    governor.on_foreground_changed(*hwnd);
                }
                os_compositor::hooks::WindowEvent::WindowHide(hwnd)
                | os_compositor::hooks::WindowEvent::WindowCloaked(hwnd) => {
                    governor.on_window_minimized(*hwnd);
                }
                os_compositor::hooks::WindowEvent::WindowShow(hwnd)
                | os_compositor::hooks::WindowEvent::WindowUncloaked(hwnd) => {
                    governor.on_window_restored(*hwnd);
                }
                _ => {}
            }
            wm.write().handle_event(event);
        }

        // Drain incoming Hotkey Actions
        while let Ok(action) = hotkey_rx.try_recv() {
            match action {
                HotkeyAction::PanicFailSafe => {
                    warn!("🚨 EMERGENCY PANIC FAIL-SAFE TRIGGERED (Ctrl+Alt+Shift+Esc)");
                    info!("Restoring standard desktop environment and exiting gracefully...");
                    HollowShellManager::restore_taskbar();
                    return Ok(());
                }
                HotkeyAction::Terminal => {
                    info!("Hotkey: Launching terminal...");
                    let _ = std::process::Command::new("powershell.exe").spawn();
                }
                HotkeyAction::CommandBar => {
                    info!("Hotkey: Summon Command Bar / Spotlight in <5ms");
                }
                other => {
                    wm.write().handle_hotkey(other);
                }
            }
        }

        // Drain live config hot-reloads (<10ms)
        while let Ok(new_cfg) = config_rx.try_recv() {
            info!("Applying hot-reloaded configuration to Compositor and Governor...");
            *compositor_config.write() = new_cfg.compositor.clone();
            *governor_config.write() = new_cfg.governor.clone();
            *config_arc.write() = new_cfg;
            wm.write().retile_workspace(1);
        }

        // Step Spring Physics solver and reposition windows at 120Hz
        wm.write().update_animations(dt);

        let elapsed = frame_start.elapsed();
        if elapsed < target_frame_time {
            std::thread::sleep(target_frame_time - elapsed);
        }
    }
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

fn enumerate_existing_windows(
    wm: Arc<RwLock<WindowManager>>,
) {
    struct EnumContext {
        wm: Arc<RwLock<WindowManager>>,
    }

    let mut context = EnumContext { wm };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::Win32::Foundation::BOOL {
        let ctx = &mut *(lparam.0 as *mut EnumContext);
        let handle = WindowHandle::new(hwnd);
        if handle.is_manageable() {
            ctx.wm.write().register_window(handle);
        }
        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut context as *mut EnumContext as isize),
        );
    }
}
