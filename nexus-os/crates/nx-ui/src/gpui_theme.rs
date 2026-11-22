//! GPUI theme and design token integration for Nexus-OS.
//!
//! GPUI styling for the monochrome Nexus theme: generous rounded surfaces,
//! UI + mono type, and shell-surface window options.

use crate::color::Color;
use crate::spring::Spring;
use crate::theme::Theme;
use crate::tokens::{TypeStep, Weight, CORNER_SMOOTHING};
use gpui::{
    div, px, App, Bounds, ClickEvent, FontWeight, Global, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, Rgba, SharedString, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowKind,
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

/// Close through the OS so the window's `on_window_should_close` hook runs
/// (Nexus app windows hide instead of being destroyed, so the dock can bring
/// them back). Off Windows, remove the window directly.
pub fn close_window(window: &mut Window) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(w) = handle.as_raw() {
                let hwnd = HWND(w.hwnd.get() as *mut std::ffi::c_void);
                if unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
                    return;
                }
            }
        }
    }
    window.remove_window();
}

/// Start dragging the window from a title/tool bar, unless the press landed
/// on the caption buttons flush right (those must get their click). A
/// mouse-down on a caption button also bubbles to the bar, and starting a
/// move there swallowed the close/minimize click.
pub fn begin_window_drag(event: &gpui::MouseDownEvent, window: &mut Window) {
    if event.click_count > 1 {
        window.zoom_window();
        return;
    }
    // The traffic lights sit top-left; a press there is a button, not a drag.
    let chrome = CHROME_WIDTH + WINDOW_FRAME_INSET + 4.0;
    let top = WINDOW_FRAME_INSET + CAPTION_BUTTON_HEIGHT + 16.0;
    if f32::from(event.position.x) <= chrome && f32::from(event.position.y) <= top {
        return;
    }
    // GPUI's Windows backend has no `start_window_move` (it is a no-op).
    // Hand the press to the system move loop instead: release GPUI's mouse
    // capture and *post* SC_MOVE|HTCAPTION, so the loop starts after this
    // handler returns rather than re-entering the window procedure.
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_MOVE, WM_SYSCOMMAND};
        const HTCAPTION: usize = 2;
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(w) = handle.as_raw() {
                let hwnd = HWND(w.hwnd.get() as *mut std::ffi::c_void);
                unsafe {
                    let _ = ReleaseCapture();
                    let _ = PostMessageW(
                        hwnd,
                        WM_SYSCOMMAND,
                        WPARAM(SC_MOVE as usize | HTCAPTION),
                        LPARAM(0),
                    );
                }
                return;
            }
        }
    }
    window.start_window_move();
}

/// Host hook for the caption buttons. The session installs one so minimize
/// and close can play the dock genie / dust animation before the window
/// goes away. Return `true` when the action was handled.
#[derive(Clone)]
pub struct WindowControlHook(pub WindowControlFn);

