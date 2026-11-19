//! GPUI Views for Nexus-OS Shell: Menu Bar, Dock with magnification,
//! Mission Control (Exposé), and Spotlight launcher.

use gpui::prelude::*;
use gpui::{
    div, px, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, Styled, Window,
};
use nx_ui::gpui_theme::{sf_font_family, ThemeStyledExt};
use nx_ui::theme::Theme;

use crate::dock::{magnification_target, DockModel};
use crate::expose::ExposeModel;
use crate::menu_bar::{bar_height, BarSlot, MenuBarModel, MENU_TITLES};
use crate::spotlight::{ProviderKind, Spotlight};

/// GPUI View for the macOS-style top Menu Bar.
pub struct MenuBarView {
    pub model: MenuBarModel,
}

impl MenuBarView {
    pub fn new(model: MenuBarModel) -> Self {
        Self { model }
    }
}

impl Render for MenuBarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if cx.has_global::<Theme>() {
            *cx.global::<Theme>()
        } else {
            Theme::dark(nx_ui::theme::Accent::Blue)
        };

        let height = bar_height(theme.scale);
        let menu_bg = theme.palette.menu_bg;
        let separator_color = theme.palette.separator;

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(height))
            .bg(Hsla::from(menu_bg))
            .border_b_1()
            .border_color(Hsla::from(separator_color))
            .font_family(sf_font_family())
            .text_size(px(13.0 * theme.scale))
            .text_color(theme.text_primary_hsla())
            .px(px(12.0 * theme.scale))
            // Left (Leading) section: Mark, App Name, Menus
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0 * theme.scale))
                    // Nexus / System Mark
                    .child(
                        div()
                            .text_color(theme.accent_hsla())
                            .font_weight(FontWeight::BOLD)
                            .child("●"),
                    )
                    // Frontmost App Title
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text_primary_hsla())
                            .child(self.model.app_name.clone()),
                    )
                    // Menu Titles (File, Edit, View, Window, Help)
                    .children(MENU_TITLES.iter().enumerate().map(|(idx, title)| {
                        let is_open = self.model.open_menu == Some(BarSlot::Menu(idx));
                        div()
                            .px(px(6.0 * theme.scale))
                            .py(px(2.0 * theme.scale))
                            .rounded(px(theme.radii.xs))
                            .when(is_open, |d| d.bg(theme.selection_hsla()))
                            .hover(|s| s.bg(theme.hover_hsla()))
                            .child(title.to_string())
                    })),
            )
            // Right (Trailing) section: Privacy chips, Volume, Clipboard, Clock
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0 * theme.scale))
                    // Privacy indicator chips (Camera, Mic, Location)
                    .children(self.model.privacy.iter().map(|chip| {
                        let hex_val = u32::from_str_radix(chip.color_hex.trim_start_matches('#'), 16)
                            .unwrap_or(0x34c759);
                        let chip_color = nx_ui::Color::hex(hex_val);
                        div()
                            .px(px(7.0 * theme.scale))
                            .py(px(2.0 * theme.scale))
                            .rounded(px(theme.radii.pill))
                            .bg(Hsla::from(chip_color))
                            .text_color(Hsla::from(nx_ui::Color::WHITE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(11.0 * theme.scale))
                            .child(chip.label.clone())
                    }))
                    // Volume widget indicator
                    .when(self.model.volume.level > 0.0 || self.model.volume.muted, |d| {
                        let vol_pct = (self.model.volume.level * 100.0) as u32;
                        let vol_label = if self.model.volume.muted {
                            "🔇 Muted".to_string()
                        } else {
                            format!("🔊 {}%", vol_pct)
                        };
                        d.child(
                            div()
                                .px(px(6.0 * theme.scale))
                                .py(px(2.0 * theme.scale))
                                .rounded(px(theme.radii.xs))
                                .text_color(theme.text_secondary_hsla())
                                .child(vol_label),
                        )
                    })
                    // Clipboard indicator if sensitive items detected
                    .when(self.model.clipboard.count > 0, |d| {
                        d.child(
                            div()
                                .px(px(6.0 * theme.scale))
                                .py(px(2.0 * theme.scale))
                                .rounded(px(theme.radii.pill))
                                .bg(theme.accent_hsla())
                                .text_color(Hsla::from(nx_ui::Color::WHITE))
                                .text_size(px(11.0 * theme.scale))
                                .child(format!("📋 {}", self.model.clipboard.count)),
                        )
                    })
                    // System Clock
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary_hsla())
                            .child(self.model.clock.format(self.model.hour24)),
                    ),
            )
    }
}

/// GPUI View for the macOS-style Dock with magnification and bounce animation.
pub struct DockView {
    pub model: DockModel,
    pub hovered_index: Option<usize>,
}

