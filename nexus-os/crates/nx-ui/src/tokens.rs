//! Design tokens: 8pt spacing, SF-like type, radii, elevation, motion.

use crate::color::Color;

/// 8-point spacing grid. Half-steps (2 and 4) are for optical alignment of icons and hairlines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub s2: f32,
    pub s4: f32,
    pub s8: f32,
    pub s12: f32,
    pub s16: f32,
    pub s20: f32,
    pub s24: f32,
    pub s32: f32,
    pub s40: f32,
    pub s48: f32,
    pub s64: f32,
}

impl Spacing {
    pub const fn unit() -> Self {
        Self {
            s2: 2.0,
            s4: 4.0,
            s8: 8.0,
            s12: 12.0,
            s16: 16.0,
            s20: 20.0,
            s24: 24.0,
            s32: 32.0,
            s40: 40.0,
            s48: 48.0,
            s64: 64.0,
        }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            s2: self.s2 * factor,
            s4: self.s4 * factor,
            s8: self.s8 * factor,
            s12: self.s12 * factor,
            s16: self.s16 * factor,
            s20: self.s20 * factor,
            s24: self.s24 * factor,
            s32: self.s32 * factor,
            s40: self.s40 * factor,
            s48: self.s48 * factor,
            s64: self.s64 * factor,
        }
    }

    pub fn steps(self) -> [f32; 11] {
        [
            self.s2, self.s4, self.s8, self.s12, self.s16, self.s20, self.s24, self.s32, self.s40,
            self.s48, self.s64,
        ]
    }
}

impl Default for Spacing {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// A step on the SF-like type ramp. Sizes are CSS pixels at 1x, matching the SF text styles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStep {
    pub size: f32,
    pub line: f32,
    pub weight: Weight,
}

impl TypeStep {
    pub const fn new(size: f32, line: f32, weight: Weight) -> Self {
        Self { size, line, weight }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            size: self.size * factor,
            line: self.line * factor,
            weight: self.weight,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    pub large_title: TypeStep,
    pub title1: TypeStep,
    pub title2: TypeStep,
    pub title3: TypeStep,
    pub headline: TypeStep,
    pub body: TypeStep,
    pub callout: TypeStep,
    pub subhead: TypeStep,
    pub footnote: TypeStep,
    pub caption1: TypeStep,
    pub caption2: TypeStep,
}

impl TypeScale {
    pub const fn sf() -> Self {
        Self {
            large_title: TypeStep::new(34.0, 41.0, Weight::Regular),
            title1: TypeStep::new(28.0, 34.0, Weight::Regular),
            title2: TypeStep::new(22.0, 28.0, Weight::Regular),
            title3: TypeStep::new(20.0, 25.0, Weight::Regular),
            headline: TypeStep::new(17.0, 22.0, Weight::Semibold),
            body: TypeStep::new(17.0, 22.0, Weight::Regular),
            callout: TypeStep::new(16.0, 21.0, Weight::Regular),
            subhead: TypeStep::new(15.0, 20.0, Weight::Regular),
            footnote: TypeStep::new(13.0, 18.0, Weight::Regular),
            caption1: TypeStep::new(12.0, 16.0, Weight::Regular),
            caption2: TypeStep::new(11.0, 13.0, Weight::Regular),
        }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            large_title: self.large_title.scaled(factor),
            title1: self.title1.scaled(factor),
            title2: self.title2.scaled(factor),
            title3: self.title3.scaled(factor),
            headline: self.headline.scaled(factor),
            body: self.body.scaled(factor),
            callout: self.callout.scaled(factor),
            subhead: self.subhead.scaled(factor),
            footnote: self.footnote.scaled(factor),
            caption1: self.caption1.scaled(factor),
            caption2: self.caption2.scaled(factor),
        }
    }
}

impl Default for TypeScale {
    fn default() -> Self {
        Self::sf()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    pub window: f32,
    pub pill: f32,
}

impl Radii {
    pub const fn unit() -> Self {
        Self {
            xs: 6.0,
            sm: 10.0,
            md: 14.0,
            lg: 18.0,
            xl: 24.0,
            window: 22.0,
            pill: 999.0,
        }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            xs: self.xs * factor,
            sm: self.sm * factor,
            md: self.md * factor,
            lg: self.lg * factor,
            xl: self.xl * factor,
            window: self.window * factor,
            pill: self.pill,
        }
    }
}

