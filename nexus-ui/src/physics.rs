use egui::{Pos2, Rect, Vec2};

/// Critically damped iOS-grade spring value (interruptible at any velocity)
#[derive(Clone, Copy, Debug)]
pub struct SpringF32 {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32, // k (e.g., 320.0 for snappy iOS feel)
    pub damping: f32,   // c (e.g., 32.0 for zero-overshoot critical damping)
}

impl SpringF32 {
    pub fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn step(&mut self, dt: f32) -> f32 {
        let dt = dt.clamp(0.001, 0.033);
        let displacement = self.value - self.target;
        let spring_force = -self.stiffness * displacement;
        let damping_force = -self.damping * self.velocity;
        let acceleration = spring_force + damping_force;

        self.velocity += acceleration * dt;
        self.value += self.velocity * dt;

        if displacement.abs() < 0.05 && self.velocity.abs() < 0.05 {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.value
    }

    pub fn is_animating(&self) -> bool {
        (self.value - self.target).abs() > 0.05 || self.velocity.abs() > 0.05
    }
}

/// 2D Rect spring for morphing focus borders, Dynamic Island, and modals
#[derive(Clone, Copy, Debug)]
pub struct SpringRect {
    pub x: SpringF32,
    pub y: SpringF32,
    pub w: SpringF32,
    pub h: SpringF32,
}

impl SpringRect {
    pub fn new(rect: Rect, stiffness: f32, damping: f32) -> Self {
        Self {
            x: SpringF32::new(rect.min.x, stiffness, damping),
            y: SpringF32::new(rect.min.y, stiffness, damping),
            w: SpringF32::new(rect.width(), stiffness, damping),
            h: SpringF32::new(rect.height(), stiffness, damping),
        }
    }

    pub fn set_target(&mut self, rect: Rect) {
        self.x.set_target(rect.min.x);
        self.y.set_target(rect.min.y);
        self.w.set_target(rect.width());
        self.h.set_target(rect.height());
    }

    pub fn step(&mut self, dt: f32) -> Rect {
        let min = Pos2::new(self.x.step(dt), self.y.step(dt));
        let size = Vec2::new(self.w.step(dt).max(1.0), self.h.step(dt).max(1.0));
        Rect::from_min_size(min, size)
    }
}