//! The dock's right-click menu and Recent Items stack as a real pop-up: a
//! small transient window above the shelf (rounded, shadowed, monochrome),
//! keyboard-driven, gone on Esc, on a pick, or when you click elsewhere.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    div, img, px, App, Context, FocusHandle, FontWeight, Hsla, ImageSource, IntoElement,
    KeyDownEvent, ParentElement, Render, RenderImage, SharedString, Styled, WeakEntity, Window,
};
use nx_ui::gpui_theme::sf_font_family;
use nx_ui::theme::Theme;
use nx_ui::{glyph_icon, GlyphIcon};

/// What a popup row does when picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupAction {
    /// Keep (true) or remove (false) the dock item at this index.
    Keep(usize, bool),
    ShowInFiles(String),
    Quit(String),
    Open(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupRow {
    Header(String),
    Separator,
    Item {
        label: String,
        action: PopupAction,
        /// File/app path whose real icon is shown (stack rows).
        icon_path: Option<String>,
    },
    Note(String),
}

impl PopupRow {
    fn pickable(&self) -> bool {
        matches!(self, PopupRow::Item { .. })
    }
}

pub const ROW_H: f32 = 28.0;
pub const PAD: f32 = 6.0;
/// Transparent margin around the panel for the shadow.
pub const MARGIN: f32 = 16.0;

/// Panel size for these rows (logical px, without the shadow margin).
pub fn panel_size(rows: &[PopupRow], stack: bool) -> (f32, f32) {
    let h: f32 = rows
        .iter()
        .map(|r| match r {
            PopupRow::Separator => 9.0,
            PopupRow::Header(_) => 26.0,
            _ => ROW_H,
        })
        .sum::<f32>()
        + PAD * 2.0;
    (if stack { 300.0 } else { 220.0 }, h.max(ROW_H + PAD * 2.0))
}

/// Host for the pick: the dock applies the action and forgets the popup.
pub type PopupSink = Arc<dyn Fn(Option<PopupAction>, &mut App)>;

pub struct DockPopupView {
    rows: Vec<PopupRow>,
    selected: Option<usize>,
    stack: bool,
    sink: PopupSink,
    focus: Option<FocusHandle>,
    watching: bool,
    closed: bool,
    opened_at: Instant,
    icons: HashMap<String, Arc<RenderImage>>,
    requested: bool,
}

impl DockPopupView {
    pub fn new(
        rows: Vec<PopupRow>,
        stack: bool,
        icons: HashMap<String, Arc<RenderImage>>,
        sink: PopupSink,
    ) -> Self {
        Self {
            rows,
            selected: None,
            stack,
            sink,
            focus: None,
            watching: false,
            closed: false,
            opened_at: Instant::now(),
            icons,
            requested: false,
        }
    }

    fn finish(&mut self, action: Option<PopupAction>, window: &mut Window, cx: &mut App) {
        if self.closed {
            return;
        }
        self.closed = true;
        let sink = self.sink.clone();
        window.remove_window();
        cx.defer(move |cx| sink(action, cx));
    }

    fn step(&mut self, dir: i32) {
        let pickable: Vec<usize> = (0..self.rows.len())
            .filter(|i| self.rows[*i].pickable())
            .collect();
        if pickable.is_empty() {
            return;
        }
        let pos = self
            .selected
            .and_then(|s| pickable.iter().position(|p| *p == s));
        let next = match (pos, dir >= 0) {
            (None, true) => 0,
            (None, false) => pickable.len() - 1,
            (Some(p), true) => (p + 1) % pickable.len(),
            (Some(p), false) => (p + pickable.len() - 1) % pickable.len(),
        };
        self.selected = Some(pickable[next]);
    }

    fn request_icons(&mut self, cx: &mut Context<Self>) {
        if self.requested {
            return;
        }
        self.requested = true;
        for row in &self.rows {
            let PopupRow::Item {
                icon_path: Some(path),
                ..
            } = row
            else {
                continue;
            };
            if self.icons.contains_key(path) {
                continue;
            }
            let path = path.clone();
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let raster = cx
                    .background_executor()
                    .spawn(async move { crate::icons::app_icon_rgba(&p) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some((w, h, bytes)) = raster {
                        if let Some(buf) = image::RgbaImage::from_raw(w, h, bytes) {
                            let frame = Arc::new(RenderImage::new(vec![image::Frame::new(buf)]));
                            this.icons.insert(path, frame);
                            cx.notify();
                        }
                    }
                });
            })
            .detach();
        }
    }
}

fn theme_of(cx: &App) -> Theme {
    if cx.has_global::<Theme>() {
        *cx.global::<Theme>()
    } else {
        Theme::stealth()
    }
}

fn glyph_for(action: &PopupAction) -> GlyphIcon {
    match action {
        PopupAction::Keep(_, true) => GlyphIcon::Tray,
        PopupAction::Keep(_, false) => GlyphIcon::Trash,
        PopupAction::ShowInFiles(_) => GlyphIcon::Folder,
        PopupAction::Quit(_) => GlyphIcon::Cpu,
        PopupAction::Open(_) => GlyphIcon::Folder,
    }
}

impl Render for DockPopupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::guard::render("dock popup", || {
            self.render_body(window, cx).into_any_element()
        })
    }
}

