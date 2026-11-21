//! Nexus signature motion: the platform-free math behind every window effect.
//!
//! Everything here is pure: springs, state machines, and geometry that turn a
//! progress value into rectangles, strips, and particles. The GPUI overlay in
//! `nx-shell` paints the result; tests pin the math down without a GPU.
//!
//! Effects:
//! - **Bloom** (open): the window springs out of its dock icon or the island
//!   with scale, blur, and opacity, overshooting slightly before it settles.
//! - **Dust** (close): the window crumbles from the top into fine monochrome
//!   particles that drift up on an updraft in about 380 ms.
//! - **Ink** (minimize/restore): a liquid genie that pours the window into its
//!   dock icon with rippling edges and an ink tint; restore runs it backwards.
//! - **Magnetic snap**: zone detection, edge glow, preview rects, drag lift,
//!   and an underdamped wobble when the window lands.
//! - **Carousel** (app switch): 3D tilted cards projected into vertical strips.
//! - **Fan-out** (Mission Control): staggered springs from window to tile.
//! - Nexus-only touches: focus ripple, breathing parallax wallpaper, and
//!   shake-to-peek.
//!
//! Every effect is interruptible: state lives in springs, and a new command
//! retargets from the current position and velocity instead of restarting.
//! [`MotionPrefs::reduce_motion`] collapses every effect to a short crossfade.

use crate::geom::Rect;
use crate::spring::{Spring, SpringConfig};

/// User and OS motion preferences.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionPrefs {
    /// Replace every effect with a short crossfade. Set from Settings, or from
    /// Windows "Animation effects" being off.
    pub reduce_motion: bool,
    /// Global speed multiplier. 1.0 is the Nexus default; 2.0 is twice as fast.
    pub speed: f32,
}

impl Default for MotionPrefs {
    fn default() -> Self {
        Self {
            reduce_motion: false,
            speed: 1.0,
        }
    }
}

impl MotionPrefs {
    pub fn reduced() -> Self {
        Self {
            reduce_motion: true,
            speed: 1.0,
        }
    }

    /// Combine the in-app setting with the OS accessibility setting. Either
    /// one asking for less motion wins.
    pub fn resolve(user_reduce: bool, os_animations_enabled: bool, speed: f32) -> Self {
        Self {
            reduce_motion: user_reduce || !os_animations_enabled,
            speed: if speed.is_finite() {
                speed.clamp(0.25, 4.0)
            } else {
                1.0
            },
        }
    }

    /// Scale a base duration in seconds by the speed setting.
    pub fn duration(&self, base: f32) -> f32 {
        if self.reduce_motion {
            CROSSFADE_SECS
        } else {
            base / self.speed.max(0.25)
        }
    }

    fn spring(&self, cfg: SpringConfig) -> SpringConfig {
        if self.reduce_motion {
            // Critically damped and quick: a fade, never a bounce.
            SpringConfig::macos(CROSSFADE_SECS * 1.6, 1.0)
        } else {
            let mut c = SpringConfig::macos(cfg_response(cfg) / self.speed.max(0.25), cfg.zeta());
            c.rest_pos = cfg.rest_pos;
            c.rest_vel = cfg.rest_vel;
            c
        }
    }
}

/// Reduced-motion crossfade length.
pub const CROSSFADE_SECS: f32 = 0.12;
/// Base close (dust) duration.
pub const DUST_SECS: f32 = 0.38;

fn cfg_response(cfg: SpringConfig) -> f32 {
    let omega = (cfg.stiffness / cfg.mass.max(1.0e-4)).sqrt().max(1.0e-3);
    std::f32::consts::TAU / omega
}

/// Spring presets that define the Nexus feel.
pub mod presets {
    use crate::spring::SpringConfig;

    fn tight(mut c: SpringConfig) -> SpringConfig {
        // Progress springs live in 0..1, so rest thresholds must be small.
        c.rest_pos = 0.002;
        c.rest_vel = 0.02;
        c
    }

    /// Open bloom: lively, about 8% overshoot.
    pub fn bloom() -> SpringConfig {
        tight(SpringConfig::macos(0.42, 0.62))
    }
    /// Minimize and restore ink: smooth, no overshoot, slightly longer.
    pub fn ink() -> SpringConfig {
        tight(SpringConfig::macos(0.5, 1.0))
    }
    /// Snap into a zone: firm with a tiny overshoot.
    pub fn snap() -> SpringConfig {
        SpringConfig::macos(0.3, 0.8)
    }
    /// Landing wobble after a drop.
    pub fn wobble() -> SpringConfig {
        tight(SpringConfig::macos(0.36, 0.32))
    }
    /// Switcher carousel and Mission Control cards.
    pub fn card() -> SpringConfig {
        tight(SpringConfig::macos(0.34, 0.84))
    }
}

// ---------------------------------------------------------------------------
// Easing helpers
// ---------------------------------------------------------------------------

pub fn clamp01(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if (e1 - e0).abs() < 1.0e-6 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    1.0 - (1.0 - t).powi(3)
}

pub fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(
        lerp(a.x, b.x, t),
        lerp(a.y, b.y, t),
        lerp(a.w, b.w, t),
        lerp(a.h, b.h, t),
    )
}

/// Scale `r` about its center.
pub fn scale_about_center(r: Rect, s: f32) -> Rect {
    let cx = r.x + r.w * 0.5;
    let cy = r.y + r.h * 0.5;
    Rect::new(cx - r.w * s * 0.5, cy - r.h * s * 0.5, r.w * s, r.h * s)
}

/// Deterministic hash noise in 0..1. No RNG state, so frames are reproducible.
pub fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

// ---------------------------------------------------------------------------
// Window lifecycle state machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Hidden,
    Opening,
    Shown,
    Minimizing,
    Minimized,
    Restoring,
    Closing,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open,
    Close,
    Minimize,
    Restore,
}

/// Which painter should draw the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    /// Nothing to draw; the real window is on screen or gone.
    None,
    Bloom,
    Ink,
    Dust,
    Fade,
}

