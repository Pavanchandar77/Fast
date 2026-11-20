//! Bottom status bar: item count, selection count and size, free space.

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

    pub fn items_text(&self) -> String {
        format_items_count(self.item_count)
    }

    pub fn selection_text(&self) -> Option<String> {
        if self.selected_count > 0 {
            Some(format_selection(self.selected_count, self.selected_bytes))
        } else {
            None
        }
    }
}

pub fn format_items_count(count: usize) -> String {
    if count == 1 {
        "1 item".to_string()
    } else {
        format!("{count} items")
    }
}

pub fn format_selection(count: usize, bytes: u64) -> String {
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
