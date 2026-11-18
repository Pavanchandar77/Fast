//! macOS-inspired light and dark themes, accent colors, and materials.

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
                if dark {
                    Color::hex(0x98989d)
                } else {
                    Color::hex(0x8e8e93)
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
        window_bg: Color::hex(0xf2f2f7),
        sidebar_bg: Color::rgba(255, 255, 255, 184),
        content_bg: Color::hex(0xffffff),
        elevated: Color::hex(0xffffff),
        text_primary: Color::hex(0x1d1d1f),
        text_secondary: Color::hex(0x6e6e73),
        text_tertiary: Color::hex(0x8e8e93),
        separator: Color::rgba(0, 0, 0, 26),
        fill: Color::hex(0xe5e5ea),
        fill_secondary: Color::rgba(0, 0, 0, 12),
        accent,
        on_accent,
        selection: accent.with_alpha(0.18),
        hover: Color::rgba(0, 0, 0, 16),
        control: Color::hex(0xffffff),
        row_alt: Color::rgba(0, 0, 0, 8),
        tooltip_bg: Color::rgba(246, 246, 246, 235),
        menu_bg: Color::rgba(255, 255, 255, 230),
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
        window_bg: Color::hex(0x1c1c1e),
        sidebar_bg: Color::rgba(28, 28, 30, 200),
        content_bg: Color::hex(0x1c1c1e),
        elevated: Color::hex(0x2c2c2e),
        text_primary: Color::hex(0xf5f5f7),
        text_secondary: Color::hex(0x98989d),
        text_tertiary: Color::hex(0x8e8e93),
        separator: Color::rgba(255, 255, 255, 30),
        fill: Color::hex(0x3a3a3c),
        fill_secondary: Color::rgba(255, 255, 255, 18),
        accent,
        on_accent,
        selection: accent.with_alpha(0.32),
        hover: Color::rgba(255, 255, 255, 20),
        control: Color::hex(0x2c2c2e),
        row_alt: Color::rgba(255, 255, 255, 10),
        tooltip_bg: Color::rgba(44, 44, 46, 235),
        menu_bg: Color::rgba(44, 44, 46, 230),
        shadow: Color::rgba(0, 0, 0, 180),
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
        assert!((theme.radii.md - 16.0).abs() < 1e-4);
        assert_eq!(
            theme.palette.accent.to_array(),
            Accent::Green.color(ThemeMode::Dark).to_array()
        );
        assert!((theme.scale - 2.0).abs() < 1e-6);
    }
}
