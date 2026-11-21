//! Dynamic Island live activities: a priority-stacked set of glanceable
//! states (calls, HUDs, timers, updates, toasts…) plus a tiny spring used for
//! the pill's morphing size. Pure model, no GPUI, fully unit-tested.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    Volume,
    Brightness,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LiveKind {
    /// Camera / microphone in use (meeting / call indicator).
    Call {
        app: String,
        mic: bool,
        cam: bool,
        since: f64,
    },
    /// Volume / brightness HUD that replaces the Windows flyout.
    Hud {
        what: HudKind,
        level: f32,
        muted: bool,
    },
    Timer {
        ends_at: f64,
        total: f64,
        label: String,
    },
    Stopwatch {
        started: f64,
    },
    Bluetooth {
        device: String,
        battery: Option<u8>,
        connected: bool,
    },
    Screenshot {
        path: PathBuf,
    },
    /// Windows Update / Nexus compatibility status (nx-compat).
    Update {
        title: String,
        detail: String,
        progress: Option<f32>,
    },
    Toast {
        glyph: &'static str,
        title: String,
        body: String,
    },
    /// Assistant needs a CONFIRM tap.
    Confirm {
        action: String,
    },
}

impl LiveKind {
    /// Base priority; higher wins the compact pill.
    pub fn priority(&self) -> u8 {
        match self {
            LiveKind::Confirm { .. } => 100,
            LiveKind::Call { .. } => 90,
            LiveKind::Hud { .. } => 85,
            LiveKind::Screenshot { .. } => 75,
            LiveKind::Bluetooth { .. } => 70,
            LiveKind::Update { .. } => 65,
            LiveKind::Timer { .. } => 60,
            LiveKind::Stopwatch { .. } => 55,
            LiveKind::Toast { .. } => 40,
        }
    }

    /// Survives Focus / Do Not Disturb.
    pub fn breaks_focus(&self) -> bool {
        matches!(
            self,
            LiveKind::Confirm { .. }
                | LiveKind::Call { .. }
                | LiveKind::Hud { .. }
                | LiveKind::Timer { .. }
                | LiveKind::Stopwatch { .. }
        )
    }

    fn slot(&self) -> u8 {
        match self {
            LiveKind::Call { .. } => 1,
            LiveKind::Hud { .. } => 2,
            LiveKind::Stopwatch { .. } => 3,
            LiveKind::Bluetooth { .. } => 4,
            LiveKind::Screenshot { .. } => 5,
            LiveKind::Update { .. } => 6,
            LiveKind::Confirm { .. } => 7,
            // Timers and toasts can stack.
            LiveKind::Timer { .. } | LiveKind::Toast { .. } => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub id: u64,
    pub kind: LiveKind,
    pub created: f64,
    pub expires: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct LiveState {
    pub items: Vec<Live>,
    pub focus: bool,
    pub reminders: Vec<(f64, String)>,
    next_id: u64,
}

impl LiveState {
    /// Show `kind` for `ttl` seconds (None = until removed). Singleton kinds
    /// (HUD, call, stopwatch…) replace their previous instance; toasts with
    /// the same title are coalesced.
    pub fn push(&mut self, kind: LiveKind, now: f64, ttl: Option<f64>) -> Option<u64> {
        if self.focus && !kind.breaks_focus() {
            return None;
        }
        let slot = kind.slot();
        self.items.retain(|l| {
            let same_slot = slot != 0 && l.kind.slot() == slot;
            let same_toast = matches!((&l.kind, &kind),
                (LiveKind::Toast { title: a, .. }, LiveKind::Toast { title: b, .. }) if a == b);
            !(same_slot || same_toast)
        });
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Live {
            id,
            kind,
            created: now,
            expires: ttl.map(|t| now + t),
        });
        Some(id)
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|l| l.id != id);
    }

    pub fn remove_where(&mut self, f: impl Fn(&LiveKind) -> bool) {
        self.items.retain(|l| !f(&l.kind));
    }

    pub fn add_reminder(&mut self, at: f64, text: String) {
        self.reminders.push((at, text));
    }

    /// Expire items; fire finished timers and due reminders as toasts.
    pub fn tick(&mut self, now: f64) {
        let mut fired = Vec::new();
        self.items.retain(|l| {
            if let LiveKind::Timer { ends_at, label, .. } = &l.kind {
                if now >= *ends_at {
                    fired.push(if label.is_empty() {
                        "TIMER DONE".to_string()
                    } else {
                        format!("{} · DONE", label.to_uppercase())
                    });
                    return false;
                }
            }
            l.expires.is_none_or(|e| now < e)
        });
        let (due, later): (Vec<_>, Vec<_>) = self.reminders.drain(..).partition(|r| now >= r.0);
        self.reminders = later;
        for title in fired {
            // Timers break focus mode: you asked for them.
            let was = self.focus;
            self.focus = false;
            self.push(
                LiveKind::Toast {
                    glyph: "◷",
                    title,
                    body: String::new(),
                },
                now,
                Some(8.0),
            );
            self.focus = was;
        }
        for (_, text) in due {
            let was = self.focus;
            self.focus = false;
            self.push(
                LiveKind::Toast {
                    glyph: "◉",
                    title: "REMINDER".into(),
                    body: text,
                },
                now,
                Some(12.0),
            );
            self.focus = was;
        }
    }

    /// Highest priority item (newest wins ties).
    pub fn top(&self) -> Option<&Live> {
        self.items.iter().max_by(|a, b| {
            a.kind
                .priority()
                .cmp(&b.kind.priority())
                .then(a.created.total_cmp(&b.created))
        })
    }

    /// All items, highest priority first.
    pub fn stack(&self) -> Vec<&Live> {
        let mut v: Vec<&Live> = self.items.iter().collect();
        v.sort_by(|a, b| {
            b.kind
                .priority()
                .cmp(&a.kind.priority())
                .then(b.created.total_cmp(&a.created))
        });
        v
    }

    pub fn has_timer(&self) -> bool {
        self.items
            .iter()
            .any(|l| matches!(l.kind, LiveKind::Timer { .. } | LiveKind::Stopwatch { .. }))
    }
}

/// Damped spring for the island's morphing width/height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring1 {
    pub pos: f32,
    pub vel: f32,
    pub target: f32,
}