impl DockPopupView {
    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme_of(cx);
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if !self.watching {
            self.watching = true;
            window.focus(&focus);
            cx.observe_window_activation(window, |this, window, cx| {
                // Ignore the activation churn of the first frames.
                if !window.is_window_active() && this.opened_at.elapsed().as_millis() > 150 {
                    this.finish(None, window, cx);
                }
            })
            .detach();
        }
        self.request_icons(cx);
        // Appear: a quick fade, stepped by real frame time.
        let t = (self.opened_at.elapsed().as_secs_f32() / 0.14).min(1.0);
        let appear = 1.0 - (1.0 - t) * (1.0 - t);
        if t < 1.0 {
            window.request_animation_frame();
        }
        let selected = self.selected;
        let (w, _) = panel_size(&self.rows, self.stack);
        let fg = theme.text_primary_hsla();
        let dim = theme.text_tertiary_hsla();
        let rows: Vec<gpui::AnyElement> = self
            .rows
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, row)| match row {
                PopupRow::Separator => div()
                    .h(px(9.0))
                    .flex()
                    .items_center()
                    .px(px(10.0))
                    .child(
                        div()
                            .h(px(1.0))
                            .w_full()
                            .bg(Hsla::from(theme.palette.separator)),
                    )
                    .into_any_element(),
                PopupRow::Header(text) => div()
                    .h(px(26.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(dim)
                    .child(text.to_uppercase())
                    .into_any_element(),
                PopupRow::Note(text) => div()
                    .h(px(ROW_H))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(px(13.0))
                    .text_color(dim)
                    .child(text)
                    .into_any_element(),
                PopupRow::Item {
                    label,
                    action,
                    icon_path,
                } => {
                    let on = selected == Some(i);
                    let ink = if on { theme.on_inverted_hsla() } else { fg };
                    let icon = icon_path.as_ref().and_then(|p| self.icons.get(p).cloned());
                    let glyph = glyph_for(&action);
                    let act = action.clone();
                    div()
                        .id(SharedString::from(format!("dock-pop-{i}")))
                        .h(px(ROW_H))
                        .px(px(10.0))
                        .mx(px(PAD))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(9.0))
                        .rounded(px(7.0))
                        .when(on, |d| d.bg(theme.inverted_hsla()))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ink)
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.selected = Some(i);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.finish(Some(act.clone()), window, cx);
                        }))
                        .map(|d| match icon {
                            Some(image) => {
                                d.child(img(ImageSource::Render(image)).w(px(18.0)).h(px(18.0)))
                            }
                            None => d.child(glyph_icon(glyph, ink, 14.0)),
                        })
                        .child(div().flex_1().overflow_hidden().child(label))
                        .into_any_element()
                }
            })
            .collect();
        div()
            .size_full()
            .p(px(MARGIN))
            .flex()
            .items_end()
            .justify_center()
            .child(
                div()
                    .track_focus(&focus)
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        match ev.keystroke.key.as_str() {
                            "escape" => this.finish(None, window, cx),
                            "up" | "arrowup" => this.step(-1),
                            "down" | "arrowdown" | "tab" => this.step(1),
                            "enter" | "return" | "space" => {
                                let act = this.selected.and_then(|s| match &this.rows[s] {
                                    PopupRow::Item { action, .. } => Some(action.clone()),
                                    _ => None,
                                });
                                if act.is_some() {
                                    this.finish(act, window, cx);
                                }
                            }
                            _ => {}
                        }
                        cx.notify();
                    }))
                    .w(px(w))
                    .py(px(PAD))
                    .flex()
                    .flex_col()
                    .opacity(appear)
                    .mb(px((1.0 - appear) * 6.0))
                    .rounded(px(12.0))
                    .bg(Hsla::from(theme.palette.menu_bg))
                    .border_1()
                    .border_color(Hsla::from(theme.palette.separator))
                    .shadow_lg()
                    .font_family(sf_font_family())
                    .children(rows),
            )
    }
}

/// Window bounds (logical) for a popup whose panel is `size`, centered on
/// `anchor_x` with its bottom edge `gap` above `shelf_top`, kept on screen.
pub fn popup_bounds(
    size: (f32, f32),
    anchor_x: f32,
    shelf_top: f32,
    gap: f32,
    screen_left: f32,
    screen_right: f32,
) -> (f32, f32, f32, f32) {
    let w = size.0 + MARGIN * 2.0;
    let h = size.1 + MARGIN * 2.0;
    let x = (anchor_x - w * 0.5).clamp(screen_left, (screen_right - w).max(screen_left));
    let y = shelf_top - gap - h + MARGIN;
    (x, y, w, h)
}

#[allow(dead_code)]
fn _assert_weak(_: WeakEntity<DockPopupView>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_skips_headers_and_wraps() {
        let rows = vec![
            PopupRow::Header("Chrome".into()),
            PopupRow::Item {
                label: "Keep in Dock".into(),
                action: PopupAction::Keep(2, true),
                icon_path: None,
            },
            PopupRow::Separator,
            PopupRow::Item {
                label: "Quit".into(),
                action: PopupAction::Quit("chrome".into()),
                icon_path: None,
            },
        ];
        let sink: PopupSink = Arc::new(|_, _| {});
        let mut v = DockPopupView::new(rows, false, HashMap::new(), sink);
        v.step(1);
        assert_eq!(v.selected, Some(1));
        v.step(1);
        assert_eq!(v.selected, Some(3));
        v.step(1);
        assert_eq!(v.selected, Some(1));
        v.step(-1);
        assert_eq!(v.selected, Some(3));
    }

    #[test]
    fn popup_sits_above_the_shelf_and_on_screen() {
        let (x, y, w, h) = popup_bounds((220.0, 100.0), 10.0, 1000.0, 8.0, 0.0, 1920.0);
        assert_eq!(x, 0.0);
        assert!((y + h - MARGIN) <= 1000.0 - 8.0 + 0.01);
        assert_eq!(w, 220.0 + 2.0 * MARGIN);
        let (_, h2) = panel_size(&[PopupRow::Separator], false);
        assert!(h2 >= ROW_H);
    }
}
