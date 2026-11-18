//! Miller Columns View (Finder hierarchical column navigation).

use super::icon_for_entry;
use crate::icons::ICON_CHEVRON_RIGHT;
use nx_explorer::pane::Pane;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::layout::{virtual_window, VirtualSpec};
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::{Frame, ScrollView};

pub const COLUMN_W: f32 = 220.0;
pub const ROW_H: f32 = 28.0;

pub struct ColumnsView {
    pub scroll: ScrollView,
    hover_row: Option<usize>,
}

impl Default for ColumnsView {
    fn default() -> Self {
        Self::new()
    }
}

impl ColumnsView {
    pub fn new() -> Self {
        Self {
            scroll: ScrollView::new(),
            hover_row: None,
        }
    }

    pub fn animating(&self) -> bool {
        self.scroll.animating()
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, pane: &Pane, dt: f32) -> Option<usize> {
        let total = pane.len() as u64;
        self.scroll.content = total as f32 * ROW_H;

        let col_rect = Rect::new(bounds.x, bounds.y, COLUMN_W.min(bounds.w), bounds.h);
        let inside = col_rect.contains(input.pointer);
        self.scroll.update(input.scroll_y, bounds.h, inside, dt);

        let mut clicked = None;
        self.hover_row = None;

        if inside && total > 0 {
            let local_y = input.pointer.y - bounds.y + self.scroll.offset.max(0.0);
            let row_idx = (local_y / ROW_H) as usize;
            if row_idx < pane.len() {
                self.hover_row = Some(row_idx);
                if input.primary_pressed {
                    clicked = Some(row_idx);
                }
            }
        }

        clicked
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect, pane: &Pane) {
        let theme = frame.theme;

        // Background
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.content_bg),
            CornerRadii::all(0.0),
            Border::default(),
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        let col_w = COLUMN_W.min(bounds.w);
        let col_rect = Rect::new(bounds.x, bounds.y, col_w, bounds.h);

        // Column separator
        frame.panel(
            Scene::root(),
            Rect::new(col_rect.right() - 1.0, bounds.y, 1.0, bounds.h),
            Paint::Solid(theme.palette.separator),
            CornerRadii::all(0.0),
            Border::default(),
            nx_ui::tokens::Shadow::none(),
            Layer::Foreground,
        );

        let total = pane.len() as u64;
        let spec = VirtualSpec {
            row_count: total,
            row_height: ROW_H,
            overscan: 2,
        };
        let window = virtual_window(&spec, self.scroll.offset.max(0.0), bounds.h);

        let style = TextStyle::new(
            theme.type_scale.callout.size,
            theme.type_scale.callout.line,
            Weight::Regular,
        );

        let pad_x = 12.0;

        for offset in 0..window.count {
            let row_idx = (window.first + offset as u64) as usize;
            let Some(entry) = pane.row(row_idx) else {
                continue;
            };

            let y = bounds.y + window.offset_y + offset as f32 * ROW_H;
            let is_selected = pane.is_selected(row_idx);
            let is_hovered = self.hover_row == Some(row_idx);

            if is_selected {
                let sel_rect = Rect::new(bounds.x + 4.0, y + 1.0, col_w - 8.0, ROW_H - 2.0);
                frame.panel(
                    Scene::root(),
                    sel_rect,
                    Paint::Solid(theme.palette.selection),
                    CornerRadii::all(theme.radii.xs),
                    Border::default(),
                    nx_ui::tokens::Shadow::none(),
                    Layer::Background,
                );
            } else if is_hovered {
                let hov_rect = Rect::new(bounds.x + 4.0, y + 1.0, col_w - 8.0, ROW_H - 2.0);
                frame.panel(
                    Scene::root(),
                    hov_rect,
                    Paint::Solid(theme.palette.hover),
                    CornerRadii::all(theme.radii.xs),
                    Border::default(),
                    nx_ui::tokens::Shadow::none(),
                    Layer::Background,
                );
            }

            let text_y = y + (ROW_H - style.line) * 0.5;

            // Icon
            let icon = icon_for_entry(entry);
            let icon_px = 16.0;
            let icon_x = bounds.x + pad_x;
            let icon_y = y + (ROW_H - icon_px) * 0.5;
            let icon_tint = if is_selected {
                theme.palette.accent
            } else {
                theme.palette.text_secondary
            };
            frame.icon(
                Scene::root(),
                Point::new(icon_x, icon_y),
                icon,
                icon_px,
                icon_tint,
            );

            // Name
            let name_x = icon_x + icon_px + 8.0;
            let name = &entry.display_name;
            frame.label(
                Scene::root(),
                Point::new(name_x, text_y),
                name,
                style,
                theme.palette.text_primary,
            );

            // Chevron for folders
            if entry.kind.is_dir() {
                let chev_x = col_rect.right() - 20.0;
                let chev_y = y + (ROW_H - 12.0) * 0.5;
                frame.icon(
                    Scene::root(),
                    Point::new(chev_x, chev_y),
                    ICON_CHEVRON_RIGHT,
                    12.0,
                    if is_selected {
                        theme.palette.accent
                    } else {
                        theme.palette.text_tertiary
                    },
                );
            }
        }
    }
}
