//! Session component startup with graceful degradation.
//!
//! Launching Nexus brings up the Dynamic Island, dock, Mission Control,
//! Spotlight, explorer-ui, the privacy dashboard, and power features.
//! One component failing logs and continues; it must never take the
//! session down.

use std::panic::{catch_unwind, AssertUnwindSafe};

use gpui::AppContext;
use nx_explorer_ui::{ExplorerView, ExplorerWindow};
use nx_power::{AltTabSwitcher, AlwaysOnTopStore, ClipboardStore, NotesStore, VolumeStore};
use nx_privacy::shield::PrivacyShield;
use nx_privacy_dash::{DashboardViewModel, PrivacyDashboardView};
use nx_shell::{
    DockModel, DockPlacement, DockView, ExposeModel, IslandModel, IslandView, MenuBarModel,
    MissionControlView, MotionOverlayView, ShellLaunch, Spotlight, SpotlightView,
};
use nx_ui::geom::Rect;
use nx_ui::motion::MotionPrefs;
use nx_ui::theme::Theme;
use nx_wm::MonitorId;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionComponent {
    Shell,
    Explorer,
    PrivacyDashboard,
    PowerFeatures,
}

impl SessionComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "nx-shell",
            Self::Explorer => "explorer-ui",
            Self::PrivacyDashboard => "privacy-dashboard",
            Self::PowerFeatures => "power-features",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::Shell,
            Self::Explorer,
            Self::PrivacyDashboard,
            Self::PowerFeatures,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatus {
    pub component: SessionComponent,
    pub started: bool,
    pub error: Option<String>,
}

pub struct ShellSession {
    pub menu_bar: MenuBarModel,
    pub dock: DockModel,
    pub expose: ExposeModel,
    pub spotlight: Spotlight,
    pub island: IslandModel,
}

pub struct ExplorerSession {
    pub window: ExplorerWindow,
}

pub struct PrivacySession {
    pub dashboard: DashboardViewModel,
    pub shield: PrivacyShield,
}

pub struct PowerSession {
    pub clipboard: ClipboardStore,
    pub volume: VolumeStore,
    pub notes: NotesStore,
    pub always_on_top: AlwaysOnTopStore,
    pub alt_tab: AltTabSwitcher,
}

/// Live session components. Dropping this tears down only the pieces that
/// actually started; the rest of Nexus keeps running.
pub struct SessionHandle {
    pub shell: Option<ShellSession>,
    pub explorer: Option<ExplorerSession>,
    pub privacy: Option<PrivacySession>,
    pub power: Option<PowerSession>,
    pub statuses: Vec<ComponentStatus>,
}

impl SessionHandle {
    pub fn started(&self, component: SessionComponent) -> bool {
        self.statuses
            .iter()
            .any(|status| status.component == component && status.started)
    }

    pub fn session_continued(&self) -> bool {
        true
    }

