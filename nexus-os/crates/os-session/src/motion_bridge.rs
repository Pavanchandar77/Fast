//! Windows bridge between WinEvents and the motion overlay.
//!
//! The compositor thread owns [`MotionBridge`]: it feeds WinEvents through
//! [`MotionPlanner`], captures snapshots with `PrintWindow`, and sends
//! [`UiMotion`] messages. The GPUI thread turns those into overlay effects
//! with [`apply`]. Hides applied to other apps' windows travel inside the
//! message as [`BloomHide`] guards, so the window comes back when the effect
//! finishes, is interrupted, or the message is dropped.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nx_shell::capture::{self, BloomHide};
use nx_shell::Snapshot;
use nx_ui::Rect;
use os_compositor::hooks::WindowEvent;
use os_core::window::WindowHandle;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, SetForegroundWindow,
};

use crate::motion_plan::{LifeEvent, MotionPlanner, Plan};
use crate::startup::{motion_anchor, with_motion, GpuiSessionWindows};

/// Captures are kept at half resolution: plenty for a 400 ms effect.
const CAPTURE_STEP: u32 = 2;
const MAX_CACHED: usize = 24;
const REFRESH: Duration = Duration::from_millis(1500);

pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

/// Messages from the compositor thread to the GPUI thread.
pub enum UiMotion {
    Open {
        key: u64,
        app: String,
        rect: Rect,
        px: Pixels,
        hide: Option<BloomHide>,
    },
    Close {
        key: u64,
        rect: Rect,
        px: Pixels,
    },
    Minimize {
        key: u64,
        app: String,
        rect: Rect,
        px: Pixels,
    },
    Restore {
        key: u64,
        app: String,
        rect: Rect,
        px: Pixels,
        hide: Option<BloomHide>,
    },
    /// Maximize or restore-down. `from` is where it was, `to` is where it lands.
    Zoom {
        key: u64,
        from: Rect,
        to: Rect,
        px: Pixels,
        hide: Option<BloomHide>,
    },
    Ripple {
        rect: Rect,
    },
    DragBegin {
        key: u64,
        cursor: (f32, f32),
        others: Vec<u64>,
    },
    DragMove {
        cursor: (f32, f32),
        rect: Option<Rect>,
    },
    DragEnd {
        key: u64,
        /// The WM leaves this window where it is dropped (floating layout),
        /// so a magnetic snap may move it.
        may_snap: bool,
    },
    Switcher {
        cards: Vec<(u64, String, Pixels)>,
    },
    MissionControl {
        cards: Vec<(u64, Rect, Pixels)>,
    },
}

struct Cached {
    rect: Rect,
    w: u32,
    h: u32,
    bgra: Arc<Vec<u8>>,
    at: Instant,
}

pub struct MotionBridge {
    planner: MotionPlanner,
    cache: HashMap<u64, Cached>,
    foreground: Option<u64>,
    dragging: Option<u64>,
    pub animate_foreign: bool,
    /// Show/hide events caused by our own hide are ignored until this time.
    fx_quiet: HashMap<u64, Instant>,
}

fn handle(key: u64) -> WindowHandle {
    WindowHandle::new(HWND(key as isize as *mut core::ffi::c_void))
}

fn key_of(h: &WindowHandle) -> u64 {
    h.raw().0 as isize as u64
}

fn sane_window(r: &Rect) -> bool {
    r.w >= 120.0 && r.h >= 80.0 && r.w <= 8_000.0 && r.h <= 8_000.0
}

fn cursor() -> (f32, f32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x as f32, p.y as f32)
}

/// Lowercase exe stem, matching dock ids (`explorer`, `code`, ...).
fn app_id(h: &WindowHandle) -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let pid = h.pid();
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let path = unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(proc) => {
                let ok = QueryFullProcessImageNameW(
                    proc,
                    PROCESS_NAME_WIN32,
                    PWSTR(buf.as_mut_ptr()),
                    &mut len,
                )
                .is_ok();
                let _ = CloseHandle(proc);
                if ok {
                    String::from_utf16_lossy(&buf[..len as usize])
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        }
    };
    let stem = path.rsplit(['\\', '/']).next().unwrap_or("");
    stem.trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .to_ascii_lowercase()
}

