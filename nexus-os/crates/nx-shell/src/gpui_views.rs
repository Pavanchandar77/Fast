//! GPUI views for the Nexus shell (the Dynamic Island lives in `island_view`): macOS-style Dock,
//! Mission Control, and Spotlight.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, px, App, Context, FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};
use nx_ui::gpui_theme::{mono_font_family, sf_font_family, window_chrome, ThemeStyledExt};
use nx_ui::theme::Theme;
use nx_ui::{glyph_icon, GlyphIcon};

use crate::dock::{magnification_target, DockModel};
use crate::expose::ExposeModel;
use crate::menu_bar::{bar_height, BarSlot, MenuBarModel, MENU_TITLES};
use crate::spotlight::{ProviderKind, Spotlight};

fn theme_of(cx: &App) -> Theme {
    if cx.has_global::<Theme>() {
        *cx.global::<Theme>()
    } else {
        Theme::stealth()
    }
}

/// Host-installed callback so Spotlight and the dock can launch or focus an
/// app without depending on the session crate.
type ShellLaunchFn = Arc<dyn Fn(&str, &mut App)>;

#[derive(Default)]
pub struct ShellLaunch {
    handler: Option<ShellLaunchFn>,
}

impl gpui::Global for ShellLaunch {}

impl ShellLaunch {
    pub fn new(handler: impl Fn(&str, &mut App) + 'static) -> Self {
        Self {
            handler: Some(Arc::new(handler)),
        }
    }
}

/// Fire the host launch hook, if the session installed one.
pub fn request_shell_launch(cx: &mut App, id: &str) {
    let handler = cx
        .try_global::<ShellLaunch>()
        .and_then(|sink| sink.handler.clone());
    if let Some(handler) = handler {
        handler(id, cx);
    }
}

/// Retained for unit tests and host tooling; the session no longer shows a menu bar.
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
        let theme = theme_of(cx);
        let height = bar_height(theme.scale);
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(height))
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_b_1()
            .border_color(Hsla::from(theme.palette.separator))
            .font_family(sf_font_family())
            .text_size(px(13.0 * theme.scale))
            .text_color(theme.text_primary_hsla())
            .px(px(12.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0 * theme.scale))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(self.model.app_name.clone()),
                    )
                    .children(MENU_TITLES.iter().enumerate().map(|(idx, title)| {
                        let is_open = self.model.open_menu == Some(BarSlot::Menu(idx));
                        div()
                            .px(px(6.0 * theme.scale))
                            .py(px(2.0 * theme.scale))
                            .rounded(px(theme.radii.sm))
                            .when(is_open, |d| d.bg(theme.selection_hsla()))
                            .hover(|s| s.bg(theme.hover_hsla()))
                            .child(*title)
                    })),
            )
            .child(
                div()
                    .font_family(mono_font_family())
                    .text_size(px(12.0 * theme.scale))
                    .child(
                        self.model
                            .clock
                            .format(self.model.hour24)
                            .to_ascii_uppercase(),
                    ),
            )
    }
}

/// macOS-style bottom dock: translucent rounded shelf, magnification, running dots, trash separator.
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
        let theme = theme_of(cx);
        let is_horizontal = self.model.placement.edge.is_horizontal();
        let base_size = self.model.mag.rest;
        let gap = self.model.mag.gap;
        let pad = self.model.mag.padding;
        let items = self.model.items().to_vec();

        let shelf = div()
            .id("dock-shelf")
            .flex()
            .when(is_horizontal, |d| d.flex_row())
            .when(!is_horizontal, |d| d.flex_col())
            .items_end()
            .justify_center()
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(theme.radii.xl))
            .p(px(pad + 4.0))
            .gap(px(gap))
            .shadow_lg()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.hovered_index = None;
                    cx.notify();
                }
            }))
            .children(items.into_iter().enumerate().map(|(idx, item)| {
                let scale = if let Some(hovered) = self.hovered_index {
                    let ptr_axis = hovered as f32 * (base_size + gap);
                    let icon_axis = idx as f32 * (base_size + gap);
                    magnification_target(ptr_axis, icon_axis, self.model.mag)
                } else {
                    1.0
                };
                let icon_size = base_size * scale;
                let _label = &item.label;
                let before_trash = item.id == "trash";

                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(gap))
                    .when(before_trash, |d| {
                        d.child(
                            div()
                                .w(px(1.0))
                                .h(px(base_size * 0.7))
                                .mb(px(10.0))
                                .bg(Hsla::from(theme.palette.separator)),
                        )
                    })
                    .child(
                        div()
                            .id(("dock-icon", idx))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.0))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                this.hovered_index = if *hovered { Some(idx) } else { None };
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = this.model.items().get(idx).map(|item| item.id.clone());
                                if let Some(id) = id {
                                    let _ = this.model.activate_index(idx);
                                    cx.defer(move |cx| request_shell_launch(cx, &id));
                                }
                                cx.notify();
                            }))
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
                                    .rounded(px(theme.radii.lg * scale.clamp(0.8, 1.4)))
                                    .shadow_sm()
                                    .hover(|s| s.bg(theme.selection_hsla()))
                                    .child(glyph_icon(
                                        GlyphIcon::for_dock_id(&item.id),
                                        theme.text_primary_hsla(),
                                        (icon_size * 0.52).clamp(14.0, 36.0),
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded(px(2.0))
                                    .when(item.running, |d| d.bg(theme.text_primary_hsla()))
                                    .when(!item.running, |d| {
                                        d.bg(Hsla::from(nx_ui::Color::TRANSPARENT))
                                    }),
                            ),
                    )
            }));

        div()
            .id("dock-root")
            .flex()
            .w_full()
            .h_full()
            .items_end()
            .justify_center()
            .child(shelf)
    }
}

