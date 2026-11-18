//! Virtualized Icon / Grid View for files and folders.

use super::icon_for_entry;
use nx_explorer::pane::Pane;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::layout::{virtual_window, VirtualSpec};
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::{Frame, ScrollView};

pub const TILE_W: f32 = 96.0;
pub const TILE_H: f32 = 96.0;

pub struct GridView {
    pub scroll: ScrollView,
    hover_index: Option<usize>,
}

impl Default for GridView {
    fn default() -> Self {
        Self::new()
    }
}

impl GridView {
    pub fn new() -> Self {
        Self {
            scroll: ScrollView::new(),
            hover_index: None,
        }
    }

    pub fn animating(&self) -> bool {
        self.scroll.animating()
    }

    pub fn columns_for_width(width: f32) -> usize {
        let pad = 24.0;
        let avail = (width - pad).max(TILE_W);
        ((avail / TILE_W).floor() as usize).max(1)
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, pane: &Pane, dt: f32) -> Option<usize> {
        let cols = Self::columns_for_width(bounds.w);
        let total = pane.len();
        let row_count = total.div_ceil(cols) as u64;

        self.scroll.content = row_count as f32 * TILE_H;
        let inside = bounds.contains(input.pointer);
        self.scroll.update(input.scroll_y, bounds.h, inside, dt);

        let mut clicked = None;
        self.hover_index = None;

        if inside && total > 0 {
            let pad_x = (bounds.w - cols as f32 * TILE_W) * 0.5;
            let local_x = input.pointer.x - bounds.x - pad_x;
            let local_y = input.pointer.y - bounds.y + self.scroll.offset.max(0.0);

            if local_x >= 0.0 && local_y >= 0.0 {
                let col = (local_x / TILE_W) as usize;
                let row = (local_y / TILE_H) as usize;
                if col < cols {
                    let idx = row * cols + col;
                    if idx < total {
                        self.hover_index = Some(idx);
                        if input.primary_pressed {
                            clicked = Some(idx);
                        }
                    }
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

        let cols = Self::columns_for_width(bounds.w);
        let total = pane.len();
        if total == 0 {
            return;
        }

        let row_count = total.div_ceil(cols) as u64;
        let spec = VirtualSpec {
            row_count,
            row_height: TILE_H,
            overscan: 1,
        };
        let window = virtual_window(&spec, self.scroll.offset.max(0.0), bounds.h);

        let pad_x = (bounds.w - cols as f32 * TILE_W) * 0.5;
        let name_style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Medium,
        );

        for row_offset in 0..window.count {
            let row_idx = (window.first + row_offset as u64) as usize;
            let y = bounds.y + window.offset_y + row_offset as f32 * TILE_H;

            for col in 0..cols {
                let item_idx = row_idx * cols + col;
                if item_idx >= total {
                    break;
                }
                let Some(entry) = pane.row(item_idx) else {
                    continue;
                };

                let x = bounds.x + pad_x + col as f32 * TILE_W;
                let tile_rect = Rect::new(x + 4.0, y + 4.0, TILE_W - 8.0, TILE_H - 8.0);

                let is_selected = pane.is_selected(item_idx);
                let is_hovered = self.hover_index == Some(item_idx);

                if is_selected {
                    frame.panel(
                        Scene::root(),
                        tile_rect,
                        Paint::Solid(theme.palette.selection),
                        CornerRadii::all(theme.radii.sm),
                        Border::default(),
                        nx_ui::tokens::Shadow::none(),
                        Layer::Background,
                    );
                } else if is_hovered {
                    frame.panel(
                        Scene::root(),
                        tile_rect,
                        Paint::Solid(theme.palette.hover),
                        CornerRadii::all(theme.radii.sm),
                        Border::default(),
                        nx_ui::tokens::Shadow::none(),
                        Layer::Background,
                    );
                }

                // Centered 44px Icon
                let icon = icon_for_entry(entry);
                let icon_px = 44.0;
                let icon_x = tile_rect.center().x - icon_px * 0.5;
                let icon_y = tile_rect.y + 8.0;
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

                // Centered truncated filename
                let name = &entry.display_name;
                let prep = frame.text.prepare(frame.atlas, name, name_style);
                let text_x = (tile_rect.center().x - prep.width * 0.5).max(tile_rect.x + 4.0);
                let text_y = icon_y + icon_px + 6.0;

                frame.label(
                    Scene::root(),
                    Point::new(text_x, text_y),
                    name,
                    name_style,
                    theme.palette.text_primary,
                );
            }
        }
    }
}
