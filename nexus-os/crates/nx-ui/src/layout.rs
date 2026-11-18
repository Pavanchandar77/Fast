//! Flex layout and virtualized row geometry.
//!
//! A 100k-row list never materializes 100k boxes. Fixed-height windows are
//! O(1). Variable heights use a prefix sum built when the data changes, then
//! a binary search per query.

use crate::geom::{Insets, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexItem {
    pub main: f32,
    pub cross: f32,
    pub flex: f32,
    pub min_main: f32,
    pub max_main: f32,
}

impl FlexItem {
    pub fn fixed(main: f32, cross: f32) -> Self {
        Self {
            main,
            cross,
            flex: 0.0,
            min_main: 0.0,
            max_main: f32::MAX,
        }
    }

    pub fn flex(flex: f32, cross: f32) -> Self {
        Self {
            main: 0.0,
            cross,
            flex,
            min_main: 0.0,
            max_main: f32::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexStyle {
    pub direction: Axis,
    pub gap: f32,
    pub padding: Insets,
    pub align: Align,
    pub justify: Justify,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            direction: Axis::Horizontal,
            gap: 0.0,
            padding: Insets::default(),
            align: Align::Start,
            justify: Justify::Start,
        }
    }
}

/// Lay out `items` into `out`. Returns false when the slices differ in length.
/// No allocation. Flex items share free space; a max clamp does not redistribute.
pub fn layout_flex(bounds: Rect, style: &FlexStyle, items: &[FlexItem], out: &mut [Rect]) -> bool {
    if items.len() != out.len() {
        return false;
    }
    let n = items.len();
    if n == 0 {
        return true;
    }
    let inner = bounds.inset(style.padding);
    let horizontal = matches!(style.direction, Axis::Horizontal);
    let main_limit = if horizontal { inner.w } else { inner.h };
    let cross_limit = if horizontal { inner.h } else { inner.w };
    let gaps = if n > 1 {
        style.gap * (n as f32 - 1.0)
    } else {
        0.0
    };
    let mut base_sum = 0.0;
    let mut flex_sum = 0.0;
    for item in items {
        base_sum += base_main(*item);
        flex_sum += item.flex.max(0.0);
    }
    let free = main_limit - gaps - base_sum;
    let extra = if flex_sum > 0.0 && free > 0.0 {
        free / flex_sum
    } else {
        0.0
    };
    let mut used = 0.0;
    for item in items {
        used += resolved_main(*item, extra);
    }
    let content = used + gaps;
    let (mut cursor, gap) = justify_start(style.justify, main_limit, content, gaps, n);
    for (item, slot) in items.iter().zip(out.iter_mut()) {
        let main = resolved_main(*item, extra);
        let cross = match style.align {
            Align::Stretch => cross_limit,
            _ => item.cross.min(cross_limit),
        };
        let cross_pos = match style.align {
            Align::Center => (cross_limit - cross) * 0.5,
            Align::End => cross_limit - cross,
            Align::Start | Align::Stretch => 0.0,
        };
        if horizontal {
            *slot = Rect::new(inner.x + cursor, inner.y + cross_pos, main, cross);
        } else {
            *slot = Rect::new(inner.x + cross_pos, inner.y + cursor, cross, main);
        }
        cursor += main + gap;
    }
    true
}

fn base_main(item: FlexItem) -> f32 {
    if item.flex > 0.0 {
        item.min_main.max(0.0)
    } else {
        item.main
            .clamp(item.min_main.max(0.0), item.max_main.max(item.min_main))
    }
}

fn resolved_main(item: FlexItem, extra_per_flex: f32) -> f32 {
    if item.flex > 0.0 {
        (item.min_main.max(0.0) + extra_per_flex * item.flex.max(0.0))
            .clamp(item.min_main.max(0.0), item.max_main.max(item.min_main))
    } else {
        base_main(item)
    }
}

fn justify_start(justify: Justify, limit: f32, content: f32, gaps: f32, n: usize) -> (f32, f32) {
    let slack = (limit - content).max(0.0);
    let base_gap = if n > 1 { gaps / (n as f32 - 1.0) } else { 0.0 };
    match justify {
        Justify::Start => (0.0, base_gap),
        Justify::Center => (slack * 0.5, base_gap),
        Justify::End => (slack, base_gap),
        Justify::SpaceBetween => {
            if n > 1 {
                (0.0, base_gap + slack / (n as f32 - 1.0))
            } else {
                (0.0, 0.0)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualSpec {
    pub row_count: u64,
    pub row_height: f32,
    pub overscan: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualWindow {
    pub first: u64,
    pub count: u32,
    pub offset_y: f32,
    pub content_height: f32,
    pub max_scroll: f32,
    pub scroll: f32,
}

impl VirtualWindow {
    pub fn contains_index(self, index: u64) -> bool {
        index >= self.first && index < self.first + u64::from(self.count)
    }

    pub fn row_rect(self, viewport: Rect, index: u64, row_height: f32) -> Rect {
        let y = viewport.y + self.offset_y + (index - self.first) as f32 * row_height;
        Rect::new(viewport.x, y, viewport.w, row_height)
    }
}

/// Fixed-height virtual window. O(1), no allocation.
pub fn virtual_window(spec: &VirtualSpec, scroll: f32, viewport_h: f32) -> VirtualWindow {
    let row_height = spec.row_height.max(1.0);
    let content = spec.row_count as f32 * row_height;
    let max_scroll = (content - viewport_h.max(0.0)).max(0.0);
    let scroll = if scroll.is_finite() {
        scroll.clamp(0.0, max_scroll)
    } else {
        0.0
    };
    if spec.row_count == 0 || viewport_h <= 0.0 {
        return VirtualWindow {
            first: 0,
            count: 0,
            offset_y: 0.0,
            content_height: content,
            max_scroll,
            scroll,
        };
    }
    let first_visible = ((scroll / row_height).floor() as u64).min(spec.row_count - 1);
    let overscan = u64::from(spec.overscan);
    let first = first_visible.saturating_sub(overscan);
    let frac = scroll - first_visible as f32 * row_height;
    let cover = ((viewport_h + frac) / row_height).ceil().max(1.0) as u64;
    let last = (first_visible + cover + overscan).min(spec.row_count);
    let count = (last - first) as u32;
    let offset_y = first as f32 * row_height - scroll;
    VirtualWindow {
        first,
        count,
        offset_y,
        content_height: content,
        max_scroll,
        scroll,
    }
}

pub fn row_index_at(spec: &VirtualSpec, scroll: f32, viewport_h: f32, local_y: f32) -> Option<u64> {
    if spec.row_count == 0 || local_y < 0.0 || local_y >= viewport_h {
        return None;
    }
    let window = virtual_window(spec, scroll, viewport_h);
    let index = ((window.scroll + local_y) / spec.row_height.max(1.0)).floor() as u64;
    if index < spec.row_count {
        Some(index)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColWidth {
    Fixed(f32),
    Flex(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub title: &'static str,
    pub width: ColWidth,
    pub align: Align,
}

pub fn layout_columns(width: f32, gap: f32, cols: &[Column], out: &mut [f32]) -> bool {
    if cols.len() != out.len() {
        return false;
    }
    if cols.is_empty() {
        return true;
    }
    let gaps = if cols.len() > 1 {
        gap * (cols.len() as f32 - 1.0)
    } else {
        0.0
    };
    let mut fixed = 0.0;
    let mut flex_sum = 0.0;
    for col in cols {
        match col.width {
            ColWidth::Fixed(w) => fixed += w.max(0.0),
            ColWidth::Flex(f) => flex_sum += f.max(0.0),
        }
    }
    let free = (width - gaps - fixed).max(0.0);
    for (col, slot) in cols.iter().zip(out.iter_mut()) {
        *slot = match col.width {
            ColWidth::Fixed(w) => w.max(0.0),
            ColWidth::Flex(f) => {
                if flex_sum > 0.0 {
                    free * f.max(0.0) / flex_sum
                } else {
                    0.0
                }
            }
        };
    }
    true
}

/// Exclusive prefix sums for variable row heights. Index 0 is 0.
#[derive(Clone, Debug)]
pub struct PrefixHeights {
    prefix: Vec<f32>,
}

impl PrefixHeights {
    pub fn from_heights(heights: &[f32]) -> Self {
        let mut prefix = Vec::with_capacity(heights.len() + 1);
        prefix.push(0.0);
        let mut acc = 0.0;
        for height in heights {
            acc += height.max(0.0);
            prefix.push(acc);
        }
        Self { prefix }
    }

    pub fn len(&self) -> u64 {
        self.prefix.len().saturating_sub(1) as u64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total(&self) -> f32 {
        self.prefix.last().copied().unwrap_or(0.0)
    }

    pub fn offset(&self, index: u64) -> f32 {
        match self.prefix.get(index as usize) {
            Some(value) => *value,
            None => self.total(),
        }
    }

    /// Returns `(first, count)` covering `viewport` plus `overscan` rows.
    pub fn window(&self, scroll: f32, viewport: f32, overscan: u32) -> (u64, u32) {
        let n = self.len();
        if n == 0 || viewport <= 0.0 {
            return (0, 0);
        }
        let max_scroll = (self.total() - viewport).max(0.0);
        let scroll = scroll.clamp(0.0, max_scroll);
        let first_visible = self.find_row(scroll).min(n - 1);
        let overscan = u64::from(overscan);
        let first = first_visible.saturating_sub(overscan);
        let end_y = scroll + viewport;
        let mut last = first_visible;
        while last + 1 < n && self.offset(last + 1) < end_y {
            last += 1;
        }
        last = (last + 1 + overscan).min(n);
        (first, (last - first) as u32)
    }

    fn find_row(&self, scroll: f32) -> u64 {
        let n = self.prefix.len().saturating_sub(1);
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.prefix[mid + 1] <= scroll {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo as u64
    }
}

/// Shell frame used by the gallery: sidebar | (toolbar, body).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellFrame {
    pub sidebar: Rect,
    pub toolbar: Rect,
    pub body: Rect,
}

pub fn layout_shell(viewport: Size, sidebar_w: f32, toolbar_h: f32) -> ShellFrame {
    let sidebar_w = sidebar_w.clamp(0.0, viewport.w);
    let toolbar_h = toolbar_h.clamp(0.0, viewport.h);
    ShellFrame {
        sidebar: Rect::new(0.0, 0.0, sidebar_w, viewport.h),
        toolbar: Rect::new(sidebar_w, 0.0, viewport.w - sidebar_w, toolbar_h),
        body: Rect::new(
            sidebar_w,
            toolbar_h,
            viewport.w - sidebar_w,
            viewport.h - toolbar_h,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_row_respects_gap_and_padding() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
        let style = FlexStyle {
            direction: Axis::Horizontal,
            gap: 8.0,
            padding: Insets::all(4.0),
            align: Align::Start,
            justify: Justify::Start,
        };
        let items = [FlexItem::fixed(20.0, 10.0), FlexItem::fixed(30.0, 10.0)];
        let mut out = [Rect::default(); 2];
        assert!(layout_flex(bounds, &style, &items, &mut out));
        assert_eq!(out[0], Rect::new(4.0, 4.0, 20.0, 10.0));
        assert_eq!(out[1].x, 32.0);
        assert_eq!(out[1].w, 30.0);
    }

    #[test]
    fn flex_column_stacks() {
        let bounds = Rect::new(10.0, 10.0, 80.0, 100.0);
        let style = FlexStyle {
            direction: Axis::Vertical,
            gap: 4.0,
            ..FlexStyle::default()
        };
        let items = [FlexItem::fixed(20.0, 40.0), FlexItem::fixed(20.0, 50.0)];
        let mut out = [Rect::default(); 2];
        assert!(layout_flex(bounds, &style, &items, &mut out));
        assert_eq!(out[0].y, 10.0);
        assert_eq!(out[1].y, 34.0);
        assert_eq!(out[0].x, 10.0);
    }

    #[test]
    fn flex_grow_fills_free_space() {
        let bounds = Rect::new(0.0, 0.0, 300.0, 40.0);
        let style = FlexStyle::default();
        let items = [FlexItem::fixed(100.0, 40.0), FlexItem::flex(1.0, 40.0)];
        let mut out = [Rect::default(); 2];
        assert!(layout_flex(bounds, &style, &items, &mut out));
        assert_eq!(out[0].w, 100.0);
        assert_eq!(out[1].x, 100.0);
        assert_eq!(out[1].w, 200.0);
    }

    #[test]
    fn align_center_on_the_cross_axis() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
        let style = FlexStyle {
            align: Align::Center,
            ..FlexStyle::default()
        };
        let items = [FlexItem::fixed(10.0, 10.0)];
        let mut out = [Rect::default(); 1];
        assert!(layout_flex(bounds, &style, &items, &mut out));
        assert!((out[0].y - 15.0).abs() < 1e-4);
    }

    #[test]
    fn virtual_window_of_100k_rows_is_small_and_in_range() {
        let spec = VirtualSpec {
            row_count: 100_000,
            row_height: 28.0,
            overscan: 2,
        };
        let window = virtual_window(&spec, 0.0, 800.0);
        assert!(window.count < 50, "count {}", window.count);
        assert_eq!(window.first, 0);
        assert!(window.contains_index(0));
        assert!(!window.contains_index(100_000));
        let scrolled = virtual_window(&spec, 28.0 * 500.0, 800.0);
        assert!(scrolled.first <= 500);
        assert!(scrolled.first + u64::from(scrolled.count) > 500);
        assert!(scrolled.first + u64::from(scrolled.count) <= 100_000);
    }

    #[test]
    fn virtual_rows_do_not_overlap_and_hit_testing_matches() {
        let spec = VirtualSpec {
            row_count: 100_000,
            row_height: 28.0,
            overscan: 0,
        };
        let viewport = Rect::new(0.0, 100.0, 400.0, 280.0);
        let window = virtual_window(&spec, 14.0, viewport.h);
        assert_eq!(window.count, 11);
        let mut previous = window.row_rect(viewport, window.first, spec.row_height);
        for index in (window.first + 1)..(window.first + u64::from(window.count)) {
            let rect = window.row_rect(viewport, index, spec.row_height);
            assert!((rect.y - previous.bottom()).abs() < 1e-3);
            assert!(
                !rect.intersects(Rect::new(
                    previous.x,
                    previous.y,
                    previous.w,
                    previous.h - 0.1
                )) || rect.y >= previous.bottom() - 0.1
            );
            previous = rect;
        }
        let hit = row_index_at(&spec, 14.0, viewport.h, 0.0);
        assert_eq!(hit, Some(0));
        let hit_next = row_index_at(&spec, 14.0, viewport.h, 20.0);
        assert_eq!(hit_next, Some(1));
        assert_eq!(row_index_at(&spec, 0.0, viewport.h, viewport.h), None);
    }

    #[test]
    fn scroll_clamps_to_the_content() {
        let spec = VirtualSpec {
            row_count: 10,
            row_height: 20.0,
            overscan: 0,
        };
        let window = virtual_window(&spec, 10_000.0, 50.0);
        assert!((window.scroll - 150.0).abs() < 1e-3);
        assert!(window.max_scroll <= 150.0 + 1e-3);
        let empty = virtual_window(
            &VirtualSpec {
                row_count: 0,
                row_height: 20.0,
                overscan: 0,
            },
            10.0,
            50.0,
        );
        assert_eq!(empty.count, 0);
    }

    #[test]
    fn columns_split_fixed_and_flex() {
        let cols = [
            Column {
                title: "Name",
                width: ColWidth::Fixed(100.0),
                align: Align::Start,
            },
            Column {
                title: "Kind",
                width: ColWidth::Flex(1.0),
                align: Align::Start,
            },
        ];
        let mut widths = [0.0; 2];
        assert!(layout_columns(300.0, 0.0, &cols, &mut widths));
        assert_eq!(widths, [100.0, 200.0]);
    }

    #[test]
    fn variable_heights_binary_search() {
        let heights = [10.0, 10.0, 10.0, 10.0, 10.0];
        let prefix = PrefixHeights::from_heights(&heights);
        assert!((prefix.total() - 50.0).abs() < 1e-4);
        assert!((prefix.offset(2) - 20.0).abs() < 1e-4);
        let (first, count) = prefix.window(25.0, 20.0, 0);
        assert_eq!(first, 2);
        assert!((2..=3).contains(&count));
    }

    #[test]
    fn shell_frame_splits_sidebar_and_toolbar() {
        let frame = layout_shell(Size::new(1000.0, 700.0), 220.0, 52.0);
        assert_eq!(frame.sidebar.w, 220.0);
        assert_eq!(frame.toolbar.x, 220.0);
        assert_eq!(frame.toolbar.h, 52.0);
        assert_eq!(frame.body.y, 52.0);
        assert!((frame.body.w - 780.0).abs() < 1e-3);
        assert!((frame.sidebar.h - 700.0).abs() < 1e-3);
    }
}