/// `(action, window, app) -> handled`, action is `close`, `min` or `max`.
pub type WindowControlFn = std::sync::Arc<dyn Fn(&'static str, &mut Window, &mut App) -> bool>;

impl Global for WindowControlHook {}

fn run_window_control(action: &'static str, window: &mut Window, cx: &mut App) {
    let hook = cx.try_global::<WindowControlHook>().cloned();
    if let Some(hook) = hook {
        if (hook.0)(action, window, cx) {
            return;
        }
    }
    match action {
        "min" => window.minimize_window(),
        "max" => window.zoom_window(),
        "close" => close_window(window),
        _ => {}
    }
}

/// Diameter of one traffic light.
pub const TRAFFIC_LIGHT: f32 = 12.0;
/// Gap between traffic lights.
pub const TRAFFIC_GAP: f32 = 8.0;
/// Width of the whole traffic-light group including its padding. Drag
/// handlers keep this zone clickable.
pub const CHROME_WIDTH: f32 = 3.0 * TRAFFIC_LIGHT + 2.0 * TRAFFIC_GAP + 2.0 * 14.0;
/// Kept for callers sized against the old caption buttons.
pub const CAPTION_BUTTON_WIDTH: f32 = CHROME_WIDTH / 3.0;
/// Height of the traffic-light strip.
pub const CAPTION_BUTTON_HEIGHT: f32 = 32.0;

/// macOS-style traffic lights (close, minimize, zoom), in the monochrome
/// palette: three soft discs that show their glyphs when the group is
/// hovered; close warms to red on its own hover. Each disc is also a
/// Windows caption-button hit target, so snap layouts and the system menu
/// keep working.
///
/// `live` wires the buttons to this window. Preview tiles pass `false` so a
/// picture of a window does not minimize the shell.
pub fn window_chrome(theme: &Theme, live: bool) -> impl IntoElement {
    let dark =
        theme.palette.window_bg.r + theme.palette.window_bg.g + theme.palette.window_bg.b < 1.5;
    let disc = if dark {
        Hsla::from(Rgba {
            r: 0.36,
            g: 0.36,
            b: 0.38,
            a: 1.0,
        })
    } else {
        Hsla::from(Rgba {
            r: 0.78,
            g: 0.78,
            b: 0.80,
            a: 1.0,
        })
    };
    let disc_hover = if dark {
        Hsla::from(Rgba {
            r: 0.62,
            g: 0.62,
            b: 0.64,
            a: 1.0,
        })
    } else {
        Hsla::from(Rgba {
            r: 0.58,
            g: 0.58,
            b: 0.60,
            a: 1.0,
        })
    };
    let close_hover = Hsla::from(Rgba {
        r: 1.0,
        g: 0.37,
        b: 0.34,
        a: 1.0,
    });
    let glyph = Hsla::from(Rgba {
        r: 0.05,
        g: 0.05,
        b: 0.06,
        a: 0.78,
    });
    let ring = if dark {
        Hsla::from(Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.10,
        })
    } else {
        Hsla::from(Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.12,
        })
    };
    let light = move |id: &'static str, mark: &'static str| {
        let area = match id {
            "min" => WindowControlArea::Min,
            "max" => WindowControlArea::Max,
            _ => WindowControlArea::Close,
        };
        let hot = if id == "close" {
            close_hover
        } else {
            disc_hover
        };
        div()
            .id(id)
            .window_control_area(area)
            .w(px(TRAFFIC_LIGHT))
            .h(px(TRAFFIC_LIGHT))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(disc)
            .border_1()
            .border_color(ring)
            .text_size(px(9.0))
            .line_height(px(TRAFFIC_LIGHT))
            .font_weight(FontWeight::BOLD)
            .text_color(Hsla::transparent_black())
            .group_hover("nx-traffic", move |s| s.text_color(glyph))
            .hover(move |s| s.bg(hot))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                if live {
                    run_window_control(id, window, cx);
                }
            })
            .child(mark)
    };
    div()
        .group("nx-traffic")
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(TRAFFIC_GAP))
        .px(px(14.0))
        .h(px(CAPTION_BUTTON_HEIGHT))
        .child(light("close", "×"))
        .child(light("min", "−"))
        .child(light("max", "+"))
}

/// Inset around the rounded window body so the hairline and soft shadow can
/// paint on a transparent frame. Must be large enough that a ~22px corner
/// plus `shadow_lg` (offset 10 / blur 15) are not clipped by the OS window.
pub const WINDOW_FRAME_INSET: f32 = 20.0;

/// Wrap an app surface in a rounded, bordered, shadowed body sitting on a
/// fully transparent frame. An inner clip layer also carries the corner
/// radius: GPUI's `overflow_hidden` masks to a rectangle, so child fills
/// (title bar, sidebar, footer) must be rounded or they will square off
/// the corners of the body.
pub fn rounded_window_frame(theme: &Theme, child: impl IntoElement) -> impl IntoElement {
    let radius = theme.radii.window;
    let inner = (radius - 1.0).max(0.0);
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
                .rounded(px(radius))
                .shadow_lg()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .h_full()
                        .rounded(px(inner))
                        .overflow_hidden()
                        .child(child),
                ),
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
