//! Bottom status bar: item count, selection count and size, free space.

use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::Frame;

pub struct StatusBar {
    pub item_count: usize,
    pub selected_count: usize,
    pub selected_bytes: u64,
    pub free_space_text: Option<String>,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub const HEIGHT: f32 = 26.0;

    pub fn new() -> Self {
        Self {
            item_count: 0,
            selected_count: 0,
            selected_bytes: 0,
            free_space_text: None,
        }
    }

    pub fn update(
        &mut self,
        item_count: usize,
        selected_count: usize,
        selected_bytes: u64,
        free_space: Option<String>,
    ) {
        self.item_count = item_count;
        self.selected_count = selected_count;
        self.selected_bytes = selected_bytes;
        self.free_space_text = free_space;
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;

        // Background panel
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.window_bg),
            CornerRadii::all(0.0),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        let style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Regular,
        );

        let text_y = bounds.y + (bounds.h - style.line) * 0.5;
        let pad_x = theme.spacing.s16;

        // Formatted count: e.g. "1,248 items"
        let count_str = format_items_count(self.item_count);
        frame.label(
            Scene::root(),
            Point::new(bounds.x + pad_x, text_y),
            &count_str,
            style,
            theme.palette.text_secondary,
        );

        // Selection info if any items selected
        if self.selected_count > 0 {
            let sel_str = format_selection(self.selected_count, self.selected_bytes);
            let sel_prep = frame.text.prepare(frame.atlas, &sel_str, style);
            let sel_x = bounds.center().x - sel_prep.width * 0.5;
            frame.label(
                Scene::root(),
                Point::new(sel_x, text_y),
                &sel_str,
                style,
                theme.palette.text_primary,
            );
        }

        // Right side: free space
        if let Some(free) = &self.free_space_text {
            let prep = frame.text.prepare(frame.atlas, free, style);
            let x = bounds.right() - pad_x - prep.width;
            frame.label(
                Scene::root(),
                Point::new(x, text_y),
                free,
                style,
                theme.palette.text_tertiary,
            );
        }
    }
}

fn format_items_count(count: usize) -> String {
    if count == 1 {
        "1 item".to_string()
    } else {
        format!("{count} items")
    }
}

fn format_selection(count: usize, bytes: u64) -> String {
    let size_str = format_bytes(bytes);
    if count == 1 {
        format!("1 item selected, {size_str}")
    } else {
        format!("{count} items selected, {size_str}")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes < TB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    }
}
