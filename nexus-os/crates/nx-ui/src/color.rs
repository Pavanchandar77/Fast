//! sRGB colors, mixing, premultiplication, and contrast.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// `0xRRGGBB`.
    pub const fn hex(hex: u32) -> Self {
        Self::rgb(
            ((hex >> 16) & 0xff) as u8,
            ((hex >> 8) & 0xff) as u8,
            (hex & 0xff) as u8,
        )
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a,
        }
    }

    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Straight alpha to premultiplied RGBA.
    pub fn premultiply(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
    }

    pub fn to_linear(self) -> Self {
        Self {
            r: srgb_channel_to_linear(self.r),
            g: srgb_channel_to_linear(self.g),
            b: srgb_channel_to_linear(self.b),
            a: self.a,
        }
    }

    pub fn to_srgb(self) -> Self {
        Self {
            r: linear_channel_to_srgb(self.r),
            g: linear_channel_to_srgb(self.g),
            b: linear_channel_to_srgb(self.b),
            a: self.a,
        }
    }

    /// Relative luminance of the linearized color, 0 to 1.
    pub fn luminance(self) -> f32 {
        let lin = self.to_linear();
        0.2126 * lin.r + 0.7152 * lin.g + 0.0722 * lin.b
    }

    pub fn contrast_ratio(self, other: Self) -> f32 {
        let l1 = self.luminance();
        let l2 = other.luminance();
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    pub fn to_bytes(self) -> [u8; 4] {
        [
            (self.r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.b.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

fn srgb_channel_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_channel_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn srgb_linear_roundtrip() {
        for byte in [0u8, 16, 64, 128, 200, 255] {
            let c = Color::rgb(byte, byte, byte);
            let back = c.to_linear().to_srgb();
            assert!(near(c.r, back.r), "{byte}: {} vs {}", c.r, back.r);
        }
    }

    #[test]
    fn mix_midpoint() {
        let c = Color::BLACK.mix(Color::WHITE, 0.5);
        assert!(near(c.r, 0.5) && near(c.a, 1.0));
    }

    #[test]
    fn premultiply_scales_rgb() {
        let c = Color::rgba(255, 0, 0, 128);
        let p = c.premultiply();
        assert!(near(p[0], c.r * c.a));
        assert!(near(p[3], c.a));
        assert!(near(p[1], 0.0));
    }

    #[test]
    fn dark_text_on_light_has_strong_contrast() {
        let text = Color::hex(0x1d1d1f);
        let bg = Color::hex(0xf2f2f7);
        assert!(text.luminance() < bg.luminance());
        assert!(text.contrast_ratio(bg) >= 7.0);
    }

    #[test]
    fn light_text_on_dark_has_strong_contrast() {
        let text = Color::hex(0xf5f5f7);
        let bg = Color::hex(0x1c1c1e);
        assert!(text.luminance() > bg.luminance());
        assert!(text.contrast_ratio(bg) >= 7.0);
    }
}