impl DockView {
    pub fn new(model: DockModel) -> Self {
        Self {
            model,
            hovered_index: None,
        }
    }
}

impl Render for DockView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if cx.has_global::<Theme>() {
            *cx.global::<Theme>()
        } else {
            Theme::dark(nx_ui::theme::Accent::Blue)
        };

        let is_horizontal = self.model.placement.edge.is_horizontal();
        let base_size = self.model.mag.rest;
        let gap = self.model.mag.gap;
        let pad = self.model.mag.padding;
        let items = self.model.items().to_vec();

        div()
            .flex()
            .when(is_horizontal, |d| d.flex_row())
            .when(!is_horizontal, |d| d.flex_col())
            .items_end()
            .justify_center()
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(theme.radii.window))
            .p(px(pad))
            .gap(px(gap))
            .shadow_lg()
            .children(items.into_iter().enumerate().map(|(idx, item)| {
                // Calculate magnification scale if hovered
                let scale = if let Some(hovered) = self.hovered_index {
                    let ptr_axis = hovered as f32 * (base_size + gap);
                    let icon_axis = idx as f32 * (base_size + gap);
                    magnification_target(ptr_axis, icon_axis, self.model.mag)
                } else {
                    1.0
                };

                let icon_size = base_size * scale;

                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.0))
                    // Icon body
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(icon_size))
                            .h(px(icon_size))
                            .bg(Hsla::from(theme.palette.elevated))
                            .border_1()
                            .border_color(Hsla::from(theme.palette.separator))
                            .rounded(px(theme.radii.sm * scale))
                            .shadow_sm()
                            .hover(|s| s.border_color(theme.accent_hsla()))
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(14.0 * scale))
                            .text_color(theme.text_primary_hsla())
                            .child(item.label.chars().next().unwrap_or('•').to_string()),
                    )
                    // Running app indicator dot
                    .child(
                        div()
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded(px(2.0))
                            .when(item.running, |d| d.bg(theme.text_primary_hsla()))
                            .when(!item.running, |d| d.bg(Hsla::from(nx_ui::Color::TRANSPARENT))),
                    )
            }))
    }
}

/// GPUI View for Mission Control / Exposé window management.
pub struct MissionControlView {
    pub model: ExposeModel,
}

impl MissionControlView {
    pub fn new(model: ExposeModel) -> Self {
        Self { model }
    }
}

impl Render for MissionControlView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if cx.has_global::<Theme>() {
            *cx.global::<Theme>()
        } else {
            Theme::dark(nx_ui::theme::Accent::Blue)
        };

        let tiles = self.model.tiles().to_vec();

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(Hsla::from(theme.palette.window_bg))
            .p(px(24.0))
            .gap(px(20.0))
            // Top: Spaces bar (virtual desktops)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(16.0))
                    .h(px(60.0))
                    .w_full()
                    .child(
                        div()
                            .theme_card(&theme)
                            .px(px(20.0))
                            .py(px(8.0))
                            .border_color(theme.accent_hsla())
                            .child("Workspace 1"),
                    )
                    .child(
                        div()
                            .theme_card(&theme)
                            .px(px(20.0))
                            .py(px(8.0))
                            .text_color(theme.text_secondary_hsla())
                            .child("Workspace 2"),
                    )
                    .child(
                        div()
                            .theme_card(&theme)
                            .px(px(20.0))
                            .py(px(8.0))
                            .text_color(theme.text_secondary_hsla())
                            .child("+ Add Space"),
                    ),
            )
            // Center: Window thumbnail tiles grid
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .items_center()
                    .gap(px(24.0))
                    .flex_1()
                    .children(tiles.into_iter().map(|tile| {
                        div()
                            .theme_card(&theme)
                            .flex()
                            .flex_col()
                            .w(px(260.0))
                            .h(px(180.0))
                            .overflow_hidden()
                            .shadow_md()
                            .hover(|s| s.border_color(theme.accent_hsla()).shadow_lg())
                            // Card header: Title and traffic lights
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .h(px(28.0))
                                    .px(px(10.0))
                                    .bg(Hsla::from(theme.palette.sidebar_bg))
                                    .border_b_1()
                                    .border_color(Hsla::from(theme.palette.separator))
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary_hsla())
                                    .child(tile.title.clone())
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(4.0))
                                            .child(div().w(px(8.0)).h(px(8.0)).rounded(px(4.0)).bg(Hsla::from(nx_ui::Color::hex(0xff5f57))))
                                            .child(div().w(px(8.0)).h(px(8.0)).rounded(px(4.0)).bg(Hsla::from(nx_ui::Color::hex(0xfebc2e))))
                                            .child(div().w(px(8.0)).h(px(8.0)).rounded(px(4.0)).bg(Hsla::from(nx_ui::Color::hex(0x28c840)))),
                                    ),
                            )
                            // Window preview canvas
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(Hsla::from(theme.palette.content_bg))
                                    .text_color(theme.text_tertiary_hsla())
                                    .child("Window Preview"),
                            )
                    })),
            )
    }
}