/// Mission Control / Exposé.
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
        let theme = theme_of(cx);
        let tiles = self.model.tiles().to_vec();

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(Hsla::from(theme.palette.window_bg))
            .p(px(28.0))
            .gap(px(24.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(16.0))
                    .h(px(64.0))
                    .w_full()
                    .child(space_chip(&theme, "WORKSPACE 1", true))
                    .child(space_chip(&theme, "WORKSPACE 2", false))
                    .child(space_chip(&theme, "+ SPACE", false)),
            )
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
                            .rounded(px(theme.radii.lg))
                            .flex()
                            .flex_col()
                            .w(px(280.0))
                            .h(px(188.0))
                            .overflow_hidden()
                            .shadow_md()
                            .hover(|s| s.border_color(theme.accent_hsla()).shadow_lg())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .h(px(32.0))
                                    .px(px(12.0))
                                    .bg(Hsla::from(theme.palette.sidebar_bg))
                                    .border_b_1()
                                    .border_color(Hsla::from(theme.palette.separator))
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text_primary_hsla())
                                    .child(tile.title.to_ascii_uppercase())
                                    .child(window_chrome(&theme, 0.85)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(Hsla::from(theme.palette.content_bg))
                                    .font_family(mono_font_family())
                                    .text_size(px(11.0))
                                    .text_color(theme.text_tertiary_hsla())
                                    .child("PREVIEW"),
                            )
                    })),
            )
    }
}

fn space_chip(theme: &Theme, label: &'static str, active: bool) -> impl IntoElement {
    div()
        .theme_card(theme)
        .rounded(px(theme.radii.pill))
        .px(px(20.0))
        .py(px(10.0))
        .when(active, |d| {
            d.bg(theme.inverted_hsla())
                .text_color(theme.on_inverted_hsla())
                .border_color(theme.inverted_hsla())
        })
        .when(!active, |d| d.text_color(theme.text_secondary_hsla()))
        .font_family(sf_font_family())
        .font_weight(FontWeight::BOLD)
        .text_size(px(12.0))
        .child(label)
}

/// Spotlight launcher: centered rounded panel, large field, live results, keyboard nav.
pub struct SpotlightView {
    pub model: Spotlight,
    pub selected: usize,
}

impl SpotlightView {
    pub fn new(model: Spotlight) -> Self {
        Self { model, selected: 0 }
    }

    /// Toggle the Spotlight palette. Returns true when it is open afterwards.
    pub fn toggle(&mut self) -> bool {
        self.model.toggle();
        if self.model.is_open() {
            self.selected = 0;
        }
        self.model.is_open()
    }

    /// Record a launch for the highlighted row, close the palette, and return
    /// the hit id so the host can focus or open the matching window.
    pub fn activate_selected(&mut self) -> Option<String> {
        let hits = self.model.query_str(&self.model.query);
        if hits.is_empty() {
            self.model.close_palette();
            return None;
        }
        let idx = self.selected.min(hits.len() - 1);
        let id = hits[idx].id.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1)
            .max(1);
        self.model.activate(&id, now);
        self.model.close_palette();
        Some(id)
    }
}

fn kind_tag(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Calculator => "CALC",
        ProviderKind::App => "APP",
        ProviderKind::Setting => "SET",
        ProviderKind::File => "FILE",
    }
}

fn kind_mark(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Calculator => "=",
        ProviderKind::App => "▶",
        ProviderKind::Setting => "⚙",
        ProviderKind::File => "▣",
    }
}

