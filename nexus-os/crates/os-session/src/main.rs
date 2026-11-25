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
    use os_session::diag;
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
        SpotlightWith(&'static str),
        ToggleAssistant,
        Motion(os_session::motion_bridge::UiMotion),
    }

    #[allow(dead_code)]
    struct LiveSession(os_session::SessionHandle);

    impl gpui::Global for LiveSession {}

    pub fn run() -> anyhow::Result<()> {
        let process_t0 = Instant::now();
        // First: the log file, panic/crash reporting and (for --diag) a
        // console, so nothing after this can end the process silently.
        // Release builds have no console: everything goes to
        // %LOCALAPPDATA%\Nexus\logs\nexus.log so a real-PC run leaves data.
        let selftest = os_session::selftest::requested(std::env::args());
        let diag_mode = diag::parse_diag_mode(std::env::args()) || selftest;
        diag::set_diag_mode(diag_mode);
        if diag_mode {
            diag::attach_console();
            if std::env::var_os("RUST_LOG").is_none() {
                std::env::set_var("RUST_LOG", "debug,wgpu_core=warn,wgpu_hal=warn,naga=warn");
            }
            std::env::set_var("NEXUS_STARTUP_TRACE", "1");
        }
        let log_path = diag::open_log_file();
        // Restore journaled mutations on panic, then record the panic in the
        // log and (when it ends the process) show the reason.
        os_core::recovery::install_panic_hook();
        diag::install_panic_logging();
        diag::install_crash_filter();
        let safe = diag::parse_safe_mode(
            std::env::args(),
            std::env::var("NEXUS_SAFE").ok().as_deref(),
        );
        diag::set_safe_mode(safe);
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_ansi(false)
            .with_thread_names(true)
            .with_writer(|| diag::LogWriter)
            .init();
        info!(
            "==== Nexus {} starting (pid {}, safe_mode={safe}, diag={diag_mode}, log={:?}) ====",
            diag::release(),
            std::process::id(),
            log_path
        );
        info!("args: {:?}", std::env::args().collect::<Vec<_>>());

        // `nexus-os.exe --restore`: put back anything a killed session left
        // changed (e.g. the hidden taskbar) and exit. Used by the uninstaller
        // and the "Restore Windows desktop" Start Menu shortcut.
        if std::env::args().any(|arg| arg == "--restore") {
            os_core::recovery::restore_journal_file()?;
            info!("--restore finished");
            return Ok(());
        }

        // One session per user: a second launch (shortcut + sign-in entry)
        // must not attach a second set of hooks.
        let _single_instance = match acquire_single_instance() {
            Some(guard) => guard,
            None => {
                warn!("another Nexus session already holds Local\\NexusOS.Session; exiting");
                diag::show_message(
                    "Nexus",
                    &format!(
                        "Nexus is already running in this Windows session, so this launch closed.\n\n\
                         If you can't see it, end nexus-os.exe in Task Manager (or run Start menu > Nexus > \
                         Restore Windows desktop) and start Nexus again.\n\nLog: {}",
                        diag::log_path().display()
                    ),
                    false,
                );
                return Ok(());
            }
        };

        // 0b. Enforce Per-Monitor DPI V2 scaling
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

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

        // 0c. Windows Update resilience runs off the UI thread. Probing and
        // re-applying policies took most of a minute and the window never
        // appeared until it finished.
        let journal_path = os_core::recovery::default_journal_path();
        let state_dir = journal_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        let notices = Arc::new(nx_compat::NoticeQueue::new());
        let notices_for_check = notices.clone();
        // The live journal is already locked by recovery::bootstrap. The
        // self-check writes through Win32Platform and only keeps a scratch
        // journal for its own rollback bookkeeping.
        let scratch_path = journal_path.with_file_name("selfcheck.nxjr");
        let _ = std::thread::Builder::new()
            .name("nexus-selfcheck".into())
            .spawn(move || {
                let store = nx_compat::BuildHistoryStore::new(&state_dir);
                let engine = nx_compat::SelfHealingEngine::new(store, notices_for_check);
                let win_platform = os_core::platform::Win32Platform::new();
                let Ok(mut journal) = nexus_journal::Journal::open(&scratch_path) else {
                    return;
                };
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
            });

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
        let (keyboard_hook, hotkey_rx) = if safe {
            warn!("safe mode: global keyboard hook not installed");
            (None, crossbeam_channel::never())
        } else {
            info!("Attaching Low-Level Keyboard Hook (WH_KEYBOARD_LL)...");
            match GlobalKeyboardHook::start_with_config(fail_safe, &config_arc.read().keybindings) {
                Ok((hook, rx)) => {
                    info!("keyboard hook installed");
                    (Some(hook), rx)
                }
                Err(err) => {
                    tracing::error!(
                        "keyboard hook failed ({err}); restoring the session and aborting startup"
                    );
                    diag::fatal(
                        "startup",
                        &format!("the global keyboard hook could not be installed ({err})"),
                    );
                    return Ok(());
                }
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
        let existing = if safe {
            Vec::new()
        } else {
            enumerate_existing_windows(wm.clone())
        };
        info!("adopted {} existing top-level windows", existing.len());

        // Elevate the current foreground window immediately
        unsafe {
            let fg = GetForegroundWindow();
            if !fg.0.is_null() {
                let handle = WindowHandle::new(fg);
                governor.on_foreground_changed(handle);
                if !safe {
                    wm.write().set_foreground(handle);
                }
            }
        }

        // 7. Enforce Hollowed Shell Trick: suppress explorer taskbar surfaces if configured
        if safe {
            warn!("safe mode: native taskbar left visible");
        } else if config_arc.read().compositor.suppress_native_taskbar {
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
            safe,
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
            let mut session = diag::timed("start_session", 500, os_session::startup::start_session);
            info!("session surfaces: {}", session.summary());
            let windows = diag::timed("open_gpui_windows", 400, || {
                os_session::startup::open_gpui_windows(cx, &mut session)
            });
            info!("GPUI windows: {}", windows.describe());
            if selftest {
                let ok = os_session::selftest::run(&windows, cx);
                info!("self-test finished ok={ok}");
                cx.quit();
                return;
            }
            if windows.none_open() {
                // Nothing on screen would be a silent "vanish": say so and quit.
                diag::fatal(
                    "startup",
                    "none of the Nexus windows could be created (see the 'failed to open' lines in the log)",
                );
                cx.quit();
                return;
            }
            info!(
                "startup: shell windows ready {} ms after process start",
                process_t0.elapsed().as_millis()
            );
            // Keep component state alive for the GPUI application lifetime.
            cx.set_global(LiveSession(session));

            let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<SessionUiEvent>();
            let running = Arc::new(AtomicBool::new(true));
            let running_for_quit = running.clone();
            cx.on_app_quit(move |_| {
                info!("GPUI quitting; lifting any window hides");
                nx_shell::capture::lift_all_hides();
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
            let mut ticks: u64 = 0;
            let mut last_tick = Instant::now();
            // 120 Hz right after shell events (hotkeys, motion); 50 Hz when
            // the session is quiet.
            let mut last_event = Instant::now();
            let mut ui_delay = Duration::from_millis(8);
            cx.spawn(async move |cx| loop {
                cx.background_executor().timer(ui_delay).await;
                let mut quit = false;
                while let Ok(event) = ui_rx.try_recv() {
                    last_event = Instant::now();
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
                        SessionUiEvent::SpotlightWith(query) => {
                            let _ = cx.update(|cx| {
                                if let Some(handle) = spotlight {
                                    os_session::startup::open_gpui_spotlight_with(
                                        handle, query, cx,
                                    );
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
                // UI-thread starvation detector: this loop should run every
                // ~8 ms. A long gap means something blocked the UI thread.
                let gap = last_tick.elapsed();
                last_tick = Instant::now();
                ui_delay = if last_event.elapsed() < Duration::from_millis(500) {
                    Duration::from_millis(8)
                } else {
                    Duration::from_millis(20)
                };
                if gap > Duration::from_millis(250) {
                    warn!("UI thread stalled for {} ms", gap.as_millis());
                }
                ticks = ticks.wrapping_add(1);
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
        _keyboard_hook: Option<GlobalKeyboardHook>,
        safe: bool,
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
            let safe = self.safe;
            let mut border_overlay = gpu.as_ref().filter(|_| !safe).and_then(|g| {
                let hwnd = create_border_overlay_window().ok()?;
                let spring_cfg =
                    WindowManager::to_spring_config(&self.config_arc.read().compositor.spring);
                BorderOverlay::new(g.clone(), hwnd, spring_cfg).ok()
            });

            info!("✨ Nexus-OS Active: Entering 120Hz Compositor Event Loop");
            let mut last_frame = Instant::now();
            let target_frame_time = Duration::from_micros(8333); // 120Hz
            let mut hot = nx_shell::hot_corners::HotCorners::default();
            let hot_epoch = Instant::now();

            while running.load(Ordering::SeqCst) {
                let now = Instant::now();
                let dt = (now - last_frame).as_secs_f32();
                last_frame = now;
                let frame_start = Instant::now();

                os_core::heartbeat::pulse();
                nx_shell::capture::bloom_watchdog();
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
                    if self.safe {
                        continue;
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
                        HotkeyAction::Custom(ref name) if name == "shell:clipboard" => {
                            info!("Hotkey: clipboard history");
                            let _ = ui_tx.send(SessionUiEvent::SpotlightWith("cb "));
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

                // Hot corners: top-left Mission Control, top-right Spotlight,
                // bottom-right Show Desktop.
                if !self.safe {
                    let now_ms = hot_epoch.elapsed().as_millis() as u64;
                    match nx_shell::hot_corners::poll(&mut hot, now_ms) {
                        Some(nx_shell::hot_corners::Corner::TopLeft) => {
                            info!("Hot corner: Mission Control");
                            let _ =
                                ui_tx.send(SessionUiEvent::Motion(self.motion.mission_control()));
                        }
                        Some(nx_shell::hot_corners::Corner::TopRight) => {
                            info!("Hot corner: Spotlight");
                            let _ = ui_tx.send(SessionUiEvent::ToggleSpotlight);
                        }
                        Some(nx_shell::hot_corners::Corner::BottomRight) => {
                            info!("Hot corner: Show Desktop");
                            nx_shell::hot_corners::show_desktop();
                        }
                        _ => {}
                    }
                }

                let mut busy = false;
                for msg in self.motion.tick().into_iter().filter(|_| !self.safe) {
                    busy = true;
                    let _ = ui_tx.send(SessionUiEvent::Motion(msg));
                }

                // Step Spring Physics solver and reposition windows at 120Hz
                busy |= self.wm.read().animating();
                self.wm.write().update_animations(dt);

                // Update and Render GPU Shell Surfaces (focus border overlay only)
                if let Some(bo) = &mut border_overlay {
                    if let Some(rect) = self.wm.read().active_window_rect() {
                        bo.set_target_rect(rect);
                    }
                    busy |= bo.update(dt);
                    let _ = bo.render();
                }

                // 120 Hz while something moves; otherwise wait up to 16 ms
                // for the next message (WinEvents wake us at once), so an
                // idle desktop costs ~60 cheap wakeups instead of 120 frames.
                let budget = if busy {
                    target_frame_time
                } else {
                    Duration::from_millis(16)
                };
                let elapsed = frame_start.elapsed();
                if elapsed < budget {
                    let wait = (budget - elapsed).as_millis().max(1) as u32;
                    unsafe {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT,
                        };
                        let _ = MsgWaitForMultipleObjectsEx(
                            None,
                            wait,
                            QS_ALLINPUT,
                            MWMO_INPUTAVAILABLE,
                        );
                    }
                }
            }
        }

        fn fail_safe_restore_and_quit(&self, ui_tx: &crossbeam_channel::Sender<SessionUiEvent>) {
            warn!("Emergency fail-safe triggered");
            nx_shell::capture::lift_all_hides();
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
fn main() -> std::process::ExitCode {
    // A startup error used to be returned from main and printed to a stderr
    // that a GUI exe does not have: the process just vanished.
    let code = match imp::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("startup failed: {err:#}");
            os_session::diag::fatal("startup error", &format!("{err:#}"));
            std::process::ExitCode::FAILURE
        }
    };
    tracing::info!("nexus-os exiting");
    os_session::diag::pause_console();
    code
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    eprintln!("Nexus-OS binary is supported on Windows NT only.");
    Ok(())
}
