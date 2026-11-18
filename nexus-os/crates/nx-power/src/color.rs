use crate::error::{PowerError, PowerResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// 8-bit RGBA color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// Parses hex color strings: `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA`.
    pub fn from_hex(hex: &str) -> PowerResult<Self> {
        let clean = hex.trim().trim_start_matches('#');
        match clean.len() {
            3 => {
                let r = u8::from_str_radix(&clean[0..1], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                let g = u8::from_str_radix(&clean[1..2], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                let b = u8::from_str_radix(&clean[2..3], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                Ok(Self::rgb(r, g, b))
            }
            4 => {
                let r = u8::from_str_radix(&clean[0..1], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                let g = u8::from_str_radix(&clean[1..2], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                let b = u8::from_str_radix(&clean[2..3], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                let a = u8::from_str_radix(&clean[3..4], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?
                    * 17;
                Ok(Self::rgba(r, g, b, a))
            }
            6 => {
                let r = u8::from_str_radix(&clean[0..2], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                let g = u8::from_str_radix(&clean[2..4], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                let b = u8::from_str_radix(&clean[4..6], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                Ok(Self::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&clean[0..2], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                let g = u8::from_str_radix(&clean[2..4], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                let b = u8::from_str_radix(&clean[4..6], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                let a = u8::from_str_radix(&clean[6..8], 16)
                    .map_err(|e| PowerError::InvalidColor(e.to_string()))?;
                Ok(Self::rgba(r, g, b, a))
            }
            _ => Err(PowerError::InvalidColor(format!(
                "Invalid hex length {}: '{}'",
                clean.len(),
                hex
            ))),
        }
    }

    /// Creates Color from HSL (hue 0.0..360.0, saturation 0.0..1.0, lightness 0.0..1.0).
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        Self::from_hsla(h, s, l, 1.0)
    }

    /// Creates Color from HSLA (hue 0.0..360.0, s 0.0..1.0, l 0.0..1.0, a 0.0..1.0).
    pub fn from_hsla(h: f32, s: f32, l: f32, a: f32) -> Self {
        let h = (h % 360.0 + 360.0) % 360.0;
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        let a = (a.clamp(0.0, 1.0) * 255.0).round() as u8;

        if s == 0.0 {
            let val = (l * 255.0).round() as u8;
            return Self::rgba(val, val, val, a);
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let hk = h / 360.0;

        let tc_r = (hk + 1.0 / 3.0).fract();
        let tc_g = hk.fract();
        let tc_b = (hk - 1.0 / 3.0 + 1.0).fract();

        let channel = |mut tc: f32| -> u8 {
            if tc < 0.0 {
                tc += 1.0;
            }
            let res = if tc < 1.0 / 6.0 {
                p + ((q - p) * 6.0 * tc)
            } else if tc < 1.0 / 2.0 {
                q
            } else if tc < 2.0 / 3.0 {
                p + ((q - p) * 6.0 * (2.0 / 3.0 - tc))
            } else {
                p
            };
            (res * 255.0).round().clamp(0.0, 255.0) as u8
        };

        Self::rgba(channel(tc_r), channel(tc_g), channel(tc_b), a)
    }

    /// Creates Color from HSV/HSB (h 0.0..360.0, s 0.0..1.0, v 0.0..1.0).
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = (h % 360.0 + 360.0) % 360.0;
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);

        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;

        let (r1, g1, b1) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Self::rgb(
            ((r1 + m) * 255.0).round() as u8,
            ((g1 + m) * 255.0).round() as u8,
            ((b1 + m) * 255.0).round() as u8,
        )
    }

    /// Creates Color from CMYK (all 0.0..=1.0).
    pub fn from_cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        let c = c.clamp(0.0, 1.0);
        let m = m.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let k = k.clamp(0.0, 1.0);

        let r = (255.0 * (1.0 - c) * (1.0 - k)).round() as u8;
        let g = (255.0 * (1.0 - m) * (1.0 - k)).round() as u8;
        let b = (255.0 * (1.0 - y) * (1.0 - k)).round() as u8;
        Self::rgb(r, g, b)
    }

    /// Formats as uppercase 6-digit hex string `#RRGGBB`.
    pub fn to_hex_rgb(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Formats as uppercase 8-digit hex string `#RRGGBBAA`.
    pub fn to_hex_rgba(&self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }

    /// Short 3-digit hex `#RGB` if eligible (nibbles match) and fully opaque.
    pub fn to_hex_short(&self) -> Option<String> {
        if self.a == 255
            && (self.r >> 4 == self.r & 0x0F)
            && (self.g >> 4 == self.g & 0x0F)
            && (self.b >> 4 == self.b & 0x0F)
        {
            Some(format!(
                "#{:1X}{:1X}{:1X}",
                self.r >> 4,
                self.g >> 4,
                self.b >> 4
            ))
        } else {
            None
        }
    }

    /// CSS `rgb(r, g, b)`
    pub fn to_rgb_css(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    /// CSS `rgba(r, g, b, alpha)`
    pub fn to_rgba_css(&self) -> String {
        format!(
            "rgba({}, {}, {}, {:.2})",
            self.r,
            self.g,
            self.b,
            (self.a as f32) / 255.0
        )
    }

    /// Converts to HSL tuple (hue [0, 360), saturation [0, 1], lightness [0, 1]).
    pub fn to_hsl(&self) -> (f32, f32, f32) {
        let r = (self.r as f32) / 255.0;
        let g = (self.g as f32) / 255.0;
        let b = (self.b as f32) / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let l = (max + min) / 2.0;

        if delta == 0.0 {
            return (0.0, 0.0, l);
        }

        let s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let mut h = if max == r {
            ((g - b) / delta) % 6.0
        } else if max == g {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };

        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }

        (h, s, l)
    }

    /// CSS `hsl(h, s%, l%)`
    pub fn to_hsl_css(&self) -> String {
        let (h, s, l) = self.to_hsl();
        format!("hsl({:.0}, {:.0}%, {:.0}%)", h, s * 100.0, l * 100.0)
    }

    /// CSS `hsla(h, s%, l%, alpha)`
    pub fn to_hsla_css(&self) -> String {
        let (h, s, l) = self.to_hsl();
        format!(
            "hsla({:.0}, {:.0}%, {:.0}%, {:.2})",
            h,
            s * 100.0,
            l * 100.0,
            (self.a as f32) / 255.0
        )
    }

    /// Converts to HSV (h, s, v).
    pub fn to_hsv(&self) -> (f32, f32, f32) {
        let r = (self.r as f32) / 255.0;
        let g = (self.g as f32) / 255.0;
        let b = (self.b as f32) / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let v = max;
        let s = if max == 0.0 { 0.0 } else { delta / max };

        if delta == 0.0 {
            return (0.0, s, v);
        }

        let mut h = if max == r {
            ((g - b) / delta) % 6.0
        } else if max == g {
            ((b - r) / delta) + 2.0
        } else {
            ((r - g) / delta) + 4.0
        };

        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }

        (h, s, v)
    }

    /// Converts to CMYK tuple (c, m, y, k).
    pub fn to_cmyk(&self) -> (f32, f32, f32, f32) {
        let r = (self.r as f32) / 255.0;
        let g = (self.g as f32) / 255.0;
        let b = (self.b as f32) / 255.0;

        let k = 1.0 - r.max(g).max(b);
        if k >= 1.0 {
            return (0.0, 0.0, 0.0, 1.0);
        }

        let c = (1.0 - r - k) / (1.0 - k);
        let m = (1.0 - g - k) / (1.0 - k);
        let y = (1.0 - b - k) / (1.0 - k);

        (c.max(0.0), m.max(0.0), y.max(0.0), k)
    }

    /// Formats as `cmyk(c%, m%, y%, k%)`
    pub fn to_cmyk_css(&self) -> String {
        let (c, m, y, k) = self.to_cmyk();
        format!(
            "cmyk({:.0}%, {:.0}%, {:.0}%, {:.0}%)",
            c * 100.0,
            m * 100.0,
            y * 100.0,
            k * 100.0
        )
    }

    /// WCAG 2.1 relative luminance (0.0 for black, 1.0 for white).
    pub fn relative_luminance(&self) -> f32 {
        let channel = |v: u8| -> f32 {
            let c = (v as f32) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };

        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG 2.1 contrast ratio against another color (range 1.0..21.0).
    pub fn contrast_ratio(&self, other: &Self) -> f32 {
        let l1 = self.relative_luminance();
        let l2 = other.relative_luminance();
        let lighter = l1.max(l2);
        let darker = l1.min(l2);
        (lighter + 0.05) / (darker + 0.05)
    }
}

/// Store for recently picked colors, custom palettes, and favorited swatches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorHistoryStore {
    capacity: usize,
    recent: VecDeque<Color>,
    favorites: Vec<Color>,
    palettes: HashMap<String, Vec<Color>>,
}

impl ColorHistoryStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            recent: VecDeque::new(),
            favorites: Vec::new(),
            palettes: HashMap::new(),
        }
    }

    /// Records a picked color: deduplicates and brings to front, enforcing capacity.
    pub fn push(&mut self, color: Color) {
        if let Some(pos) = self.recent.iter().position(|c| c == &color) {
            self.recent.remove(pos);
        }
        self.recent.push_front(color);
        while self.recent.len() > self.capacity {
            self.recent.pop_back();
        }
    }

    pub fn recent(&self) -> &VecDeque<Color> {
        &self.recent
    }

    pub fn favorites(&self) -> &[Color] {
        &self.favorites
    }

    pub fn add_favorite(&mut self, color: Color) {
        if !self.favorites.contains(&color) {
            self.favorites.push(color);
        }
    }

    pub fn remove_favorite(&mut self, color: &Color) {
        self.favorites.retain(|c| c != color);
    }

    pub fn create_palette(&mut self, name: impl Into<String>, colors: Vec<Color>) {
        self.palettes.insert(name.into(), colors);
    }

    pub fn get_palette(&self, name: &str) -> Option<&[Color]> {
        self.palettes.get(name).map(|v| v.as_slice())
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
    }
}

impl Default for ColorHistoryStore {
    fn default() -> Self {
        Self::new(32)
    }
}
