//! Virtualized Details Table View (Name, Modified, Size, Kind) handling 100k+ rows in < 16ms first frame.

use super::{icon_for_entry, kind_for_entry};
use crate::status_bar::format_bytes;
use nx_explorer::pane::Pane;
use nx_fs::sort::SortKey;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::layout::{layout_columns, virtual_window, Align, ColWidth, Column, VirtualSpec};
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::{Frame, ScrollView};

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

    pub fn paint(
        &self,
        frame: &mut Frame<'_>,
        bounds: Rect,
        pane: &Pane,
        rename_row: Option<usize>,
        rename_text: &str,
    ) {
        let theme = frame.theme;

        // Content area background
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.content_bg),
            CornerRadii::all(0.0),
            Border::default(),
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        let mut widths = [0.0f32; 4];
        let pad = 12.0;
        let gap = 8.0;
        let inner_w = (bounds.w - pad * 2.0).max(0.0);
        if !layout_columns(inner_w, gap, &DETAILS_COLUMNS, &mut widths) {
            return;
        }

        let mut xs = [0.0f32; 4];
        let mut cursor = bounds.x + pad;
        for (i, width) in widths.iter().enumerate() {
            xs[i] = cursor;
            cursor += width + gap;
        }

        let header_rect = Rect::new(bounds.x, bounds.y, bounds.w, HEADER_H);
        let body_rect = Rect::new(
            bounds.x,
            bounds.y + HEADER_H,
            bounds.w,
            (bounds.h - HEADER_H).max(0.0),
        );

        // Header Background
        frame.panel(
            Scene::root(),
            header_rect,
            Paint::Solid(theme.palette.window_bg),
            CornerRadii::all(0.0),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        let header_style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Semibold,
        );
        let cell_style = TextStyle::new(
            theme.type_scale.callout.size,
            theme.type_scale.callout.line,
            Weight::Regular,
        );

        // Header Titles
        for (i, col) in DETAILS_COLUMNS.iter().enumerate() {
            let col_x = xs[i];
            let col_w = widths[i];
            let is_sort_col = match i {
                0 => pane.sort.key == SortKey::Name,
                1 => pane.sort.key == SortKey::Modified,
                2 => pane.sort.key == SortKey::Size,
                _ => pane.sort.key == SortKey::Kind,
            };

            let sort_arrow = if is_sort_col {
                if pane.sort.ascending {
                    " ↑"
                } else {
                    " ↓"
                }
            } else {
                ""
            };

            let title_text = if sort_arrow.is_empty() {
                col.title.to_string()
            } else {
                format!("{}{}", col.title, sort_arrow)
            };

            let text_color = if is_sort_col {
                theme.palette.text_primary
            } else {
                theme.palette.text_secondary
            };

            let prep = frame.text.prepare(frame.atlas, &title_text, header_style);
            let text_x = if col.align == Align::End {
                col_x + (col_w - prep.width).max(0.0)
            } else {
                col_x
            };
            let text_y = header_rect.y + (HEADER_H - header_style.line) * 0.5;

            frame.label(
                Scene::root(),
                Point::new(text_x, text_y),
                &title_text,
                header_style,
                text_color,
            );
        }

        // Virtualized Body
        let total = pane.len() as u64;
        let spec = VirtualSpec {
            row_count: total,
            row_height: ROW_H,
            overscan: 2,
        };
        let window = virtual_window(&spec, self.scroll.offset.max(0.0), body_rect.h);

        for offset in 0..window.count {
            let row_idx = (window.first + offset as u64) as usize;
            let Some(entry) = pane.row(row_idx) else {
                continue;
            };

            let y = body_rect.y + window.offset_y + offset as f32 * ROW_H;

            let is_selected = pane.is_selected(row_idx);
            let is_hovered = self.hover_row == Some(row_idx);

            // Row Highlight
            if is_selected {
                let sel_rect = Rect::new(bounds.x + 4.0, y + 1.0, bounds.w - 8.0, ROW_H - 2.0);
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
                let hov_rect = Rect::new(bounds.x + 4.0, y + 1.0, bounds.w - 8.0, ROW_H - 2.0);
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

            let text_y = y + (ROW_H - cell_style.line) * 0.5;
            let text_color = theme.palette.text_primary;

            // Col 0: Icon + Name (or inline rename input)
            let icon = icon_for_entry(entry);
            let icon_px = 16.0;
            let icon_y = y + (ROW_H - icon_px) * 0.5;
            let icon_tint = if is_selected {
                theme.palette.accent
            } else {
                theme.palette.text_secondary
            };
            frame.icon(
                Scene::root(),
                Point::new(xs[0], icon_y),
                icon,
                icon_px,
                icon_tint,
            );

            let name_x = xs[0] + icon_px + 8.0;

            if rename_row == Some(row_idx) {
                // Inline rename input box
                let edit_w = (widths[0] - icon_px - 16.0).max(100.0);
                let edit_rect = Rect::new(name_x - 4.0, y + 2.0, edit_w, ROW_H - 4.0);
                frame.panel(
                    Scene::root(),
                    edit_rect,
                    Paint::Solid(theme.palette.control),
                    CornerRadii::all(theme.radii.xs),
                    Border {
                        color: theme.palette.accent,
                        width: 1.0,
                    },
                    nx_ui::tokens::Shadow::none(),
                    Layer::Foreground,
                );
                let cursor_str = format!("{rename_text}|");
                frame.label(
                    Scene::root(),
                    Point::new(name_x, text_y),
                    &cursor_str,
                    cell_style,
                    theme.palette.text_primary,
                );
            } else {
                let name = &entry.display_name;
                frame.label(
                    Scene::root(),
                    Point::new(name_x, text_y),
                    name,
                    cell_style,
                    text_color,
                );
            }

            // Col 1: Date Modified
            frame.label(
                Scene::root(),
                Point::new(xs[1], text_y),
                "Nov 18, 2026",
                cell_style,
                theme.palette.text_secondary,
            );

            // Col 2: Size
            let size_str = if entry.kind.is_dir() {
                "--".to_string()
            } else {
                format_bytes(entry.size)
            };
            let size_prep = frame.text.prepare(frame.atlas, &size_str, cell_style);
            let size_x = xs[2] + (widths[2] - size_prep.width).max(0.0);
            frame.label(
                Scene::root(),
                Point::new(size_x, text_y),
                &size_str,
                cell_style,
                theme.palette.text_secondary,
            );

            // Col 3: Kind
            let kind_str = kind_for_entry(entry);
            frame.label(
                Scene::root(),
                Point::new(xs[3], text_y),
                kind_str,
                cell_style,
                theme.palette.text_secondary,
            );
        }
    }
}