    pub fn summary(&self) -> String {
        self.statuses
            .iter()
            .map(|status| {
                if status.started {
                    format!("{}=ok", status.component.as_str())
                } else {
                    format!(
                        "{}=degraded({})",
                        status.component.as_str(),
                        status.error.as_deref().unwrap_or("failed")
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn record(
    statuses: &mut Vec<ComponentStatus>,
    component: SessionComponent,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            info!("{} ready", component.as_str());
            statuses.push(ComponentStatus {
                component,
                started: true,
                error: None,
            });
        }
        Err(error) => {
            error!(
                component = component.as_str(),
                error = error.as_str(),
                "session component failed; continuing without it"
            );
            statuses.push(ComponentStatus {
                component,
                started: false,
                error: Some(error),
            });
        }
    }
}

fn run_guarded<T>(
    component: SessionComponent,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            warn!(
                component = component.as_str(),
                "session component panicked; continuing"
            );
            Err(format!("{} panicked", component.as_str()))
        }
    }
}

fn start_shell() -> Result<ShellSession, String> {
    let canvas = Rect::new(0.0, 0.0, 1920.0, 1080.0);
    let mut menu_bar = MenuBarModel::new();
    menu_bar.set_clock(nx_shell::ClockSample::now_local());
    let mut dock = DockModel::new(DockPlacement::bottom(MonitorId(1), canvas));
    dock.pin_defaults();
    dock.pin_taskbar();
    // The user's own order and "Keep in Dock" choices win over the defaults.
    dock.apply_saved(&nx_shell::dock::load_dock_pins());
    dock.set_auto_hide(true);
    dock.set_running("explorer", true);
    dock.set_running("privacy", true);
    Ok(ShellSession {
        menu_bar,
        dock,
        expose: ExposeModel::new(canvas),
        spotlight: Spotlight::with_builtins(),
        island: IslandModel::new(),
    })
}

fn start_explorer() -> Result<ExplorerSession, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let (mut window, fx) = ExplorerWindow::new(home);
    window.fulfill_effects(fx);
    Ok(ExplorerSession { window })
}

fn start_privacy() -> Result<PrivacySession, String> {
    Ok(PrivacySession {
        dashboard: DashboardViewModel::new(),
        shield: PrivacyShield::default(),
    })
}

fn start_power() -> Result<PowerSession, String> {
    Ok(PowerSession {
        clipboard: ClipboardStore::with_default_exclusions(64),
        volume: VolumeStore::new(),
        notes: NotesStore::new(),
        always_on_top: AlwaysOnTopStore::new(),
        alt_tab: AltTabSwitcher::new(),
    })
}

/// Start every product surface. Failures are logged; the session continues.
pub fn start_session() -> SessionHandle {
    start_session_with(None)
}

/// Test hook: override a single component starter with a failure.
pub fn start_session_with(override_fail: Option<SessionComponent>) -> SessionHandle {
    let mut statuses = Vec::with_capacity(4);
    let mut handle = SessionHandle {
        shell: None,
        explorer: None,
        privacy: None,
        power: None,
        statuses: Vec::new(),
    };

    match forced_or(override_fail, SessionComponent::Shell, start_shell) {
        Ok(shell) => {
            handle.shell = Some(shell);
            record(&mut statuses, SessionComponent::Shell, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::Shell, Err(err)),
    }

    match forced_or(override_fail, SessionComponent::Explorer, start_explorer) {
        Ok(explorer) => {
            handle.explorer = Some(explorer);
            record(&mut statuses, SessionComponent::Explorer, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::Explorer, Err(err)),
    }

    match forced_or(
        override_fail,
        SessionComponent::PrivacyDashboard,
        start_privacy,
    ) {
        Ok(privacy) => {
            handle.privacy = Some(privacy);
            record(&mut statuses, SessionComponent::PrivacyDashboard, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::PrivacyDashboard, Err(err)),
    }

    match forced_or(override_fail, SessionComponent::PowerFeatures, start_power) {
        Ok(power) => {
            handle.power = Some(power);
            record(&mut statuses, SessionComponent::PowerFeatures, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::PowerFeatures, Err(err)),
    }

    handle.statuses = statuses;
    info!("session startup: {}", handle.summary());
    handle
}

fn forced_or<T>(
    override_fail: Option<SessionComponent>,
    component: SessionComponent,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if override_fail == Some(component) {
        return Err(format!("injected failure for {}", component.as_str()));
    }
    run_guarded(component, f)
}

/// Represents handles to the opened GPUI windows in the session.
#[derive(Clone)]
pub struct GpuiSessionWindows {
    pub island_window: Option<gpui::WindowHandle<IslandView>>,
    pub dock_window: Option<gpui::WindowHandle<DockView>>,
    pub spotlight_window: Option<gpui::WindowHandle<SpotlightView>>,
    pub expose_window: Option<gpui::WindowHandle<MissionControlView>>,
    pub explorer_window: Option<gpui::WindowHandle<ExplorerView>>,
    pub privacy_dashboard_window: Option<gpui::WindowHandle<PrivacyDashboardView>>,
    /// Full-screen, click-through surface that paints window motion.
    pub motion_window: Option<gpui::WindowHandle<MotionOverlayView>>,
    /// Screen rects the motion effects launch from and pour into.
    pub dock_rect: Rect,
    pub island_rect: Rect,
}

impl GpuiSessionWindows {
    /// True when not a single shell window could be created (nothing visible).
    pub fn none_open(&self) -> bool {
        self.island_window.is_none()
            && self.dock_window.is_none()
            && self.spotlight_window.is_none()
            && self.explorer_window.is_none()
            && self.privacy_dashboard_window.is_none()
    }

    /// One-line summary for the startup log.
    pub fn describe(&self) -> String {
        let yn = |b: bool| if b { "ok" } else { "MISSING" };
        format!(
            "island={} dock={} spotlight={} expose={} explorer={} privacy={} motion_overlay={}",
            yn(self.island_window.is_some()),
            yn(self.dock_window.is_some()),
            yn(self.spotlight_window.is_some()),
            yn(self.expose_window.is_some()),
            yn(self.explorer_window.is_some()),
            yn(self.privacy_dashboard_window.is_some()),
            if crate::diag::safe_mode() {
                "off(safe)"
            } else {
                yn(self.motion_window.is_some())
            },
        )
    }
}

/// Open all user-facing GPUI windows adhering to macOS design tokens, Mica/Acrylic backdrops,
/// and SF typography.
pub fn open_gpui_windows(cx: &mut gpui::App, handle: &mut SessionHandle) -> GpuiSessionWindows {
    let theme = Theme::stealth();
    if !cx.has_global::<Theme>() {
        cx.set_global(theme);
    }

    let mut windows = GpuiSessionWindows {
        island_window: None,
        dock_window: None,
        spotlight_window: None,
        expose_window: None,
        explorer_window: None,
        privacy_dashboard_window: None,
        motion_window: None,
        dock_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        island_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
    };

    let screen = cx
        .primary_display()
        .map(|d| d.bounds())
        .unwrap_or(gpui::Bounds {
            origin: gpui::point(gpui::px(0.0), gpui::px(0.0)),
            size: gpui::size(gpui::px(1920.0), gpui::px(1080.0)),
        });
    let sx = f32::from(screen.origin.x);
    let sy = f32::from(screen.origin.y);
    let sw = f32::from(screen.size.width);
    let sh = f32::from(screen.size.height);

    let island_w = 680.0;
    let island_h = nx_shell::COLLAPSED_HEIGHT;
    let island_x = sx + ((sw - island_w) * 0.5).max(0.0);
    // The island hugs the top edge so the fullscreen slit can sit flush.
    let island_y = sy;

    // Full width. Icons are stored at 128px and drawn at 44, so a slightly
    // taller bar stays sharp.
    let dock_w = sw.max(320.0);
    // Tall enough for the hover bloom (icons grow to about 1.55x) without clipping.
    let dock_h = 112.0;
    let dock_x = sx;
    let dock_y = sy + sh - dock_h;

    let spot_w = 1000.0;
    let spot_h = 580.0;
    let spot_x = sx + ((sw - spot_w) * 0.5).max(0.0);
    let spot_y = sy + (sh * 0.14).max(80.0);

    let explorer_w = 980.0;
    let explorer_x = sx + 48.0;
    let explorer_y = sy + 80.0;
    let explorer_h = 720.0_f32.min((dock_y - explorer_y - 16.0).max(420.0));

    let privacy_w = 820.0;
    let privacy_x =
        (explorer_x + explorer_w + 28.0).min((sx + sw - privacy_w - 24.0).max(sx + 24.0));
    let privacy_y = sy + 120.0;
    let privacy_h = 680.0_f32.min((dock_y - privacy_y - 16.0).max(420.0));

    windows.dock_rect = Rect::new(dock_x, dock_y, dock_w, dock_h);
    windows.island_rect = Rect::new(island_x, island_y, island_w, island_h);

    if let Some(shell) = &handle.shell {
        let island_model = shell.island.clone();
        let open_t = std::time::Instant::now();
        windows.island_window = cx
            .open_window(
                theme.shell_window_options(
                    gpui::Bounds {
                        origin: gpui::point(gpui::px(island_x), gpui::px(island_y)),
                        size: gpui::size(gpui::px(island_w), gpui::px(island_h)),
                    },
                    false,
                    true,
                    true,
                ),
                move |window, cx| {
                    crate::chrome::install(window, Some(cx), "island", crate::chrome::Kind::Shell);
                    #[cfg(windows)]
                    nx_shell::capture::raise_topmost(window);
                    cx.new(|cx| IslandView::new(island_model, cx))
                },
            )
            .inspect_err(|e| error!("window 'island' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("island", open_t);

        let mut dock_model = shell.dock.clone();
        let mut place = dock_model.placement;
        place.bounds = Rect::new(sx, sy, sw, sh);
        // Edge-only reveal (macOS). The view also holds the dock open from
        // a physical-pixel edge test, so DPI rounding cannot strand it.
        place.reveal_px = 2.0;
        place.margin = 0.0;
        dock_model.set_placement(place);
        // Auto-hide: parked until the pointer touches the bottom edge, held
        // open while the pointer is over the shelf (physical-pixel test in
        // the view, so hover and clicks reach the icons), click-through
        // whenever the pointer is not over the shelf.
        dock_model.set_auto_hide(true);
        dock_model.mag.rest = 44.0;
        dock_model.mag.max_scale = 1.55;
        dock_model.mag.padding = 8.0;
        dock_model.mag.gap = 8.0;
        let open_t = std::time::Instant::now();
        windows.dock_window = cx
            .open_window(
                theme.shell_window_options(
                    gpui::Bounds {
                        origin: gpui::point(gpui::px(dock_x), gpui::px(dock_y)),
                        size: gpui::size(gpui::px(dock_w), gpui::px(dock_h)),
                    },
                    true,
                    true,
                    true,
                ),
                move |window, cx| {
                    crate::chrome::install(window, Some(cx), "dock", crate::chrome::Kind::Shell);
                    #[cfg(windows)]
                    nx_shell::capture::raise_topmost(window);
                    cx.new(|_cx| DockView::new(dock_model))
                },
            )
            .inspect_err(|e| error!("window 'dock' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("dock", open_t);

        // Spotlight starts hidden; HotkeyAction::CommandBar toggles it.
        let mut spotlight_model = Spotlight::with_settings();
        nx_shell::clip_history::start_watcher();
        spotlight_model.query = shell.spotlight.query.clone();
        let mut spotlight_opts = theme.shell_window_options(
            gpui::Bounds {
                origin: gpui::point(gpui::px(spot_x), gpui::px(spot_y)),
                size: gpui::size(gpui::px(spot_w), gpui::px(spot_h)),
            },
            true,
            true,
            false,
        );
        spotlight_opts.focus = false;
        let open_t = std::time::Instant::now();
        windows.spotlight_window = cx
            .open_window(spotlight_opts, move |window, cx| {
                crate::chrome::install(window, Some(cx), "spotlight", crate::chrome::Kind::Overlay);
                // Linux ignores WindowOptions.show; park minimized until toggled.
                hide_auxiliary_window(window);
                cx.new(|_cx| SpotlightView::new(spotlight_model))
            })
            .inspect_err(|e| error!("window 'spotlight' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("spotlight", open_t);
        // Index apps and recent files off the UI thread; Spotlight opens at
        // once with settings and fills in as soon as the scan lands.
        if let Some(handle) = windows.spotlight_window {
            cx.spawn(async move |cx| {
                let t = std::time::Instant::now();
                let (apps, docs) = cx
                    .background_executor()
                    .spawn(async {
                        (
                            nx_shell::catalog::installed_apps(),
                            nx_shell::dock::recent_items(40),
                        )
                    })
                    .await;
                info!(
                    "spotlight: indexed {} apps, {} recent items in {} ms",
                    apps.len(),
                    docs.len(),
                    t.elapsed().as_millis()
                );
                let _ = handle.update(cx, |view, _window, cx| {
                    nx_shell::catalog::fill(&mut view.model, &apps, &docs);
                    cx.notify();
                });
            })
            .detach();
        }

        let expose_model = shell.expose.clone();
        let mut expose_opts = theme.shell_window_options(
            gpui::Bounds {
                origin: gpui::point(gpui::px(sx), gpui::px(sy)),
                size: gpui::size(gpui::px(sw), gpui::px(sh)),
            },
            true,
            false,
            false,
        );
        expose_opts.focus = false;
        let open_t = std::time::Instant::now();
        windows.expose_window = cx
            .open_window(expose_opts, move |window, cx| {
                crate::chrome::install(
                    window,
                    Some(cx),
                    "mission-control",
                    crate::chrome::Kind::Overlay,
                );
                hide_auxiliary_window(window);
                cx.new(|_cx| MissionControlView::new(expose_model))
            })
            .inspect_err(|e| error!("window 'expose' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("expose", open_t);
    }

    if let Some(explorer) = handle.explorer.take() {
        let open_t = std::time::Instant::now();
        windows.explorer_window = cx
            .open_window(
                theme.blurred_window_options(gpui::Bounds {
                    origin: gpui::point(gpui::px(explorer_x), gpui::px(explorer_y)),
                    size: gpui::size(gpui::px(explorer_w), gpui::px(explorer_h)),
                }),
                move |window, cx| {
                    crate::chrome::install(window, Some(cx), "files", crate::chrome::Kind::App);
                    cx.new(|_cx| ExplorerView::new(explorer.window))
                },
            )
            .inspect_err(|e| error!("window 'explorer' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("explorer", open_t);
    }

    if let Some(privacy) = &handle.privacy {
        let dash_vm = privacy.dashboard.clone();
        let open_t = std::time::Instant::now();
        windows.privacy_dashboard_window = cx
            .open_window(
                theme.blurred_window_options(gpui::Bounds {
                    origin: gpui::point(gpui::px(privacy_x), gpui::px(privacy_y)),
                    size: gpui::size(gpui::px(privacy_w), gpui::px(privacy_h)),
                }),
                move |window, cx| {
                    crate::chrome::install(window, Some(cx), "privacy", crate::chrome::Kind::App);
                    cx.new(|_cx| PrivacyDashboardView::new(dash_vm))
                },
            )
            .inspect_err(|e| error!("window 'privacy_dashboard' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("privacy_dashboard", open_t);
    }

    // Opened last so it stacks above every other Nexus surface, but it stays
    // hidden until an effect is actually playing. A visible full-screen layer
    // catches every click, including Privacy Shield, Files, and the dock.
    let work = Rect::new(sx, sy, sw, sh);
    let mut motion_opts = theme.shell_window_options(
        gpui::Bounds {
            origin: gpui::point(gpui::px(sx), gpui::px(sy)),
            size: gpui::size(gpui::px(sw), gpui::px(sh)),
        },
        false,
        true,
        false,
    );
    motion_opts.focus = false;
    if crate::diag::safe_mode() {
        warn!("safe mode: motion overlay not created");
    } else {
        let open_t = std::time::Instant::now();
        windows.motion_window = cx
            .open_window(motion_opts, move |window, cx| {
                let _ = &window;
                #[cfg(windows)]
                {
                    nx_shell::capture::set_overlay_visible(window, false);
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(handle) = HasWindowHandle::window_handle(window) {
                        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                            nx_shell::capture::keep_overlay_hidden(win32.hwnd.get());
                        }
                    }
                }
                cx.new(|_cx| MotionOverlayView::new((sx, sy), work, motion_prefs(false, 1.0)))
            })
            .inspect_err(|e| error!("window 'motion' failed to open: {e:#}"))
            .ok();
        crate::diag::window_opened("motion", open_t);
    }
    if windows.island_window.is_none() {
        error!("island window failed to open");
    }

    install_shell_launch(&windows, cx);
    install_island_hooks(&windows, cx);
    // The motion overlay is created last and would cover the pill. Lift the
    // island after every other window exists.
    if let Some(island) = windows.island_window {
        let _ = island.update(cx, |_, window, _| {
            #[cfg(windows)]
            nx_shell::capture::raise_topmost(window);
            let _ = window;
        });
    }
    if let Some(dock) = windows.dock_window {
        let _ = dock.update(cx, |_, window, _| {
            #[cfg(windows)]
            nx_shell::capture::raise_topmost(window);
            let _ = window;
        });
    }
    windows
}

/// Let the island assistant open Spotlight with a query.
fn install_island_hooks(windows: &GpuiSessionWindows, cx: &mut gpui::App) {
    let spotlight = windows.spotlight_window;
    cx.set_global(nx_shell::IslandHooks::new(move |query, cx| {
        if let Some(handle) = spotlight {
            let _ = handle.update(cx, |view, window, cx| {
                if !view.model.is_open() {
                    view.toggle();
                }
                view.model.query = query.to_string();
                apply_spotlight_visibility(window, true);
                cx.notify();
            });
        }
    }));
}

fn install_shell_launch(windows: &GpuiSessionWindows, cx: &mut gpui::App) {
    let explorer = windows.explorer_window;
    let privacy = windows.privacy_dashboard_window;
    let spotlight = windows.spotlight_window;
    let all = windows.clone();
    install_window_controls(windows, cx);
    // Dock menus and stacks open as their own small pop-up windows.
    cx.set_global(nx_shell::gpui_views::PopupWindowHook(std::sync::Arc::new(
        |window: &mut gpui::Window| {
            crate::chrome::install(window, None, "dock-popup", crate::chrome::Kind::Shell);
        },
    )));
    cx.set_global(ShellLaunch::new(move |id, cx| {
        match id {
            "explorer" | "finder" => {
                if let Some(handle) = explorer {
                    bring_back(&all, handle.into(), "explorer", cx);
                }
            }
            "privacy" | "camera" | "microphone" | "location" => {
                if let Some(handle) = privacy {
                    bring_back(&all, handle.into(), "privacy", cx);
                }
            }
            other => {
                // "files:<folder>" opens Files at that folder (dock menu
                // "Show in Files", recent-items stack).
                if other.starts_with("clip:") {
                    nx_shell::clip_history::copy_back(other);
                } else if other.starts_with("app:") || other.starts_with("doc:") {
                    if let Some(target) = nx_shell::catalog::target_of(other) {
                        nx_shell::gpui_views::open_target(&target);
                    }
                } else if let Some(uri) = nx_shell::catalog::settings_uri(other) {
                    nx_shell::gpui_views::open_target(uri);
                } else if let (Some(path), Some(handle)) = (other.strip_prefix("files:"), explorer)
                {
                    let path = std::path::PathBuf::from(path);
                    let _ = handle.update(cx, |view, _window, cx| {
                        view.navigate_to(path);
                        cx.notify();
                    });
                    bring_back(&all, handle.into(), "explorer", cx);
                }
            }
        }
        if let Some(handle) = spotlight {
            let _ = handle.update(cx, |view, window, cx| {
                if view.model.is_open() {
                    view.model.close_palette();
                }
                apply_spotlight_visibility(window, false);
                cx.notify();
            });
        }
    }));
}

/// Show a Nexus app window again. When it was minimized or closed (hidden)
/// it blooms back out of its dock icon first.
fn bring_back(
    windows: &GpuiSessionWindows,
    handle: gpui::AnyWindowHandle,
    app: &'static str,
    cx: &mut gpui::App,
) {
    #[cfg(windows)]
    {
        let raw = handle
            .update(cx, |_, window, _| {
                let raw = crate::chrome::window_hwnd(window)?;
                let h = windows::Win32::Foundation::HWND(raw as *mut std::ffi::c_void);
                let away = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::IsIconic(h).as_bool()
                        || !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(h).as_bool()
                };
                away.then_some(raw)
            })
            .ok()
            .flatten();
        if let Some(raw) = raw {
            let show: crate::motion_bridge::OwnShow = Box::new(move |cx: &mut gpui::App| {
                let _ = handle.update(cx, |_, window, _| crate::chrome::show_app_window(window));
            });
            match crate::motion_bridge::own_restore(windows, cx, raw, app, show) {
                Ok(()) => return,
                Err(show) => {
                    show(cx);
                    return;
                }
            }
        }
    }
    let _ = (windows, app);
    let _ = handle.update(cx, |_, window, _| crate::chrome::show_app_window(window));
}

/// Caption buttons on Files / Privacy: genie into the dock icon on
/// minimize, dust on close (the window hides; the dock brings it back).
fn install_window_controls(windows: &GpuiSessionWindows, cx: &mut gpui::App) {
    let all = windows.clone();
    let files = windows.explorer_window.map(|h| h.window_id());
    let privacy = windows.privacy_dashboard_window.map(|h| h.window_id());
    cx.set_global(nx_ui::gpui_theme::WindowControlHook(std::sync::Arc::new(
        move |action, window, cx| {
            let id = window.window_handle().window_id();
            let app = if Some(id) == files {
                "explorer"
            } else if Some(id) == privacy {
                "privacy"
            } else {
                return false;
            };
            #[cfg(windows)]
            if let Some(raw) = crate::chrome::window_hwnd(window) {
                match action {
                    "min" => {
                        crate::motion_bridge::own_minimize(&all, cx, raw, app);
                        window.minimize_window();
                        return true;
                    }
                    "close" => {
                        crate::motion_bridge::own_close(&all, cx, raw);
                        nx_ui::gpui_theme::close_window(window);
                        return true;
                    }
                    _ => {}
                }
            }
            let _ = (&all, action, app, &cx);
            false
        },
    )));
}

/// Open (or close) the island's assistant prompt — Alt+Space / `shell:assistant`.
pub fn toggle_island_assistant(handle: gpui::WindowHandle<IslandView>, cx: &mut gpui::App) -> bool {
    handle
        .update(cx, |view, window, cx| {
            if view.model.expanded {
                view.set_expanded(false);
            } else {
                view.open_prompt(window);
            }
            cx.notify();
            view.model.expanded
        })
        .unwrap_or(false)
}

/// Resolve motion preferences from the config switch and, on Windows, the
/// OS "Animation effects" accessibility setting.
pub fn motion_prefs(user_reduce: bool, speed: f32) -> MotionPrefs {
    #[cfg(windows)]
    let os_on = nx_shell::capture::os_animations_enabled();
    #[cfg(not(windows))]
    let os_on = true;
    MotionPrefs::resolve(user_reduce, os_on, speed)
}

/// Run `f` against the motion overlay, if it opened.
pub fn with_motion(
    windows: &GpuiSessionWindows,
    cx: &mut gpui::App,
    f: impl FnOnce(&mut MotionOverlayView),
) {
    if let Some(handle) = windows.motion_window {
        let _ = handle.update(cx, |view, window, cx| {
            view.sync_window(window);
            f(view);
            cx.notify();
        });
    }
}

/// Screen rect of the dock icon for `app_id`, or the island when the app
/// is not in the dock (assistant launches, background apps).
pub fn motion_anchor(windows: &GpuiSessionWindows, cx: &gpui::App, app_id: &str) -> Rect {
    let Some(dock) = windows.dock_window else {
        return windows.island_rect;
    };
    let Ok(view) = dock.read(cx) else {
        return windows.island_rect;
    };
    let items = view.model.items();
    match items
        .iter()
        .position(|item| item.id.eq_ignore_ascii_case(app_id))
    {
        Some(index) => {
            nx_shell::dock_icon_rect(windows.dock_rect, items.len(), index, view.model.mag)
        }
        None => windows.island_rect,
    }
}

/// Expand or collapse the island (click-equivalent; hover never expands).
pub fn set_island_expanded(
    handle: gpui::WindowHandle<IslandView>,
    expanded: bool,
    cx: &mut gpui::App,
) -> bool {
    handle
        .update(cx, |view, window, cx| {
            if expanded {
                view.open_prompt(window);
            } else {
                view.set_expanded(false);
            }
            cx.notify();
            view.model.expanded
        })
        .unwrap_or(false)
}

/// Open Spotlight with a prefilled query (Ctrl+Alt+V → clipboard history).
pub fn open_gpui_spotlight_with(
    handle: gpui::WindowHandle<SpotlightView>,
    query: &str,
    cx: &mut gpui::App,
) {
    let query = query.to_string();
    let _ = handle.update(cx, |view, window, cx| {
        if !view.model.is_open() {
            view.toggle();
        }
        view.model.query = query;
        view.selected = 0;
        apply_spotlight_visibility(window, true);
        window.activate_window();
        cx.notify();
    });
}

pub fn toggle_gpui_spotlight(
    handle: gpui::WindowHandle<SpotlightView>,
    cx: &mut gpui::App,
) -> bool {
    handle
        .update(cx, |view, window, cx| {
            let open = view.toggle();
            apply_spotlight_visibility(window, open);
            if open {
                window.activate_window();
            }
            cx.notify();
            open
        })
        .unwrap_or(false)
}

/// Test GPUI windows panic on minimize; Linux ignores WindowOptions.show, so we
/// only park auxiliary surfaces when running a real session.
fn hide_auxiliary_window(window: &mut gpui::Window) {
    #[cfg(all(unix, not(test)))]
    {
        window.minimize_window();
    }
    let _ = window;
}

fn apply_spotlight_visibility(window: &mut gpui::Window, visible: bool) {
    if visible {
        window.resize(gpui::size(gpui::px(1000.0), gpui::px(580.0)));
        window.activate_window();
    } else {
        hide_auxiliary_window(window);
    }
    #[cfg(windows)]
    if let Some(raw) = crate::chrome::window_hwnd(window) {
        if visible {
            nx_ui::platform::restore_and_focus(raw);
        } else {
            nx_ui::platform::hide_async(raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_session_brings_up_all_surfaces() {
        let session = start_session();
        assert!(session.session_continued());
        for component in SessionComponent::all() {
            assert!(
                session.started(component),
                "{} should start",
                component.as_str()
            );
        }
        assert!(session.shell.is_some());
        assert!(session.explorer.is_some());
        assert!(session.privacy.is_some());
        assert!(session.power.is_some());
        let summary = session.summary();
        assert!(summary.contains("nx-shell=ok"));
        assert!(summary.contains("explorer-ui=ok"));
        assert!(summary.contains("privacy-dashboard=ok"));
        assert!(summary.contains("power-features=ok"));
    }

    #[test]
    fn one_component_failing_does_not_take_down_the_session() {
        let session = start_session_with(Some(SessionComponent::Explorer));
        assert!(session.session_continued());
        assert!(!session.started(SessionComponent::Explorer));
        assert!(session.started(SessionComponent::Shell));
        assert!(session.started(SessionComponent::PrivacyDashboard));
        assert!(session.started(SessionComponent::PowerFeatures));
        assert!(session.explorer.is_none());
        assert!(session.shell.is_some());
        let summary = session.summary();
        assert!(summary.contains("explorer-ui=degraded"));
        assert!(summary.contains("nx-shell=ok"));
    }

    #[test]
    fn privacy_dashboard_failure_leaves_power_and_shell_running() {
        let session = start_session_with(Some(SessionComponent::PrivacyDashboard));
        assert!(session.session_continued());
        assert!(!session.started(SessionComponent::PrivacyDashboard));
        assert!(session.started(SessionComponent::Shell));
        assert!(session.started(SessionComponent::PowerFeatures));
        assert!(session.privacy.is_none());
    }

    #[gpui::test]
    fn test_gpui_session_startup_opens_windows(cx: &mut gpui::TestAppContext) {
        let mut session = start_session();
        cx.update(|cx| {
            let windows = open_gpui_windows(cx, &mut session);
            assert!(windows.island_window.is_some());
            assert!(windows.dock_window.is_some());
            assert!(windows.spotlight_window.is_some());
            assert!(windows.expose_window.is_some());
            assert!(windows.explorer_window.is_some());
            assert!(windows.privacy_dashboard_window.is_some());
            assert!(windows.motion_window.is_some());
            assert!(windows.dock_rect.w > 0.0 && windows.island_rect.w > 0.0);
        });
    }

    #[gpui::test]
    fn test_motion_overlay_plays_and_hands_back(cx: &mut gpui::TestAppContext) {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::Arc;

        let mut session = start_session();
        let handed_back = Rc::new(Cell::new(false));
        let flag = handed_back.clone();
        cx.update(|cx| {
            let windows = open_gpui_windows(cx, &mut session);
            let anchor = motion_anchor(&windows, cx, "explorer");
            assert!(anchor.w > 0.0);
            let snap = Arc::new(nx_shell::Snapshot::placeholder(64, 48));
            let rect = Rect::new(100.0, 100.0, 640.0, 480.0);
            with_motion(&windows, cx, move |o| {
                assert!(o.open(
                    7,
                    snap,
                    rect,
                    anchor,
                    Some(Box::new(move |_cx| flag.set(true))),
                ));
                assert!(o.is_animating());
            });
        });
        // Drive frames until the bloom settles and the done hook fires.
        for _ in 0..120 {
            cx.run_until_parked();
            cx.update(|cx| cx.refresh_windows());
            std::thread::sleep(std::time::Duration::from_millis(8));
            if handed_back.get() {
                break;
            }
        }
        assert!(handed_back.get(), "real window is handed back after bloom");
    }

    #[gpui::test]
    fn test_command_bar_toggles_gpui_spotlight(cx: &mut gpui::TestAppContext) {
        let mut session = start_session();
        cx.update(|cx| {
            let windows = open_gpui_windows(cx, &mut session);
            let handle = windows
                .spotlight_window
                .expect("SpotlightView window must be created");
            assert!(
                !handle.read(cx).expect("spotlight view").model.is_open(),
                "Spotlight starts closed"
            );
            assert!(toggle_gpui_spotlight(handle, cx));
            assert!(handle.read(cx).expect("spotlight view").model.is_open());
            assert!(!toggle_gpui_spotlight(handle, cx));
            assert!(!handle.read(cx).expect("spotlight view").model.is_open());
        });
    }
}
