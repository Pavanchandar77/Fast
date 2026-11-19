//! Vsync-paced frame scheduler.
//!
//! A frame is submitted only when something is dirty or a spring is still
//! moving. Idle input produces zero redraws, so the host does not acquire a
//! drawable and the process sleeps. The present mode is FIFO (waitable
//! swapchain). On Windows the host also calls `DwmFlush` after present.

use nx_ui::{FrameClock, FrameDecision, TARGET_REFRESH_HZ};

/// How the host should present. FIFO blocks on the display refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentMode {
    /// wgpu `PresentMode::Fifo`. One image per vblank.
    Fifo,
}

/// Pacing the Win32 host applies around the pure scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentPace {
    pub mode: PresentMode,
    /// Call `DwmFlush` after present so the CPU waits in dwm.dll instead of spinning.
    pub dwm_flush: bool,
}

pub fn present_pace() -> PresentPace {
    PresentPace {
        mode: PresentMode::Fifo,
        dwm_flush: true,
    }
}

#[derive(Clone, Debug)]
pub struct FrameScheduler {
    clock: FrameClock,
    last: FrameDecision,
}

impl FrameScheduler {
    pub fn new(hz: f32) -> Self {
        let hz = if hz.is_finite() && hz >= 1.0 {
            hz
        } else {
            TARGET_REFRESH_HZ
        };
        Self {
            clock: FrameClock::new(hz),
            last: FrameDecision::Idle,
        }
    }

    pub fn period(&self) -> f64 {
        self.clock.period()
    }

    pub fn redraws(&self) -> u64 {
        self.clock.redraws
    }

    pub fn skipped(&self) -> u64 {
        self.clock.skipped
    }

    pub fn last(&self) -> FrameDecision {
        self.last
    }

    /// True only for the decision that should build a scene and acquire a drawable.
    pub fn should_paint(&self) -> bool {
        matches!(self.last, FrameDecision::Draw { .. })
    }

    pub fn begin(&mut self, now_s: f64, animating: bool, dirty: bool) -> FrameDecision {
        let now_s = if now_s.is_finite() { now_s } else { 0.0 };
        let decision = self.clock.begin_frame(now_s, animating, dirty);
        self.last = decision;
        decision
    }
}

impl Default for FrameScheduler {
    fn default() -> Self {
        Self::new(TARGET_REFRESH_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_produces_zero_redraws() {
        let mut scheduler = FrameScheduler::new(120.0);
        for i in 0..60 {
            let decision = scheduler.begin(i as f64 * 0.05, false, false);
            assert!(matches!(decision, FrameDecision::Idle));
            assert!(!scheduler.should_paint());
        }
        assert_eq!(scheduler.redraws(), 0);
        assert_eq!(scheduler.skipped(), 60);
    }

    #[test]
    fn a_dirty_frame_draws_once_then_idles() {
        let mut scheduler = FrameScheduler::new(120.0);
        assert!(matches!(
            scheduler.begin(0.0, false, true),
            FrameDecision::Draw { .. }
        ));
        assert!(scheduler.should_paint());
        assert!(matches!(
            scheduler.begin(0.05, false, false),
            FrameDecision::Idle
        ));
        assert!(!scheduler.should_paint());
        assert_eq!(scheduler.redraws(), 1);
    }

    #[test]
    fn animation_is_quantized_to_the_refresh_period() {
        let mut scheduler = FrameScheduler::new(120.0);
        assert!(matches!(
            scheduler.begin(0.0, true, true),
            FrameDecision::Draw { .. }
        ));
        match scheduler.begin(0.001, true, false) {
            FrameDecision::Wait { seconds } => assert!(seconds > 0.0 && seconds < 0.02),
            other => panic!("expected wait, got {other:?}"),
        }
        assert_eq!(scheduler.redraws(), 1);
        match scheduler.begin(0.009, true, false) {
            FrameDecision::Draw { dt } => assert!(dt > 0.006 && dt < 0.012),
            other => panic!("expected draw, got {other:?}"),
        }
        assert_eq!(scheduler.redraws(), 2);
    }

    #[test]
    fn present_pace_is_fifo_plus_dwm_flush() {
        let pace = present_pace();
        assert_eq!(pace.mode, PresentMode::Fifo);
        assert!(pace.dwm_flush);
    }

    #[test]
    fn non_finite_clock_does_not_panic() {
        let mut scheduler = FrameScheduler::new(f32::NAN);
        let decision = scheduler.begin(f64::NAN, false, true);
        assert!(matches!(decision, FrameDecision::Draw { .. }));
        assert!(scheduler.period().is_finite());
    }
}
