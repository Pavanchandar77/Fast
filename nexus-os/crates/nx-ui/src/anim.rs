//! Vsync-paced frame clock.
//!
//! A frame is submitted only when something is dirty or a spring is still
//! moving. Idle input produces zero redraws. Draw deltas are clamped to a
//! small multiple of the refresh period so a stall does not fast-forward
//! every animation.

/// Product budgets. First frame is CPU scene build; frame time is GPU+CPU.
pub const FIRST_FRAME_BUDGET_MS: f32 = 100.0;
pub const FRAME_BUDGET_MS: f32 = 4.0;
pub const TARGET_REFRESH_HZ: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameDecision {
    /// Nothing is animating and nothing is dirty. Do not acquire a drawable.
    Idle,
    /// A frame is due, but not yet. `seconds` is the wait until the next slot.
    Wait { seconds: f32 },
    /// Submit one frame and step springs by `dt` seconds.
    Draw { dt: f32 },
}

#[derive(Clone, Debug)]
pub struct FrameClock {
    pub hz: f32,
    pub redraws: u64,
    pub skipped: u64,
    last: Option<f64>,
}

impl FrameClock {
    pub fn new(hz: f32) -> Self {
        Self {
            hz: hz.max(1.0),
            redraws: 0,
            skipped: 0,
            last: None,
        }
    }

    pub fn period(&self) -> f64 {
        1.0 / f64::from(self.hz)
    }

    /// `now_s` is a monotonic clock in seconds. `animating` is true while any
    /// spring is live. `dirty` is true for input that changes the picture.
    pub fn begin_frame(&mut self, now_s: f64, animating: bool, dirty: bool) -> FrameDecision {
        if !animating && !dirty {
            self.skipped += 1;
            return FrameDecision::Idle;
        }
        let period = self.period();
        let Some(last) = self.last else {
            self.last = Some(now_s);
            self.redraws += 1;
            return FrameDecision::Draw { dt: period as f32 };
        };
        let elapsed = (now_s - last).max(0.0);
        if elapsed < period * 0.85 && !dirty {
            self.skipped += 1;
            return FrameDecision::Wait {
                seconds: ((period - elapsed).max(0.0)) as f32,
            };
        }
        let dt = if elapsed > period * 3.0 {
            period
        } else {
            elapsed
        };
        self.last = Some(now_s);
        self.redraws += 1;
        FrameDecision::Draw { dt: dt as f32 }
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(TARGET_REFRESH_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spring::{Spring, SpringConfig};

    #[test]
    fn idle_produces_zero_redraws() {
        let mut clock = FrameClock::new(120.0);
        for i in 0..30 {
            let decision = clock.begin_frame(i as f64 * 0.05, false, false);
            assert!(matches!(decision, FrameDecision::Idle));
        }
        assert_eq!(clock.redraws, 0);
        assert_eq!(clock.skipped, 30);
    }

    #[test]
    fn dirty_draws_once_then_idles() {
        let mut clock = FrameClock::new(120.0);
        assert!(matches!(
            clock.begin_frame(0.0, false, true),
            FrameDecision::Draw { .. }
        ));
        assert!(matches!(
            clock.begin_frame(0.05, false, false),
            FrameDecision::Idle
        ));
        assert!(matches!(
            clock.begin_frame(1.0, false, false),
            FrameDecision::Idle
        ));
        assert_eq!(clock.redraws, 1);
    }

    #[test]
    fn vsync_quantizes_animation_frames() {
        let mut clock = FrameClock::new(120.0);
        assert!(matches!(
            clock.begin_frame(0.0, true, true),
            FrameDecision::Draw { .. }
        ));
        assert!(matches!(
            clock.begin_frame(0.001, true, false),
            FrameDecision::Wait { .. }
        ));
        assert!(matches!(
            clock.begin_frame(0.002, true, false),
            FrameDecision::Wait { .. }
        ));
        match clock.begin_frame(0.009, true, false) {
            FrameDecision::Draw { dt } => assert!(dt > 0.007 && dt < 0.012),
            other => panic!("expected draw, got {other:?}"),
        }
        assert_eq!(clock.redraws, 2);
    }

    #[test]
    fn springs_stop_requesting_redraws() {
        let mut cfg = SpringConfig::macos(0.25, 1.0);
        cfg.rest_pos = 0.05;
        cfg.rest_vel = 0.5;
        let mut spring = Spring::new(0.0, cfg);
        spring.set_target(240.0);
        let mut clock = FrameClock::new(120.0);
        let mut now = 0.0f64;
        let mut guard = 0;
        loop {
            let animating = !spring.settled();
            match clock.begin_frame(now, animating, guard == 0 && animating) {
                FrameDecision::Draw { dt } => {
                    spring.step(dt);
                    now += f64::from(dt.max(1.0 / 120.0));
                }
                FrameDecision::Wait { seconds } => now += f64::from(seconds.max(1.0 / 1000.0)),
                FrameDecision::Idle => break,
            }
            guard += 1;
            assert!(guard < 800, "spring did not settle");
        }
        assert!(spring.settled());
        assert!(clock.redraws > 0);
        let redraws = clock.redraws;
        for _ in 0..8 {
            assert!(matches!(
                clock.begin_frame(now, false, false),
                FrameDecision::Idle
            ));
            now += 0.1;
        }
        assert_eq!(clock.redraws, redraws);
    }
}
