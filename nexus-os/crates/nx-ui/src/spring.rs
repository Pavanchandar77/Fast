//! Interruptible springs with a closed-form step.
//!
//! The default is critically damped, which is the macOS control feel: fast,
//! no bounce. Retargeting keeps the current velocity and re-solves from there,
//! so a gesture can change direction mid-flight. The step is analytic, so a
//! long frame cannot explode the way forward Euler can.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
    pub rest_pos: f32,
    pub rest_vel: f32,
}

impl SpringConfig {
    /// Apple's response/damping-ratio parameterization.
    ///
    /// `response` is the undamped period in seconds. `damping_ratio` of 1 is
    /// critically damped. Values below 1 overshoot.
    pub fn macos(response: f32, damping_ratio: f32) -> Self {
        let response = response.max(0.01);
        let mass = 1.0;
        let omega = std::f32::consts::TAU / response;
        Self {
            stiffness: mass * omega * omega,
            damping: 2.0 * damping_ratio.max(0.0) * mass * omega,
            mass,
            rest_pos: 0.05,
            rest_vel: 0.5,
        }
    }

    pub fn zeta(self) -> f32 {
        let mass = self.mass.max(1.0e-4);
        let k = self.stiffness.max(0.0);
        if k <= 1.0e-8 {
            return 1.0;
        }
        self.damping / (2.0 * (k * mass).sqrt())
    }
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::macos(0.32, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub position: f32,
    pub velocity: f32,
    pub target: f32,
    pub config: SpringConfig,
}

impl Spring {
    pub fn new(position: f32, config: SpringConfig) -> Self {
        Self {
            position,
            velocity: 0.0,
            target: position,
            config,
        }
    }

    pub fn critical(position: f32) -> Self {
        Self::new(position, SpringConfig::default())
    }

    /// Retarget without touching velocity. The next [`Self::step`] re-solves.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn settled(&self) -> bool {
        (self.position - self.target).abs() <= self.config.rest_pos
            && self.velocity.abs() <= self.config.rest_vel
    }

    pub fn step(&mut self, dt: f32) {
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        if self.settled() {
            self.position = self.target;
            self.velocity = 0.0;
            return;
        }
        self.integrate(dt);
        if self.settled() {
            self.position = self.target;
            self.velocity = 0.0;
        }
    }

    fn integrate(&mut self, dt: f32) {
        let mass = self.config.mass.max(1.0e-4);
        let k = self.config.stiffness.max(0.0);
        if k <= 1.0e-6 {
            self.position = self.target;
            self.velocity = 0.0;
            return;
        }
        let omega = (k / mass).sqrt();
        let zeta = self.config.damping.max(0.0) / (2.0 * (k * mass).sqrt());
        let y0 = self.position - self.target;
        let v0 = self.velocity;
        let (y, v) = if (zeta - 1.0).abs() < 1.0e-3 {
            critical(y0, v0, omega, dt)
        } else if zeta < 1.0 {
            underdamped(y0, v0, omega, zeta, dt)
        } else {
            overdamped(y0, v0, omega, zeta, dt)
        };
        if y.is_finite() && v.is_finite() {
            self.position = self.target + y;
            self.velocity = v;
        } else {
            self.position = self.target;
            self.velocity = 0.0;
        }
    }
}

fn critical(y0: f32, v0: f32, omega: f32, dt: f32) -> (f32, f32) {
    let a = y0;
    let b = v0 + omega * a;
    let e = (-omega * dt).exp();
    let y = (a + b * dt) * e;
    let v = (b - omega * (a + b * dt)) * e;
    (y, v)
}

fn underdamped(y0: f32, v0: f32, omega: f32, zeta: f32, dt: f32) -> (f32, f32) {
    let wd = omega * (1.0 - zeta * zeta).max(0.0).sqrt();
    if wd < 1.0e-6 {
        return critical(y0, v0, omega, dt);
    }
    let sigma = zeta * omega;
    let a = y0;
    let b = (v0 + sigma * a) / wd;
    let e = (-sigma * dt).exp();
    let cos = (wd * dt).cos();
    let sin = (wd * dt).sin();
    let y = e * (a * cos + b * sin);
    let v = -sigma * y + e * (-a * wd * sin + b * wd * cos);
    (y, v)
}

