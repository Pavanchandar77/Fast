//! Miller Columns View (Finder hierarchical column navigation).

use nx_explorer::pane::Pane;
use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::widgets::ScrollView;

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
}
