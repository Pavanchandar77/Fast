//! High-precision interruptible damped harmonic oscillator spring physics solver.
//! Supports mid-flight target retargeting while preserving continuous velocity vectors.

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
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

    /// Advances the spring simulation by dt seconds using symplectic semi-implicit Euler.
    /// Highly stable for dynamic UI/window animations up to high frequencies.
    pub fn step(&mut self, dt: f32) {
        let displacement = self.current - self.target;
        let spring_force = -self.config.stiffness * displacement;
        let damping_force = -self.config.damping * self.velocity;
        let acceleration = (spring_force + damping_force) / self.config.mass;

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

/// 4D Spring Solver for Window Rectangles (X, Y, Width, Height)
#[derive(Debug, Clone, Copy)]
pub struct SpringRect {
    pub x: Spring1D,
    pub y: Spring1D,
    pub width: Spring1D,
    pub height: Spring1D,
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

    pub fn current_rect(&self) -> (i32, i32, i32, i32) {
        (
            self.x.current.round() as i32,
            self.y.current.round() as i32,
            self.width.current.round().max(10.0) as i32,
            self.height.current.round().max(10.0) as i32,
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
        // Step halfway
        for _ in 0..15 {
            spring.step(dt);
        }

        let mid_velocity = spring.velocity;
        assert!(mid_velocity > 10.0);

        // Interrupt mid-flight with new target
        spring.set_target(200.0);
        assert_eq!(spring.velocity, mid_velocity, "Velocity must be preserved across retargeting");
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
}
