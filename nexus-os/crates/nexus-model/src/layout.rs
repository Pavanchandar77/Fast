//! Pure tiling layout engine for Nexus-OS.
//! Supports Master-Stack, Dwindle (BSP), Monocle, and Scrolling layouts.
//! Deterministic, mathematical, fully testable on Linux, zero OS dependencies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Window identifier abstraction (agnostic to Win32 HWND)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

impl WindowId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

impl From<u64> for WindowId {
    fn from(val: u64) -> Self {
        Self(val)
    }
}

impl From<usize> for WindowId {
    fn from(val: usize) -> Self {
        Self(val as u64)
    }
}

/// 2D Rectangle with integer coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width: if width < 0 { 0 } else { width },
            height: if height < 0 { 0 } else { height },
        }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn area(&self) -> i64 {
        (self.width as i64) * (self.height as i64)
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Checks if this rect completely contains another rect
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// Checks if two rectangles overlap with strictly positive intersection area
    pub fn intersects(&self, other: &Rect) -> bool {
        let x_overlap = self.x < other.right() && self.right() > other.x;
        let y_overlap = self.y < other.bottom() && self.bottom() > other.y;
        x_overlap && y_overlap
    }

    /// Computes intersection rectangle if any
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());

        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, x2 - x1, y2 - y1))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayoutKind {
    MasterStack,
    #[default]
    Dwindle,
    Monocle,
    Scrolling,
    Floating,
}

impl std::str::FromStr for LayoutKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "master_stack" | "master-stack" | "masterstack" | "stack" => Ok(Self::MasterStack),
            "dwindle" | "bsp" => Ok(Self::Dwindle),
            "monocle" | "fullscreen" => Ok(Self::Monocle),
            "scrolling" | "niri" => Ok(Self::Scrolling),
            "floating" => Ok(Self::Floating),
            _ => Err("Unknown layout kind"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutOptions {
    pub inner_gap: i32,
    pub outer_gap: i32,
    pub master_ratio: f32,
    pub top_bar_height: i32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            inner_gap: 8,
            outer_gap: 12,
            master_ratio: 0.55,
            top_bar_height: 28,
        }
    }
}

impl LayoutOptions {
    pub fn sanitize(&self) -> Self {
        Self {
            inner_gap: self.inner_gap.max(0),
            outer_gap: self.outer_gap.max(0),
            master_ratio: if self.master_ratio.is_finite() {
                self.master_ratio.clamp(0.1, 0.9)
            } else {
                0.55
            },
            top_bar_height: self.top_bar_height.max(0),
        }
    }
}

/// Pure Layout Engine
pub struct LayoutEngine;

impl LayoutEngine {
    /// Computes usable area inside the monitor screen bounds, accounting for top bar and outer gap.
    pub fn compute_usable_area(screen: Rect, opts: &LayoutOptions) -> Rect {
        let opts = opts.sanitize();
        let x = screen.x + opts.outer_gap;
        let y = screen.y + opts.top_bar_height + opts.outer_gap;
        let w = (screen.width - (opts.outer_gap * 2)).max(0);
        let h = (screen.height - opts.top_bar_height - (opts.outer_gap * 2)).max(0);

        Rect::new(x, y, w, h)
    }

    /// Computes tile placements for a slice of windows in deterministic order.
    /// Window at index 0 is always the master/primary window.
    pub fn compute_layout(
        kind: LayoutKind,
        screen: Rect,
        windows: &[WindowId],
        opts: &LayoutOptions,
    ) -> HashMap<WindowId, Rect> {
        let mut result = HashMap::new();
        if windows.is_empty() {
            return result;
        }

        let opts = opts.sanitize();
        let usable = Self::compute_usable_area(screen, &opts);
        if usable.is_empty() {
            // Degenerate work area: every window gets that rect. Empty rects do not
            // overlap and stay inside the work area. Never invent geometry outside it.
            for &w in windows {
                result.insert(w, usable);
            }
            return result;
        }

        match kind {
            LayoutKind::Floating => {
                // Floating windows retain their own bounds, no tiling computed
            }
            LayoutKind::Monocle => {
                for &w in windows {
                    result.insert(w, usable);
                }
            }
            LayoutKind::MasterStack => {
                Self::layout_master_stack(&usable, windows, &opts, &mut result);
            }
            LayoutKind::Dwindle => {
                Self::layout_dwindle(&usable, windows, &opts, &mut result);
            }
            LayoutKind::Scrolling => {
                Self::layout_scrolling(&usable, windows, &opts, &mut result);
            }
        }

        result
    }

