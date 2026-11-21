//! Expressive monochrome eyes for the Dynamic Island. Pure state machine:
//! the view feeds it time (+ optional cursor direction) and draws `EyeFrame`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Listening,
    Thinking,
    Happy,
    Error,
    Sleepy,
}

/// What to draw this frame, for each eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeFrame {
    /// 0 = closed, 1 = open.
    pub openness: f32,
    /// Pupil offset in -1..1 (x right, y down).
    pub gaze: (f32, f32),
    pub mood: Mood,
    /// 0..1 phase for looping animations (thinking orbit, listening pulse).
    pub phase: f32,
}

pub struct Eyes {
    mood: Mood,
    mood_until: Option<f32>,
    next_blink: f32,
    blink_start: Option<f32>,
    gaze: (f32, f32),
    gaze_target: (f32, f32),
    next_glance: f32,
    last_t: f32,
    seed: u64,
}

const BLINK_SECS: f32 = 0.16;

impl Default for Eyes {
    fn default() -> Self {
        Self::new(0x9E37_79B9_7F4A_7C15)
    }
}

impl Eyes {
    pub fn new(seed: u64) -> Self {
        let mut e = Self {
            mood: Mood::Idle,
            mood_until: None,
            next_blink: 0.0,
            blink_start: None,
            gaze: (0.0, 0.0),
            gaze_target: (0.0, 0.0),
            next_glance: 1.5,
            last_t: 0.0,
            seed: seed | 1,
        };
        e.next_blink = 1.0 + e.rand() * 3.0;
        e
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let v = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// Set a mood; transient moods (Happy/Error) revert to Idle after `hold`.
    pub fn set_mood(&mut self, mood: Mood, t: f32, hold: Option<f32>) {
        self.mood = mood;
        self.mood_until = hold.map(|h| t + h);
    }

    /// Advance to time `t` (seconds). `cursor` is a direction in -1..1 if known.
    pub fn update(&mut self, t: f32, cursor: Option<(f32, f32)>) {
        let dt = (t - self.last_t).clamp(0.0, 0.25);
        self.last_t = t;
        if let Some(until) = self.mood_until {
            if t >= until {
                self.mood = Mood::Idle;
                self.mood_until = None;
            }
        }
        if self.blink_start.is_none() && t >= self.next_blink && self.mood != Mood::Happy {
            self.blink_start = Some(t);
            // Occasional double blink.
            let gap = if self.rand() < 0.18 {
                0.3
            } else {
                2.2 + self.rand() * 4.5
            };
            self.next_blink = t + gap;
        }
        if let Some(s) = self.blink_start {
            if t - s > BLINK_SECS {
                self.blink_start = None;
            }
        }
        match self.mood {
            Mood::Thinking => {
                let a = t * std::f32::consts::TAU * 0.9;
                self.gaze_target = (a.cos() * 0.7, a.sin() * 0.5 - 0.2);
            }
            Mood::Listening => self.gaze_target = (0.0, 0.15),
            Mood::Error => self.gaze_target = (0.0, 0.0),
            _ => {
                if let Some(c) = cursor {
                    self.gaze_target = (c.0.clamp(-1.0, 1.0), c.1.clamp(-1.0, 1.0));
                } else if t >= self.next_glance {
                    let r1 = self.rand();
                    let r2 = self.rand();
                    let r3 = self.rand();
                    self.gaze_target = if r1 < 0.45 {
                        (0.0, 0.0)
                    } else {
                        (r2 * 2.0 - 1.0, (r3 * 2.0 - 1.0) * 0.5)
                    };
                    self.next_glance = t + 1.2 + self.rand() * 3.0;
                }
            }
        }
        // Critically-damped-ish follow (fast saccade, soft settle).
        let k = 1.0 - (-dt * 18.0).exp();
        self.gaze.0 += (self.gaze_target.0 - self.gaze.0) * k;
        self.gaze.1 += (self.gaze_target.1 - self.gaze.1) * k;
    }

    pub fn frame(&self, t: f32) -> EyeFrame {
        let blink = self.blink_start.map(|s| {
            let p = ((t - s) / BLINK_SECS).clamp(0.0, 1.0);
            1.0 - (p * std::f32::consts::PI).sin()
        });
        let base = match self.mood {
            Mood::Listening => 0.55,
            Mood::Sleepy => 0.25,
            Mood::Thinking => 0.8,
            _ => 1.0,
        };
        let phase = match self.mood {
            Mood::Thinking => (t * 1.6).fract(),
            Mood::Listening => (t * 1.2).fract(),
            _ => (t * 0.5).fract(),
        };
        EyeFrame {
            openness: base * blink.unwrap_or(1.0),
            gaze: self.gaze,
            mood: self.mood,
            phase,
        }
    }

    /// True while something is moving and the view should keep 60fps frames.
    pub fn animating(&self) -> bool {
        self.blink_start.is_some()
            || matches!(self.mood, Mood::Thinking | Mood::Listening)
            || (self.gaze.0 - self.gaze_target.0).abs() > 0.01
            || (self.gaze.1 - self.gaze_target.1).abs() > 0.01
    }

    /// Seconds until the next scheduled change when idle.
    pub fn next_event_in(&self, t: f32) -> f32 {
        let mut n = (self.next_blink - t).min(self.next_glance - t);
        if let Some(u) = self.mood_until {
            n = n.min(u - t);
        }
        n.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eyes_blink_and_reopen() {
        let mut e = Eyes::new(42);
        let mut saw_closed = false;
        let mut t = 0.0;
        while t < 12.0 {
            e.update(t, None);
            if e.frame(t).openness < 0.2 {
                saw_closed = true;
            }
            t += 1.0 / 120.0;
        }
        assert!(saw_closed, "eyes should blink within 12s");
        let mut later = e.frame(t).openness;
        while later < 0.99 {
            t += 1.0 / 120.0;
            e.update(t, None);
            later = e.frame(t).openness;
        }
        assert!(later > 0.99);
    }

    #[test]
    fn transient_mood_reverts() {
        let mut e = Eyes::new(1);
        e.set_mood(Mood::Happy, 0.0, Some(1.0));
        e.update(0.5, None);
        assert_eq!(e.mood(), Mood::Happy);
        e.update(1.1, None);
        assert_eq!(e.mood(), Mood::Idle);
    }

    #[test]
    fn eyes_follow_cursor() {
        let mut e = Eyes::new(3);
        for i in 0..60 {
            e.update(i as f32 / 60.0, Some((1.0, 0.0)));
        }
        assert!(e.frame(1.0).gaze.0 > 0.9);
    }
}
