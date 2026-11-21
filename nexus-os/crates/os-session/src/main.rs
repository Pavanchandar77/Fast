// Release builds are a normal GUI app: no console window behind Nexus.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod imp {
    use os_compositor::hooks::WinEventListener;
    use os_compositor::hotkeys::{pump_messages, GlobalKeyboardHook, HotkeyAction};
    use os_compositor::manager::WindowManager;
    use os_compositor::{create_border_overlay_window, BorderOverlay, GpuContext};
    use os_config::watcher::ConfigWatcher;
    use os_core::hollow_shell::HollowShellManager;
    use os_core::window::WindowHandle;
    use os_governor::Governor;
    use parking_lot::RwLock;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tracing::{info, warn};
    use tracing_subscriber::EnvFilter;
    use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetForegroundWindow};

    enum SessionUiEvent {
        FailSafeQuit,
        ToggleSpotlight,
        ToggleAssistant,
        Motion(os_session::motion_bridge::UiMotion),
    }

    #[allow(dead_code)]
    struct LiveSession(os_session::SessionHandle);

    impl gpui::Global for LiveSession {}

    pub fn run() -> anyhow::Result<()> {
        // `nexus-os.exe --restore`: put back anything a killed session left
        // changed (e.g. the hidden taskbar) and exit. Used by the uninstaller
        // and the "Restore Windows desktop" Start Menu shortcut.
        if std::env::args().any(|arg| arg == "--restore") {
            os_core::recovery::restore_journal_file()?;
            return Ok(());
        }

        // One session per user: a second launch (shortcut + sign-in entry)
        // must not attach a second set of hooks.
        let _single_instance = match acquire_single_instance() {
            Some(guard) => guard,
            None => return Ok(()),
        };

        // Restore journaled mutations on panic before anything else runs.
        os_core::recovery::install_panic_hook();

        // 0b. Enforce Per-Monitor DPI V2 scaling
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

        let dry_run = std::env::args().any(|arg| arg == "--dry-run");
        os_core::recovery::bootstrap(dry_run)?;
        // The binding must stay live for the whole process. `let _ =` drops it now.
        let _restore = os_core::recovery::RestoreOnDrop::new();
        os_core::heartbeat::start()?;
        os_shield::attach(true, os_core::recovery::is_dry_run());

        // 0c. Windows Update Resilience & Capability Self-Check
        let journal_path = os_core::recovery::default_journal_path();
        let state_dir = journal_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let store = nx_compat::BuildHistoryStore::new(state_dir);
        let notices = Arc::new(nx_compat::NoticeQueue::new());
        let engine = nx_compat::SelfHealingEngine::new(store, notices.clone());
        let win_platform = os_core::platform::Win32Platform::new();
        // The live journal at `journal_path` is already open and exclusively
        // locked by `recovery::bootstrap`; opening it again here blocked
        // forever on the lock and the UI never appeared. Every mutation the
        // self-check applies goes through `Win32Platform`, which already
        // commits it to the live journal, so the self-check only needs a
        // scratch journal for its own rollback bookkeeping.
        let scratch_path = journal_path.with_file_name("selfcheck.nxjr");
        if let Ok(mut journal) = nexus_journal::Journal::open(&scratch_path) {
            let build = nx_compat::detect_build();
            info!(
                "Nexus-OS Compat: Windows build detected: {}",
                build.full_version_string()
            );
            match engine.run_startup_self_check(
                &build,
                &win_platform,
                &mut journal,
                &nx_compat::LiveProbeProvider,
                true,
            ) {
                Ok(report) => {
                    info!(
                        "Resilience self-check finished: clean={}, healed={:?}, regressions={:?}",
                        report.is_clean(),
                        report.settings_healed,
                        report.regressions
                    );
                }
                Err(e) => {
                    warn!("Resilience startup self-check non-fatal error: {e}");
                }
            }
            drop(journal);
            let _ = std::fs::remove_file(&scratch_path);
            let mut lock = scratch_path.into_os_string();
            lock.push(".lock");
            let _ = std::fs::remove_file(lock);
        }

        // 0d. Mid-session Explorer.exe and DWM restart watcher
        let restart_handler = Arc::new(nx_compat::RestartHandler::new(
            nx_compat::RestartConfig::default(),
            notices,
        ));
        let taskbar_created_msg = restart_handler.register_taskbar_created_message();
        let rh_clone = restart_handler.clone();
        os_compositor::hotkeys::set_message_callback(move |msg_id| {
            if msg_id == taskbar_created_msg {
                let p = os_core::platform::Win32Platform::new();
                let _ = rh_clone.handle_explorer_restart(&p);
            } else if rh_clone.is_dwm_change_message(msg_id) {
                let _ = rh_clone.handle_dwm_restart(&nx_compat::LiveProbeProvider);
            }
        });

        // 1. Locate and watch declarative configuration
        let config_path = find_config_path();
        info!("Loading system configuration from: {:?}", config_path);

        let (watcher, config_rx) = ConfigWatcher::new(&config_path)?;
        let initial_config = watcher.get();
        let config_arc = Arc::new(RwLock::new(initial_config));
        let fail_safe = os_config::parse_or_default(&config_arc.read().general.fail_safe_hotkey);

        // The keyboard hook is installed before any system mutation. If it cannot
        // be installed, startup stops and the drop guard restores the journal.
        info!("Attaching Low-Level Keyboard Hook (WH_KEYBOARD_LL)...");
        let (keyboard_hook, hotkey_rx) = match GlobalKeyboardHook::start_with_config(
            fail_safe,
            &config_arc.read().keybindings,
        ) {
            Ok(hook) => hook,
            Err(err) => {
                tracing::error!(
                    "keyboard hook failed ({err}); restoring the session and aborting startup"
                );
                return Ok(());
            }
        };

        // 2. Initialize OS-Governor (Process Priority, RAM Purge, App Nap, Timer Res)
        let governor_config = Arc::new(RwLock::new(config_arc.read().governor.clone()));
        let governor = Arc::new(Governor::new(governor_config.clone()));
        governor.start()?;

        // 3. Initialize OS-Compositor & Window Manager
        let compositor_config = Arc::new(RwLock::new(config_arc.read().compositor.clone()));
        let wm = Arc::new(RwLock::new(WindowManager::new(compositor_config.clone())));

        // 4. Start WinEvent Hooks (Window lifecycle & foreground transitions)
        info!("Attaching NT WinEvent Hook subsystem...");
        let (win_event_listener, win_event_rx) = WinEventListener::start()?;

        // 6. Enumerate and ingest all existing desktop windows into the compositor
        info!("Scanning initial desktop surface hierarchy...");
        let existing = enumerate_existing_windows(wm.clone());

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

        let motion_cfg = config_arc.read().compositor.motion;
        let mut motion =
            os_session::motion_bridge::MotionBridge::new(motion_cfg.animate_foreign_windows);
        for handle in existing {
            motion.adopt(handle);
        }

        let compositor = CompositorLoop {
            motion,
            _keyboard_hook: keyboard_hook,
            hotkey_rx,
            _win_event_listener: win_event_listener,
            win_event_rx,
            governor,
            wm,
            _watcher: watcher,
            config_rx,
            compositor_config,
            governor_config,
            config_arc,
        };

        // 8b. Real GPUI windows: menu bar, dock, Spotlight, Mission Control,
        // explorer, and privacy dashboard. The compositor/governor/hotkey loop
        // continues on a background thread at 120Hz.
        info!("Starting GPUI application...");
        gpui::Application::new().run(move |cx| {
            let mut session = os_session::startup::start_session();
            info!("session surfaces: {}", session.summary());
            let windows = os_session::startup::open_gpui_windows(cx, &mut session);
            // Keep component state alive for the GPUI application lifetime.
            cx.set_global(LiveSession(session));

            let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<SessionUiEvent>();
            let running = Arc::new(AtomicBool::new(true));
            let running_for_quit = running.clone();
            cx.on_app_quit(move |_| {
                running_for_quit.store(false, Ordering::SeqCst);
                async {}
            })
            .detach();

            let spotlight = windows.spotlight_window;
            let island = windows.island_window;
            let motion_windows = windows.clone();
            let peek_flag = std::rc::Rc::new(std::cell::Cell::new(None::<bool>));
            {
                let peek_flag = peek_flag.clone();
                let prefs =
                    os_session::startup::motion_prefs(motion_cfg.reduce_motion, motion_cfg.speed);
                os_session::startup::with_motion(&windows, cx, move |o| {
                    o.prefs = prefs;
                    o.on_event(move |ev, _cx| {
                        let nx_shell::OverlayEvent::Peek(on) = ev;
                        peek_flag.set(Some(on));
                    });
                });
            }
            if let Err(err) = std::thread::Builder::new()
                .name("nexus-compositor-loop".into())
                .spawn(move || compositor.run(ui_tx, running))
            {
                warn!("failed to spawn compositor loop thread: {err}");
            }

            let mut motion_state = os_session::motion_bridge::UiMotionState::default();
            cx.spawn(async move |cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let mut quit = false;
                while let Ok(event) = ui_rx.try_recv() {
                    match event {
                        SessionUiEvent::FailSafeQuit => {
                            let _ = cx.update(|cx| cx.quit());
                            quit = true;
                        }
                        SessionUiEvent::Motion(msg) => {
                            let _ = cx.update(|cx| {
                                os_session::motion_bridge::apply(
                                    msg,
                                    &mut motion_state,
                                    &motion_windows,
                                    cx,
                                );
                            });
                        }
                        SessionUiEvent::ToggleSpotlight => {
                            let _ = cx.update(|cx| {
                                if let Some(handle) = spotlight {
                                    os_session::startup::toggle_gpui_spotlight(handle, cx);
                                }
                            });
                        }
                        SessionUiEvent::ToggleAssistant => {
                            let _ = cx.update(|cx| {
                                if let Some(handle) = island {
                                    os_session::startup::toggle_island_assistant(handle, cx);
                                }
                            });
                        }
                    }
                }
                if quit {
                    break;
                }
                if let Some(on) = peek_flag.take() {
                    motion_state.set_peek(on);
                }
                let _ = cx.update(|cx| motion_state.tick(&motion_windows, cx));
            })
            .detach();
        });

        Ok(())
    }

    struct CompositorLoop {
        motion: os_session::motion_bridge::MotionBridge,
        _keyboard_hook: GlobalKeyboardHook,
        hotkey_rx: crossbeam_channel::Receiver<HotkeyAction>,
        _win_event_listener: WinEventListener,
        win_event_rx: crossbeam_channel::Receiver<os_compositor::hooks::WindowEvent>,
        governor: Arc<Governor>,
        wm: Arc<RwLock<WindowManager>>,
        _watcher: ConfigWatcher,
        config_rx: crossbeam_channel::Receiver<os_config::schema::SystemConfig>,
        compositor_config: Arc<RwLock<os_config::schema::CompositorConfig>>,
        governor_config: Arc<RwLock<os_config::schema::GovernorConfig>>,
        config_arc: Arc<RwLock<os_config::schema::SystemConfig>>,
    }

    impl CompositorLoop {
        fn run(
            mut self,
            ui_tx: crossbeam_channel::Sender<SessionUiEvent>,
            running: Arc<AtomicBool>,
        ) {
            // Create the focus-border overlay on this thread so the HWND has
            // the compositor loop's thread affinity.
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
                let spring_cfg =
                    WindowManager::to_spring_config(&self.config_arc.read().compositor.spring);
                BorderOverlay::new(g.clone(), hwnd, spring_cfg).ok()
            });

            info!("✨ Nexus-OS Active: Entering 120Hz Compositor Event Loop");
            let mut last_frame = Instant::now();
            let target_frame_time = Duration::from_micros(8333); // 120Hz

            while running.load(Ordering::SeqCst) {
                let now = Instant::now();
                let dt = (now - last_frame).as_secs_f32();
                last_frame = now;
                let frame_start = Instant::now();

                os_core::heartbeat::pulse();
                if pump_messages() {
                    self.fail_safe_restore_and_quit(&ui_tx);
                    return;
                }

                // Drain incoming WinEvents (Window creation, destruction, focus changes)
                while let Ok(event) = self.win_event_rx.try_recv() {
                    match &event {
                        os_compositor::hooks::WindowEvent::ForegroundChanged(hwnd) => {
                            self.governor.on_foreground_changed(*hwnd);
                        }
                        os_compositor::hooks::WindowEvent::WindowHide(hwnd)
                        | os_compositor::hooks::WindowEvent::WindowCloaked(hwnd) => {
                            self.governor.on_window_minimized(*hwnd);
                        }
                        os_compositor::hooks::WindowEvent::WindowShow(hwnd)
                        | os_compositor::hooks::WindowEvent::WindowUncloaked(hwnd) => {
                            self.governor.on_window_restored(*hwnd);
                        }
                        os_compositor::hooks::WindowEvent::WindowDestroyed(hwnd) => {
                            self.governor.on_process_exit(hwnd.pid());
                        }
                        _ => {}
                    }
                    let floating = self.compositor_config.read().layout == "floating";
                    for msg in self.motion.on_window_event(&event, floating) {
                        let _ = ui_tx.send(SessionUiEvent::Motion(msg));
                    }
                    self.wm.write().handle_event(event);
                }

                // Drain incoming Hotkey Actions
                while let Ok(action) = self.hotkey_rx.try_recv() {
                    match action {
                        HotkeyAction::PanicFailSafe => {
                            self.fail_safe_restore_and_quit(&ui_tx);
                            return;
                        }
                        HotkeyAction::Terminal => {
                            info!("Hotkey: Launching terminal...");
                            let _ = std::process::Command::new("powershell.exe").spawn();
                        }
                        HotkeyAction::Custom(ref name) if name == "motion:switcher" => {
                            let _ = ui_tx.send(SessionUiEvent::Motion(self.motion.switcher()));
                        }
                        HotkeyAction::Custom(ref name) if name == "motion:mission_control" => {
                            let _ =
                                ui_tx.send(SessionUiEvent::Motion(self.motion.mission_control()));
                        }
                        HotkeyAction::CommandBar => {
                            info!("Hotkey: Toggle GPUI SpotlightView");
                            let _ = ui_tx.send(SessionUiEvent::ToggleSpotlight);
                        }
                        HotkeyAction::Custom(ref name) if name == "shell:assistant" => {
                            info!("Hotkey: Toggle island assistant");
                            let _ = ui_tx.send(SessionUiEvent::ToggleAssistant);
                        }
                        other => {
                            self.wm.write().handle_hotkey(other);
                        }
                    }
                }

                // Drain live config hot-reloads (<10ms)
                while let Ok(new_cfg) = self.config_rx.try_recv() {
                    info!("Applying hot-reloaded configuration to Compositor and Governor...");
                    *self.compositor_config.write() = new_cfg.compositor.clone();
                    *self.governor_config.write() = new_cfg.governor.clone();
                    os_compositor::hotkeys::GlobalKeyboardHook::update_keybindings(
                        &new_cfg.keybindings,
                    );
                    let active_ws = self.wm.read().active_workspace();
                    *self.config_arc.write() = new_cfg;
                    self.wm.write().retile_workspace(active_ws);
                }

                for msg in self.motion.tick() {
                    let _ = ui_tx.send(SessionUiEvent::Motion(msg));
                }

                // Step Spring Physics solver and reposition windows at 120Hz
                self.wm.write().update_animations(dt);

                // Update and Render GPU Shell Surfaces (focus border overlay only)
                if let Some(bo) = &mut border_overlay {
                    if let Some(rect) = self.wm.read().active_window_rect() {
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

        fn fail_safe_restore_and_quit(&self, ui_tx: &crossbeam_channel::Sender<SessionUiEvent>) {
            warn!("Emergency fail-safe triggered");
            info!("Restoring the journaled session and exiting");
            self.wm.write().restore_all_on_exit();
            let _ = ui_tx.send(SessionUiEvent::FailSafeQuit);
        }
    }

    /// Holds the per-user session mutex for the life of the process.
    struct SingleInstance(windows::Win32::Foundation::HANDLE);

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }

    fn acquire_single_instance() -> Option<SingleInstance> {
        use windows::core::w;
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;
        unsafe {
            // If the mutex cannot be created at all, run anyway rather than
            // refusing to start.
            let Ok(handle) = CreateMutexW(None, true, w!("Local\\NexusOS.Session")) else {
                return Some(SingleInstance(windows::Win32::Foundation::HANDLE::default()));
            };
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return None;
            }
            Some(SingleInstance(handle))
        }
    }

    fn find_config_path() -> PathBuf {
        let local = PathBuf::from("config/system.toml");
        if local.exists() {
            return local;
        }

        // Installed builds ship `config\system.toml` next to nexus-os.exe; the
        // sign-in Run key starts us with an unrelated working directory.
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("config").join("system.toml")));

        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            let user_cfg = PathBuf::from(user_profile)
                .join(".config")
                .join("nexus-os")
                .join("system.toml");
            if user_cfg.exists() {
                return user_cfg;
            }
        }

        if let Some(installed) = beside_exe {
            if installed.exists() {
                return installed;
            }
        }

        local
    }

    fn enumerate_existing_windows(wm: Arc<RwLock<WindowManager>>) -> Vec<WindowHandle> {
        struct EnumContext {
            wm: Arc<RwLock<WindowManager>>,
            found: Vec<WindowHandle>,
        }

        let mut context = EnumContext {
            wm,
            found: Vec::new(),
        };

        unsafe extern "system" fn enum_proc(
            hwnd: HWND,
            lparam: LPARAM,
        ) -> windows::Win32::Foundation::BOOL {
            let ctx = &mut *(lparam.0 as *mut EnumContext);
            let handle = WindowHandle::new(hwnd);
            if handle.is_manageable() {
                ctx.wm.write().register_window(handle);
                ctx.found.push(handle);
            }
            TRUE
        }

        unsafe {
            let _ = EnumWindows(
                Some(enum_proc),
                LPARAM(&mut context as *mut EnumContext as isize),
            );
        }
        context.found
    }
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    imp::run()
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    eprintln!("Nexus-OS binary is supported on Windows NT only.");
    Ok(())
}