impl Render for SpotlightView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme_of(cx);
        let hits = self.model.query_str(&self.model.query);
        let hit_count = hits.len();
        if self.selected >= hit_count && hit_count > 0 {
            self.selected = hit_count - 1;
        }
        let selected = self.selected;
        let query_empty = self.model.query.is_empty();
        let field_text = if query_empty {
            "Search apps, settings, files…".to_string()
        } else {
            self.model.query.clone()
        };

        let card = div()
            .id("spotlight")
            .flex()
            .flex_col()
            .w(px(680.0))
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(20.0))
            .shadow_lg()
            .overflow_hidden()
            .font_family(sf_font_family())
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let mods = event.keystroke.modifiers;
                if mods.control || mods.alt || mods.platform || mods.function {
                    return;
                }
                let key = event.keystroke.key.as_str();
                match key {
                    "up" | "arrowup" => {
                        this.selected = this.selected.saturating_sub(1);
                    }
                    "down" | "arrowdown" => {
                        this.selected = this.selected.saturating_add(1);
                    }
                    "enter" | "return" => {
                        if let Some(id) = this.activate_selected() {
                            cx.defer(move |cx| request_shell_launch(cx, &id));
                        }
                    }
                    "escape" => {
                        this.model.close_palette();
                    }
                    "backspace" => {
                        this.model.query.pop();
                        this.selected = 0;
                    }
                    "space" => {
                        this.model.query.push(' ');
                        this.selected = 0;
                    }
                    k if k.chars().count() == 1 => {
                        let ch = k.chars().next().unwrap_or(' ');
                        let ch = if mods.shift {
                            ch.to_ascii_uppercase()
                        } else {
                            ch
                        };
                        this.model.query.push(ch);
                        this.selected = 0;
                    }
                    _ => {}
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(22.0))
                    .py(px(18.0))
                    .gap(px(14.0))
                    .border_b_1()
                    .border_color(Hsla::from(theme.palette.separator))
                    .rounded_t(px(20.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(32.0))
                            .h(px(32.0))
                            .rounded(px(theme.radii.sm))
                            .bg(Hsla::from(theme.palette.fill))
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0))
                            .text_color(theme.text_primary_hsla())
                            .child("⌕"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(22.0))
                            .font_weight(FontWeight::MEDIUM)
                            .when(query_empty, |d| d.text_color(theme.text_tertiary_hsla()))
                            .when(!query_empty, |d| d.text_color(theme.text_primary_hsla()))
                            .child(field_text),
                    )
                    .child(
                        div()
                            .font_family(mono_font_family())
                            .text_size(px(11.0))
                            .text_color(theme.text_tertiary_hsla())
                            .child("⌘␣"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(10.0))
                    .gap(px(4.0))
                    .max_h(px(420.0))
                    .children(hits.into_iter().enumerate().map(|(idx, hit)| {
                        let is_selected = idx == selected;
                        div()
                            .id(("spotlight-hit", idx))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(12.0))
                            .py(px(10.0))
                            .rounded(px(theme.radii.md))
                            .when(is_selected, |d| {
                                d.bg(theme.inverted_hsla())
                                    .text_color(theme.on_inverted_hsla())
                            })
                            .when(!is_selected, |d| d.hover(|s| s.bg(theme.hover_hsla())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = idx;
                                if let Some(id) = this.activate_selected() {
                                    cx.defer(move |cx| request_shell_launch(cx, &id));
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .w(px(32.0))
                                            .h(px(32.0))
                                            .rounded(px(theme.radii.sm))
                                            .when(is_selected, |d| {
                                                d.bg(theme.on_inverted_hsla())
                                                    .text_color(theme.inverted_hsla())
                                            })
                                            .when(!is_selected, |d| {
                                                d.bg(Hsla::from(theme.palette.fill))
                                                    .text_color(theme.text_primary_hsla())
                                            })
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(14.0))
                                            .child(kind_mark(hit.kind)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.0))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_size(px(15.0))
                                                    .when(is_selected, |d| {
                                                        d.text_color(theme.on_inverted_hsla())
                                                    })
                                                    .when(!is_selected, |d| {
                                                        d.text_color(theme.text_primary_hsla())
                                                    })
                                                    .child(hit.title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .when(is_selected, |d| {
                                                        d.text_color(theme.on_inverted_hsla())
                                                    })
                                                    .when(!is_selected, |d| {
                                                        d.text_color(theme.text_secondary_hsla())
                                                    })
                                                    .child(hit.subtitle),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when_some(hit.value, |d, val| {
                                        d.child(
                                            div()
                                                .font_family(mono_font_family())
                                                .font_weight(FontWeight::BOLD)
                                                .text_size(px(16.0))
                                                .child(val),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(theme.radii.pill))
                                            .when(is_selected, |d| {
                                                d.bg(theme.on_inverted_hsla())
                                                    .text_color(theme.inverted_hsla())
                                            })
                                            .when(!is_selected, |d| {
                                                d.bg(Hsla::from(theme.palette.fill))
                                                    .text_color(theme.text_secondary_hsla())
                                            })
                                            .font_family(mono_font_family())
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::BOLD)
                                            .child(kind_tag(hit.kind)),
                                    ),
                            )
                    })),
            );

        div()
            .id("spotlight-root")
            .flex()
            .w_full()
            .h_full()
            .items_start()
            .justify_center()
            .p(px(20.0))
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::DockPlacement;
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
    fn test_spotlight_enter_activates_selected_and_closes(cx: &mut TestAppContext) {
        let model = Spotlight::with_builtins();
        cx.update(|cx| {
            let entity = cx.new(|_cx| SpotlightView::new(model));
            entity.update(cx, |this, _cx| {
                assert!(this.toggle());
                this.model.query = "explorer".to_string();
                this.selected = 0;
                let id = this.activate_selected().expect("explorer hit");
                assert_eq!(id, "explorer");
                assert!(!this.model.is_open());
            });
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
