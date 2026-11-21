//! GPUI theme and design token integration for Nexus-OS.
//!
//! GPUI styling for the monochrome Nexus theme: generous rounded surfaces,
//! UI + mono type, and shell-surface window options.

use crate::color::Color;
use crate::spring::Spring;
use crate::theme::Theme;
use crate::tokens::{TypeStep, Weight, CORNER_SMOOTHING};
use gpui::{
    div, px, Bounds, FontWeight, Global, Hsla, IntoElement, ParentElement, Pixels, Rgba,
    SharedString, Styled, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind,
    WindowOptions,
};

impl Global for Theme {}

impl From<Color> for Rgba {
    fn from(c: Color) -> Self {
        Rgba {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a,
        }
    }
}

impl From<Color> for Hsla {
    fn from(c: Color) -> Self {
        let rgba: Rgba = c.into();
        rgba.into()
    }
}

impl From<Weight> for FontWeight {
    fn from(w: Weight) -> Self {
        match w {
            Weight::Regular => FontWeight::NORMAL,
            Weight::Medium => FontWeight::MEDIUM,
            Weight::Semibold => FontWeight::SEMIBOLD,
            Weight::Bold => FontWeight::BOLD,
        }
    }
}

/// Preferred system typography family: SF Pro Text on Apple systems,
/// Segoe UI Variable on Windows, falling back to system-ui.
pub fn sf_font_family() -> SharedString {
    #[cfg(target_os = "macos")]
    {
        ".SystemUIFont".into()
    }
    #[cfg(target_os = "windows")]
    {
        "Segoe UI Variable Text".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Inter".into()
    }
}

/// Monospace family for clock, meters, and status (dot-matrix energy).
/// Prefers Geist Mono / IBM Plex / Cascadia, then platform classics.
pub fn mono_font_family() -> SharedString {
    #[cfg(target_os = "macos")]
    {
        "SF Mono".into()
    }
    #[cfg(target_os = "windows")]
    {
        "Cascadia Mono".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Geist Mono".into()
    }
}

/// Uppercase a label for typographic hierarchy.
pub fn upper_label(text: &str) -> String {
    text.to_ascii_uppercase()
}

impl Theme {
    /// Convert any theme color directly to a GPUI `Hsla` color.
    pub fn to_hsla(&self, color: Color) -> Hsla {
        color.into()
    }

    /// Primary window background color as GPUI `Hsla`.
    pub fn window_bg_hsla(&self) -> Hsla {
        self.palette.window_bg.into()
    }

    /// Sidebar background color as GPUI `Hsla`.
    pub fn sidebar_bg_hsla(&self) -> Hsla {
        self.palette.sidebar_bg.into()
    }

    /// Elevated surface background color as GPUI `Hsla`.
    pub fn elevated_hsla(&self) -> Hsla {
        self.palette.elevated.into()
    }

    /// Primary text color as GPUI `Hsla`.
    pub fn text_primary_hsla(&self) -> Hsla {
        self.palette.text_primary.into()
    }

    /// Secondary text color as GPUI `Hsla`.
    pub fn text_secondary_hsla(&self) -> Hsla {
        self.palette.text_secondary.into()
    }

    /// Tertiary text color as GPUI `Hsla`.
    pub fn text_tertiary_hsla(&self) -> Hsla {
        self.palette.text_tertiary.into()
    }

    /// System accent color as GPUI `Hsla`.
    pub fn accent_hsla(&self) -> Hsla {
        self.palette.accent.into()
    }

    /// Selection color as GPUI `Hsla`.
    pub fn selection_hsla(&self) -> Hsla {
        self.palette.selection.into()
    }

    /// Hover color as GPUI `Hsla`.
    pub fn hover_hsla(&self) -> Hsla {
        self.palette.hover.into()
    }

    /// Separator border color as GPUI `Hsla`.
    pub fn separator_hsla(&self) -> Hsla {
        self.palette.separator.into()
    }

    /// Continuous squircle corner radius helper.
    pub fn corner_radius_px(&self, base_radius: f32) -> Pixels {
        px(base_radius * (1.0 + (CORNER_SMOOTHING - 0.5) * 0.2))
    }

