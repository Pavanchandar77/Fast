//! High-precision interruptible damped harmonic oscillator spring physics solver.
//! Supports mid-flight target retargeting while preserving continuous velocity vectors.
//! Completely pure, zero OS/platform dependencies, deterministic and testable.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpringConfig {
    pub stiffness: f32, // Tension k
    pub damping: f32,   // Damping coefficient c
    pub mass: f32,      // Mass m
    pub rest_velocity_threshold: f32,
    pub rest_displacement_threshold: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self {
            stiffness: 280.0,
            damping: 26.0,
            mass: 1.0,
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }
}

impl SpringConfig {
    pub fn new(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            stiffness,
            damping,
            mass: if mass <= 0.0 { 1.0 } else { mass },
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }

    /// Snappy animation profile: high stiffness, moderate damping
    pub fn snappy() -> Self {
        Self {
            stiffness: 350.0,
            damping: 30.0,
            mass: 1.0,
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }

    /// Smooth macOS-like gentle deceleration profile
    pub fn smooth() -> Self {
        Self {
            stiffness: 220.0,
            damping: 28.0,
            mass: 1.0,
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }

    /// Bouncy spring profile: high stiffness, low damping
    pub fn bouncy() -> Self {
        Self {
            stiffness: 300.0,
            damping: 18.0,
            mass: 1.0,
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.stiffness <= 0.0 {
            return Err("Spring stiffness must be positive");
        }
        if self.damping < 0.0 {
            return Err("Spring damping must be non-negative");
        }
        if self.mass <= 0.0 {
            return Err("Spring mass must be positive");
        }
        if self.rest_velocity_threshold <= 0.0 {
            return Err("Rest velocity threshold must be positive");
        }
        if self.rest_displacement_threshold <= 0.0 {
            return Err("Rest displacement threshold must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring1D {
    pub current: f32,
    pub velocity: f32,
    pub target: f32,
    pub config: SpringConfig,
}

impl Spring1D {
    pub fn new(initial: f32, config: SpringConfig) -> Self {
        Self {
            current: initial,
            velocity: 0.0,
            target: initial,
            config,
        }
    }

    /// Retargets the spring mid-flight, preserving the current instantaneous velocity.
    pub fn set_target(&mut self, new_target: f32) {
        self.target = new_target;
    }

    /// Sets the current position directly.
    pub fn set_current(&mut self, new_current: f32) {
        self.current = new_current;
    }

    /// Snaps directly to target, zeroes velocity.
    pub fn snap_to_target(&mut self) {
        self.current = self.target;
        self.velocity = 0.0;
    }

    /// Advances the spring simulation by dt seconds using symplectic semi-implicit Euler.
    /// Highly stable for dynamic UI/window animations up to high frequencies.
    pub fn step(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        let displacement = self.current - self.target;
        let spring_force = -self.config.stiffness * displacement;
        let damping_force = -self.config.damping * self.velocity;
        let mass = if self.config.mass <= 0.0 {
            1.0
        } else {
            self.config.mass
        };
        let acceleration = (spring_force + damping_force) / mass;

        self.velocity += acceleration * dt;
        self.current += self.velocity * dt;

        if self.is_at_rest() {
            self.current = self.target;
            self.velocity = 0.0;
        }
    }

    pub fn is_at_rest(&self) -> bool {
        (self.current - self.target).abs() < self.config.rest_displacement_threshold
            && self.velocity.abs() < self.config.rest_velocity_threshold
    }
}

/// 2D Spring Solver for Point/Position Animations (X, Y)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring2D {
    pub x: Spring1D,
    pub y: Spring1D,
}

impl Spring2D {
    pub fn new(x: f32, y: f32, config: SpringConfig) -> Self {
        Self {
            x: Spring1D::new(x, config),
            y: Spring1D::new(y, config),
        }
    }

    pub fn set_target(&mut self, target_x: f32, target_y: f32) {
        self.x.set_target(target_x);
        self.y.set_target(target_y);
    }

    pub fn step(&mut self, dt: f32) {
        self.x.step(dt);
        self.y.step(dt);
    }

    pub fn is_at_rest(&self) -> bool {
        self.x.is_at_rest() && self.y.is_at_rest()
    }

    pub fn current_pos(&self) -> (f32, f32) {
        (self.x.current, self.y.current)
    }

    pub fn snap_to_target(&mut self) {
        self.x.snap_to_target();
        self.y.snap_to_target();
    }
}

/// 4D Spring Solver for Window Rectangles (X, Y, Width, Height)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringRect {
    pub x: Spring1D,
    pub y: Spring1D,
    pub width: Spring1D,
    pub height: Spring1D,
}

impl Default for SpringRect {
    fn default() -> Self {
        Self::new(0.0, 0.0, 800.0, 600.0, SpringConfig::default())
    }
}

impl SpringRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32, config: SpringConfig) -> Self {
        Self {
            x: Spring1D::new(x, config),
            y: Spring1D::new(y, config),
            width: Spring1D::new(width, config),
            height: Spring1D::new(height, config),
        }
    }

    pub fn set_target(&mut self, target_x: f32, target_y: f32, target_w: f32, target_h: f32) {
        self.x.set_target(target_x);
        self.y.set_target(target_y);
        self.width.set_target(target_w);
        self.height.set_target(target_h);
    }

    pub fn step(&mut self, dt: f32) {
        self.x.step(dt);
        self.y.step(dt);
        self.width.step(dt);
        self.height.step(dt);
    }

    pub fn is_at_rest(&self) -> bool {
        self.x.is_at_rest()
            && self.y.is_at_rest()
            && self.width.is_at_rest()
            && self.height.is_at_rest()
    }

    pub fn snap_to_target(&mut self) {
        self.x.snap_to_target();
        self.y.snap_to_target();
        self.width.snap_to_target();
        self.height.snap_to_target();
    }

    pub fn current_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.x.current.round() as i32,
            self.y.current.round() as i32,
            self.width.current.round().max(10.0) as i32,
            self.height.current.round().max(10.0) as i32,
        )
    }

