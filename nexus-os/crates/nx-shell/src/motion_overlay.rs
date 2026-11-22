//! The Nexus motion overlay: a transparent, click-through GPUI surface that
//! paints window effects from snapshots.
//!
//! The overlay never owns the real window. The host captures a snapshot
//! (`PrintWindow` on Windows, or a GPUI window of our own), hides or shows the
//! real window at the right moment, and the overlay animates pixels between.
//! When an effect settles the host's `done` callback runs, so the real
//! window is handed back on the same frame the snapshot disappears.
//!
//! All effect math lives in [`nx_ui::motion`]; this file only paints it.

use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    canvas, div, fill, hsla, point, px, size, AnyElement, App, BorderStyle, Bounds, BoxShadow,
    ContentMask, Context, Corners, Hsla, Pixels, Render, RenderImage, Window,
};
use nx_ui::motion::{
    self, CardPose, Command, DragMotion, DustField, EffectKind, MotionPrefs, Phase, ShakeDetector,
    SnapZone, WindowMotion,
};
use nx_ui::spring::Spring;
use nx_ui::Rect;

/// Horizontal bands per snapshot for the ink genie.
pub const BANDS: usize = 48;
/// Vertical slices per snapshot for the tilted switcher cards.
pub const SLICES: usize = 24;
/// Overlap between handing the real window back and removing the snapshot.
pub const HANDOFF_SECS: f32 = 0.12;
/// Window corner radius used by every effect, matching the shell chrome.
pub const CORNER: f32 = 14.0;

/// Captured pixels of one window, pre-cut for the effects.
pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    bgra: Arc<Vec<u8>>,
    image: Arc<RenderImage>,
    bands: Vec<Arc<RenderImage>>,
    slices: Vec<Arc<RenderImage>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

fn frame_from_bgra(w: u32, h: u32, bytes: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = image::RgbaImage::from_raw(w, h, bytes)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

impl Snapshot {
    /// Build from tightly packed BGRA rows (what GDI and GPUI both use).
    pub fn from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 || bgra.len() != (width * height * 4) as usize {
            return None;
        }
        let stride = (width * 4) as usize;
        let row = |y: u32| &bgra[y as usize * stride..(y as usize + 1) * stride];
        let mut bands = Vec::with_capacity(BANDS);
        for i in 0..BANDS as u32 {
            let y0 = (i * height) / BANDS as u32;
            let y1 = (((i + 1) * height) / BANDS as u32).max(y0 + 1).min(height);
            let mut bytes = Vec::with_capacity(stride * (y1 - y0) as usize);
            for y in y0..y1 {
                bytes.extend_from_slice(row(y));
            }
            bands.push(frame_from_bgra(width, y1 - y0, bytes)?);
        }
        let mut slices = Vec::with_capacity(SLICES);
        for i in 0..SLICES as u32 {
            let x0 = (i * width) / SLICES as u32;
            let x1 = (((i + 1) * width) / SLICES as u32).max(x0 + 1).min(width);
            let mut bytes = Vec::with_capacity(((x1 - x0) * 4 * height) as usize);
            for y in 0..height {
                let r = row(y);
                bytes.extend_from_slice(&r[(x0 * 4) as usize..(x1 * 4) as usize]);
            }
            slices.push(frame_from_bgra(x1 - x0, height, bytes)?);
        }
        let image = frame_from_bgra(width, height, bgra.clone())?;
        Some(Self {
            width,
            height,
            bgra: Arc::new(bgra),
            image,
            bands,
            slices,
        })
    }

    /// Build from RGBA rows (ImageMagick `rgba:`, PNG decoders).
    pub fn from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Option<Self> {
        for px in rgba.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        Self::from_bgra(width, height, rgba)
    }

    /// A monochrome window silhouette for when capture is unavailable
    /// (protected content, elevated windows, a window already destroyed).
    pub fn placeholder(width: u32, height: u32) -> Self {
        let (w, h) = (width.max(8), height.max(8));
        let mut bytes = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let title = y < 38.min(h / 4);
                let bar = !title && (y / 26) % 3 == 1 && x > 24 && x < w * 3 / 5;
                let v: u8 = if title {
                    44
                } else if bar {
                    58
                } else {
                    22
                };
                let i = ((y * w + x) * 4) as usize;
                bytes[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        Self::from_bgra(w, h, bytes).expect("placeholder dimensions are valid")
    }

    /// Mean luminance of each cell of a `cols x rows` grid, 0..1.
    pub fn lum_grid(&self, cols: usize, rows: usize) -> Vec<f32> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let mut out = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                // Sample a 2x2 pattern inside the cell: cheap and stable.
                let mut acc = 0.0;
                for (fx, fy) in [(0.3, 0.3), (0.7, 0.3), (0.3, 0.7), (0.7, 0.7)] {
                    let x = (((c as f32 + fx) / cols as f32) * self.width as f32) as u32;
                    let y = (((r as f32 + fy) / rows as f32) * self.height as f32) as u32;
                    let i = ((y.min(self.height - 1) * self.width + x.min(self.width - 1)) * 4)
                        as usize;
                    let (b, g, rr) = (
                        self.bgra[i] as f32,
                        self.bgra[i + 1] as f32,
                        self.bgra[i + 2] as f32,
                    );
                    acc += (0.0722 * b + 0.7152 * g + 0.2126 * rr) / 255.0;
                }
                out.push(acc / 4.0);
            }
        }
        out
    }

    fn all_images(&self) -> impl Iterator<Item = &Arc<RenderImage>> {
        std::iter::once(&self.image)
            .chain(self.bands.iter())
            .chain(self.slices.iter())
    }
}

/// Host callback run when an effect settles.
pub type Done = Box<dyn FnOnce(&mut App) + 'static>;