    /// Creates standard GPUI `WindowOptions` adhering to the theme's
    /// backdrop requirements.
    pub fn window_options(&self, bounds: Bounds<Pixels>) -> WindowOptions {
        self.window_options_with_backdrop(bounds, self.materials.window.kind.blurs())
    }

    /// Creates GPUI `WindowOptions` with explicit control over Mica/Acrylic blur.
    pub fn window_options_with_backdrop(
        &self,
        bounds: Bounds<Pixels>,
        blurred: bool,
    ) -> WindowOptions {
        let appearance = if blurred {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        };

        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: appearance,
            titlebar: None,
            ..Default::default()
        }
    }

    /// Creates standard GPUI `WindowOptions` for blurred surfaces (dock, Spotlight, etc).
    pub fn blurred_window_options(&self, bounds: Bounds<Pixels>) -> WindowOptions {
        self.app_window_options(bounds)
    }

    /// Opened app windows: transparent, client decorations, no OS titlebar.
    pub fn app_window_options(&self, bounds: Bounds<Pixels>) -> WindowOptions {
        self.shell_window_options(bounds, false, false, true)
    }

    /// Shell chrome (island, dock, spotlight): no system titlebar, client decorations,
    /// optional popup stacking, optional visibility.
    pub fn shell_window_options(
        &self,
        bounds: Bounds<Pixels>,
        blurred: bool,
        popup: bool,
        show: bool,
    ) -> WindowOptions {
        // Always Transparent so we can draw a rounded body and let the desktop
        // show through the corners. The `blurred` flag is kept for callers but
        // no longer fills an opaque/blurred hard rectangle.
        let _ = blurred;
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: None,
            focus: !popup,
            show,
            kind: if popup {
                WindowKind::PopUp
            } else {
                WindowKind::Floating
            },
            is_movable: !popup,
            is_resizable: !popup,
            is_minimizable: !popup,
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        }
    }

    /// Inverted status fill (ENGAGED, selected, on).
    pub fn inverted_hsla(&self) -> Hsla {
        self.palette.text_primary.into()
    }

    /// Ink used on inverted fills.
    pub fn on_inverted_hsla(&self) -> Hsla {
        self.palette.window_bg.into()
    }
}

/// Monochrome window-control cluster: close / minimize / zoom as glyphs, no traffic-light colors.
pub fn window_chrome(theme: &Theme, scale: f32) -> impl IntoElement {
    let glyph = |mark: &'static str| {
        div()
            .w(px(18.0 * scale))
            .h(px(18.0 * scale))
            .rounded(px(theme.radii.xs))
            .border_1()
            .border_color(theme.separator_hsla())
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.0 * scale))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_secondary_hsla())
            .child(mark)
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0 * scale))
        .child(glyph("×"))
        .child(glyph("–"))
        .child(glyph("□"))
}

/// Inset around the rounded window body so the hairline and shadow can paint
/// on a transparent frame. Corners of the OS window stay see-through.
pub const WINDOW_FRAME_INSET: f32 = 10.0;

/// Wrap an app or palette surface in a rounded, bordered, shadowed body sitting
/// on a fully transparent frame.
pub fn rounded_window_frame(theme: &Theme, child: impl IntoElement) -> impl IntoElement {
    div()
        .w_full()
        .h_full()
        .bg(Hsla::from(Color::TRANSPARENT))
        .p(px(WINDOW_FRAME_INSET))
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .h_full()
                .bg(theme.window_bg_hsla())
                .border_1()
                .border_color(theme.separator_hsla())
                .rounded(px(theme.radii.window))
                .shadow_lg()
                .overflow_hidden()
                .child(child),
        )
}

/// Extension trait on GPUI `Styled` elements for rapid theme application.
pub trait ThemeStyledExt: Styled {
    /// Apply an SF-style typographic step (size, line height, font weight and font family).
    fn apply_type_step(self, step: TypeStep) -> Self {
        self.font_family(sf_font_family())
            .text_size(px(step.size))
            .line_height(px(step.line))
            .font_weight(step.weight.into())
    }

    /// Apply standard translucent card styling with smooth corners.
    fn theme_card(self, theme: &Theme) -> Self {
        let card_bg = theme.palette.elevated;
        let border_color = theme.palette.separator;
        self.bg(Hsla::from(card_bg))
            .border_1()
            .border_color(Hsla::from(border_color))
            .rounded(px(theme.radii.md))
    }