impl MotionBridge {
    pub fn new(animate_foreign: bool) -> Self {
        Self {
            planner: MotionPlanner::new(),
            cache: HashMap::new(),
            foreground: None,
            dragging: None,
            animate_foreign,
            fx_quiet: HashMap::new(),
        }
    }

    fn quiet(&self, key: u64) -> bool {
        self.fx_quiet
            .get(&key)
            .is_some_and(|until| Instant::now() < *until)
    }

    fn arm(&mut self, key: u64) {
        self.fx_quiet
            .insert(key, Instant::now() + Duration::from_millis(700));
    }

    /// Windows already on screen at startup: no open animation, but cache
    /// pixels so closing or minimizing them still plays.
    pub fn adopt(&mut self, h: WindowHandle) {
        let key = key_of(&h);
        self.planner.adopt(key);
        if self.cache.len() < MAX_CACHED {
            self.capture(key);
        }
    }

    fn capture(&mut self, key: u64) -> bool {
        let raw = key as isize;
        let (Some(rect), Some((w, h, bgra))) = (
            capture::window_rect(raw),
            capture::capture_bgra(raw, CAPTURE_STEP),
        ) else {
            return false;
        };
        if self.cache.len() >= MAX_CACHED && !self.cache.contains_key(&key) {
            // Evict the stalest entry.
            if let Some(old) = self.cache.iter().min_by_key(|(_, c)| c.at).map(|(k, _)| *k) {
                self.cache.remove(&old);
            }
        }
        self.cache.insert(
            key,
            Cached {
                rect,
                w,
                h,
                bgra: Arc::new(bgra),
                at: Instant::now(),
            },
        );
        self.planner.mark_cached(key);
        true
    }

    fn cached_px(&self, key: u64) -> Option<(Rect, Pixels)> {
        let c = self.cache.get(&key)?;
        Some((
            c.rect,
            Pixels {
                w: c.w,
                h: c.h,
                bgra: (*c.bgra).clone(),
            },
        ))
    }

    fn fresh_px(&mut self, key: u64) -> Option<(Rect, Pixels)> {
        if self.capture(key) {
            self.cached_px(key)
        } else {
            None
        }
    }

    /// Maximize and restore-down. One bloom, then the real window is shown
    /// again at the size Windows already chose. We never resize it ourselves.
    fn on_location(&mut self, h: &WindowHandle) -> Vec<UiMotion> {
        if !self.animate_foreign {
            return Vec::new();
        }
        let key = key_of(h);
        if self.quiet(key) || self.dragging == Some(key) || !h.is_manageable() {
            return Vec::new();
        }
        let Some(now) = capture::window_rect(key as isize) else {
            return Vec::new();
        };
        let Some(old) = self.cache.get(&key).map(|c| c.rect) else {
            return Vec::new();
        };
        if !sane_window(&now) || !sane_window(&old) {
            return Vec::new();
        }
        let old_area = old.w * old.h;
        let now_area = now.w * now.h;
        let grew = now_area > old_area * 1.55 && now.w > old.w + 100.0;
        let shrank = old_area > now_area * 1.55 && old.w > now.w + 100.0;
        if let Some(cached) = self.cache.get_mut(&key) {
            cached.rect = now;
        }
        if !grew && !shrank {
            return Vec::new();
        }
        let Some((_, px)) = self.cached_px(key) else {
            return Vec::new();
        };
        self.arm(key);
        let from = if grew {
            old
        } else {
            Rect::new(
                now.x + now.w * 0.5 - 36.0,
                now.y + now.h * 0.5 - 36.0,
                72.0,
                72.0,
            )
        };
        vec![UiMotion::Zoom {
            key,
            from,
            to: now,
            px,
            hide: BloomHide::new(key as isize),
        }]
    }