struct WinFx {
    key: u64,
    snap: Arc<Snapshot>,
    rect: Rect,
    anchor: Rect,
    motion: WindowMotion,
    dust: Option<DustField>,
    done: Option<Done>,
    last_phase: Phase,
    /// After an open/restore hands the real window back, keep painting the
    /// final frame briefly so the swap never flashes the desktop.
    linger: Option<f32>,
}

struct Ripple {
    rect: Rect,
    age: f32,
}

struct Trail {
    rect: Rect,
    age: f32,
}

struct DragFx {
    proxy: Option<(Arc<Snapshot>, Rect)>,
    drag: DragMotion,
    cursor: (f32, f32),
    zone: Option<SnapZone>,
    shake: ShakeDetector,
    /// Snap glide after drop: from, to, progress spring.
    glide: Option<(Rect, Rect, Spring)>,
    released: bool,
    done: Option<Done>,
    trail_clock: f32,
}

struct SwitcherFx {
    cards: Vec<(Arc<Snapshot>, String)>,
    focus: Spring,
    open: Spring,
    closing: bool,
}

struct MissionFx {
    items: Vec<(Arc<Snapshot>, Rect, Rect)>,
    /// Window keys (HWNDs) for the tiles, same order as `items`.
    keys: Vec<u64>,
    p: Spring,
    selected: Option<usize>,
}

/// Events the overlay raises for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    /// The user shook a dragged window: peek at the desktop (true) or stop.
    Peek(bool),
}

type EventSink = Box<dyn Fn(OverlayEvent, &mut App) + 'static>;

pub struct MotionOverlayView {
    /// Screen position of the overlay window's top-left corner.
    pub origin: (f32, f32),
    /// Work area in screen coordinates, used for snap zones.
    pub work: Rect,
    pub prefs: MotionPrefs,
    windows: Vec<WinFx>,
    ripples: Vec<Ripple>,
    trails: Vec<Trail>,
    drag: Option<DragFx>,
    switcher: Option<SwitcherFx>,
    mission: Option<MissionFx>,
    peeking: bool,
    last: Option<Instant>,
    retired: Vec<Arc<Snapshot>>,
    pending_done: Vec<Done>,
    events: Option<EventSink>,
    pending_events: Vec<OverlayEvent>,
    os_visible: Option<bool>,
    /// Mission Control currently takes clicks and keys.
    mission_input: bool,
    focus: Option<gpui::FocusHandle>,
}

fn progress_spring(start: f32, cfg: nx_ui::SpringConfig) -> Spring {
    let mut s = Spring::new(start, cfg);
    s.config.rest_pos = 0.002;
    s.config.rest_vel = 0.02;
    s
}

impl MotionOverlayView {
    pub fn new(origin: (f32, f32), work: Rect, prefs: MotionPrefs) -> Self {
        Self {
            origin,
            work,
            prefs,
            windows: Vec::new(),
            ripples: Vec::new(),
            trails: Vec::new(),
            drag: None,
            switcher: None,
            mission: None,
            peeking: false,
            last: None,
            retired: Vec::new(),
            pending_done: Vec::new(),
            events: None,
            pending_events: Vec::new(),
            os_visible: None,
            mission_input: false,
            focus: None,
        }
    }

    pub fn on_event(&mut self, sink: impl Fn(OverlayEvent, &mut App) + 'static) {
        self.events = Some(Box::new(sink));
    }

    /// True while any effect is painting.
    pub fn is_animating(&self) -> bool {
        !self.windows.is_empty()
            || !self.ripples.is_empty()
            || !self.trails.is_empty()
            || self.drag.is_some()
            || self.switcher.is_some()
            || self.mission.is_some()
    }

    fn entry(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        anchor: Rect,
        shown: bool,
    ) -> &mut WinFx {
        if let Some(i) = self.windows.iter().position(|w| w.key == key) {
            let w = &mut self.windows[i];
            w.rect = rect;
            w.anchor = anchor;
            if !Arc::ptr_eq(&w.snap, &snap) {
                let old = std::mem::replace(&mut w.snap, snap);
                self.retired.push(old);
            }
            return &mut self.windows[i];
        }
        let motion = if shown {
            WindowMotion::shown(self.prefs)
        } else {
            WindowMotion::hidden(self.prefs)
        };
        let last_phase = motion.phase();
        self.windows.push(WinFx {
            key,
            snap,
            rect,
            anchor,
            motion,
            dust: None,
            done: None,
            last_phase,
            linger: None,
        });
        self.windows.last_mut().expect("just pushed")
    }

    #[allow(clippy::too_many_arguments)]
    fn command(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        anchor: Rect,
        shown: bool,
        cmd: Command,
        done: Option<Done>,
    ) -> bool {
        let prefs = self.prefs;
        let fx = self.entry(key, snap, rect, anchor, shown);
        let accepted = fx.motion.apply(cmd);
        if cmd == Command::Close && accepted {
            let (cols, rows) = motion::dust_grid(rect, 9.0, 2600);
            let lum = fx.snap.lum_grid(cols, rows);
            fx.dust = Some(DustField::new(rect, cols, rows, &lum, prefs));
        }
        if let Some(prev) = fx.done.take() {
            // An interrupted effect still hands its window back.
            self.pending_done.push(prev);
        }
        let fx = self
            .windows
            .iter_mut()
            .find(|w| w.key == key)
            .expect("entry exists");
        fx.done = done;
        if !accepted {
            if let Some(d) = fx.done.take() {
                self.pending_done.push(d);
            }
        }
        accepted
    }

    /// Grow from `anchor` to `rect`, even if this window was already on screen.
    /// Used for maximize and for restore-from-maximize.
    pub fn grow(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        anchor: Rect,
        done: Option<Done>,
    ) -> bool {
        if let Some(w) = self.windows.iter_mut().find(|w| w.key == key) {
            w.motion = WindowMotion::hidden(self.prefs);
            w.linger = None;
        }
        self.command(key, snap, rect, anchor, false, Command::Open, done)
    }

