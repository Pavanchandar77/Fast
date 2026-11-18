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
            master_ratio: self.master_ratio.clamp(0.1, 0.9),
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
            for &w in windows {
                result.insert(w, Rect::new(screen.x, screen.y, 10, 10));
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

        let inner_gap = opts.inner_gap;
        let master_ratio = opts.master_ratio;

        // If usable width is very small, fall back to simple stack
        if usable.width <= inner_gap * 2 {
            for &w in windows {
                out.insert(w, *usable);
            }
            return;
        }

        let master_w = (((usable.width as f32 * master_ratio) - (inner_gap as f32 / 2.0)).round()
            as i32)
            .clamp(20, usable.width - inner_gap - 20);
        let stack_w = usable.width - master_w - inner_gap;

        // Master pane
        out.insert(
            windows[0],
            Rect::new(usable.x, usable.y, master_w, usable.height),
        );

        let stack_count = count - 1;
        let total_stack_gaps = inner_gap * (stack_count as i32 - 1);
        let available_stack_h = (usable.height - total_stack_gaps).max(stack_count as i32);
        let slot_h = available_stack_h / stack_count as i32;
        let stack_x = usable.x + master_w + inner_gap;

        for (i, &w) in windows.iter().skip(1).enumerate() {
            let stack_y = usable.y + (i as i32 * (slot_h + inner_gap));
            let h = if i == stack_count - 1 {
                // Absorb integer rounding differences into final tile
                (usable.bottom() - stack_y).max(1)
            } else {
                slot_h.max(1)
            };

            out.insert(w, Rect::new(stack_x, stack_y, stack_w.max(1), h));
        }
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

        let inner_gap = opts.inner_gap;
        let mut cur_rect = *usable;
        let mut split_horizontal = true; // Horizontal split creates left & right; vertical creates top & bottom

        for (i, &w) in windows.iter().enumerate() {
            if i == count - 1 {
                // Last window takes the entire remaining rectangle
                out.insert(w, cur_rect);
                break;
            }

            if split_horizontal {
                let available_w = (cur_rect.width - inner_gap).max(2);
                let half_w = (available_w as f32 * opts.master_ratio).round() as i32;
                let w1 = half_w.clamp(1, available_w - 1);
                let w2 = cur_rect.width - w1 - inner_gap;

                out.insert(w, Rect::new(cur_rect.x, cur_rect.y, w1, cur_rect.height));

                cur_rect = Rect::new(
                    cur_rect.x + w1 + inner_gap,
                    cur_rect.y,
                    w2.max(1),
                    cur_rect.height,
                );
            } else {
                let available_h = (cur_rect.height - inner_gap).max(2);
                let half_h = (available_h as f32 * opts.master_ratio).round() as i32;
                let h1 = half_h.clamp(1, available_h - 1);
                let h2 = cur_rect.height - h1 - inner_gap;

                out.insert(w, Rect::new(cur_rect.x, cur_rect.y, cur_rect.width, h1));

                cur_rect = Rect::new(
                    cur_rect.x,
                    cur_rect.y + h1 + inner_gap,
                    cur_rect.width,
                    h2.max(1),
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

    /// Validates that pairwise horizontal/vertical distance between distinct non-overlapping
    /// tiles respects the minimum inner gap when adjacent.
    pub fn verify_gaps_respected(inner_gap: i32, rects: &[Rect]) -> bool {
        if inner_gap <= 0 || rects.len() <= 1 {
            return true;
        }

        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = &rects[i];
                let b = &rects[j];

                // If they share horizontal span
                let y_overlap = a.y < b.bottom() && a.bottom() > b.y;
                if y_overlap {
                    let dx = if a.right() <= b.x {
                        b.x - a.right()
                    } else if b.right() <= a.x {
                        a.x - b.right()
                    } else {
                        0
                    };
                    if dx < inner_gap && dx != 0 {
                        return false;
                    }
                }

                // If they share vertical span
                let x_overlap = a.x < b.right() && a.right() > b.x;
                if x_overlap {
                    let dy = if a.bottom() <= b.y {
                        b.y - a.bottom()
                    } else if b.bottom() <= a.y {
                        a.y - b.bottom()
                    } else {
                        0
                    };
                    if dy < inner_gap && dy != 0 {
                        return false;
                    }
                }
            }
        }
        true
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
            screen_w in 800i32..3840i32,
            screen_h in 600i32..2160i32,
            inner_gap in 0i32..30i32,
            outer_gap in 0i32..30i32,
            master_ratio in 0.2f32..0.8f32,
            win_count in 1usize..12usize,
        ) {
            let screen = Rect::new(0, 0, screen_w, screen_h);
            let opts = LayoutOptions {
                inner_gap,
                outer_gap,
                master_ratio,
                top_bar_height: 28,
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
            screen_w in 800i32..3840i32,
            screen_h in 600i32..2160i32,
            inner_gap in 0i32..20i32,
            outer_gap in 0i32..20i32,
            master_ratio in 0.3f32..0.7f32,
            win_count in 1usize..10usize,
        ) {
            let screen = Rect::new(0, 0, screen_w, screen_h);
            let opts = LayoutOptions {
                inner_gap,
                outer_gap,
                master_ratio,
                top_bar_height: 28,
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