    /// Feed one WinEvent. Only manageable top-level windows animate.
    pub fn on_window_event(&mut self, ev: &WindowEvent, floating: bool) -> Vec<UiMotion> {
        if let WindowEvent::Location(h) = ev {
            return self.on_location(h);
        }
        let (life, h) = match ev {
            WindowEvent::ForegroundChanged(h) => (LifeEvent::Foreground(key_of(h)), h),
            WindowEvent::WindowShow(h) => (LifeEvent::Shown(key_of(h)), h),
            WindowEvent::WindowHide(h) => (LifeEvent::Hidden(key_of(h)), h),
            WindowEvent::WindowDestroyed(h) => (LifeEvent::Destroyed(key_of(h)), h),
            WindowEvent::MinimizeStart(h) => (LifeEvent::MinimizeStart(key_of(h)), h),
            WindowEvent::MinimizeEnd(h) => (LifeEvent::MinimizeEnd(key_of(h)), h),
            WindowEvent::MoveSizeStart(h) => (LifeEvent::DragStart(key_of(h)), h),
            WindowEvent::MoveSizeEnd(h) => (LifeEvent::DragEnd(key_of(h)), h),
            _ => return Vec::new(),
        };
        let key = key_of(h);
        let echo = matches!(
            ev,
            WindowEvent::WindowShow(_)
                | WindowEvent::WindowHide(_)
                | WindowEvent::WindowCloaked(_)
                | WindowEvent::WindowUncloaked(_)
        );
        if echo && self.quiet(key) {
            return Vec::new();
        }
        // Destroyed windows can no longer be queried; trust the planner.
        let destroyed = matches!(ev, WindowEvent::WindowDestroyed(_));
        if !destroyed && !h.is_manageable() && !matches!(ev, WindowEvent::WindowHide(_)) {
            return Vec::new();
        }
        if let WindowEvent::ForegroundChanged(_) = ev {
            self.foreground = Some(key_of(h));
        }
        let mut out = Vec::new();
        for plan in self.planner.on(life) {
            match plan {
                Plan::Capture(k) => {
                    self.capture(k);
                }
                Plan::Forget(k) => {
                    self.cache.remove(&k);
                }
                Plan::Ripple(k) => {
                    if let Some(rect) = capture::window_rect(k as isize) {
                        out.push(UiMotion::Ripple { rect });
                    }
                }
                Plan::Open(k) if self.animate_foreign => {
                    if let Some((rect, px)) = self.fresh_px(k) {
                        if sane_window(&rect) {
                            self.arm(k);
                            out.push(UiMotion::Open {
                                key: k,
                                app: app_id(h),
                                rect,
                                px,
                                hide: BloomHide::new(k as isize),
                            });
                        }
                    }
                }
                Plan::Restore(k) if self.animate_foreign => {
                    if let Some((rect, px)) = self.fresh_px(k) {
                        if sane_window(&rect) {
                            self.arm(k);
                            out.push(UiMotion::Restore {
                                key: k,
                                app: app_id(h),
                                rect,
                                px,
                                hide: BloomHide::new(k as isize),
                            });
                        }
                    }
                }
                Plan::Close(k) if self.animate_foreign => {
                    if let Some((rect, px)) = self.cached_px(k) {
                        if sane_window(&rect) {
                            self.arm(k);
                            out.push(UiMotion::Close { key: k, rect, px });
                        }
                    }
                }
                Plan::Minimize(k) if self.animate_foreign => {
                    if let Some((rect, px)) = self.cached_px(k) {
                        if sane_window(&rect) {
                            self.arm(k);
                            out.push(UiMotion::Minimize {
                                key: k,
                                app: app_id(h),
                                rect,
                                px,
                            });
                        }
                    }
                }
                Plan::DragBegin(k) => {
                    self.dragging = Some(k);
                    out.push(UiMotion::DragBegin {
                        key: k,
                        cursor: cursor(),
                        others: self.planner.others(k),
                    });
                }
                Plan::DragEnd(k) => {
                    self.dragging = None;
                    out.push(UiMotion::DragEnd {
                        key: k,
                        may_snap: floating,
                    });
                }
                _ => {}
            }
        }
        out
    }