    pub fn target_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.x.target.round() as i32,
            self.y.target.round() as i32,
            self.width.target.round().max(10.0) as i32,
            self.height.target.round().max(10.0) as i32,
        )
    }
}

pub struct SpringSolver;

impl SpringSolver {
    pub fn create_window_spring(
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        config: SpringConfig,
    ) -> SpringRect {
        SpringRect::new(x as f32, y as f32, w as f32, h as f32, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spring_convergence() {
        let config = SpringConfig::default();
        let mut spring = Spring1D::new(0.0, config);
        spring.set_target(500.0);

        let dt = 1.0 / 120.0;
        let mut iterations = 0;
        while !spring.is_at_rest() && iterations < 600 {
            spring.step(dt);
            iterations += 1;
        }

        assert!(spring.is_at_rest());
        assert!((spring.current - 500.0).abs() < 1.0);
    }

    #[test]
    fn test_mid_flight_interruption_preserves_velocity() {
        let config = SpringConfig::default();
        let mut spring = Spring1D::new(0.0, config);
        spring.set_target(1000.0);

        let dt = 1.0 / 120.0;
        for _ in 0..15 {
            spring.step(dt);
        }

        let mid_velocity = spring.velocity;
        assert!(mid_velocity > 10.0);

        spring.set_target(200.0);
        assert_eq!(
            spring.velocity, mid_velocity,
            "Velocity must be preserved across retargeting"
        );
    }

    #[test]
    fn test_window_rect_spring() {
        let config = SpringConfig::default();
        let mut rect_spring = SpringRect::new(0.0, 0.0, 800.0, 600.0, config);
        rect_spring.set_target(100.0, 200.0, 1200.0, 800.0);

        let dt = 1.0 / 120.0;
        for _ in 0..200 {
            rect_spring.step(dt);
        }

        let (x, y, w, h) = rect_spring.current_rect();
        assert_eq!(x, 100);
        assert_eq!(y, 200);
        assert_eq!(w, 1200);
        assert_eq!(h, 800);
    }

    #[test]
    fn test_snap_to_target() {
        let mut s = Spring1D::new(10.0, SpringConfig::default());
        s.set_target(100.0);
        s.velocity = 55.0;
        s.snap_to_target();
        assert_eq!(s.current, 100.0);
        assert_eq!(s.velocity, 0.0);
        assert!(s.is_at_rest());
    }

    #[test]
    fn test_spring_config_profiles() {
        let snappy = SpringConfig::snappy();
        let smooth = SpringConfig::smooth();
        let bouncy = SpringConfig::bouncy();
        assert!(snappy.validate().is_ok());
        assert!(smooth.validate().is_ok());
        assert!(bouncy.validate().is_ok());
        assert!(snappy.stiffness > smooth.stiffness);
        assert!(bouncy.damping < smooth.damping);
    }

    #[test]
    fn test_spring_config_validation_invalid() {
        let mut cfg = SpringConfig {
            stiffness: -10.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
        cfg.stiffness = 200.0;
        cfg.damping = -1.0;
        assert!(cfg.validate().is_err());
        cfg.damping = 20.0;
        cfg.mass = 0.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_spring_2d() {
        let mut s2 = Spring2D::new(0.0, 0.0, SpringConfig::default());
        s2.set_target(100.0, 200.0);
        let dt = 1.0 / 60.0;
        for _ in 0..300 {
            s2.step(dt);
        }
        assert!(s2.is_at_rest());
        let (x, y) = s2.current_pos();
        assert!((x - 100.0).abs() < 1.0);
        assert!((y - 200.0).abs() < 1.0);
    }

    #[test]
    fn test_spring_2d_snap_to_target() {
        let mut s2 = Spring2D::new(0.0, 0.0, SpringConfig::default());
        s2.set_target(300.0, 400.0);
        s2.snap_to_target();
        assert!(s2.is_at_rest());
        assert_eq!(s2.current_pos(), (300.0, 400.0));
    }

    #[test]
    fn test_spring_zero_or_negative_dt() {
        let mut s = Spring1D::new(0.0, SpringConfig::default());
        s.set_target(100.0);
        s.step(0.0);
        assert_eq!(s.current, 0.0);
        s.step(-0.01);
        assert_eq!(s.current, 0.0);
    }

    #[test]
    fn test_spring_set_current() {
        let mut s = Spring1D::new(0.0, SpringConfig::default());
        s.set_current(42.0);
        assert_eq!(s.current, 42.0);
    }

    #[test]
    fn test_spring_rect_snap_and_target() {
        let mut sr = SpringRect::new(0.0, 0.0, 100.0, 100.0, SpringConfig::default());
        sr.set_target(50.0, 60.0, 500.0, 600.0);
        assert_eq!(sr.target_rect(), (50, 60, 500, 600));
        sr.snap_to_target();
        assert!(sr.is_at_rest());
        assert_eq!(sr.current_rect(), (50, 60, 500, 600));
    }

    #[test]
    fn test_spring_rect_min_dimensions() {
        let sr = SpringRect::new(0.0, 0.0, 2.0, 3.0, SpringConfig::default());
        let (_, _, w, h) = sr.current_rect();
        assert_eq!(w, 10, "Minimum width must be clamped to 10");
        assert_eq!(h, 10, "Minimum height must be clamped to 10");
    }

    #[test]
    fn test_spring_solver_create() {
        let sr = SpringSolver::create_window_spring(10, 20, 300, 400, SpringConfig::default());
        assert_eq!(sr.current_rect(), (10, 20, 300, 400));
    }
}