    /// Apply menu or bar container styling with acrylic backdrop.
    fn theme_surface(self, theme: &Theme) -> Self {
        let bg_color = theme.palette.menu_bg;
        let border_color = theme.palette.separator;
        self.bg(Hsla::from(bg_color))
            .border_1()
            .border_color(Hsla::from(border_color))
            .rounded(px(theme.radii.lg))
    }

    /// Apply an accent pill badge styling.
    fn theme_accent_pill(self, theme: &Theme) -> Self {
        let accent = theme.palette.accent;
        let on_accent = theme.palette.on_accent;
        self.bg(Hsla::from(accent))
            .text_color(Hsla::from(on_accent))
            .rounded(px(theme.radii.pill))
    }

    /// Inverted status block (ENGAGED / selected).
    fn theme_inverted_block(self, theme: &Theme) -> Self {
        self.bg(theme.inverted_hsla())
            .text_color(theme.on_inverted_hsla())
            .rounded(px(theme.radii.sm))
    }

    /// Apply the mono family at a given size (clock, meters, badges).
    fn apply_mono(self, size: f32) -> Self {
        self.font_family(mono_font_family())
            .text_size(px(size))
            .font_weight(FontWeight::MEDIUM)
    }
}

impl<T: Styled> ThemeStyledExt for T {}

/// Animate a spring step and return the new value.
pub fn step_spring(spring: &mut Spring, dt: f32) -> f32 {
    spring.step(dt);
    spring.position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Accent, ThemeMode};
    use gpui::{point, size, TestAppContext};

    #[test]
    fn test_color_to_gpui_conversions() {
        let white = Color::WHITE;
        let rgba: Rgba = white.into();
        assert_eq!(rgba.r, 1.0);
        assert_eq!(rgba.g, 1.0);
        assert_eq!(rgba.b, 1.0);
        assert_eq!(rgba.a, 1.0);

        let hsla: Hsla = white.into();
        assert!(hsla.l > 0.99);

        let trans = Color::TRANSPARENT;
        let trans_rgba: Rgba = trans.into();
        assert_eq!(trans_rgba.a, 0.0);
    }

    #[test]
    fn test_weight_conversion() {
        assert_eq!(FontWeight::from(Weight::Regular), FontWeight::NORMAL);
        assert_eq!(FontWeight::from(Weight::Medium), FontWeight::MEDIUM);
        assert_eq!(FontWeight::from(Weight::Semibold), FontWeight::SEMIBOLD);
        assert_eq!(FontWeight::from(Weight::Bold), FontWeight::BOLD);
    }

    #[test]
    fn test_window_options_blurred_backdrop() {
        let theme = Theme::dark(Accent::Blue);
        let bounds = Bounds::new(point(px(100.0), px(100.0)), size(px(800.0), px(600.0)));
        let opts = theme.blurred_window_options(bounds);
        assert_eq!(
            opts.window_background,
            WindowBackgroundAppearance::Transparent
        );
        assert_eq!(opts.window_decorations, Some(WindowDecorations::Client));
        assert!(opts.titlebar.is_none());
        assert!(opts.window_bounds.is_some());

        let solid_opts = theme.window_options(bounds);
        assert_eq!(
            solid_opts.window_background,
            WindowBackgroundAppearance::Opaque
        );
    }

    #[gpui::test]
    fn test_theme_as_global(cx: &mut TestAppContext) {
        let theme = Theme::dark(Accent::Purple);
        cx.update(|cx| {
            cx.set_global(theme);
            let retrieved = cx.global::<Theme>();
            assert_eq!(retrieved.mode, ThemeMode::Dark);
            assert_eq!(retrieved.accent, Accent::Purple);
            assert!(retrieved.accent_hsla().l > 0.0);
        });
    }

    #[test]
    fn test_spring_step_animation() {
        let mut spring = Spring::critical(0.0);
        spring.set_target(100.0);
        let pos1 = step_spring(&mut spring, 0.016);
        assert!(pos1 > 0.0);
        assert!(pos1 < 100.0);
        // Step until settled
        for _ in 0..100 {
            step_spring(&mut spring, 0.016);
        }
        assert!(spring.settled());
        assert!((spring.position - 100.0).abs() < 0.1);
    }
}