    /// Open: bloom from `anchor` (dock icon or island) to `rect`.
    pub fn open(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        anchor: Rect,
        done: Option<Done>,
    ) -> bool {
        self.command(key, snap, rect, anchor, false, Command::Open, done)
    }

    /// Close: dissolve into dust.
    pub fn close(&mut self, key: u64, snap: Arc<Snapshot>, rect: Rect, done: Option<Done>) -> bool {
        self.command(key, snap, rect, rect, true, Command::Close, done)
    }

    /// Minimize: pour into `icon` as liquid ink.
    pub fn minimize(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        icon: Rect,
        done: Option<Done>,
    ) -> bool {
        self.command(key, snap, rect, icon, true, Command::Minimize, done)
    }

    /// Restore: the ink pours back out of `icon`.
    pub fn restore(
        &mut self,
        key: u64,
        snap: Arc<Snapshot>,
        rect: Rect,
        icon: Rect,
        done: Option<Done>,
    ) -> bool {
        let prefs = self.prefs;
        if !self.windows.iter().any(|w| w.key == key) {
            let mut m = WindowMotion::shown(prefs);
            m.apply(Command::Minimize);
            // Start fully poured in.
            for _ in 0..240 {
                m.step(1.0 / 60.0);
            }
            let last_phase = m.phase();
            self.windows.push(WinFx {
                key,
                snap: snap.clone(),
                rect,
                anchor: icon,
                motion: m,
                dust: None,
                done: None,
                last_phase,
                linger: None,
            });
        }
        self.command(key, snap, rect, icon, true, Command::Restore, done)
    }

    /// Nexus-only: a thin ring radiates out of a window when it takes focus.
    pub fn focus_ripple(&mut self, rect: Rect) {
        if !self.prefs.reduce_motion {
            self.ripples.push(Ripple { rect, age: 0.0 });
        }
    }

    /// Start a drag. With `proxy` the overlay draws the lifted window itself
    /// (Nexus windows); without it, only zone previews and edge glow are drawn
    /// and the WM moves the real window (foreign windows).
    pub fn drag_begin(&mut self, proxy: Option<(Arc<Snapshot>, Rect)>, cursor: (f32, f32)) {
        let mut drag = DragMotion::new();
        drag.grab();
        self.drag = Some(DragFx {
            proxy,
            drag,
            cursor,
            zone: None,
            shake: ShakeDetector::default(),
            glide: None,
            released: false,
            done: None,
            trail_clock: 0.0,
        });
    }

    /// Pointer moved while dragging. `rect` is the window's new rect.
    pub fn drag_move(
        &mut self,
        cursor: (f32, f32),
        rect: Option<Rect>,
        dt: f32,
    ) -> Option<SnapZone> {
        let work = self.work;
        let drag = self.drag.as_mut()?;
        drag.cursor = cursor;
        if let (Some((_, r)), Some(nr)) = (drag.proxy.as_mut(), rect) {
            *r = nr;
        }
        let zone = motion::snap_zone(cursor.0, cursor.1, work, 14.0);
        drag.zone = zone;
        drag.drag.hover(zone);
        if drag.shake.feed(cursor.0, dt) {
            self.peeking = !self.peeking;
            self.pending_events.push(OverlayEvent::Peek(self.peeking));
        }
        zone
    }

    /// Released. Returns the snap target rect when dropped in a zone; the
    /// proxy glides there with a magnetic spring and wobbles on landing, then
    /// `done` runs.
    pub fn drag_end(&mut self, done: Option<Done>) -> Option<Rect> {
        let work = self.work;
        let peeking = std::mem::replace(&mut self.peeking, false);
        if peeking {
            self.pending_events.push(OverlayEvent::Peek(false));
        }
        let drag = self.drag.as_mut()?;
        drag.released = true;
        let zone = drag.drag.drop_window();
        let target = zone.map(|z| motion::zone_rect(z, work, 8.0));
        if let (Some(t), Some((_, r))) = (target, drag.proxy.as_ref()) {
            drag.glide = Some((*r, t, progress_spring(0.0, motion::presets::snap())));
            drag.glide.as_mut().expect("set").2.set_target(1.0);
        }
        drag.done = done;
        target
    }

    /// Current rect of the lifted proxy window, if a proxy drag is active.
    pub fn drag_proxy_rect(&self) -> Option<Rect> {
        self.drag.as_ref()?.proxy.as_ref().map(|p| p.1)
    }

    /// Show the 3D card switcher with `cards`, focused on `start`.
    pub fn switcher_show(&mut self, cards: Vec<(Arc<Snapshot>, String)>, start: usize) {
        if cards.is_empty() {
            return;
        }
        let start = start.min(cards.len() - 1) as f32;
        let mut focus = Spring::new(start, motion::presets::card());
        focus.config.rest_pos = 0.002;
        let mut open = progress_spring(0.0, motion::presets::card());
        open.set_target(1.0);
        self.switcher = Some(SwitcherFx {
            cards,
            focus,
            open,
            closing: false,
        });
    }

    /// Move the selection by `delta`, wrapping.
    pub fn switcher_step(&mut self, delta: i32) {
        if let Some(s) = &mut self.switcher {
            let n = s.cards.len() as i32;
            let cur = s.focus.target.round() as i32;
            let next = (cur + delta).rem_euclid(n.max(1));
            s.focus.set_target(next as f32);
        }
    }

    /// Commit and dismiss; returns the chosen card index.
    pub fn switcher_commit(&mut self) -> Option<usize> {
        let s = self.switcher.as_mut()?;
        s.closing = true;
        s.open.set_target(0.0);
        Some(s.focus.target.round().max(0.0) as usize)
    }