/// One frame of a window effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowFrame {
    pub kind: EffectKind,
    /// 1.0 means fully on screen at its real rect, 0.0 fully gone or in the dock.
    /// Bloom may exceed 1.0 briefly (overshoot).
    pub visibility: f32,
    /// Seconds since the close started (dust only).
    pub dust_age: f32,
    /// Whether the real window should be visible now. The overlay hides the
    /// real window while it animates a snapshot, then hands back.
    pub real_window_visible: bool,
}

/// Per-window lifecycle with interruptible springs.
#[derive(Clone, Debug)]
pub struct WindowMotion {
    phase: Phase,
    vis: Spring,
    dust_age: f32,
    prefs: MotionPrefs,
}

impl WindowMotion {
    /// A window that has never been shown.
    pub fn hidden(prefs: MotionPrefs) -> Self {
        Self {
            phase: Phase::Hidden,
            vis: Spring::new(0.0, prefs.spring(presets::bloom())),
            dust_age: 0.0,
            prefs,
        }
    }

    /// A window already on screen when Nexus started.
    pub fn shown(prefs: MotionPrefs) -> Self {
        let mut m = Self::hidden(prefs);
        m.phase = Phase::Shown;
        m.vis.position = 1.0;
        m.vis.target = 1.0;
        m
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn prefs(&self) -> MotionPrefs {
        self.prefs
    }

    pub fn visibility(&self) -> f32 {
        self.vis.position
    }

    /// True while an effect needs frames.
    pub fn animating(&self) -> bool {
        matches!(
            self.phase,
            Phase::Opening | Phase::Minimizing | Phase::Restoring | Phase::Closing
        )
    }

    /// Apply a command. Returns false when it does not apply in this phase.
    /// Commands mid-flight retarget from the current position and velocity.
    pub fn apply(&mut self, cmd: Command) -> bool {
        use Phase::*;
        let next = match (cmd, self.phase) {
            (Command::Open, Hidden | Closing) => Opening,
            (Command::Close, Closed) => return false,
            (Command::Close, _) => Closing,
            (Command::Minimize, Shown | Opening | Restoring) => Minimizing,
            (Command::Restore, Minimized | Minimizing) => Restoring,
            _ => return false,
        };
        let cfg = match next {
            Opening => presets::bloom(),
            Minimizing | Restoring => presets::ink(),
            _ => self.vis.config,
        };
        if next == Opening && self.phase == Closing {
            // Reopen while dissolving: rebuild from where the dust is.
            self.vis.position = (1.0 - self.dust_progress()).clamp(0.0, 1.0);
            self.vis.velocity = 0.0;
        }
        self.vis.config = self.prefs.spring(cfg);
        self.vis.set_target(match next {
            Opening | Restoring => 1.0,
            _ => 0.0,
        });
        if next == Closing {
            self.dust_age = 0.0;
        }
        self.phase = next;
        true
    }

    fn dust_progress(&self) -> f32 {
        clamp01(self.dust_age / self.prefs.duration(DUST_SECS))
    }

    /// Advance by `dt` seconds. Long or bogus frames are clamped so a stall
    /// cannot skip an effect entirely.
    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.05)
        } else {
            0.0
        };
        match self.phase {
            Phase::Closing => {
                self.dust_age += dt;
                if self.dust_progress() >= 1.0 {
                    self.phase = Phase::Closed;
                    self.vis.position = 0.0;
                    self.vis.velocity = 0.0;
                    self.vis.target = 0.0;
                }
            }
            Phase::Opening | Phase::Minimizing | Phase::Restoring => {
                self.vis.step(dt);
                if self.vis.settled() {
                    self.phase = match self.phase {
                        Phase::Minimizing => Phase::Minimized,
                        _ => Phase::Shown,
                    };
                }
            }
            _ => {}
        }
    }

    pub fn frame(&self) -> WindowFrame {
        let kind = match (self.prefs.reduce_motion, self.phase) {
            (_, Phase::Hidden | Phase::Shown | Phase::Minimized | Phase::Closed) => {
                EffectKind::None
            }
            (true, _) => EffectKind::Fade,
            (false, Phase::Opening) => EffectKind::Bloom,
            (false, Phase::Minimizing | Phase::Restoring) => EffectKind::Ink,
            (false, Phase::Closing) => EffectKind::Dust,
        };
        let visibility = if self.phase == Phase::Closing && self.prefs.reduce_motion {
            1.0 - self.dust_progress()
        } else {
            self.vis.position
        };
        WindowFrame {
            kind,
            visibility,
            dust_age: self.dust_age,
            real_window_visible: kind == EffectKind::None && self.phase == Phase::Shown,
        }
    }
}

// ---------------------------------------------------------------------------
// Bloom (open)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomFrame {
    pub rect: Rect,
    pub opacity: f32,
    /// Gaussian blur radius in px, high at the start.
    pub blur: f32,
    pub corner_radius: f32,
    /// Shadow strength 0..1.
    pub shadow: f32,
}

/// The window grows from `source` (dock icon or island) to `target`.
/// `p` comes from the bloom spring and may overshoot past 1.0.
pub fn bloom(source: Rect, target: Rect, p: f32, corner: f32) -> BloomFrame {
    let travel = ease_out_cubic(clamp01(p * 1.15));
    let from_center = (source.x + source.w * 0.5, source.y + source.h * 0.5);
    let to_center = (target.x + target.w * 0.5, target.y + target.h * 0.5);
    let cx = lerp(from_center.0, to_center.0, travel);
    let cy = lerp(from_center.1, to_center.1, travel);
    // Size follows the raw spring so the overshoot reads as a bloom.
    let min_sx = (source.w / target.w.max(1.0)).clamp(0.02, 1.0);
    let min_sy = (source.h / target.h.max(1.0)).clamp(0.02, 1.0);
    let s = p.max(0.0);
    let sx = lerp(min_sx, 1.0, s);
    let sy = lerp(min_sy, 1.0, s);
    let w = target.w * sx;
    let h = target.h * sy;
    BloomFrame {
        rect: Rect::new(cx - w * 0.5, cy - h * 0.5, w, h),
        opacity: clamp01(p * 2.2),
        blur: (1.0 - clamp01(p)) * 22.0,
        corner_radius: lerp(source.w.min(source.h) * 0.24, corner, clamp01(p)),
        shadow: clamp01(p),
    }
}