    /// Per compositor frame: drag tracking and a slow foreground refresh so
    /// the close/minimize snapshot is never stale.
    pub fn tick(&mut self) -> Vec<UiMotion> {
        let mut out = Vec::new();
        if let Some(k) = self.dragging {
            out.push(UiMotion::DragMove {
                cursor: cursor(),
                rect: capture::window_rect(k as isize),
            });
        }
        if let Some(k) = self.foreground {
            let stale = self.cache.get(&k).is_none_or(|c| c.at.elapsed() > REFRESH);
            if stale && self.dragging.is_none() && !handle(k).is_minimized() {
                self.capture(k);
            }
        }
        out
    }

    pub fn switcher(&self) -> UiMotion {
        let cards = self
            .planner
            .switcher_order()
            .into_iter()
            .filter_map(|k| {
                let (_, px) = self.cached_px(k)?;
                Some((k, handle(k).title(), px))
            })
            .collect();
        UiMotion::Switcher { cards }
    }

    pub fn mission_control(&self) -> UiMotion {
        let cards = self
            .planner
            .others(0)
            .into_iter()
            .filter_map(|k| {
                let (rect, px) = self.cached_px(k)?;
                Some((k, rect, px))
            })
            .collect();
        UiMotion::MissionControl { cards }
    }
}

fn snap(px: Pixels) -> Arc<Snapshot> {
    Arc::new(
        Snapshot::from_bgra(px.w, px.h, px.bgra).unwrap_or_else(|| Snapshot::placeholder(64, 48)),
    )
}

/// GPUI-thread state for the bridge.
#[derive(Default)]
pub struct UiMotionState {
    drag_key: Option<u64>,
    peek_others: Vec<u64>,
    peek_hides: Vec<BloomHide>,
    switcher_keys: Vec<u64>,
    switcher_last: Option<Instant>,
    mission_open: bool,
}

impl UiMotionState {
    /// Shake-to-peek: other windows fade out while the shaken window is held.
    pub fn set_peek(&mut self, on: bool) {
        if on {
            self.peek_hides = self
                .peek_others
                .iter()
                .filter_map(|k| BloomHide::new(*k as isize))
                .collect();
        } else {
            self.peek_hides.clear();
        }
    }

    /// Called every UI tick: commits the switcher once the user pauses.
    pub fn tick(&mut self, windows: &GpuiSessionWindows, cx: &mut gpui::App) {
        if let Some(t) = self.switcher_last {
            if t.elapsed() > Duration::from_millis(700) {
                self.switcher_last = None;
                let keys = std::mem::take(&mut self.switcher_keys);
                with_motion(windows, cx, |o| {
                    if let Some(i) = o.switcher_commit() {
                        if let Some(k) = keys.get(i) {
                            unsafe {
                                let _ = SetForegroundWindow(HWND(*k as isize as *mut _));
                            }
                        }
                    }
                });
            }
        }
    }
}