    /// Mission Control: fan windows from their rects into `to` tiles.
    pub fn mission_show(&mut self, items: Vec<(Arc<Snapshot>, Rect, Rect)>) {
        self.mission_show_keys(items, Vec::new());
    }

    /// Mission Control with window keys, so a tile click or Enter can focus
    /// that window.
    pub fn mission_show_keys(&mut self, items: Vec<(Arc<Snapshot>, Rect, Rect)>, keys: Vec<u64>) {
        let mut p = progress_spring(0.0, motion::presets::card());
        p.set_target(1.0);
        let selected = (!items.is_empty()).then_some(0);
        self.mission = Some(MissionFx {
            items,
            keys,
            p,
            selected,
        });
    }

    /// True while Mission Control is open or opening (not while closing).
    pub fn mission_open(&self) -> bool {
        self.mission.as_ref().is_some_and(|m| m.p.target > 0.5)
    }

    /// Close Mission Control and bring tile `pick` (if any) to the front.
    fn mission_pick(&mut self, pick: Option<usize>) {
        let key = pick.and_then(|i| self.mission.as_ref()?.keys.get(i).copied());
        self.mission_hide();
        if let Some(key) = key {
            crate::dock::focus_hwnd(key as isize);
        }
    }

    /// Arrow-key move across the tile grid.
    fn mission_move(&mut self, dx: i32, dy: i32) {
        let Some(m) = &mut self.mission else {
            return;
        };
        let n = m.items.len();
        if n == 0 {
            return;
        }
        let cols = ((n as f32).sqrt().ceil() as i32).max(1);
        let cur = m.selected.unwrap_or(0) as i32;
        let next = (cur + dx + dy * cols).clamp(0, n as i32 - 1);
        m.selected = Some(next as usize);
    }

    /// True while something moves (as opposed to a settled Mission Control).
    fn is_moving(&self) -> bool {
        !self.windows.is_empty()
            || !self.ripples.is_empty()
            || !self.trails.is_empty()
            || self.drag.is_some()
            || self.switcher.is_some()
            || self.mission.as_ref().is_some_and(|m| !m.p.settled())
    }

    pub fn mission_hide(&mut self) {
        if let Some(m) = &mut self.mission {
            m.p.set_target(0.0);
        }
    }

    fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        for w in &mut self.windows {
            w.motion.step(dt);
            let phase = w.motion.phase();
            if phase != w.last_phase {
                w.last_phase = phase;
                if matches!(phase, Phase::Shown | Phase::Minimized | Phase::Closed) {
                    if let Some(d) = w.done.take() {
                        self.pending_done.push(d);
                    }
                }
            }
        }
        let mut keep = Vec::with_capacity(self.windows.len());
        for mut w in self.windows.drain(..) {
            if w.motion.phase() == Phase::Shown && w.motion.frame().kind == EffectKind::None {
                let left = w.linger.get_or_insert(HANDOFF_SECS);
                *left -= dt;
                if *left > 0.0 {
                    keep.push(w);
                    continue;
                }
            }
            if w.motion.frame().kind == EffectKind::None {
                if let Some(d) = w.done {
                    self.pending_done.push(d);
                }
                self.retired.push(w.snap);
            } else {
                keep.push(w);
            }
        }
        self.windows = keep;

        let prefs = self.prefs;
        for r in &mut self.ripples {
            r.age += dt;
        }
        self.ripples
            .retain(|r| motion::focus_ripple(r.rect, r.age, prefs).is_some());
        for t in &mut self.trails {
            t.age += dt;
        }
        self.trails.retain(|t| t.age < 0.22);

        let mut finished_drag = false;
        if let Some(d) = &mut self.drag {
            d.drag.step(dt);
            if let Some((from, to, s)) = &mut d.glide {
                s.step(dt);
                if let Some((_, r)) = d.proxy.as_mut() {
                    *r = motion::lerp_rect(*from, *to, s.position);
                }
            } else if !d.released && !prefs.reduce_motion {
                // Nexus-only: a glass afterimage trails the lifted window.
                d.trail_clock += dt;
                if d.trail_clock > 0.03 {
                    d.trail_clock = 0.0;
                    if let Some((_, r)) = &d.proxy {
                        self.trails.push(Trail { rect: *r, age: 0.0 });
                    }
                }
            }
            let glide_done = d.glide.as_ref().is_none_or(|g| g.2.settled());
            if d.released && glide_done && !d.drag.animating() {
                finished_drag = true;
            }
        }
        if finished_drag {
            if let Some(d) = self.drag.take() {
                if let Some(done) = d.done {
                    self.pending_done.push(done);
                }
                if let Some((snap, _)) = d.proxy {
                    self.retired.push(snap);
                }
            }
        }