fn overdamped(y0: f32, v0: f32, omega: f32, zeta: f32, dt: f32) -> (f32, f32) {
    let disc = (zeta * zeta - 1.0).max(0.0).sqrt();
    let r1 = -omega * zeta + omega * disc;
    let r2 = -omega * zeta - omega * disc;
    let denom = r1 - r2;
    if denom.abs() < 1.0e-8 {
        return critical(y0, v0, omega, dt);
    }
    let c1 = (v0 - r2 * y0) / denom;
    let c2 = y0 - c1;
    let e1 = (r1 * dt).exp();
    let e2 = (r2 * dt).exp();
    (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tight() -> SpringConfig {
        let mut cfg = SpringConfig::macos(0.30, 1.0);
        cfg.rest_pos = 1.0e-4;
        cfg.rest_vel = 1.0e-3;
        cfg
    }

    #[test]
    fn macos_ratio_one_is_critically_damped() {
        let cfg = SpringConfig::macos(0.4, 1.0);
        assert!((cfg.zeta() - 1.0).abs() < 1.0e-3);
        let loose = SpringConfig::macos(0.4, 0.5);
        assert!(loose.zeta() < 0.6);
    }

    #[test]
    fn critical_spring_does_not_overshoot() {
        let mut spring = Spring::new(0.0, tight());
        spring.set_target(1.0);
        let dt = 1.0 / 120.0;
        for _ in 0..360 {
            spring.step(dt);
            assert!(
                spring.position <= 1.0 + 1.0e-3,
                "overshoot {}",
                spring.position
            );
            assert!(spring.position >= -1.0e-3, "undershoot {}", spring.position);
        }
        assert!(spring.settled());
        assert!((spring.position - 1.0).abs() < 1.0e-3);
    }

    #[test]
    fn underdamped_spring_does_overshoot() {
        let mut cfg = SpringConfig::macos(0.35, 0.35);
        cfg.rest_pos = 1.0e-4;
        cfg.rest_vel = 1.0e-3;
        let mut spring = Spring::new(0.0, cfg);
        spring.set_target(1.0);
        let mut peak = 0.0f32;
        for _ in 0..360 {
            spring.step(1.0 / 120.0);
            peak = peak.max(spring.position);
        }
        assert!(peak > 1.02, "peak {peak}");
        assert!(spring.settled());
    }

    #[test]
    fn retarget_preserves_velocity_and_reaches_the_new_target() {
        let mut spring = Spring::new(0.0, tight());
        spring.set_target(200.0);
        spring.step(0.05);
        let velocity = spring.velocity;
        assert!(velocity > 0.0);
        spring.set_target(40.0);
        assert!((spring.velocity - velocity).abs() < 1.0e-5);
        for _ in 0..400 {
            spring.step(1.0 / 120.0);
        }
        assert!(spring.settled());
        assert!((spring.position - 40.0).abs() < 0.05);
    }

    #[test]
    fn settled_spring_stays_put() {
        let mut spring = Spring::critical(12.0);
        let before = spring.position;
        spring.step(1.0 / 60.0);
        assert_eq!(spring.position, before);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn zero_dt_does_not_move() {
        let mut spring = Spring::new(0.0, tight());
        spring.set_target(10.0);
        spring.step(0.0);
        spring.step(-1.0);
        assert_eq!(spring.position, 0.0);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn large_dt_lands_on_the_target() {
        let mut spring = Spring::new(0.0, SpringConfig::default());
        spring.set_target(800.0);
        spring.step(5.0);
        assert!(spring.settled());
        assert!((spring.position - 800.0).abs() < 0.1);
    }

    #[test]
    fn analytic_step_matches_a_single_closed_form_sample() {
        let cfg = tight();
        let mut stepped = Spring::new(0.0, cfg);
        stepped.set_target(50.0);
        let dt = 0.08;
        stepped.step(dt);
        let omega = (cfg.stiffness / cfg.mass).sqrt();
        let (y, v) = critical(-50.0, 0.0, omega, dt);
        assert!((stepped.position - (50.0 + y)).abs() < 1.0e-3);
        assert!((stepped.velocity - v).abs() < 1.0e-3);
    }
}