// ---------------------------------------------------------------------------
// Ink genie (minimize / restore)
// ---------------------------------------------------------------------------

/// One horizontal band of the snapshot placed on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// Source band in texture space, 0 = top of the window, 1 = bottom.
    pub v0: f32,
    pub v1: f32,
    /// Where the band lands on screen.
    pub dst: Rect,
    pub alpha: f32,
    /// Ink tint 0..1, drawn as black over the band.
    pub ink: f32,
}

/// Liquid-ink genie. `t` is 0 when the window is at `win` and 1 when it has
/// fully poured into `icon`. Restore passes the same `t` running backwards.
pub fn ink_genie(win: Rect, icon: Rect, t: f32, strips: usize) -> Vec<Strip> {
    let t = clamp01(t);
    let n = strips.clamp(2, 256);
    // Phase 1 pinches the window toward the icon's column, phase 2 pours it in.
    let pinch = smoothstep(0.0, 0.55, t);
    let pour = smoothstep(0.3, 1.0, t);
    let below = icon.y + icon.h * 0.5 >= win.y + win.h * 0.5;
    // Top and bottom of the poured column on screen.
    let (top, bottom) = if below {
        (
            lerp(win.y, icon.y, pour),
            lerp(win.bottom(), icon.bottom(), pour.sqrt()),
        )
    } else {
        (
            lerp(win.y, icon.y, pour.sqrt()),
            lerp(win.bottom(), icon.bottom(), pour),
        )
    };
    let span = (icon.bottom() - win.y).abs().max(1.0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let v0 = i as f32 / n as f32;
        let v1 = (i + 1) as f32 / n as f32;
        let vm = (v0 + v1) * 0.5;
        let y0 = lerp(top, bottom, v0);
        let y1 = lerp(top, bottom, v1);
        let ym = (y0 + y1) * 0.5;
        // How close this band is to the icon, 0 at the far edge, 1 at the icon.
        let d = if below {
            clamp01((ym - win.y) / span)
        } else {
            clamp01((win.bottom() - ym) / span)
        };
        let curve = 0.5 - 0.5 * (std::f32::consts::PI * d).cos();
        let k = clamp01(curve * pinch + pour * (1.0 - curve) * pour);
        let mut left = lerp(win.x, icon.x, k);
        let mut right = lerp(win.right(), icon.right(), k);
        // Liquid edges: a travelling ripple that is strongest mid-flight.
        let wave_amp = 9.0 * (t * (1.0 - t) * 4.0) * (1.0 - d * 0.6);
        let wave = (vm * 9.0 * std::f32::consts::PI - t * 14.0).sin() * wave_amp;
        left += wave;
        right += wave * 0.6;
        if right - left < 1.0 {
            let mid = (left + right) * 0.5;
            left = mid - 0.5;
            right = mid + 0.5;
        }
        let alpha = 1.0 - smoothstep(0.82, 1.0, t);
        out.push(Strip {
            v0,
            v1,
            dst: Rect::new(left, y0, right - left, (y1 - y0).max(0.5) + 0.5),
            alpha,
            ink: clamp01(t * 0.55 + d * pinch * 0.35),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Dust (close)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// Cell center at rest.
    pub ox: f32,
    pub oy: f32,
    pub size: f32,
    /// Luminance 0..1 sampled from the snapshot.
    pub lum: f32,
    /// Seconds before this particle lets go.
    pub delay: f32,
    pub vx: f32,
    pub vy: f32,
    pub phase: f32,
    /// Row fraction, 0 at the top.
    pub row: f32,
}

/// A drawn particle this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DustSpeck {
    pub rect: Rect,
    pub alpha: f32,
    pub lum: f32,
}

#[derive(Clone, Debug)]
pub struct DustField {
    pub rect: Rect,
    pub cols: usize,
    pub rows: usize,
    pub particles: Vec<Particle>,
    pub duration: f32,
    sweep: f32,
    life: f32,
}

/// Pick a grid that keeps cells near `target_cell` px but under `max` total.
pub fn dust_grid(rect: Rect, target_cell: f32, max: usize) -> (usize, usize) {
    let mut cell = target_cell.max(2.0);
    loop {
        let cols = ((rect.w / cell).ceil() as usize).max(1);
        let rows = ((rect.h / cell).ceil() as usize).max(1);
        if cols * rows <= max.max(1) {
            return (cols, rows);
        }
        cell *= 1.15;
    }
}

impl DustField {
    /// Build a field from a `cols x rows` luminance grid of the snapshot.
    /// Missing samples default to mid-grey.
    pub fn new(rect: Rect, cols: usize, rows: usize, lum: &[f32], prefs: MotionPrefs) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let duration = prefs.duration(DUST_SECS);
        let sweep = duration * 0.45;
        let life = duration * 0.55;
        let cw = rect.w / cols as f32;
        let ch = rect.h / rows as f32;
        let mut particles = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let seed = i as u32;
                let row = r as f32 / rows as f32;
                let jitter = hash01(seed.wrapping_mul(3).wrapping_add(1));
                particles.push(Particle {
                    ox: rect.x + (c as f32 + 0.5) * cw,
                    oy: rect.y + (r as f32 + 0.5) * ch,
                    size: cw.min(ch) * lerp(0.55, 0.95, hash01(seed.wrapping_add(77))),
                    lum: lum.get(i).copied().unwrap_or(0.5).clamp(0.0, 1.0),
                    delay: (row * 0.8 + jitter * 0.2) * sweep,
                    vx: lerp(-30.0, 110.0, hash01(seed.wrapping_mul(7).wrapping_add(5))),
                    vy: lerp(
                        -150.0,
                        -420.0,
                        hash01(seed.wrapping_mul(11).wrapping_add(9)),
                    ),
                    phase: hash01(seed.wrapping_mul(13).wrapping_add(3)) * std::f32::consts::TAU,
                    row,
                });
            }
        }
        Self {
            rect,
            cols,
            rows,
            particles,
            duration,
            sweep,
            life,
        }
    }

    /// Fraction of the window from the top that has started to crumble. The
    /// painter draws the intact snapshot below this line.
    pub fn intact_from(&self, age: f32) -> f32 {
        clamp01(age / (self.sweep * 0.8).max(1.0e-4))
    }

    pub fn done(&self, age: f32) -> bool {
        age >= self.duration
    }

    /// Specks to draw at `age` seconds. Particles above the intact line that
    /// have not let go yet are drawn in place, so the edge looks like it crumbles.
    pub fn specks(&self, age: f32) -> Vec<DustSpeck> {
        let edge = self.intact_from(age);
        let mut out = Vec::with_capacity(self.particles.len());
        for p in &self.particles {
            if p.row >= edge {
                continue;
            }
            let a = age - p.delay;
            if a <= 0.0 {
                let s = p.size / 0.75;
                out.push(DustSpeck {
                    rect: Rect::new(p.ox - s * 0.5, p.oy - s * 0.5, s, s),
                    alpha: 1.0,
                    lum: p.lum,
                });
                continue;
            }
            let k = a / self.life.max(1.0e-4);
            if k >= 1.0 {
                continue;
            }
            // Updraft: accelerates up and drifts right, with a gentle swirl.
            let x = p.ox + p.vx * a + 120.0 * a * a + (a * 14.0 + p.phase).sin() * 10.0 * a;
            let y = p.oy + p.vy * a - 900.0 * a * a;
            let s = p.size * lerp(1.0, 0.35, k);
            out.push(DustSpeck {
                rect: Rect::new(x - s * 0.5, y - s * 0.5, s, s),
                alpha: (1.0 - k) * (1.0 - k),
                lum: lerp(p.lum, 1.0, k * 0.5),
            });
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Magnetic snap, drag lift, wobble, shake-to-peek
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapZone {
    Maximize,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Which zone the cursor is pulling toward, if any. `reach` is how far from
/// an edge the magnet starts, in px.
pub fn snap_zone(cx: f32, cy: f32, work: Rect, reach: f32) -> Option<SnapZone> {
    let left = cx - work.x <= reach;
    let right = work.right() - cx <= reach;
    let top = cy - work.y <= reach;
    let bottom = work.bottom() - cy <= reach;
    let corner = reach * 3.0;
    let near_top = cy - work.y <= corner;
    let near_bottom = work.bottom() - cy <= corner;
    match (left, right, top, bottom) {
        (true, _, _, _) if near_top => Some(SnapZone::TopLeft),
        (true, _, _, _) if near_bottom => Some(SnapZone::BottomLeft),
        (true, _, _, _) => Some(SnapZone::Left),
        (_, true, _, _) if near_top => Some(SnapZone::TopRight),
        (_, true, _, _) if near_bottom => Some(SnapZone::BottomRight),
        (_, true, _, _) => Some(SnapZone::Right),
        (_, _, true, _) => Some(SnapZone::Maximize),
        _ => None,
    }
}

/// Destination rect for a zone, with a `gap` from the work-area edges.
pub fn zone_rect(zone: SnapZone, work: Rect, gap: f32) -> Rect {
    let w = work.shrink(gap);
    let hw = (w.w - gap) * 0.5;
    let hh = (w.h - gap) * 0.5;
    match zone {
        SnapZone::Maximize => w,
        SnapZone::Left => Rect::new(w.x, w.y, hw, w.h),
        SnapZone::Right => Rect::new(w.x + hw + gap, w.y, hw, w.h),
        SnapZone::TopLeft => Rect::new(w.x, w.y, hw, hh),
        SnapZone::TopRight => Rect::new(w.x + hw + gap, w.y, hw, hh),
        SnapZone::BottomLeft => Rect::new(w.x, w.y + hh + gap, hw, hh),
        SnapZone::BottomRight => Rect::new(w.x + hw + gap, w.y + hh + gap, hw, hh),
    }
}

/// Glow strength 0..1 on the edge the window is being pulled to: grows as the
/// cursor closes in, so the edge feels magnetic before the snap commits.
pub fn edge_glow(cx: f32, cy: f32, work: Rect, reach: f32) -> f32 {
    let d = (cx - work.x)
        .min(work.right() - cx)
        .min(cy - work.y)
        .max(0.0);
    1.0 - smoothstep(0.0, reach * 2.5, d)
}

/// Drag and drop feel for one window: lift while held, magnetic glide into a
/// zone, wobble on landing.
#[derive(Clone, Debug)]
pub struct DragMotion {
    lift: Spring,
    wobble: Spring,
    pub zone: Option<SnapZone>,
    pub preview: Spring,
}

impl Default for DragMotion {
    fn default() -> Self {
        Self::new()
    }
}

impl DragMotion {
    pub fn new() -> Self {
        let mut lift = Spring::new(0.0, presets::snap());
        lift.config.rest_pos = 0.002;
        lift.config.rest_vel = 0.02;
        let mut preview = Spring::new(0.0, presets::snap());
        preview.config.rest_pos = 0.002;
        preview.config.rest_vel = 0.02;
        Self {
            lift,
            wobble: Spring::new(0.0, presets::wobble()),
            zone: None,
            preview,
        }
    }

    pub fn grab(&mut self) {
        self.lift.set_target(1.0);
    }

    /// Cursor moved while dragging.
    pub fn hover(&mut self, zone: Option<SnapZone>) {
        if zone != self.zone {
            self.zone = zone;
            // Re-bloom the preview each time the zone changes.
            self.preview.position = 0.0;
        }
        self.preview
            .set_target(if zone.is_some() { 1.0 } else { 0.0 });
    }

    /// Released: returns the zone to snap to, and starts the landing wobble.
    pub fn drop_window(&mut self) -> Option<SnapZone> {
        self.lift.set_target(0.0);
        self.preview.set_target(0.0);
        self.wobble.position = 1.0;
        self.wobble.velocity = 0.0;
        self.wobble.set_target(0.0);
        self.zone.take()
    }

    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.lift.step(dt);
        self.wobble.step(dt);
        self.preview.step(dt);
    }

    pub fn animating(&self) -> bool {
        !(self.lift.settled() && self.wobble.settled() && self.preview.settled())
    }

    /// Scale applied to the window: up to +2.5% while lifted, plus a damped
    /// squash/stretch while it lands.
    pub fn scale(&self) -> f32 {
        1.0 + 0.025 * self.lift.position
            + 0.03 * self.wobble.position.sin() * self.wobble.position.abs().min(1.0)
    }

    /// Shadow blur radius in px: deeper while lifted.
    pub fn shadow_blur(&self) -> f32 {
        lerp(22.0, 56.0, clamp01(self.lift.position))
    }

    pub fn shadow_offset(&self) -> f32 {
        lerp(8.0, 22.0, clamp01(self.lift.position))
    }

    pub fn preview_progress(&self) -> f32 {
        self.preview.position
    }
}

/// Detects a quick side-to-side shake while dragging (shake-to-peek).
#[derive(Clone, Debug, Default)]
pub struct ShakeDetector {
    last_x: Option<f32>,
    dir: i8,
    travel: f32,
    reversals: Vec<f32>,
    now: f32,
}

impl ShakeDetector {
    /// Need this many direction changes within [`Self::WINDOW`] seconds.
    pub const REVERSALS: usize = 4;
    pub const WINDOW: f32 = 0.7;
    /// Minimum travel per swing, px.
    pub const MIN_SWING: f32 = 28.0;

    /// Feed a pointer x at `dt` seconds after the previous sample. Returns
    /// true on the sample that completes a shake.
    pub fn feed(&mut self, x: f32, dt: f32) -> bool {
        self.now += dt.max(0.0);
        let Some(prev) = self.last_x.replace(x) else {
            return false;
        };
        let dx = x - prev;
        if dx.abs() < 0.5 {
            return false;
        }
        let dir = if dx > 0.0 { 1 } else { -1 };
        if dir == self.dir {
            self.travel += dx.abs();
            return false;
        }
        let swing_ok = self.travel >= Self::MIN_SWING;
        self.dir = dir;
        self.travel = dx.abs();
        if !swing_ok {
            return false;
        }
        let now = self.now;
        self.reversals.push(now);
        self.reversals.retain(|t| now - *t <= Self::WINDOW);
        if self.reversals.len() >= Self::REVERSALS {
            self.reversals.clear();
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// Carousel switcher (Alt+Tab) and Mission Control fan-out
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardPose {
    pub index: usize,
    /// Untilted card rect on screen.
    pub rect: Rect,
    /// Rotation about the vertical axis in radians; positive turns the right
    /// edge away from the viewer.
    pub angle: f32,
    pub alpha: f32,
    /// Draw order; higher is in front.
    pub z: f32,
}

/// Lay out `n` cards around a virtual cylinder. `focus` is the spring-driven
/// selected index (fractional mid-flight), `open` 0..1 is the entry spring.
pub fn carousel(
    n: usize,
    focus: f32,
    open: f32,
    screen: Rect,
    card_w: f32,
    card_h: f32,
) -> Vec<CardPose> {
    let cx = screen.x + screen.w * 0.5;
    let cy = screen.y + screen.h * 0.48;
    let spacing = card_w * 0.58;
    let open = clamp01(open);
    let mut out: Vec<CardPose> = (0..n)
        .map(|i| {
            let off = i as f32 - focus;
            let angle = (off * 0.55).clamp(-1.05, 1.05) * open;
            let depth = off.abs().min(4.0);
            let scale = lerp(0.7, 1.0 - depth * 0.1, open).max(0.45);
            let x = cx + off.signum() * (off.abs().powf(0.85)) * spacing * open;
            let w = card_w * scale;
            let h = card_h * scale;
            CardPose {
                index: i,
                rect: Rect::new(x - w * 0.5, cy - h * 0.5 + depth * 10.0 * open, w, h),
                angle,
                alpha: clamp01(open * 1.4) * (1.0 - depth * 0.16).max(0.25),
                z: -depth,
            }
        })
        .collect();
    out.sort_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// A vertical slice of a tilted card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    /// Source band in texture space, 0 = left edge, 1 = right edge.
    pub u0: f32,
    pub u1: f32,
    pub dst: Rect,
    /// Shade 0..1 drawn as black over the slice; edges turned away are darker.
    pub shade: f32,
}

/// Project a card rotated by `angle` about its vertical axis with a camera
/// at `focal` px. Each slice keeps its own height, which reads as perspective.
pub fn perspective_slices(rect: Rect, angle: f32, slices: usize, focal: f32) -> Vec<Slice> {
    let n = slices.clamp(1, 128);
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let (sin, cos) = angle.sin_cos();
    let f = focal.max(100.0);
    let project = |u: f32| -> (f32, f32) {
        let lx = (u - 0.5) * rect.w;
        let z = lx * sin;
        let k = f / (f + z);
        (cx + lx * cos * k, k)
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let u0 = i as f32 / n as f32;
        let u1 = (i + 1) as f32 / n as f32;
        let (x0, k0) = project(u0);
        let (x1, k1) = project(u1);
        let k = (k0 + k1) * 0.5;
        let h = rect.h * k;
        let (l, r) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        out.push(Slice {
            u0,
            u1,
            dst: Rect::new(l, cy - h * 0.5, (r - l).max(0.5) + 0.5, h),
            shade: clamp01((1.0 - k) * 2.2),
        });
    }
    out
}

/// Mission Control: rect of window `i` of `n` moving from `from` to `to`
/// with a stagger so cards fan out one after another. `p` is the shared
/// progress spring (0 closed, 1 fanned).
pub fn fan_out(from: Rect, to: Rect, i: usize, n: usize, p: f32) -> (Rect, f32) {
    let n = n.max(1) as f32;
    let stagger = 0.35;
    let start = (i as f32 / n) * stagger;
    let local = clamp01((p - start) / (1.0 - stagger));
    let e = ease_out_cubic(local);
    // A small arc: cards lift up before settling into the grid.
    let mut r = lerp_rect(from, to, e);
    r.y -= (std::f32::consts::PI * e).sin() * 18.0;
    (r, e)
}

// ---------------------------------------------------------------------------
// Nexus-only touches
// ---------------------------------------------------------------------------

/// Focus ripple: a thin ring that radiates out of a window when it takes
/// focus. Returns the ring rect and alpha, or None when finished.
pub fn focus_ripple(rect: Rect, age: f32, prefs: MotionPrefs) -> Option<(Rect, f32)> {
    if prefs.reduce_motion {
        return None;
    }
    let dur = prefs.duration(0.55);
    if !(0.0..dur).contains(&age) {
        return None;
    }
    let k = age / dur;
    let grow = ease_out_cubic(k) * 26.0;
    Some((
        Rect::new(
            rect.x - grow,
            rect.y - grow,
            rect.w + grow * 2.0,
            rect.h + grow * 2.0,
        ),
        (1.0 - k) * 0.55,
    ))
}

/// Breathing parallax for the wallpaper. `t` is seconds since start,
/// `cursor` is the pointer in -1..1 on both axes. Returns (dx, dy, scale).
pub fn breath(t: f32, cursor: (f32, f32), prefs: MotionPrefs) -> (f32, f32, f32) {
    if prefs.reduce_motion {
        return (0.0, 0.0, 1.0);
    }
    let cycle = (t * std::f32::consts::TAU / 9.0).sin();
    let scale = 1.02 + 0.008 * cycle;
    let dx = -cursor.0.clamp(-1.0, 1.0) * 14.0 + (t * 0.37).sin() * 3.0;
    let dy = -cursor.1.clamp(-1.0, 1.0) * 9.0 + (t * 0.29).cos() * 2.0;
    (dx, dy, scale)
}

/// Dock running-dot pulse: a short glow when an app gets attention, then a
/// slow idle shimmer. Returns opacity 0..1.
pub fn running_dot_pulse(t: f32, attention_age: Option<f32>) -> f32 {
    let idle = 0.78 + 0.12 * (t * std::f32::consts::TAU / 2.8).sin();
    match attention_age {
        Some(a) if (0.0..1.2).contains(&a) => {
            let k = a / 1.2;
            idle.max(1.0 - k * 0.3) + (a * 18.0).sin().abs() * (1.0 - k) * 0.2
        }
        _ => idle,
    }
    .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &mut WindowMotion, secs: f32) {
        let dt = 1.0 / 120.0;
        let mut t = 0.0;
        while t < secs {
            m.step(dt);
            t += dt;
        }
    }

    #[test]
    fn open_overshoots_then_settles_shown() {
        let mut m = WindowMotion::hidden(MotionPrefs::default());
        assert!(m.apply(Command::Open));
        assert_eq!(m.frame().kind, EffectKind::Bloom);
        let mut peak: f32 = 0.0;
        for _ in 0..240 {
            m.step(1.0 / 120.0);
            peak = peak.max(m.visibility());
        }
        assert!(peak > 1.02, "bloom should overshoot, peak {peak}");
        assert!(peak < 1.2, "overshoot stays tasteful, peak {peak}");
        assert_eq!(m.phase(), Phase::Shown);
        assert!(m.frame().real_window_visible);
    }

    #[test]
    fn minimize_mid_open_keeps_position_and_velocity() {
        let mut m = WindowMotion::hidden(MotionPrefs::default());
        m.apply(Command::Open);
        run(&mut m, 0.08);
        let before = m.visibility();
        assert!(before > 0.0 && before < 1.0);
        assert!(m.apply(Command::Minimize));
        assert_eq!(m.phase(), Phase::Minimizing);
        assert_eq!(m.visibility(), before, "no jump when interrupted");
        run(&mut m, 2.0);
        assert_eq!(m.phase(), Phase::Minimized);
        assert!(m.visibility().abs() < 0.01);
    }

    #[test]
    fn restore_reverses_minimize() {
        let mut m = WindowMotion::shown(MotionPrefs::default());
        m.apply(Command::Minimize);
        run(&mut m, 0.1);
        assert!(m.apply(Command::Restore));
        assert_eq!(m.frame().kind, EffectKind::Ink);
        run(&mut m, 2.0);
        assert_eq!(m.phase(), Phase::Shown);
    }

    #[test]
    fn close_runs_dust_for_its_duration() {
        let mut m = WindowMotion::shown(MotionPrefs::default());
        assert!(m.apply(Command::Close));
        assert_eq!(m.frame().kind, EffectKind::Dust);
        run(&mut m, DUST_SECS * 0.5);
        assert_eq!(m.phase(), Phase::Closing);
        run(&mut m, DUST_SECS);
        assert_eq!(m.phase(), Phase::Closed);
        assert!(!m.apply(Command::Close));
        assert!(!m.apply(Command::Minimize));
    }

    #[test]
    fn reopen_while_closing_rebuilds() {
        let mut m = WindowMotion::shown(MotionPrefs::default());
        m.apply(Command::Close);
        run(&mut m, DUST_SECS * 0.25);
        assert!(m.apply(Command::Open));
        let v = m.visibility();
        assert!(v > 0.5 && v < 1.0, "starts from remaining dust, got {v}");
    }

    #[test]
    fn invalid_commands_are_rejected() {
        let mut m = WindowMotion::hidden(MotionPrefs::default());
        assert!(!m.apply(Command::Minimize));
        assert!(!m.apply(Command::Restore));
        let mut s = WindowMotion::shown(MotionPrefs::default());
        assert!(!s.apply(Command::Open));
        assert!(!s.apply(Command::Restore));
    }

    #[test]
    fn reduce_motion_is_a_fast_fade() {
        let prefs = MotionPrefs::resolve(false, false, 1.0);
        assert!(
            prefs.reduce_motion,
            "OS animations off implies reduce motion"
        );
        let mut m = WindowMotion::hidden(prefs);
        m.apply(Command::Open);
        assert_eq!(m.frame().kind, EffectKind::Fade);
        let mut peak: f32 = 0.0;
        let mut t = 0.0;
        while m.animating() && t < 1.0 {
            m.step(1.0 / 120.0);
            t += 1.0 / 120.0;
            peak = peak.max(m.visibility());
        }
        assert!(t < 0.4, "fade finishes quickly, took {t}");
        assert!(peak <= 1.0 + 1.0e-3, "no overshoot when reduced");
        let mut c = WindowMotion::shown(prefs);
        c.apply(Command::Close);
        assert_eq!(c.frame().kind, EffectKind::Fade);
    }

    #[test]
    fn speed_scales_durations_and_is_clamped() {
        let fast = MotionPrefs::resolve(false, true, 2.0);
        assert!((fast.duration(0.4) - 0.2).abs() < 1.0e-5);
        let silly = MotionPrefs::resolve(false, true, f32::NAN);
        assert_eq!(silly.speed, 1.0);
        let huge = MotionPrefs::resolve(false, true, 100.0);
        assert_eq!(huge.speed, 4.0);
    }

    #[test]
    fn bogus_dt_never_breaks_state() {
        let mut m = WindowMotion::hidden(MotionPrefs::default());
        m.apply(Command::Open);
        m.step(f32::NAN);
        m.step(-1.0);
        m.step(10.0);
        assert!(m.visibility().is_finite());
    }

    #[test]
    fn bloom_starts_at_icon_and_ends_at_target() {
        let icon = Rect::new(900.0, 1000.0, 60.0, 60.0);
        let win = Rect::new(200.0, 100.0, 1000.0, 700.0);
        let f0 = bloom(icon, win, 0.0, 14.0);
        assert!((f0.rect.x + f0.rect.w * 0.5 - 930.0).abs() < 1.0);
        assert!(f0.rect.w <= 61.0);
        assert_eq!(f0.opacity, 0.0);
        assert!(f0.blur > 20.0);
        let f1 = bloom(icon, win, 1.0, 14.0);
        assert!((f1.rect.x - win.x).abs() < 0.5 && (f1.rect.w - win.w).abs() < 0.5);
        assert_eq!(f1.blur, 0.0);
        let over = bloom(icon, win, 1.06, 14.0);
        assert!(over.rect.w > win.w, "overshoot grows past target");
    }

    #[test]
    fn genie_covers_window_at_start_and_icon_at_end() {
        let win = Rect::new(100.0, 100.0, 800.0, 600.0);
        let icon = Rect::new(600.0, 980.0, 64.0, 64.0);
        let s0 = ink_genie(win, icon, 0.0, 32);
        assert_eq!(s0.len(), 32);
        assert!((s0[0].dst.y - win.y).abs() < 0.01);
        assert!((s0[31].dst.bottom() - win.bottom()).abs() < 1.0);
        for s in &s0 {
            assert!((s.dst.x - win.x).abs() < 0.5 && (s.dst.w - win.w).abs() < 1.0);
            assert_eq!(s.alpha, 1.0);
        }
        let s1 = ink_genie(win, icon, 1.0, 32);
        for s in &s1 {
            assert!(s.dst.x >= icon.x - 1.0 && s.dst.right() <= icon.right() + 1.0);
            assert!(s.dst.y >= icon.y - 1.0 && s.dst.bottom() <= icon.bottom() + 1.5);
            assert_eq!(s.alpha, 0.0);
        }
        // Mid-flight the bottom is narrower than the top: the pinch.
        let mid = ink_genie(win, icon, 0.45, 32);
        assert!(mid[31].dst.w < mid[0].dst.w * 0.7);
        // Strips are contiguous top to bottom in texture space.
        for pair in mid.windows(2) {
            assert!((pair[0].v1 - pair[1].v0).abs() < 1.0e-6);
        }
    }

    #[test]
    fn genie_handles_icon_above_window() {
        let win = Rect::new(100.0, 300.0, 800.0, 600.0);
        let icon = Rect::new(400.0, 10.0, 64.0, 32.0);
        let s1 = ink_genie(win, icon, 1.0, 16);
        for s in &s1 {
            assert!(s.dst.y >= icon.y - 1.0 && s.dst.bottom() <= icon.bottom() + 1.5);
        }
    }

    #[test]
    fn dust_crumbles_top_down_and_finishes() {
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let (c, r) = dust_grid(rect, 10.0, 2000);
        assert!(c * r <= 2000);
        let field = DustField::new(rect, c, r, &[], MotionPrefs::default());
        assert!(field.specks(0.0).is_empty(), "intact at t=0");
        let early = field.specks(field.duration * 0.25);
        assert!(!early.is_empty());
        let mean_y: f32 = early.iter().map(|s| s.rect.y).sum::<f32>() / early.len() as f32;
        assert!(mean_y < 150.0, "top half goes first, mean y {mean_y}");
        let strip = Rect::new(0.0, 0.0, 400.0, 10.0);
        let top = DustField::new(strip, 40, 1, &[], MotionPrefs::default());
        let flying = top.specks(top.duration * 0.5);
        assert!(!flying.is_empty());
        assert!(
            flying.iter().all(|s| s.rect.y < -10.0),
            "particles drift up"
        );
        assert!(field.specks(field.duration + 0.01).is_empty());
        assert!(field.done(field.duration));
        assert_eq!(field.intact_from(field.duration), 1.0);
    }

    #[test]
    fn dust_is_deterministic() {
        let rect = Rect::new(0.0, 0.0, 200.0, 100.0);
        let a = DustField::new(rect, 20, 10, &[0.2; 200], MotionPrefs::default());
        let b = DustField::new(rect, 20, 10, &[0.2; 200], MotionPrefs::default());
        assert_eq!(a.specks(0.2), b.specks(0.2));
    }

    #[test]
    fn snap_zones_cover_edges_and_corners() {
        let work = Rect::new(0.0, 0.0, 1920.0, 1040.0);
        assert_eq!(snap_zone(2.0, 500.0, work, 12.0), Some(SnapZone::Left));
        assert_eq!(snap_zone(1918.0, 500.0, work, 12.0), Some(SnapZone::Right));
        assert_eq!(snap_zone(900.0, 3.0, work, 12.0), Some(SnapZone::Maximize));
        assert_eq!(snap_zone(3.0, 10.0, work, 12.0), Some(SnapZone::TopLeft));
        assert_eq!(
            snap_zone(1917.0, 1035.0, work, 12.0),
            Some(SnapZone::BottomRight)
        );
        assert_eq!(snap_zone(900.0, 500.0, work, 12.0), None);
        let l = zone_rect(SnapZone::Left, work, 8.0);
        let r = zone_rect(SnapZone::Right, work, 8.0);
        assert!((l.right() + 8.0 - r.x).abs() < 0.01, "gap between halves");
        assert!((r.right() - (work.right() - 8.0)).abs() < 0.01);
        assert!(edge_glow(1.0, 500.0, work, 12.0) > 0.9);
        assert_eq!(edge_glow(900.0, 500.0, work, 12.0), 0.0);
    }

    #[test]
    fn drag_lifts_and_wobbles_on_drop() {
        let mut d = DragMotion::new();
        d.grab();
        for _ in 0..120 {
            d.step(1.0 / 120.0);
        }
        assert!((d.scale() - 1.025).abs() < 0.003);
        assert!(d.shadow_blur() > 50.0);
        d.hover(Some(SnapZone::Left));
        d.step(0.1);
        assert!(d.preview_progress() > 0.3);
        assert_eq!(d.drop_window(), Some(SnapZone::Left));
        let mut crossed_below = false;
        for _ in 0..240 {
            d.step(1.0 / 120.0);
            if d.scale() < 0.999 {
                crossed_below = true;
            }
        }
        assert!(crossed_below, "landing wobble squashes");
        assert!(!d.animating());
        assert!((d.scale() - 1.0).abs() < 0.002);
    }

    #[test]
    fn shake_needs_fast_wide_reversals() {
        let mut s = ShakeDetector::default();
        let mut hit = false;
        let xs = [0.0, 40.0, 0.0, 40.0, 0.0, 40.0, 0.0];
        for x in xs {
            hit |= s.feed(x, 0.05);
        }
        assert!(hit, "four wide reversals in 0.3s shake");
        let mut slow = ShakeDetector::default();
        let mut hit = false;
        for x in xs {
            hit |= slow.feed(x, 0.5);
        }
        assert!(!hit, "slow wiggle is not a shake");
        let mut tiny = ShakeDetector::default();
        let mut hit = false;
        for x in [0.0, 5.0, 0.0, 5.0, 0.0, 5.0, 0.0] {
            hit |= tiny.feed(x, 0.05);
        }
        assert!(!hit, "jitter is not a shake");
    }

    #[test]
    fn carousel_centers_focus_and_orders_depth() {
        let screen = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let cards = carousel(5, 2.0, 1.0, screen, 420.0, 280.0);
        assert_eq!(cards.len(), 5);
        let front = cards.last().unwrap();
        assert_eq!(front.index, 2, "focused card is drawn last");
        assert!(front.angle.abs() < 1.0e-6);
        assert!((front.rect.x + front.rect.w * 0.5 - 960.0).abs() < 0.5);
        let left = cards.iter().find(|c| c.index == 0).unwrap();
        let right = cards.iter().find(|c| c.index == 4).unwrap();
        assert!(left.angle < 0.0 && right.angle > 0.0);
        assert!(left.rect.w < front.rect.w);
    }

    #[test]
    fn perspective_makes_far_edge_shorter() {
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let flat = perspective_slices(rect, 0.0, 8, 900.0);
        for s in &flat {
            assert!((s.dst.h - 300.0).abs() < 0.01);
        }
        let tilted = perspective_slices(rect, 0.6, 8, 900.0);
        assert!(tilted[7].dst.h < tilted[0].dst.h, "right edge turned away");
        let total: f32 = tilted.iter().map(|s| s.dst.w - 0.5).sum();
        assert!(total < 400.0, "foreshortened");
    }

    #[test]
    fn fan_out_staggers() {
        let from = Rect::new(0.0, 0.0, 100.0, 100.0);
        let to = Rect::new(500.0, 500.0, 50.0, 50.0);
        let (_, e0) = fan_out(from, to, 0, 4, 0.3);
        let (_, e3) = fan_out(from, to, 3, 4, 0.3);
        assert!(e0 > e3);
        let (end, e) = fan_out(from, to, 3, 4, 1.0);
        assert_eq!(e, 1.0);
        assert!((end.x - 500.0).abs() < 0.01 && (end.y - 500.0).abs() < 0.01);
    }

    #[test]
    fn touches_respect_reduce_motion() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(focus_ripple(r, 0.1, MotionPrefs::default()).is_some());
        assert!(focus_ripple(r, 5.0, MotionPrefs::default()).is_none());
        assert!(focus_ripple(r, 0.1, MotionPrefs::reduced()).is_none());
        assert_eq!(
            breath(3.0, (1.0, 1.0), MotionPrefs::reduced()),
            (0.0, 0.0, 1.0)
        );
        let (dx, _, s) = breath(0.0, (1.0, 0.0), MotionPrefs::default());
        assert!(dx < -10.0 && s > 1.0);
        let p = running_dot_pulse(0.3, Some(0.1));
        assert!((0.0..=1.0).contains(&p));
    }
}