        if let Some(s) = &mut self.switcher {
            s.focus.step(dt);
            s.open.step(dt);
            if s.closing && s.open.settled() {
                if let Some(s) = self.switcher.take() {
                    self.retired.extend(s.cards.into_iter().map(|c| c.0));
                }
            }
        }
        if let Some(m) = &mut self.mission {
            m.p.step(dt);
            if m.p.target == 0.0 && m.p.settled() {
                if let Some(m) = self.mission.take() {
                    self.retired.extend(m.items.into_iter().map(|i| i.0));
                }
            }
        }
    }

    fn local(&self, r: Rect) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(r.x - self.origin.0), px(r.y - self.origin.1)),
            size: size(px(r.w.max(0.0)), px(r.h.max(0.0))),
        }
    }

    fn window_layer(&self, w: &WinFx) -> Option<AnyElement> {
        let frame = w.motion.frame();
        let snap = w.snap.clone();
        let rect = self.local(w.rect);
        let origin = self.origin;
        match frame.kind {
            EffectKind::None if w.linger.is_some() => Some(
                layer(1.0, move |win| {
                    let _ = win.paint_image(rect, corners(CORNER), snap.image.clone(), 0, false);
                })
                .into_any_element(),
            ),
            EffectKind::None => None,
            EffectKind::Fade => Some(
                layer(frame.visibility.clamp(0.0, 1.0), move |win| {
                    let _ = win.paint_image(rect, corners(CORNER), snap.image.clone(), 0, false);
                })
                .into_any_element(),
            ),
            EffectKind::Bloom => {
                let b = motion::bloom(w.anchor, w.rect, frame.visibility, CORNER);
                let bounds = to_bounds(b.rect, origin);
                Some(
                    layer(b.opacity, move |win| {
                        win.paint_shadows(
                            bounds,
                            corners(b.corner_radius),
                            &[
                                BoxShadow {
                                    color: hsla(0.0, 0.0, 0.0, 0.45 * b.shadow),
                                    offset: point(px(0.0), px(18.0 * b.shadow)),
                                    blur_radius: px(48.0),
                                    spread_radius: px(0.0),
                                },
                                // The bloom: a white halo that burns off as it lands.
                                BoxShadow {
                                    color: hsla(0.0, 0.0, 1.0, 0.32 * (1.0 - b.shadow.min(1.0))),
                                    offset: point(px(0.0), px(0.0)),
                                    blur_radius: px(18.0 + b.blur * 2.0),
                                    spread_radius: px(b.blur * 0.5),
                                },
                            ],
                        );
                        let _ = win.paint_image(
                            bounds,
                            corners(b.corner_radius),
                            snap.image.clone(),
                            0,
                            false,
                        );
                        // Haze over the image while it is still "out of focus".
                        win.paint_quad(
                            fill(bounds, hsla(0.0, 0.0, 1.0, 0.18 * (b.blur / 22.0)))
                                .corner_radii(corners(b.corner_radius)),
                        );
                    })
                    .into_any_element(),
                )
            }
            EffectKind::Ink => {
                let t = 1.0 - frame.visibility.clamp(0.0, 1.0);
                let strips = motion::ink_genie(w.rect, w.anchor, t, BANDS);
                let alpha = strips.first().map(|s| s.alpha).unwrap_or(1.0);
                Some(
                    layer(alpha, move |win| {
                        for (i, s) in strips.iter().enumerate() {
                            let b = to_bounds(s.dst, origin);
                            if let Some(img) = snap.bands.get(i) {
                                let _ = win.paint_image(b, corners(0.0), img.clone(), 0, false);
                            }
                            win.paint_quad(fill(b, hsla(0.0, 0.0, 0.0, s.ink * 0.7)));
                        }
                    })
                    .into_any_element(),
                )
            }
            EffectKind::Dust => {
                let field = w.dust.as_ref()?;
                let age = frame.dust_age;
                let edge = field.intact_from(age);
                let specks = field.specks(age);
                let full = w.rect;
                Some(
                    layer(1.0, move |win| {
                        let intact = Rect::new(
                            full.x,
                            full.y + full.h * edge,
                            full.w,
                            full.h * (1.0 - edge),
                        );
                        if intact.h > 0.5 {
                            let mask = ContentMask {
                                bounds: to_bounds(intact, origin),
                            };
                            let fb = to_bounds(full, origin);
                            win.with_content_mask(Some(mask), |win| {
                                let _ = win.paint_image(
                                    fb,
                                    corners(CORNER),
                                    snap.image.clone(),
                                    0,
                                    false,
                                );
                            });
                        }
                        for s in &specks {
                            let b = to_bounds(s.rect, origin);
                            win.paint_quad(
                                fill(b, hsla(0.0, 0.0, 0.15 + s.lum * 0.85, s.alpha))
                                    .corner_radii(corners(s.rect.w * 0.3)),
                            );
                        }
                    })
                    .into_any_element(),
                )
            }
        }
    }

    fn drag_layer(&self, d: &DragFx) -> AnyElement {
        let origin = self.origin;
        let work = self.work;
        let preview = d.drag.preview_progress();
        let zone_rect = d.zone.map(|z| motion::zone_rect(z, work, 8.0));
        let glow = motion::edge_glow(d.cursor.0, d.cursor.1, work, 14.0);
        let proxy = d.proxy.clone();
        let scale = d.drag.scale();
        let blur = d.drag.shadow_blur();
        let off = d.drag.shadow_offset();
        let cursor = d.cursor;
        let trails: Vec<(Rect, f32)> = self.trails.iter().map(|t| (t.rect, t.age)).collect();
        layer(1.0, move |win| {
            if let Some(zr) = zone_rect {
                // Magnetic preview: the target zone blooms in as frosted glass.
                let r = motion::scale_about_center(zr, motion::lerp(0.92, 1.0, preview));
                let b = to_bounds(r, origin);
                win.paint_quad(
                    fill(b, hsla(0.0, 0.0, 1.0, 0.08 * preview))
                        .corner_radii(corners(CORNER))
                        .border_widths(px(1.5))
                        .border_color(hsla(0.0, 0.0, 1.0, 0.55 * preview)),
                );
            }
            if glow > 0.01 {
                // The edge being pulled toward lights up near the cursor.
                let (x, y) = cursor;
                let len = 420.0;
                let near_left = x - work.x < work.right() - x;
                let side_x = (x - work.x).min(work.right() - x);
                let edge = if (y - work.y) < side_x {
                    Rect::new(x - len * 0.5, work.y, len, 3.0)
                } else if near_left {
                    Rect::new(work.x, y - len * 0.5, 3.0, len)
                } else {
                    Rect::new(work.right() - 3.0, y - len * 0.5, 3.0, len)
                };
                let b = to_bounds(edge, origin);
                win.paint_shadows(
                    b,
                    corners(2.0),
                    &[BoxShadow {
                        color: hsla(0.0, 0.0, 1.0, 0.6 * glow),
                        offset: point(px(0.0), px(0.0)),
                        blur_radius: px(22.0),
                        spread_radius: px(2.0),
                    }],
                );
                win.paint_quad(fill(b, hsla(0.0, 0.0, 1.0, 0.9 * glow)).corner_radii(corners(2.0)));
            }
            for (r, age) in &trails {
                let a = (1.0 - age / 0.22).max(0.0) * 0.18;
                let b = to_bounds(*r, origin);
                win.paint_quad(
                    fill(b, hsla(0.0, 0.0, 1.0, a * 0.25))
                        .corner_radii(corners(CORNER))
                        .border_widths(px(1.0))
                        .border_color(hsla(0.0, 0.0, 1.0, a)),
                );
            }
            if let Some((snap, r)) = &proxy {
                let lifted = motion::scale_about_center(*r, scale);
                let b = to_bounds(lifted, origin);
                win.paint_shadows(
                    b,
                    corners(CORNER),
                    &[BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.55),
                        offset: point(px(0.0), px(off)),
                        blur_radius: px(blur),
                        spread_radius: px(0.0),
                    }],
                );
                let _ = win.paint_image(b, corners(CORNER), snap.image.clone(), 0, false);
            }
        })
        .into_any_element()
    }

    fn switcher_layer(&self, s: &SwitcherFx) -> AnyElement {
        let origin = self.origin;
        let screen = Rect::new(
            origin.0,
            origin.1,
            self.work.w,
            self.work.h + self.work.y - origin.1,
        );
        let open = s.open.position.clamp(0.0, 1.2);
        let card_w = (screen.w * 0.3).clamp(240.0, 640.0);
        let aspect = s
            .cards
            .first()
            .map(|c| c.0.height as f32 / c.0.width.max(1) as f32)
            .unwrap_or(0.66)
            .clamp(0.4, 1.2);
        let poses: Vec<CardPose> = motion::carousel(
            s.cards.len(),
            s.focus.position,
            open,
            screen,
            card_w,
            card_w * aspect,
        );
        let cards: Vec<Arc<Snapshot>> = s.cards.iter().map(|c| c.0.clone()).collect();
        let focus_idx = s.focus.position.round().max(0.0) as usize;
        let title = s
            .cards
            .get(focus_idx.min(s.cards.len().saturating_sub(1)))
            .map(|c| c.1.to_uppercase())
            .unwrap_or_default();
        let dim = 0.42 * open.min(1.0);
        let label_y = screen.y + screen.h * 0.48 + card_w * aspect * 0.5 + 34.0 - origin.1;
        div()
            .absolute()
            .inset_0()
            .child(layer(1.0, move |win| {
                let vp = win.viewport_size();
                win.paint_quad(fill(
                    Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: vp,
                    },
                    hsla(0.0, 0.0, 0.0, dim),
                ));
                for pose in &poses {
                    let Some(snap) = cards.get(pose.index) else {
                        continue;
                    };
                    let slices = motion::perspective_slices(pose.rect, pose.angle, SLICES, 1100.0);
                    let outline = to_bounds(pose.rect, origin);
                    win.paint_shadows(
                        outline,
                        corners(CORNER),
                        &[BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.5 * pose.alpha),
                            offset: point(px(0.0), px(24.0)),
                            blur_radius: px(40.0),
                            spread_radius: px(0.0),
                        }],
                    );
                    for (i, sl) in slices.iter().enumerate() {
                        let b = to_bounds(sl.dst, origin);
                        let radius = if i == 0 || i + 1 == slices.len() {
                            CORNER * 0.6
                        } else {
                            0.0
                        };
                        if let Some(img) = snap.slices.get(i) {
                            let _ = win.paint_image(b, corners(radius), img.clone(), 0, false);
                        }
                        let shade = sl.shade + (1.0 - pose.alpha) * 0.8;
                        win.paint_quad(fill(b, hsla(0.0, 0.0, 0.0, shade.clamp(0.0, 0.92))));
                    }
                }
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(label_y))
                    .flex()
                    .justify_center()
                    .opacity(open.min(1.0))
                    .child(
                        div()
                            .px(px(14.0))
                            .py(px(6.0))
                            .rounded(px(999.0))
                            .bg(hsla(0.0, 0.0, 1.0, 0.92))
                            .text_color(hsla(0.0, 0.0, 0.0, 1.0))
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(title),
                    ),
            )
            .into_any_element()
    }

    /// Click targets over the Mission Control tiles (plus the backdrop,
    /// which closes it), and the keyboard: arrows, Enter, Esc.
    fn mission_hits(&self, m: &MissionFx, cx: &mut Context<Self>) -> AnyElement {
        let origin = self.origin;
        let n = m.items.len();
        let selected = m.selected;
        let mut layer = div()
            .id("mission-hits")
            .absolute()
            .inset_0()
            .on_click(cx.listener(|this, _, _, cx| {
                this.mission_pick(None);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "escape" => this.mission_pick(None),
                    "left" | "arrowleft" => this.mission_move(-1, 0),
                    "right" | "arrowright" | "tab" => this.mission_move(1, 0),
                    "up" | "arrowup" => this.mission_move(0, -1),
                    "down" | "arrowdown" => this.mission_move(0, 1),
                    "enter" | "return" | "space" => {
                        let pick = this.mission.as_ref().and_then(|m| m.selected);
                        this.mission_pick(pick);
                    }
                    _ => {}
                }
                cx.notify();
            }));
        for (i, (_, from, to)) in m.items.iter().enumerate() {
            let (r, _) = motion::fan_out(*from, *to, i, n, 1.0);
            let on = selected == Some(i);
            layer = layer.child(
                div()
                    .id(("mission-tile", i))
                    .absolute()
                    .left(px(r.x - origin.0))
                    .top(px(r.y - origin.1))
                    .w(px(r.w.max(0.0)))
                    .h(px(r.h.max(0.0)))
                    .rounded(px(CORNER))
                    .border_2()
                    .border_color(if on {
                        hsla(0.0, 0.0, 1.0, 0.9)
                    } else {
                        hsla(0.0, 0.0, 1.0, 0.0)
                    })
                    .hover(|s| s.border_color(hsla(0.0, 0.0, 1.0, 0.6)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            if let Some(m) = &mut this.mission {
                                m.selected = Some(i);
                            }
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.mission_pick(Some(i));
                        cx.notify();
                    })),
            );
        }
        layer.into_any_element()
    }

    fn mission_layer(&self, m: &MissionFx) -> AnyElement {
        let origin = self.origin;
        let p = m.p.position;
        let items: Vec<(Arc<Snapshot>, Rect, Rect)> = m.items.clone();
        let n = items.len();
        layer(1.0, move |win| {
            let vp = win.viewport_size();
            win.paint_quad(fill(
                Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: vp,
                },
                hsla(0.0, 0.0, 0.0, 0.5 * p.clamp(0.0, 1.0)),
            ));
            for (i, (snap, from, to)) in items.iter().enumerate() {
                let (r, e) = motion::fan_out(*from, *to, i, n, p);
                let b = to_bounds(r, origin);
                win.paint_shadows(
                    b,
                    corners(CORNER),
                    &[BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.5),
                        offset: point(px(0.0), px(10.0 + 10.0 * e)),
                        blur_radius: px(30.0),
                        spread_radius: px(0.0),
                    }],
                );
                let _ = win.paint_image(b, corners(CORNER), snap.image.clone(), 0, false);
                win.paint_quad(
                    fill(b, hsla(0.0, 0.0, 0.0, 0.0))
                        .corner_radii(corners(CORNER))
                        .border_widths(px(1.0))
                        .border_color(hsla(0.0, 0.0, 1.0, 0.35 * e)),
                );
            }
        })
        .into_any_element()
    }

    fn ripple_layer(&self) -> Option<AnyElement> {
        let prefs = self.prefs;
        let rings: Vec<(Rect, f32)> = self
            .ripples
            .iter()
            .filter_map(|r| motion::focus_ripple(r.rect, r.age, prefs))
            .collect();
        if rings.is_empty() {
            return None;
        }
        let origin = self.origin;
        Some(
            layer(1.0, move |win| {
                for (r, a) in &rings {
                    let b = to_bounds(*r, origin);
                    win.paint_quad(
                        fill(b, hsla(0.0, 0.0, 1.0, 0.0))
                            .corner_radii(corners(CORNER + 10.0))
                            .border_widths(px(1.5))
                            .border_color(hsla(0.0, 0.0, 1.0, *a)),
                    );
                }
            })
            .into_any_element(),
        )
    }

    /// Show/hide the OS window to match whether any effect is running and
    /// keep frames coming while one is. Call after every state change from
    /// outside `render` (a hidden window gets no paint, so `render` alone
    /// can never bring it back).
    pub fn sync_window(&mut self, window: &mut Window) {
        let active = self.is_animating();
        self.sync_os_visibility(window, active);
        if active {
            window.refresh();
        }
    }

    fn sync_os_visibility(&mut self, window: &mut Window, active: bool) {
        if self.os_visible == Some(active) {
            return;
        }
        self.os_visible = Some(active);
        #[cfg(windows)]
        crate::capture::set_overlay_visible(window, active);
        let _ = (window, active);
    }
}