    /// Full requested gap when every tile can keep at least one pixel, otherwise zero.
    /// A partial gap would fail the "gaps respected" check (distance must be 0 or >= gap).
    fn gap_that_fits(span: i32, count: i32, requested_gap: i32) -> i32 {
        if count <= 1 || span <= 0 {
            return 0;
        }
        let gap = requested_gap.max(0);
        let gaps = count - 1;
        let room = span.saturating_sub(count);
        if gaps > 0 && room / gaps >= gap {
            gap
        } else {
            0
        }
    }

    /// Split `span` into (first, gap_used, second) with first + gap_used + second == span.
    /// Returns None when two positive panes cannot fit.
    fn split_span(span: i32, ratio: f32, requested_gap: i32) -> Option<(i32, i32, i32)> {
        if span < 2 {
            return None;
        }
        let gap_used = Self::gap_that_fits(span, 2, requested_gap);
        let available = span - gap_used;
        let ratio = if ratio.is_finite() {
            ratio.clamp(0.1, 0.9)
        } else {
            0.55
        };
        let mut first = (available as f32 * ratio).round() as i32;
        if first < 1 {
            first = 1;
        }
        if first > available - 1 {
            first = available - 1;
        }
        Some((first, gap_used, available - first))
    }

    fn place_column(
        area: Rect,
        windows: &[WindowId],
        inner_gap: i32,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }
        if n == 1 || area.height <= 0 || area.width <= 0 {
            for &w in windows {
                out.insert(w, area);
            }
            return;
        }
        let n_i = n as i32;
        let gap_used = Self::gap_that_fits(area.height, n_i, inner_gap);
        let slot = area.height.saturating_sub(gap_used.saturating_mul(n_i - 1)) / n_i;
        let mut cursor = area.y;
        for (i, &w) in windows.iter().enumerate() {
            let h = if i + 1 == n {
                area.bottom().saturating_sub(cursor).max(0)
            } else {
                slot
            };
            out.insert(w, Rect::new(area.x, cursor, area.width, h));
            if i + 1 != n {
                cursor = cursor.saturating_add(h).saturating_add(gap_used);
            }
        }
    }

    fn place_row(
        area: Rect,
        windows: &[WindowId],
        inner_gap: i32,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }
        if n == 1 || area.width <= 0 || area.height <= 0 {
            for &w in windows {
                out.insert(w, area);
            }
            return;
        }
        let n_i = n as i32;
        let gap_used = Self::gap_that_fits(area.width, n_i, inner_gap);
        let slot = area.width.saturating_sub(gap_used.saturating_mul(n_i - 1)) / n_i;
        let mut cursor = area.x;
        for (i, &w) in windows.iter().enumerate() {
            let w_px = if i + 1 == n {
                area.right().saturating_sub(cursor).max(0)
            } else {
                slot
            };
            out.insert(w, Rect::new(cursor, area.y, w_px, area.height));
            if i + 1 != n {
                cursor = cursor.saturating_add(w_px).saturating_add(gap_used);
            }
        }
    }

    /// Pack windows into `area` on the axis that can separate them.
    fn pack_tiles(
        area: Rect,
        windows: &[WindowId],
        inner_gap: i32,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        if windows.is_empty() {
            return;
        }
        let n = windows.len() as i32;
        let col_ok = area.height >= n && area.width > 0;
        let row_ok = area.width >= n && area.height > 0;
        if col_ok && (area.height >= area.width || !row_ok) {
            Self::place_column(area, windows, inner_gap, out);
        } else if row_ok {
            Self::place_row(area, windows, inner_gap, out);
        } else {
            for &w in windows {
                out.insert(w, area);
            }
        }
    }

    fn layout_master_stack(
        usable: &Rect,
        windows: &[WindowId],
        opts: &LayoutOptions,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        let count = windows.len();
        if count == 1 {
            out.insert(windows[0], *usable);
            return;
        }

        let inner_gap = opts.inner_gap.max(0);
        // A horizontal master/stack split needs two positive columns. Otherwise
        // pack every window along the axis that can actually separate them.
        let Some((master_w, gap_x, stack_w)) =
            Self::split_span(usable.width, opts.master_ratio, inner_gap)
        else {
            Self::pack_tiles(*usable, windows, inner_gap, out);
            return;
        };

        out.insert(
            windows[0],
            Rect::new(usable.x, usable.y, master_w, usable.height),
        );

        let stack_area = Rect::new(
            usable.x + master_w + gap_x,
            usable.y,
            stack_w,
            usable.height,
        );
        Self::place_column(stack_area, &windows[1..], inner_gap, out);
    }

    fn layout_dwindle(
        usable: &Rect,
        windows: &[WindowId],
        opts: &LayoutOptions,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        let count = windows.len();
        if count == 1 {
            out.insert(windows[0], *usable);
            return;
        }

        let inner_gap = opts.inner_gap.max(0);
        let mut cur_rect = *usable;
        // Horizontal split creates left and right; vertical creates top and bottom.
        let mut split_horizontal = true;

        for (i, &w) in windows.iter().enumerate() {
            if i == count - 1 {
                out.insert(w, cur_rect);
                break;
            }

            let span = if split_horizontal {
                cur_rect.width
            } else {
                cur_rect.height
            };
            let Some((pane, gap_used, rest)) = Self::split_span(span, opts.master_ratio, inner_gap)
            else {
                Self::pack_tiles(cur_rect, &windows[i..], inner_gap, out);
                break;
            };

            if split_horizontal {
                out.insert(w, Rect::new(cur_rect.x, cur_rect.y, pane, cur_rect.height));
                cur_rect = Rect::new(
                    cur_rect.x + pane + gap_used,
                    cur_rect.y,
                    rest,
                    cur_rect.height,
                );
            } else {
                out.insert(w, Rect::new(cur_rect.x, cur_rect.y, cur_rect.width, pane));
                cur_rect = Rect::new(
                    cur_rect.x,
                    cur_rect.y + pane + gap_used,
                    cur_rect.width,
                    rest,
                );
            }

            split_horizontal = !split_horizontal;
        }
    }

    fn layout_scrolling(
        usable: &Rect,
        windows: &[WindowId],
        opts: &LayoutOptions,
        out: &mut HashMap<WindowId, Rect>,
    ) {
        let inner_gap = opts.inner_gap;
        let column_width = ((usable.width as f32 * opts.master_ratio).round() as i32).max(100);

        for (i, &w) in windows.iter().enumerate() {
            let x = usable.x + (i as i32 * (column_width + inner_gap));
            out.insert(w, Rect::new(x, usable.y, column_width, usable.height));
        }
    }
}