impl Spring1 {
    pub fn new(v: f32) -> Self {
        Self {
            pos: v,
            vel: 0.0,
            target: v,
        }
    }

    /// Semi-implicit Euler, slightly under-damped for a lively settle.
    pub fn step(&mut self, dt: f32) {
        const K: f32 = 420.0;
        const C: f32 = 32.0;
        let mut t = dt.clamp(0.0, 0.05);
        while t > 0.0 {
            let h = t.min(1.0 / 240.0);
            let a = K * (self.target - self.pos) - C * self.vel;
            self.vel += a * h;
            self.pos += self.vel * h;
            t -= h;
        }
    }

    pub fn settled(&self) -> bool {
        (self.pos - self.target).abs() < 0.5 && self.vel.abs() < 0.5
    }
}

pub fn fmt_clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn fmt_rate(bytes_per_sec: f64) -> String {
    let b = bytes_per_sec.max(0.0);
    if b >= 1_000_000.0 {
        format!("{:.1} MB/s", b / 1_000_000.0)
    } else if b >= 1_000.0 {
        format!("{:.0} KB/s", b / 1_000.0)
    } else {
        format!("{b:.0} B/s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(t: &str) -> LiveKind {
        LiveKind::Toast {
            glyph: "*",
            title: t.into(),
            body: String::new(),
        }
    }

    #[test]
    fn priority_stacking_picks_call_over_timer_and_toast() {
        let mut s = LiveState::default();
        s.push(toast("CAPS LOCK"), 0.0, Some(2.0));
        s.push(
            LiveKind::Timer {
                ends_at: 100.0,
                total: 100.0,
                label: String::new(),
            },
            0.0,
            None,
        );
        s.push(
            LiveKind::Call {
                app: "Teams".into(),
                mic: true,
                cam: false,
                since: 0.0,
            },
            1.0,
            None,
        );
        assert!(matches!(s.top().unwrap().kind, LiveKind::Call { .. }));
        assert_eq!(s.stack().len(), 3);
    }

    #[test]
    fn hud_replaces_previous_hud_and_expires() {
        let mut s = LiveState::default();
        for lvl in [0.1, 0.2, 0.3] {
            s.push(
                LiveKind::Hud {
                    what: HudKind::Volume,
                    level: lvl,
                    muted: false,
                },
                0.0,
                Some(1.5),
            );
        }
        assert_eq!(s.items.len(), 1);
        s.tick(2.0);
        assert!(s.items.is_empty());
    }

    #[test]
    fn timer_fires_toast_even_in_focus() {
        let mut s = LiveState::default();
        s.push(
            LiveKind::Timer {
                ends_at: 5.0,
                total: 5.0,
                label: "tea".into(),
            },
            0.0,
            None,
        );
        s.focus = true;
        assert!(s.push(toast("USB"), 1.0, Some(3.0)).is_none());
        s.tick(5.0);
        assert!(
            matches!(&s.top().unwrap().kind, LiveKind::Toast { title, .. } if title == "TEA · DONE")
        );
    }

    #[test]
    fn reminders_fire_when_due() {
        let mut s = LiveState::default();
        s.add_reminder(10.0, "stretch".into());
        s.tick(9.0);
        assert!(s.items.is_empty());
        s.tick(10.0);
        assert!(
            matches!(&s.top().unwrap().kind, LiveKind::Toast { body, .. } if body == "stretch")
        );
    }

    #[test]
    fn spring_settles_on_target() {
        let mut sp = Spring1::new(280.0);
        sp.target = 664.0;
        for _ in 0..240 {
            sp.step(1.0 / 120.0);
        }
        assert!(sp.settled());
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_clock(65.0), "1:05");
        assert_eq!(fmt_clock(3725.0), "1:02:05");
        assert_eq!(fmt_rate(2_500_000.0), "2.5 MB/s");
        assert_eq!(fmt_rate(1500.0), "2 KB/s");
    }
}
