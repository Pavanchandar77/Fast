//! nexus-compositor: Isolated window manager, spring physics, and shell surface micro-daemon.
//! Runs at 120Hz with interruptible spring physics and hollowed shell taskbar suppression.

#[cfg(windows)]
mod imp {
    use os_compositor::hooks::WinEventListener;
    use os_compositor::hotkeys::{pump_messages, GlobalKeyboardHook, HotkeyAction};
    use os_compositor::manager::WindowManager;
    use os_compositor::{
        create_border_overlay_window, BorderOverlay, GpuContext,
    };
    use os_config::watcher::ConfigWatcher;
    use os_core::window::WindowHandle;
    use parking_lot::RwLock;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tracing::{info, warn};
    use tracing_subscriber::EnvFilter;
    use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetForegroundWindow};

    use os_core::hollow_shell::HollowShellManager;

    pub fn run() -> anyhow::Result<()> {
        os_core::recovery::install_panic_hook();

        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();

        info!("Starting nexus-compositor micro-daemon...");

        let dry_run = std::env::args().any(|arg| arg == "--dry-run");
        os_core::recovery::bootstrap(dry_run)?;
        let _restore = os_core::recovery::RestoreOnDrop::new();
        os_shield::attach(true, os_core::recovery::is_dry_run());

        let config_path = find_config_path();
        let (watcher, config_rx) = ConfigWatcher::new(&config_path)?;
        let initial_config = watcher.get();
        let compositor_config = Arc::new(RwLock::new(initial_config.compositor));
        let fail_safe = os_config::parse_or_default(&initial_config.general.fail_safe_hotkey);

        let (_keyboard, hotkey_rx) =
            match GlobalKeyboardHook::start_with_config(fail_safe, &initial_config.keybindings) {
                Ok(hook) => hook,
                Err(err) => {
                    tracing::error!(
                        "keyboard hook failed ({err}); restoring the session and aborting startup"
                    );
                    return Ok(());
                }
            };

        let wm = Arc::new(RwLock::new(WindowManager::new(compositor_config.clone())));

        let (_listener, win_event_rx) = WinEventListener::start()?;

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

        // 4. Initialize Hardware-Accelerated Shell Surface Subsystem
        info!("Initializing Single Shared GPU Context (DX12 / Vulkan)...");
        let gpu = match GpuContext::new() {
            Ok(g) => Some(g),
            Err(e) => {
                warn!("GPU rendering initialization bypassed: {:?}", e);
                None
            }
        };

        let mut border_overlay = gpu.as_ref().and_then(|g| {
            let hwnd = create_border_overlay_window().ok()?;
            let spring_cfg = WindowManager::to_spring_config(&compositor_config.read().spring);
            BorderOverlay::new(g.clone(), hwnd, spring_cfg).ok()
        });

        info!("✅ nexus-compositor active: 120Hz GPU shell & spring physics loop running.");

        let target_frame_time = Duration::from_micros(8333); // ~120 Hz
        let mut last_frame = Instant::now();

        loop {
            let frame_start = Instant::now();
            let dt = (frame_start - last_frame).as_secs_f32().min(0.05);
            last_frame = frame_start;

            if pump_messages() {
                warn!("Emergency fail-safe triggered!");
                wm.write().restore_all_on_exit();
                return Ok(());
            }

            while let Ok(event) = win_event_rx.try_recv() {
                wm.write().handle_event(event);
            }

            while let Ok(action) = hotkey_rx.try_recv() {
                match action {
                    HotkeyAction::PanicFailSafe => {
                        warn!("Emergency fail-safe triggered!");
                        wm.write().restore_all_on_exit();
                        return Ok(());
                    }
                    other => wm.write().handle_hotkey(other),
                }
            }

            while let Ok(new_cfg) = config_rx.try_recv() {
                *compositor_config.write() = new_cfg.compositor;
                os_compositor::hotkeys::GlobalKeyboardHook::update_keybindings(
                    &new_cfg.keybindings,
                );
                let active_ws = wm.read().active_workspace();
                wm.write().retile_workspace(active_ws);
            }

            wm.write().update_animations(dt);

            // Update and Render GPU Shell Surfaces (Focus border overlay)
            if let Some(bo) = &mut border_overlay {
                if let Some(rect) = wm.read().active_window_rect() {
                    bo.set_target_rect(rect);
                }
                bo.update(dt);
                let _ = bo.render();
            }

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

        unsafe extern "system" fn enum_proc(
            hwnd: HWND,
            lparam: LPARAM,
        ) -> windows::Win32::Foundation::BOOL {
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
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    imp::run()
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    eprintln!("nexus-compositor requires Windows NT");
    Ok(())
}