impl Default for Radii {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub blur: f32,
    pub x: f32,
    pub y: f32,
    pub spread: f32,
}

impl Shadow {
    pub const fn none() -> Self {
        Self {
            color: Color::TRANSPARENT,
            blur: 0.0,
            x: 0.0,
            y: 0.0,
            spread: 0.0,
        }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            color: self.color,
            blur: self.blur * factor,
            x: self.x * factor,
            y: self.y * factor,
            spread: self.spread * factor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Elevation {
    pub sm: Shadow,
    pub md: Shadow,
    pub lg: Shadow,
}

impl Elevation {
    pub fn light() -> Self {
        let color = Color::rgba(0, 0, 0, 36);
        Self {
            sm: Shadow {
                color,
                blur: 8.0,
                x: 0.0,
                y: 1.0,
                spread: 0.0,
            },
            md: Shadow {
                color: Color::rgba(0, 0, 0, 48),
                blur: 18.0,
                x: 0.0,
                y: 6.0,
                spread: 0.0,
            },
            lg: Shadow {
                color: Color::rgba(0, 0, 0, 64),
                blur: 40.0,
                x: 0.0,
                y: 16.0,
                spread: 0.0,
            },
        }
    }

    pub fn dark() -> Self {
        let color = Color::rgba(0, 0, 0, 140);
        Self {
            sm: Shadow {
                color,
                blur: 10.0,
                x: 0.0,
                y: 1.0,
                spread: 0.0,
            },
            md: Shadow {
                color: Color::rgba(0, 0, 0, 170),
                blur: 22.0,
                x: 0.0,
                y: 8.0,
                spread: 0.0,
            },
            lg: Shadow {
                color: Color::rgba(0, 0, 0, 200),
                blur: 48.0,
                x: 0.0,
                y: 18.0,
                spread: 0.0,
            },
        }
    }

    pub fn scaled(self, factor: f32) -> Self {
        Self {
            sm: self.sm.scaled(factor),
            md: self.md.scaled(factor),
            lg: self.lg.scaled(factor),
        }
    }
}

/// Continuous-corner smoothing. 0 is a circular corner, 1 is an iOS-like squircle.
pub const CORNER_SMOOTHING: f32 = 0.85;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_follows_eight_point_grid() {
        let s = Spacing::unit();
        for step in [s.s8, s.s16, s.s24, s.s32, s.s40, s.s48, s.s64] {
            assert_eq!(step % 8.0, 0.0);
        }
        assert_eq!(s.s4 * 2.0, s.s8);
        assert_eq!(s.s2 * 4.0, s.s8);
    }

    #[test]
    fn type_scale_matches_sf_body() {
        let t = TypeScale::sf();
        assert_eq!(t.body.size, 17.0);
        assert_eq!(t.body.line, 22.0);
        assert_eq!(t.large_title.size, 34.0);
        assert_eq!(t.headline.weight, Weight::Semibold);
        assert!(t.caption2.size < t.caption1.size);
        assert!(t.caption1.size < t.footnote.size);
        assert!(t.footnote.size < t.body.size);
        assert!(t.body.size < t.title3.size);
        assert!(t.title3.size < t.title1.size);
        assert!(t.title1.size < t.large_title.size);
    }

    #[test]
    fn radii_are_monotonic() {
        let r = Radii::unit();
        assert!(r.xs < r.sm && r.sm < r.md && r.md < r.lg && r.lg < r.xl);
        assert!(r.window >= r.md);
    }

    #[test]
    fn elevation_blur_grows() {
        let e = Elevation::light();
        assert!(e.sm.blur < e.md.blur && e.md.blur < e.lg.blur);
        assert!(e.sm.y < e.md.y && e.md.y < e.lg.y);
        let dark = Elevation::dark();
        assert!(dark.lg.blur > e.lg.blur);
    }
}