fn corners(r: f32) -> Corners<Pixels> {
    Corners::all(px(r.max(0.0)))
}

fn to_bounds(r: Rect, origin: (f32, f32)) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(r.x - origin.0), px(r.y - origin.1)),
        size: size(px(r.w.max(0.0)), px(r.h.max(0.0))),
    }
}

/// One full-overlay paint layer at `opacity`.
fn layer(opacity: f32, paint: impl FnOnce(&mut Window) + 'static) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .opacity(opacity.clamp(0.0, 1.0))
        .child(
            canvas(|_, _, _| {}, move |_, _, window, _| paint(window))
                .absolute()
                .size_full(),
        )
}

impl Render for MotionOverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let dt = self
            .last
            .map(|l| (now - l).as_secs_f32())
            .unwrap_or(1.0 / 120.0);
        self.last = Some(now);
        self.step(dt);

        // Free GPU atlas tiles of snapshots no effect uses any more.
        for snap in self.retired.drain(..) {
            if Arc::strong_count(&snap) == 1 {
                for img in snap.all_images() {
                    let _ = window.drop_image(img.clone());
                }
            }
        }
        let app: &mut App = cx;
        for done in self.pending_done.drain(..) {
            app.defer(done);
        }
        if let Some(sink) = self.events.take() {
            for ev in self.pending_events.drain(..) {
                sink(ev, app);
            }
            self.events = Some(sink);
        }

        let active = self.is_animating();
        self.sync_os_visibility(window, active);
        if self.is_moving() {
            window.request_animation_frame();
        } else {
            // A settled Mission Control repaints only on input.
            self.last = None;
        }
        let want_input = self.mission_open();
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if want_input != self.mission_input {
            self.mission_input = want_input;
            #[cfg(windows)]
            if let Some(raw) = crate::capture::gpui_hwnd(window) {
                crate::capture::set_click_through(raw, !want_input);
            }
            if want_input {
                window.activate_window();
                window.focus(&focus);
            }
        }

        let mut root = div()
            .id("nexus-motion-overlay")
            .size_full()
            .relative()
            .track_focus(&focus);
        if let Some(m) = &self.mission {
            root = root.child(self.mission_layer(m));
            if self.mission_input && m.p.position > 0.9 {
                root = root.child(self.mission_hits(m, cx));
            }
        }
        for w in &self.windows {
            if let Some(el) = self.window_layer(w) {
                root = root.child(el);
            }
        }
        if let Some(el) = self.ripple_layer() {
            root = root.child(el);
        }
        if let Some(d) = &self.drag {
            root = root.child(self.drag_layer(d));
        }
        if let Some(s) = &self.switcher {
            root = root.child(self.switcher_layer(s));
        }
        let _ = (BorderStyle::Solid, Hsla::default());
        root
    }
}

