//! Monochrome Nexus theme: off-white on graphite, generous macOS radii.

use crate::color::Color;
use crate::material::{Material, MaterialKind};
use crate::platform::{BackdropRequest, CornerPreference, SystemBackdrop};
use crate::tokens::{Elevation, Radii, Spacing, TypeScale, CORNER_SMOOTHING};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// System accent, matching the macOS control colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Blue,
    Purple,
    Pink,
    Orange,
    Green,
    Graphite,
}

impl Accent {
    pub fn color(self, mode: ThemeMode) -> Color {
        let dark = mode.is_dark();
        match self {
            Self::Blue => {
                if dark {
                    Color::hex(0x0a84ff)
                } else {
                    Color::hex(0x007aff)
                }
            }
            Self::Purple => {
                if dark {
                    Color::hex(0xbf5af2)
                } else {
                    Color::hex(0xaf52de)
                }
            }
            Self::Pink => {
                if dark {
                    Color::hex(0xff375f)
                } else {
                    Color::hex(0xff2d55)
                }
            }
            Self::Orange => {
                if dark {
                    Color::hex(0xff9f0a)
                } else {
                    Color::hex(0xff9500)
                }
            }
            Self::Green => {
                if dark {
                    Color::hex(0x30d158)
                } else {
                    Color::hex(0x34c759)
                }
            }
            Self::Graphite => {
                // High-contrast ink: off-white on dark, charcoal on light.
                if dark {
                    Color::hex(0xf4f1ea)
                } else {
                    Color::hex(0x111111)
                }
            }
        }
    }