/// Play one bridge message on the overlay.
pub fn apply(
    msg: UiMotion,
    state: &mut UiMotionState,
    windows: &GpuiSessionWindows,
    cx: &mut gpui::App,
) {
    match msg {
        UiMotion::Open {
            key,
            app,
            rect,
            px,
            hide,
        } => {
            let anchor = motion_anchor(windows, cx, &app);
            let s = snap(px);
            with_motion(windows, cx, move |o| {
                o.open(key, s, rect, anchor, Some(Box::new(move |_| drop(hide))));
            });
        }
        UiMotion::Restore {
            key,
            app,
            rect,
            px,
            hide,
        } => {
            let anchor = motion_anchor(windows, cx, &app);
            let s = snap(px);
            with_motion(windows, cx, move |o| {
                o.restore(key, s, rect, anchor, Some(Box::new(move |_| drop(hide))));
            });
        }
        UiMotion::Close { key, rect, px } => {
            let s = snap(px);
            with_motion(windows, cx, move |o| {
                o.close(key, s, rect, None);
            });
        }
        UiMotion::Minimize { key, app, rect, px } => {
            let anchor = motion_anchor(windows, cx, &app);
            let s = snap(px);
            with_motion(windows, cx, move |o| {
                o.minimize(key, s, rect, anchor, None);
            });
        }
        UiMotion::Zoom {
            key,
            from,
            to,
            px,
            hide,
        } => {
            let s = snap(px);
            with_motion(windows, cx, move |o| {
                o.grow(key, s, to, from, Some(Box::new(move |_| drop(hide))));
            });
        }
        UiMotion::Ripple { rect } => with_motion(windows, cx, |o| o.focus_ripple(rect)),
        UiMotion::DragBegin {
            key,
            cursor,
            others,
        } => {
            state.drag_key = Some(key);
            state.peek_others = others;
            with_motion(windows, cx, |o| o.drag_begin(None, cursor));
        }
        UiMotion::DragMove { cursor, rect } => {
            with_motion(windows, cx, |o| {
                o.drag_move(cursor, rect, 1.0 / 120.0);
            });
        }
        UiMotion::DragEnd { key, may_snap } => {
            state.set_peek(false);
            let mut target = None;
            with_motion(windows, cx, |o| target = o.drag_end(None));
            // The snap rect was landing on a screen edge and leaving the
            // window there. The real window stays where Windows put it.
            let _ = (may_snap, target, key);
        }
        UiMotion::Switcher { cards } => {
            if cards.is_empty() {
                return;
            }
            let open = state.switcher_last.is_some();
            state.switcher_last = Some(Instant::now());
            if open {
                with_motion(windows, cx, |o| o.switcher_step(1));
            } else {
                state.switcher_keys = cards.iter().map(|c| c.0).collect();
                let cards: Vec<(Arc<Snapshot>, String)> =
                    cards.into_iter().map(|(_, t, px)| (snap(px), t)).collect();
                let start = 1.min(cards.len() - 1);
                with_motion(windows, cx, move |o| o.switcher_show(cards, start));
            }
        }
        UiMotion::MissionControl { cards } => {
            state.mission_open = !state.mission_open;
            if !state.mission_open {
                with_motion(windows, cx, |o| o.mission_hide());
                return;
            }
            let n = cards.len().max(1);
            let cols = (n as f32).sqrt().ceil() as usize;
            let rows = n.div_ceil(cols);
            let work = windows.dock_rect;
            let screen_w = work.x * 2.0 + work.w;
            let top = windows.island_rect.bottom() + 40.0;
            let avail_h = (work.y - top - 40.0).max(200.0);
            let tile_w = (screen_w - 80.0) / cols as f32;
            let tile_h = avail_h / rows as f32;
            let items: Vec<(Arc<Snapshot>, Rect, Rect)> = cards
                .into_iter()
                .enumerate()
                .map(|(i, (_, rect, px))| {
                    let (c, r) = (i % cols, i / cols);
                    let scale = ((tile_w - 32.0) / rect.w)
                        .min((tile_h - 32.0) / rect.h)
                        .min(1.0);
                    let (w, h) = (rect.w * scale, rect.h * scale);
                    let cx0 = 40.0 + tile_w * (c as f32 + 0.5);
                    let cy0 = top + tile_h * (r as f32 + 0.5);
                    (
                        snap(px),
                        rect,
                        Rect::new(cx0 - w * 0.5, cy0 - h * 0.5, w, h),
                    )
                })
                .collect();
            with_motion(windows, cx, move |o| o.mission_show(items));
        }
    }
}

/// Convert a WM rect for logging and tests.
pub fn rect_of(r: RECT) -> Rect {
    Rect::new(
        r.left as f32,
        r.top as f32,
        (r.right - r.left) as f32,
        (r.bottom - r.top) as f32,
    )
}
