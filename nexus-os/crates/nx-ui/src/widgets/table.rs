//! Virtualized table. A 100k-row document list only builds the visible window.

use crate::geom::Rect;
use crate::input::Input;
use crate::layout::{virtual_window, Align, ColWidth, Column, VirtualSpec};
use crate::widgets::ScrollView;

pub const TABLE_COLUMNS: [Column; 4] = [
    Column {
        title: "Name",
        width: ColWidth::Flex(2.4),
        align: Align::Start,
    },
    Column {
        title: "Kind",
        width: ColWidth::Fixed(88.0),
        align: Align::Start,
    },
    Column {
        title: "Size",
        width: ColWidth::Fixed(80.0),
        align: Align::End,
    },
    Column {
        title: "Modified",
        width: ColWidth::Fixed(120.0),
        align: Align::Start,
    },
];

const HEADER_H: f32 = 32.0;
const ROW_H: f32 = 28.0;

pub struct VirtualTable {
    row_count: u64,
    row_height: f32,
    selected: Option<u64>,
    hover: Option<u64>,
    query: String,
    kind: Option<&'static str>,
    filtered: bool,
    matches: Vec<u64>,
    scroll: ScrollView,
}

impl VirtualTable {
    pub fn new(row_count: u64) -> Self {
        Self {
            row_count,
            row_height: ROW_H,
            selected: None,
            hover: None,
            query: String::new(),
            kind: None,
            filtered: false,
            matches: Vec::new(),
            scroll: ScrollView::new(),
        }
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn visible_total(&self) -> u64 {
        if self.filtered {
            self.matches.len() as u64
        } else {
            self.row_count
        }
    }

    pub fn visible_window(&self, viewport_h: f32) -> u32 {
        let body_h = (viewport_h - HEADER_H).max(0.0);
        let spec = self.spec();
        virtual_window(&spec, self.scroll.offset.max(0.0), body_h).count
    }

    pub fn matches_capacity(&self) -> usize {
        self.matches.capacity()
    }

    pub fn animating(&self) -> bool {
        self.scroll.animating()
    }

    pub fn set_query(&mut self, query: &str) {
        if self.query == query {
            return;
        }
        self.query.clear();
        self.query.push_str(query);
        self.rebuild();
    }

    pub fn set_kind(&mut self, kind: Option<&'static str>) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        self.rebuild();
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) {
        let body = self.body(bounds);
        let inside = body.contains(input.pointer);
        self.scroll.content = self.visible_total() as f32 * self.row_height;
        self.scroll.update(input.scroll_y, body.h, inside, dt);
        if inside {
            self.hover = self.row_at(input.pointer.y - body.y);
            if input.primary_pressed || input.secondary_pressed {
                self.selected = self.hover;
            }
        } else {
            self.hover = None;
        }
    }

    fn body(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.y + HEADER_H,
            bounds.w,
            (bounds.h - HEADER_H).max(0.0),
        )
    }

    fn spec(&self) -> VirtualSpec {
        VirtualSpec {
            row_count: self.visible_total(),
            row_height: self.row_height,
            overscan: 2,
        }
    }

    fn source_index(&self, visible: u64) -> Option<u64> {
        if self.filtered {
            self.matches.get(visible as usize).copied()
        } else if visible < self.row_count {
            Some(visible)
        } else {
            None
        }
    }

    fn row_at(&self, local_y: f32) -> Option<u64> {
        let y = local_y + self.scroll.visual_offset();
        if y < 0.0 {
            return None;
        }
        let index = (y / self.row_height.max(1.0)).floor() as u64;
        if index < self.visible_total() {
            self.source_index(index)
        } else {
            None
        }
    }

    fn rebuild(&mut self) {
        self.matches.clear();
        if self.query.is_empty() && self.kind.is_none() {
            self.filtered = false;
            return;
        }
        self.filtered = true;
        let mut buf = [0u8; 24];
        for index in 0..self.row_count {
            if let Some(kind) = self.kind {
                if kind_of(index) != kind {
                    continue;
                }
            }
            if !self.query.is_empty() {
                let name = row_name(index, &mut buf);
                if !contains_ascii_ignore(name, &self.query) {
                    continue;
                }
            }
            self.matches.push(index);
        }
    }
}

pub fn row_name(index: u64, buf: &mut [u8; 24]) -> &str {
    const PREFIX: &[u8] = b"Document ";
    buf[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut n = index;
    for digit in (0..6).rev() {
        buf[PREFIX.len() + digit] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    match std::str::from_utf8(&buf[..PREFIX.len() + 6]) {
        Ok(text) => text,
        Err(_) => "Document",
    }
}

fn kind_of(index: u64) -> &'static str {
    match index % 3 {
        0 => "Folder",
        1 => "File",
        _ => "Image",
    }
}

fn contains_ascii_ignore(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let hay = hay.as_bytes();
    let needle = needle.as_bytes();
    if needle.len() > hay.len() {
        return false;
    }
    let last = hay.len() - needle.len();
    for start in 0..=last {
        if hay[start..start + needle.len()].eq_ignore_ascii_case(needle) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;
    use crate::input::Input;

    #[test]
    fn one_hundred_thousand_rows_stay_virtualized() {
        let mut table = VirtualTable::new(100_000);
        let bounds = Rect::new(0.0, 0.0, 640.0, 480.0);
        table.update(&Input::default(), bounds, 0.016);
        assert_eq!(table.visible_total(), 100_000);
        assert!(
            table.visible_window(bounds.h) < 40,
            "window {}",
            table.visible_window(bounds.h)
        );
        let input = Input {
            pointer: Point::new(40.0, HEADER_H + 10.0),
            primary_pressed: true,
            ..Default::default()
        };
        table.update(&input, bounds, 0.016);
        assert_eq!(table.selected(), Some(0));
    }

    #[test]
    fn filter_matches_one_document_and_does_not_grow_when_repeated() {
        let mut table = VirtualTable::new(100_000);
        table.set_query("000042");
        assert_eq!(table.visible_total(), 1);
        assert_eq!(table.matches.first().copied(), Some(42));
        let cap = table.matches_capacity();
        table.set_query("000042");
        assert_eq!(table.matches_capacity(), cap);
        table.set_kind(Some("File"));
        assert_eq!(table.visible_total(), 0);
        table.set_kind(Some("Folder"));
        assert_eq!(table.visible_total(), 1);
    }

    #[test]
    fn row_name_is_stack_formatted() {
        let mut buf = [0u8; 24];
        assert_eq!(row_name(42, &mut buf), "Document 000042");
        assert_eq!(row_name(100_000, &mut buf), "Document 100000");
    }
}