    pub const ALL: [Accent; 6] = [
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Orange,
        Self::Green,
        Self::Graphite,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub window_bg: Color,
    pub sidebar_bg: Color,
    pub content_bg: Color,
    pub elevated: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub separator: Color,
    pub fill: Color,
    pub fill_secondary: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub selection: Color,
    pub hover: Color,
    pub control: Color,
    pub row_alt: Color,
    pub tooltip_bg: Color,
    pub menu_bg: Color,
    pub shadow: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Materials {
    pub window: Material,
    pub sidebar: Material,
    pub menu: Material,
    pub tooltip: Material,
    pub bar: Material,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub mode: ThemeMode,
    pub accent: Accent,
    pub palette: Palette,
    pub type_scale: TypeScale,
    pub spacing: Spacing,
    pub radii: Radii,
    pub elevation: Elevation,
    pub materials: Materials,
    /// 0 = circular corners, 1 = continuous squircle.
    pub corner_smoothing: f32,
    pub scale: f32,
}

impl Theme {
    pub fn light(accent: Accent) -> Self {
        Self::build(ThemeMode::Light, accent, 1.0)
    }

    pub fn dark(accent: Accent) -> Self {
        Self::build(ThemeMode::Dark, accent, 1.0)
    }

    /// Product theme: dark monochrome graphite, no chromatic accent.
    pub fn stealth() -> Self {
        Self::dark(Accent::Graphite)
    }

    pub fn at_scale(self, scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self::build(self.mode, self.accent, scale)
    }

    /// DWM backdrop for this theme. Attributes live on the HWND only.
    pub fn backdrop_request(&self) -> BackdropRequest {
        BackdropRequest {
            kind: self.materials.sidebar.backdrop,
            dark: self.mode.is_dark(),
            corners: CornerPreference::Round,
        }
    }

    fn build(mode: ThemeMode, accent: Accent, scale: f32) -> Self {
        let accent_color = accent.color(mode);
        let on_accent = if accent_color.luminance() > 0.62 {
            Color::hex(0x1d1d1f)
        } else {
            Color::WHITE
        };
        let (palette, elevation, materials) = match mode {
            ThemeMode::Light => light_parts(accent_color, on_accent),
            ThemeMode::Dark => dark_parts(accent_color, on_accent),
        };
        Self {
            mode,
            accent,
            palette,
            type_scale: TypeScale::sf().scaled(scale),
            spacing: Spacing::unit().scaled(scale),
            radii: Radii::unit().scaled(scale),
            elevation: elevation.scaled(scale),
            materials: scale_materials(materials, scale),
            corner_smoothing: CORNER_SMOOTHING,
            scale,
        }
    }
}

fn light_parts(accent: Color, on_accent: Color) -> (Palette, Elevation, Materials) {
    let palette = Palette {
        window_bg: Color::hex(0xf4f1ea),
        sidebar_bg: Color::rgba(244, 241, 234, 220),
        content_bg: Color::hex(0xfaf8f3),
        elevated: Color::hex(0xffffff),
        text_primary: Color::hex(0x111111),
        text_secondary: Color::hex(0x5c5c59),
        text_tertiary: Color::hex(0x8a8a86),
        separator: Color::rgba(0, 0, 0, 28),
        fill: Color::hex(0xe8e4db),
        fill_secondary: Color::rgba(0, 0, 0, 12),
        accent,
        on_accent,
        selection: accent.with_alpha(0.16),
        hover: Color::rgba(0, 0, 0, 14),
        control: Color::hex(0xffffff),
        row_alt: Color::rgba(0, 0, 0, 8),
        tooltip_bg: Color::rgba(250, 248, 243, 235),
        menu_bg: Color::rgba(250, 248, 243, 230),
        shadow: Color::rgba(0, 0, 0, 48),
    };
    let materials = Materials {
        window: Material::solid(palette.window_bg),
        sidebar: Material {
            kind: MaterialKind::Vibrancy,
            tint: Color::rgba(255, 255, 255, 168),
            blur: 28.0,
            noise: 0.035,
            saturation: 1.45,
            backdrop: SystemBackdrop::Mica,
        },
        menu: Material {
            kind: MaterialKind::Acrylic,
            tint: Color::rgba(255, 255, 255, 200),
            blur: 22.0,
            noise: 0.04,
            saturation: 1.3,
            backdrop: SystemBackdrop::Acrylic,
        },
        tooltip: Material {
            kind: MaterialKind::Thin,
            tint: Color::rgba(246, 246, 246, 220),
            blur: 16.0,
            noise: 0.03,
            saturation: 1.2,
            backdrop: SystemBackdrop::Acrylic,
        },
        bar: Material {
            kind: MaterialKind::Vibrancy,
            tint: Color::rgba(246, 246, 246, 190),
            blur: 24.0,
            noise: 0.03,
            saturation: 1.4,
            backdrop: SystemBackdrop::Mica,
        },
    };
    (palette, Elevation::light(), materials)
}

fn dark_parts(accent: Color, on_accent: Color) -> (Palette, Elevation, Materials) {
    let palette = Palette {
        window_bg: Color::hex(0x0a0a0a),
        sidebar_bg: Color::rgba(14, 14, 14, 220),
        content_bg: Color::hex(0x0e0e0e),
        elevated: Color::hex(0x161616),
        text_primary: Color::hex(0xf4f1ea),
        text_secondary: Color::hex(0x8a8a86),
        text_tertiary: Color::hex(0x5c5c59),
        separator: Color::rgba(244, 241, 234, 32),
        fill: Color::hex(0x1c1c1c),
        fill_secondary: Color::rgba(244, 241, 234, 16),
        accent,
        on_accent,
        selection: accent.with_alpha(0.20),
        hover: Color::rgba(244, 241, 234, 18),
        control: Color::hex(0x161616),
        row_alt: Color::rgba(244, 241, 234, 10),
        tooltip_bg: Color::rgba(22, 22, 22, 235),
        menu_bg: Color::rgba(12, 12, 12, 236),
        shadow: Color::rgba(0, 0, 0, 200),
    };
    let materials = Materials {
        window: Material::solid(palette.window_bg),
        sidebar: Material {
            kind: MaterialKind::Mica,
            tint: Color::rgba(28, 28, 30, 190),
            blur: 36.0,
            noise: 0.045,
            saturation: 1.25,
            backdrop: SystemBackdrop::Mica,
        },
        menu: Material {
            kind: MaterialKind::Acrylic,
            tint: Color::rgba(44, 44, 46, 210),
            blur: 24.0,
            noise: 0.05,
            saturation: 1.2,
            backdrop: SystemBackdrop::Acrylic,
        },
        tooltip: Material {
            kind: MaterialKind::Thin,
            tint: Color::rgba(58, 58, 60, 220),
            blur: 18.0,
            noise: 0.04,
            saturation: 1.15,
            backdrop: SystemBackdrop::Acrylic,
        },
        bar: Material {
            kind: MaterialKind::Vibrancy,
            tint: Color::rgba(28, 28, 30, 180),
            blur: 28.0,
            noise: 0.04,
            saturation: 1.3,
            backdrop: SystemBackdrop::Tabbed,
        },
    };
    (palette, Elevation::dark(), materials)
}

fn scale_materials(mut materials: Materials, scale: f32) -> Materials {
    materials.window.blur *= scale;
    materials.sidebar.blur *= scale;
    materials.menu.blur *= scale;
    materials.tooltip.blur *= scale;
    materials.bar.blur *= scale;
    materials
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_primary_text_is_darker_than_the_window() {
        let theme = Theme::light(Accent::Blue);
        assert!(theme.palette.text_primary.luminance() < theme.palette.window_bg.luminance());
        assert!(
            theme
                .palette
                .text_primary
                .contrast_ratio(theme.palette.content_bg)
                >= 7.0
        );
    }

    #[test]
    fn dark_primary_text_is_lighter_than_the_window() {
        let theme = Theme::dark(Accent::Blue);
        assert!(theme.palette.text_primary.luminance() > theme.palette.window_bg.luminance());
        assert!(
            theme
                .palette
                .text_primary
                .contrast_ratio(theme.palette.content_bg)
                >= 7.0
        );
    }

    #[test]
    fn accent_override_recolors_selection() {
        let blue = Theme::light(Accent::Blue);
        let pink = Theme::light(Accent::Pink);
        assert_ne!(
            blue.palette.accent.to_array(),
            pink.palette.accent.to_array()
        );
        assert_eq!(pink.palette.selection.r, pink.palette.accent.r);
        assert!(pink.palette.selection.a < 1.0);
    }

    #[test]
    fn vibrancy_materials_blur_and_map_to_dwm() {
        let light = Theme::light(Accent::Blue);
        assert!(light.materials.sidebar.blur > 0.0);
        assert!(light.materials.sidebar.noise > 0.0 && light.materials.sidebar.noise < 0.2);
        assert_eq!(light.materials.sidebar.backdrop, SystemBackdrop::Mica);
        assert_eq!(light.materials.menu.backdrop, SystemBackdrop::Acrylic);
        let dark = Theme::dark(Accent::Purple);
        assert_eq!(dark.materials.sidebar.kind, MaterialKind::Mica);
        assert!(dark.materials.sidebar.blur >= light.materials.sidebar.blur);
    }

    #[test]
    fn scale_changes_type_and_spacing_not_colors() {
        let theme = Theme::dark(Accent::Green).at_scale(2.0);
        assert!((theme.type_scale.body.size - 34.0).abs() < 1e-4);
        assert!((theme.spacing.s8 - 16.0).abs() < 1e-4);
        assert!((theme.radii.md - 28.0).abs() < 1e-4);
        assert_eq!(
            theme.palette.accent.to_array(),
            Accent::Green.color(ThemeMode::Dark).to_array()
        );
        assert!((theme.scale - 2.0).abs() < 1e-6);
    }

    #[test]
    fn stealth_theme_is_monochrome_graphite() {
        let theme = Theme::stealth();
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.accent, Accent::Graphite);
        let a = theme.palette.accent.to_bytes();
        let span = a[0]
            .abs_diff(a[1])
            .max(a[1].abs_diff(a[2]))
            .max(a[0].abs_diff(a[2]));
        assert!(
            span <= 16,
            "graphite accent should stay near-neutral, got {a:?}"
        );
        assert!(
            theme
                .palette
                .text_primary
                .contrast_ratio(theme.palette.window_bg)
                >= 7.0
        );
        assert!(theme.radii.window >= 20.0);
    }
}