/// Screen rect of dock icon `index` for a dock window at `dock` with
/// `count` icons laid out the way [`crate::DockView`] centers them.
pub fn dock_icon_rect(dock: Rect, count: usize, index: usize, mag: crate::Magnification) -> Rect {
    let count = count.max(1);
    let size = mag.rest;
    let gap = mag.gap;
    let pad = mag.padding + 4.0;
    // One separator before the trash adds a hairline and a gap.
    let row_w = count as f32 * size + (count as f32 - 1.0) * gap + gap + 1.0;
    let x0 = dock.x + (dock.w - row_w) * 0.5;
    let x = x0 + index.min(count - 1) as f32 * (size + gap);
    let y = dock.bottom() - pad - 8.0 - size;
    Rect::new(x, y, size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    }

    #[test]
    fn snapshot_cuts_bands_and_slices() {
        let s = Snapshot::from_bgra(97, 61, checker(97, 61)).unwrap();
        assert_eq!(s.bands.len(), BANDS);
        assert_eq!(s.slices.len(), SLICES);
        let band_rows: u32 = s.bands.iter().map(|b| b.size(0).height.0 as u32).sum();
        assert!(band_rows >= 61, "bands cover every row");
        let slice_cols: u32 = s.slices.iter().map(|b| b.size(0).width.0 as u32).sum();
        assert!(slice_cols >= 97, "slices cover every column");
    }

    #[test]
    fn snapshot_rejects_bad_buffers() {
        assert!(Snapshot::from_bgra(10, 10, vec![0; 12]).is_none());
        assert!(Snapshot::from_bgra(0, 10, vec![]).is_none());
    }

    #[test]
    fn lum_grid_reads_brightness() {
        let white = Snapshot::from_rgba(8, 8, vec![255; 256]).unwrap();
        assert!(white
            .lum_grid(4, 4)
            .iter()
            .all(|l| (*l - 1.0).abs() < 1.0e-3));
        let ph = Snapshot::placeholder(200, 120);
        let g = ph.lum_grid(10, 6);
        assert_eq!(g.len(), 60);
        assert!(g.iter().all(|l| *l < 0.3), "placeholder is dark mono");
    }

    #[test]
    fn overlay_runs_effects_and_hands_back() {
        let mut o = MotionOverlayView::new(
            (0.0, 0.0),
            Rect::new(0.0, 0.0, 1920.0, 1040.0),
            MotionPrefs::default(),
        );
        let snap = Arc::new(Snapshot::placeholder(64, 48));
        let win = Rect::new(100.0, 100.0, 640.0, 480.0);
        let icon = Rect::new(900.0, 990.0, 56.0, 56.0);
        assert!(o.open(1, snap.clone(), win, icon, None));
        assert!(
            o.minimize(1, snap.clone(), win, icon, None),
            "interrupts open"
        );
        assert!(o.close(2, snap.clone(), win, None));
        o.focus_ripple(win);
        for _ in 0..300 {
            o.step(1.0 / 120.0);
        }
        assert!(!o.is_animating(), "all effects finish");
    }

    #[test]
    fn drag_snaps_proxy_into_zone() {
        let mut o = MotionOverlayView::new(
            (0.0, 0.0),
            Rect::new(0.0, 0.0, 1920.0, 1040.0),
            MotionPrefs::default(),
        );
        let snap = Arc::new(Snapshot::placeholder(64, 48));
        o.drag_begin(
            Some((snap, Rect::new(500.0, 300.0, 600.0, 400.0))),
            (800.0, 320.0),
        );
        o.drag_move(
            (5.0, 500.0),
            Some(Rect::new(0.0, 480.0, 600.0, 400.0)),
            0.016,
        );
        let target = o.drag_end(None).expect("left edge snaps");
        assert!(target.x <= 10.0 && target.w < 1000.0);
        for _ in 0..400 {
            o.step(1.0 / 120.0);
        }
        assert!(o.drag.is_none());
    }

    #[test]
    fn switcher_wraps_and_commits() {
        let mut o = MotionOverlayView::new(
            (0.0, 0.0),
            Rect::new(0.0, 0.0, 1920.0, 1040.0),
            MotionPrefs::default(),
        );
        let snap = Arc::new(Snapshot::placeholder(64, 48));
        o.switcher_show((0..3).map(|i| (snap.clone(), format!("w{i}"))).collect(), 0);
        o.switcher_step(-1);
        assert_eq!(o.switcher_commit(), Some(2));
        for _ in 0..240 {
            o.step(1.0 / 120.0);
        }
        assert!(o.switcher.is_none());
    }

    #[test]
    fn dock_icons_step_left_to_right() {
        let dock = Rect::new(480.0, 980.0, 960.0, 96.0);
        let mag = crate::Magnification::macos(1.0);
        let a = dock_icon_rect(dock, 6, 0, mag);
        let b = dock_icon_rect(dock, 6, 1, mag);
        assert!(b.x > a.x);
        assert!(a.y > dock.y - 10.0 && a.bottom() <= dock.bottom());
    }
}