/// Validation helpers for layout property checking
pub struct LayoutValidator;

impl LayoutValidator {
    /// Validates that no two rects in the slice overlap (intersection area == 0)
    pub fn verify_no_overlap(rects: &[Rect]) -> bool {
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if rects[i].intersects(&rects[j]) {
                    return false;
                }
            }
        }
        true
    }

    /// Validates that all rects are completely inside the bounding rect
    pub fn verify_within_bounds(bounds: &Rect, rects: &[Rect]) -> bool {
        rects.iter().all(|r| bounds.contains_rect(r))
    }

    /// Validates that directly adjacent tiles either touch or keep the full inner gap.
    /// A tile sitting between two others blocks the pair, so only real neighbors are checked.
    pub fn verify_gaps_respected(inner_gap: i32, rects: &[Rect]) -> bool {
        if inner_gap <= 0 || rects.len() <= 1 {
            return true;
        }

        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if !Self::adjacent_gap_ok(inner_gap, i, j, rects) {
                    return false;
                }
            }
        }
        true
    }

    fn adjacent_gap_ok(inner_gap: i32, i: usize, j: usize, rects: &[Rect]) -> bool {
        let a = &rects[i];
        let b = &rects[j];

        let overlap_top = a.y.max(b.y);
        let overlap_bottom = a.bottom().min(b.bottom());
        if overlap_bottom > overlap_top {
            let (left, right) = if a.right() <= b.x {
                (a, b)
            } else if b.right() <= a.x {
                (b, a)
            } else {
                return true;
            };
            let dx = right.x - left.right();
            if dx > 0 && dx < inner_gap {
                let strip = Rect::new(left.right(), overlap_top, dx, overlap_bottom - overlap_top);
                if !Self::strip_blocked(strip, i, j, rects) {
                    return false;
                }
            }
        }

        let overlap_left = a.x.max(b.x);
        let overlap_right = a.right().min(b.right());
        if overlap_right > overlap_left {
            let (top, bottom) = if a.bottom() <= b.y {
                (a, b)
            } else if b.bottom() <= a.y {
                (b, a)
            } else {
                return true;
            };
            let dy = bottom.y - top.bottom();
            if dy > 0 && dy < inner_gap {
                let strip = Rect::new(overlap_left, top.bottom(), overlap_right - overlap_left, dy);
                if !Self::strip_blocked(strip, i, j, rects) {
                    return false;
                }
            }
        }
        true
    }

    fn strip_blocked(strip: Rect, i: usize, j: usize, rects: &[Rect]) -> bool {
        rects
            .iter()
            .enumerate()
            .any(|(k, rect)| k != i && k != j && rect.intersects(&strip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_single_window_takes_full_usable_area() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows = vec![WindowId::new(1)];

        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[&WindowId::new(1)], usable);
    }

    #[test]
    fn test_master_stack_two_windows() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions {
            inner_gap: 10,
            outer_gap: 20,
            master_ratio: 0.5,
            top_bar_height: 30,
        };
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows = vec![WindowId::new(1), WindowId::new(2)];

        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
        assert_eq!(layout.len(), 2);

        let w1 = layout[&WindowId::new(1)];
        let w2 = layout[&WindowId::new(2)];

        assert!(usable.contains_rect(&w1));
        assert!(usable.contains_rect(&w2));
        assert!(!w1.intersects(&w2));
        assert_eq!(w2.x - w1.right(), opts.inner_gap);
    }

    #[test]
    fn test_dwindle_three_windows() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows = vec![WindowId::new(1), WindowId::new(2), WindowId::new(3)];

        let layout = LayoutEngine::compute_layout(LayoutKind::Dwindle, screen, &windows, &opts);
        assert_eq!(layout.len(), 3);

        let rects: Vec<Rect> = layout.values().copied().collect();
        assert!(LayoutValidator::verify_within_bounds(&usable, &rects));
        assert!(LayoutValidator::verify_no_overlap(&rects));
    }

    #[test]
    fn test_monocle_all_full_screen() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows = vec![WindowId::new(1), WindowId::new(2), WindowId::new(3)];

        let layout = LayoutEngine::compute_layout(LayoutKind::Monocle, screen, &windows, &opts);
        for &w in &windows {
            assert_eq!(layout[&w], usable);
        }
    }

    #[test]
    fn test_rect_intersection() {
        let r1 = Rect::new(0, 0, 100, 100);
        let r2 = Rect::new(50, 50, 100, 100);
        let r3 = Rect::new(200, 200, 50, 50);

        assert!(r1.intersects(&r2));
        assert!(!r1.intersects(&r3));
        let inter = r1.intersection(&r2).unwrap();
        assert_eq!(inter, Rect::new(50, 50, 50, 50));
    }

    #[test]
    fn test_empty_windows() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &[], &opts);
        assert!(layout.is_empty());
    }

    #[test]
    fn test_layout_kind_from_str() {
        assert_eq!(
            "dwindle".parse::<LayoutKind>().unwrap(),
            LayoutKind::Dwindle
        );
        assert_eq!("bsp".parse::<LayoutKind>().unwrap(), LayoutKind::Dwindle);
        assert_eq!(
            "master_stack".parse::<LayoutKind>().unwrap(),
            LayoutKind::MasterStack
        );
        assert_eq!(
            "monocle".parse::<LayoutKind>().unwrap(),
            LayoutKind::Monocle
        );
        assert_eq!(
            "scrolling".parse::<LayoutKind>().unwrap(),
            LayoutKind::Scrolling
        );
        assert_eq!(
            "floating".parse::<LayoutKind>().unwrap(),
            LayoutKind::Floating
        );
        assert!("invalid_xyz".parse::<LayoutKind>().is_err());
    }

    #[test]
    fn test_layout_options_sanitize() {
        let bad = LayoutOptions {
            inner_gap: -10,
            outer_gap: -20,
            master_ratio: 1.5,
            top_bar_height: -5,
        };
        let sanitized = bad.sanitize();
        assert_eq!(sanitized.inner_gap, 0);
        assert_eq!(sanitized.outer_gap, 0);
        assert_eq!(sanitized.master_ratio, 0.9);
        assert_eq!(sanitized.top_bar_height, 0);

        let nan = LayoutOptions {
            inner_gap: 4,
            outer_gap: 4,
            master_ratio: f32::NAN,
            top_bar_height: 8,
        };
        assert_eq!(nan.sanitize().master_ratio, 0.55);
    }

    #[test]
    fn test_master_stack_tight_work_area_does_not_panic() {
        // Old master width used clamp(20, width - gap - 20), which panics when
        // the upper bound drops below 20. 80px screen, 20px outer gap → 40px usable.
        let screen = Rect::new(0, 0, 80, 240);
        let opts = LayoutOptions {
            inner_gap: 10,
            outer_gap: 20,
            master_ratio: 0.55,
            top_bar_height: 28,
        };
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows: Vec<WindowId> = (1..=4).map(WindowId::new).collect();
        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
        assert_eq!(layout.len(), windows.len());
        let rects: Vec<Rect> = windows.iter().map(|w| layout[w]).collect();
        assert!(LayoutValidator::verify_within_bounds(&usable, &rects));
        assert!(LayoutValidator::verify_no_overlap(&rects));
        assert!(LayoutValidator::verify_gaps_respected(
            opts.inner_gap,
            &rects
        ));
    }

    #[test]
    fn test_dwindle_narrow_area_stays_inside() {
        let screen = Rect::new(10, 10, 160, 120);
        let opts = LayoutOptions {
            inner_gap: 16,
            outer_gap: 8,
            master_ratio: 0.62,
            top_bar_height: 12,
        };
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        let windows: Vec<WindowId> = (1..=6).map(WindowId::new).collect();
        let layout = LayoutEngine::compute_layout(LayoutKind::Dwindle, screen, &windows, &opts);
        assert_eq!(layout.len(), windows.len());
        let rects: Vec<Rect> = windows.iter().map(|w| layout[w]).collect();
        assert!(LayoutValidator::verify_within_bounds(&usable, &rects));
        assert!(LayoutValidator::verify_no_overlap(&rects));
        assert!(LayoutValidator::verify_gaps_respected(
            opts.inner_gap,
            &rects
        ));
    }

    #[test]
    fn test_empty_usable_area_does_not_escape_or_panic() {
        let screen = Rect::new(0, 0, 40, 40);
        let opts = LayoutOptions {
            inner_gap: 4,
            outer_gap: 30,
            master_ratio: 0.5,
            top_bar_height: 20,
        };
        let usable = LayoutEngine::compute_usable_area(screen, &opts);
        assert!(usable.is_empty());
        let windows = vec![WindowId::new(1), WindowId::new(2)];
        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
        let rects: Vec<Rect> = layout.values().copied().collect();
        assert!(LayoutValidator::verify_within_bounds(&usable, &rects));
        assert!(LayoutValidator::verify_no_overlap(&rects));
    }

    #[test]
    fn test_master_stack_three_windows() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let windows = vec![WindowId::new(1), WindowId::new(2), WindowId::new(3)];
        let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
        assert_eq!(layout.len(), 3);
        let rects: Vec<Rect> = layout.values().copied().collect();
        assert!(LayoutValidator::verify_no_overlap(&rects));
    }

    #[test]
    fn test_dwindle_four_windows() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let windows = vec![
            WindowId::new(1),
            WindowId::new(2),
            WindowId::new(3),
            WindowId::new(4),
        ];
        let layout = LayoutEngine::compute_layout(LayoutKind::Dwindle, screen, &windows, &opts);
        assert_eq!(layout.len(), 4);
        let rects: Vec<Rect> = layout.values().copied().collect();
        assert!(LayoutValidator::verify_no_overlap(&rects));
    }

    #[test]
    fn test_scrolling_layout() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let opts = LayoutOptions::default();
        let windows = vec![WindowId::new(1), WindowId::new(2), WindowId::new(3)];
        let layout = LayoutEngine::compute_layout(LayoutKind::Scrolling, screen, &windows, &opts);
        assert_eq!(layout.len(), 3);
        let w1 = layout[&WindowId::new(1)];
        let w2 = layout[&WindowId::new(2)];
        assert_eq!(w2.x - w1.right(), opts.inner_gap);
    }

    #[test]
    fn test_rect_helpers() {
        let r = Rect::new(10, 20, 100, 200);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 220);
        assert_eq!(r.area(), 20000);
        assert!(!r.is_empty());

        let empty = Rect::new(0, 0, 0, 10);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_layout_validator_overlap_and_bounds() {
        let bounds = Rect::new(0, 0, 1000, 1000);
        let r1 = Rect::new(10, 10, 100, 100);
        let r2 = Rect::new(50, 50, 100, 100);
        assert!(!LayoutValidator::verify_no_overlap(&[r1, r2]));

        let r_out = Rect::new(950, 950, 100, 100);
        assert!(!LayoutValidator::verify_within_bounds(&bounds, &[r_out]));

        let touching = [Rect::new(0, 0, 10, 10), Rect::new(10, 0, 10, 10)];
        assert!(LayoutValidator::verify_gaps_respected(8, &touching));
        let short_gap = [Rect::new(0, 0, 10, 10), Rect::new(12, 0, 10, 10)];
        assert!(!LayoutValidator::verify_gaps_respected(8, &short_gap));
        let full_gap = [Rect::new(0, 0, 10, 10), Rect::new(18, 0, 10, 10)];
        assert!(LayoutValidator::verify_gaps_respected(8, &full_gap));
    }

    #[test]
    fn test_window_id_conversions() {
        let w1 = WindowId::from(42u64);
        assert_eq!(w1.as_u64(), 42);
        let w2 = WindowId::from(77usize);
        assert_eq!(w2.as_u64(), 77);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn proptest_master_stack_no_overlap_and_inside_bounds(
            screen_w in 64i32..3840i32,
            screen_h in 64i32..2160i32,
            inner_gap in 0i32..48i32,
            outer_gap in 0i32..40i32,
            top_bar in 0i32..64i32,
            master_ratio in 0.1f32..0.9f32,
            win_count in 1usize..12usize,
        ) {
            let screen = Rect::new(0, 0, screen_w, screen_h);
            let opts = LayoutOptions {
                inner_gap,
                outer_gap,
                master_ratio,
                top_bar_height: top_bar,
            };
            let usable = LayoutEngine::compute_usable_area(screen, &opts);
            let windows: Vec<WindowId> = (1..=win_count).map(|i| WindowId::new(i as u64)).collect();

            let layout = LayoutEngine::compute_layout(LayoutKind::MasterStack, screen, &windows, &opts);
            prop_assert_eq!(layout.len(), win_count);

            let rects: Vec<Rect> = windows.iter().map(|w| layout[w]).collect();

            // 1. All rects must be inside the work area
            prop_assert!(
                LayoutValidator::verify_within_bounds(&usable, &rects),
                "Rects must be within usable bounds: usable={:?}, rects={:?}",
                usable,
                rects
            );

            // 2. No overlaps between distinct tiles
            prop_assert!(
                LayoutValidator::verify_no_overlap(&rects),
                "Tiles must not overlap: {:?}",
                rects
            );

            // 3. Gaps respected
            prop_assert!(
                LayoutValidator::verify_gaps_respected(inner_gap, &rects),
                "Inner gaps must be respected: inner_gap={}, rects={:?}",
                inner_gap,
                rects
            );
        }

        #[test]
        fn proptest_dwindle_no_overlap_and_inside_bounds(
            screen_w in 120i32..3840i32,
            screen_h in 120i32..2160i32,
            inner_gap in 0i32..32i32,
            outer_gap in 0i32..24i32,
            top_bar in 0i32..40i32,
            master_ratio in 0.2f32..0.8f32,
            win_count in 1usize..10usize,
        ) {
            let screen = Rect::new(0, 0, screen_w, screen_h);
            let opts = LayoutOptions {
                inner_gap,
                outer_gap,
                master_ratio,
                top_bar_height: top_bar,
            };
            let usable = LayoutEngine::compute_usable_area(screen, &opts);
            let windows: Vec<WindowId> = (1..=win_count).map(|i| WindowId::new(i as u64)).collect();

            let layout = LayoutEngine::compute_layout(LayoutKind::Dwindle, screen, &windows, &opts);
            prop_assert_eq!(layout.len(), win_count);

            let rects: Vec<Rect> = windows.iter().map(|w| layout[w]).collect();

            // 1. All rects inside usable area
            prop_assert!(
                LayoutValidator::verify_within_bounds(&usable, &rects),
                "Dwindle rects must be inside usable bounds: usable={:?}, rects={:?}",
                usable,
                rects
            );

            // 2. No overlaps
            prop_assert!(
                LayoutValidator::verify_no_overlap(&rects),
                "Dwindle tiles must not overlap: {:?}",
                rects
            );

            // 3. Gaps respected
            prop_assert!(
                LayoutValidator::verify_gaps_respected(inner_gap, &rects),
                "Dwindle inner gaps must be respected: inner_gap={}, rects={:?}",
                inner_gap,
                rects
            );
        }
    }
}
