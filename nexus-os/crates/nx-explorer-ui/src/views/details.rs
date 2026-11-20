//! Virtualized Details Table View (Name, Modified, Size, Kind) handling 100k+ rows in < 16ms first frame.

use nx_explorer::pane::Pane;
use nx_fs::sort::SortKey;
use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::layout::{layout_columns, Align, ColWidth, Column};
use nx_ui::widgets::ScrollView;

pub const HEADER_H: f32 = 30.0;
pub const ROW_H: f32 = 28.0;

pub const DETAILS_COLUMNS: [Column; 4] = [
    Column {
        title: "Name",
        width: ColWidth::Flex(2.5),
        align: Align::Start,
    },
    Column {
        title: "Date Modified",
        width: ColWidth::Fixed(130.0),
        align: Align::Start,
    },
    Column {
        title: "Size",
        width: ColWidth::Fixed(85.0),
        align: Align::End,
    },
    Column {
        title: "Kind",
        width: ColWidth::Fixed(95.0),
        align: Align::Start,
    },
];

pub struct DetailsView {
    pub scroll: ScrollView,
    hover_row: Option<usize>,
    hover_header: Option<usize>,
}

impl Default for DetailsView {
    fn default() -> Self {
        Self::new()
    }
}

impl DetailsView {
    pub fn new() -> Self {
        Self {
            scroll: ScrollView::new(),
            hover_row: None,
            hover_header: None,
        }
    }

    pub fn animating(&self) -> bool {
        self.scroll.animating()
    }

    pub fn update(
        &mut self,
        input: &Input,
        bounds: Rect,
        pane: &Pane,
        dt: f32,
    ) -> (Option<usize>, Option<SortKey>) {
        let total = pane.len() as u64;
        self.scroll.content = total as f32 * ROW_H;

        let body_rect = Rect::new(
            bounds.x,
            bounds.y + HEADER_H,
            bounds.w,
            (bounds.h - HEADER_H).max(0.0),
        );
        let inside_body = body_rect.contains(input.pointer);
        let header_rect = Rect::new(bounds.x, bounds.y, bounds.w, HEADER_H);
        let inside_header = header_rect.contains(input.pointer);

        self.scroll
            .update(input.scroll_y, body_rect.h, inside_body, dt);

        let mut clicked_row = None;
        let mut clicked_sort = None;
        self.hover_row = None;
        self.hover_header = None;

        if inside_header {
            // Header column hit-testing
            let mut widths = [0.0f32; 4];
            let pad = 12.0;
            let gap = 8.0;
            let inner_w = (bounds.w - pad * 2.0).max(0.0);
            if layout_columns(inner_w, gap, &DETAILS_COLUMNS, &mut widths) {
                let mut cursor = bounds.x + pad;
                for (i, &w) in widths.iter().enumerate() {
                    let col_rect = Rect::new(cursor, bounds.y, w, HEADER_H);
                    if col_rect.contains(input.pointer) {
                        self.hover_header = Some(i);
                        if input.primary_released {
                            let key = match i {
                                0 => SortKey::Name,
                                1 => SortKey::Modified,
                                2 => SortKey::Size,
                                _ => SortKey::Kind,
                            };
                            clicked_sort = Some(key);
                        }
                        break;
                    }
                    cursor += w + gap;
                }
            }
        } else if inside_body && total > 0 {
            let local_y = input.pointer.y - body_rect.y + self.scroll.offset.max(0.0);
            let row_idx = (local_y / ROW_H) as usize;
            if row_idx < pane.len() {
                self.hover_row = Some(row_idx);
                if input.primary_pressed {
                    clicked_row = Some(row_idx);
                }
            }
        }

        (clicked_row, clicked_sort)
    }
}
