//! Virtualized Icon / Grid View for files and folders.

use nx_explorer::pane::Pane;
use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::widgets::ScrollView;

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
}
