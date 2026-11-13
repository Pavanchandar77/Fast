//! nexus-compositor: Isolated window manager, spring physics, and shell surface micro-daemon.
//! Runs at 120Hz with interruptible spring physics and hollowed shell taskbar suppression.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetForegroundWindow};
use os_config::watcher::ConfigWatcher;
use os_compositor::hooks::WinEventListener;
use os_compositor::hotkeys::{GlobalKeyboardHook, HotkeyAction};
use os_compositor::manager::WindowManager;
use os_core::window::WindowHandle;

#[path = "../hollow_shell.rs"]
mod hollow_shell;
use hollow_shell::HollowShellManager;

fn main() -> anyhow::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    info!("Starting nexus-compositor micro-daemon...");

    let config_path = find_config_path();
    let (watcher, config_rx) = ConfigWatcher::new(&config_path)?;
    let initial_config = watcher.get();
    let compositor_config = Arc::new(RwLock::new(initial_config.compositor));

    let wm = Arc::new(RwLock::new(WindowManager::new(compositor_config.clone())));

    // 1. Attach hooks
    let (_listener, win_event_rx) = WinEventListener::start()?;
    let (_keyboard, hotkey_rx) = GlobalKeyboardHook::start()?;

    // 2. Ingest existing windows
    enumerate_existing_windows(wm.clone());

    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            wm.write().set_foreground(WindowHandle::new(fg));
        }
    }

    // 3. Hollowed Shell taskbar suppression
    if compositor_config.read().suppress_native_taskbar {
        let _ = HollowShellManager::suppress_taskbar();
    }

    info!("✅ nexus-compositor active: 120Hz spring physics loop running.");

    let target_frame_time = Duration::from_micros(8333); // ~120 Hz
    let mut last_frame = Instant::now();

    loop {
        let frame_start = Instant::now();
        let dt = (frame_start - last_frame).as_secs_f32().min(0.05);
        last_frame = frame_start;

        while let Ok(event) = win_event_rx.try_recv() {
            wm.write().handle_event(event);
        }

        while let Ok(action) = hotkey_rx.try_recv() {
            match action {
                HotkeyAction::PanicFailSafe => {
                    warn!("Emergency fail-safe triggered!");
                    HollowShellManager::restore_taskbar();
                    return Ok(());
                }
                other => wm.write().handle_hotkey(other),
            }
        }

        while let Ok(new_cfg) = config_rx.try_recv() {
            *compositor_config.write() = new_cfg.compositor;
            wm.write().retile_workspace(1);
        }

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

fn enumerate_existing_windows(wm: Arc<RwLock<WindowManager>>) {
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