/// GPUI View for Spotlight search launcher.
pub struct SpotlightView {
    pub model: Spotlight,
}

impl SpotlightView {
    pub fn new(model: Spotlight) -> Self {
        Self { model }
    }
}

impl Render for SpotlightView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if cx.has_global::<Theme>() {
            *cx.global::<Theme>()
        } else {
            Theme::dark(nx_ui::theme::Accent::Blue)
        };

        let hits = self.model.query_str(&self.model.query);

        div()
            .flex()
            .flex_col()
            .w(px(640.0))
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(theme.radii.window))
            .shadow_lg()
            .overflow_hidden()
            .font_family(sf_font_family())
            // Top: Search input row
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(18.0))
                    .py(px(14.0))
                    .gap(px(12.0))
                    .border_b_1()
                    .border_color(Hsla::from(theme.palette.separator))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .text_color(theme.text_secondary_hsla())
                            .child("🔍"),
                    )
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.text_primary_hsla())
                            .child(if self.model.query.is_empty() {
                                "Spotlight Search".to_string()
                            } else {
                                self.model.query.clone()
                            }),
                    ),
            )
            // Results list
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(8.0))
                    .gap(px(2.0))
                    .children(hits.into_iter().enumerate().map(|(idx, hit)| {
                        let is_selected = idx == 0;
                        let badge_color = match hit.kind {
                            ProviderKind::Calculator => nx_ui::Color::hex(0xbf5af2), // Purple
                            ProviderKind::App => nx_ui::Color::hex(0x0a84ff),        // Blue
                            ProviderKind::Setting => nx_ui::Color::hex(0xff9f0a),    // Orange
                            ProviderKind::File => nx_ui::Color::hex(0x8e8e93),       // Gray
                        };

                        let kind_name = match hit.kind {
                            ProviderKind::Calculator => "Calculation",
                            ProviderKind::App => "Application",
                            ProviderKind::Setting => "System Setting",
                            ProviderKind::File => "Document",
                        };

                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(theme.radii.md))
                            .when(is_selected, |d| d.bg(theme.selection_hsla()))
                            .hover(|s| s.bg(theme.hover_hsla()))
                            // Left: Title and Subtitle
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_size(px(14.0))
                                            .text_color(theme.text_primary_hsla())
                                            .child(hit.title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(theme.text_secondary_hsla())
                                            .child(hit.subtitle),
                                    ),
                            )
                            // Right: Badge / value
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when_some(hit.value, |d, val| {
                                        d.child(
                                            div()
                                                .font_weight(FontWeight::BOLD)
                                                .text_size(px(16.0))
                                                .text_color(theme.accent_hsla())
                                                .child(val),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded(px(theme.radii.xs))
                                            .bg(Hsla::from(badge_color.with_alpha(0.2)))
                                            .text_color(Hsla::from(badge_color))
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(kind_name),
                                    ),
                            )
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::DockPlacement;
    use crate::menu_bar::MenuBarModel;
    use crate::spotlight::Spotlight;
    use gpui::TestAppContext;
    use nx_ui::geom::Rect;
    use nx_wm::MonitorId;

    #[gpui::test]
    fn test_menu_bar_view_renders(cx: &mut TestAppContext) {
        let model = MenuBarModel::new();
        cx.update(|cx| {
            let entity = cx.new(|_cx| MenuBarView::new(model));
            let view = entity.read(cx);
            assert_eq!(view.model.app_name, "Nexus");
        });
    }

    #[gpui::test]
    fn test_dock_view_renders(cx: &mut TestAppContext) {
        let placement = DockPlacement::bottom(MonitorId(1), Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let model = DockModel::new(placement);
        cx.update(|cx| {
            let entity = cx.new(|_cx| DockView::new(model));
            let view = entity.read(cx);
            assert!(view.model.items().is_empty());
        });
    }

    #[gpui::test]
    fn test_spotlight_view_renders(cx: &mut TestAppContext) {
        let mut model = Spotlight::new();
        model.query = "calc".to_string();
        cx.update(|cx| {
            let entity = cx.new(|_cx| SpotlightView::new(model));
            let view = entity.read(cx);
            assert_eq!(view.model.query, "calc");
        });
    }

    #[gpui::test]
    fn test_mission_control_view_renders(cx: &mut TestAppContext) {
        let canvas = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let model = ExposeModel::new(canvas);
        cx.update(|cx| {
            let entity = cx.new(|_cx| MissionControlView::new(model));
            let view = entity.read(cx);
            assert!(view.model.tiles().is_empty());
        });
    }
}
