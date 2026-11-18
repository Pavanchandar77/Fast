//! Vibrancy, Mica, and Acrylic materials.
//!
//! The GPU path blurs a backdrop, then tints it and adds film grain.
//! On Windows the same material also maps to a DWM system backdrop.

use crate::color::Color;
use crate::platform::SystemBackdrop;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialKind {
    /// Opaque fill. No blur pass.
    Solid,
    /// Sidebar / menu-bar vibrancy: blur, saturation, tint, noise.
    Vibrancy,
    /// Softer, larger blur. Maps to `DWMSBT_MAINWINDOW` (Mica).
    Mica,
    /// Tighter blur. Maps to `DWMSBT_TRANSIENTWINDOW` (Acrylic).
    Acrylic,
    /// Tooltips and thin popovers.
    Thin,
}

impl MaterialKind {
    pub fn blurs(self) -> bool {
        !matches!(self, Self::Solid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub kind: MaterialKind,
    pub tint: Color,
    pub blur: f32,
    pub noise: f32,
    pub saturation: f32,
    pub backdrop: SystemBackdrop,
}

impl Material {
    pub const fn solid(color: Color) -> Self {
        Self {
            kind: MaterialKind::Solid,
            tint: color,
            blur: 0.0,
            noise: 0.0,
            saturation: 1.0,
            backdrop: SystemBackdrop::None,
        }
    }

    pub fn needs_blur_pass(self) -> bool {
        self.kind.blurs() && self.blur > 0.0
    }
}

/// 9-tap gaussian weights. The blur shader consumes this exact kernel.
pub fn gaussian_kernel(sigma: f32) -> [f32; 9] {
    let sigma = sigma.max(0.25);
    let mut weights = [0.0f32; 9];
    let mut sum = 0.0;
    for (i, weight) in weights.iter_mut().enumerate() {
        let x = i as f32 - 4.0;
        let w = (-0.5 * x * x / (sigma * sigma)).exp();
        *weight = w;
        sum += w;
    }
    if sum > 0.0 {
        for weight in &mut weights {
            *weight /= sum;
        }
    }
    weights
}

/// Deterministic film-grain hash in `[-0.5, 0.5]`. Matches `hash21` in `composite.wgsl`.
#[allow(clippy::excessive_precision)]
pub fn film_grain(x: f32, y: f32) -> f32 {
    let n = (x * 127.1 + y * 311.7).sin() * 43758.5453;
    n.fract() - 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_and_acrylic_request_blur() {
        let mica = Material {
            kind: MaterialKind::Mica,
            tint: Color::rgba(255, 255, 255, 180),
            blur: 36.0,
            noise: 0.04,
            saturation: 1.2,
            backdrop: SystemBackdrop::Mica,
        };
        assert!(mica.needs_blur_pass());
        assert!(mica.blur > 20.0);
        assert!(!Material::solid(Color::WHITE).needs_blur_pass());
    }

    #[test]
    fn gaussian_kernel_is_normalized_and_symmetric() {
        let k = gaussian_kernel(2.0);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        assert!((k[0] - k[8]).abs() < 1e-6);
        assert!(k[4] > k[0]);
    }

    #[test]
    fn film_grain_stays_in_range_and_is_stable() {
        let a = film_grain(12.0, 40.0);
        let b = film_grain(12.0, 40.0);
        assert_eq!(a.to_bits(), b.to_bits());
        assert!((-0.5..0.5).contains(&a));
        assert!(film_grain(1.0, 2.0) != film_grain(3.0, 4.0));
    }
}
