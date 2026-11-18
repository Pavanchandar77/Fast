use crate::geometry::Rect;
use crate::types::WindowId;
use serde::{Deserialize, Serialize};

/// Tiling layout algorithms supported by Nexus-OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LayoutKind {
    #[default]
    Dwindle,
    MasterStack,
    Scrolling,
    Monocle,
    Floating,
}

impl std::str::FromStr for LayoutKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "dwindle" | "bsp" => Ok(Self::Dwindle),
            "master_stack" | "master-stack" | "stack" => Ok(Self::MasterStack),
            "scrolling" | "niri" | "scroll" => Ok(Self::Scrolling),
            "monocle" | "fullscreen" => Ok(Self::Monocle),
            "floating" | "float" => Ok(Self::Floating),
            other => Err(format!("Unknown layout kind: '{other}'")),
        }
    }
}

/// Parameters for fine-tuning layout calculations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutParams {
    pub inner_gap: u32,
    pub outer_gap: u32,
    pub master_ratio: f32,
    pub master_count: usize,
    pub scroll_column_width: u32,
    pub scroll_active_index: usize,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            inner_gap: 8,
            outer_gap: 12,
            master_ratio: 0.55,
            master_count: 1,
            scroll_column_width: 0, // 0 indicates automatic (e.g. 55% of work area)
            scroll_active_index: 0,
        }
    }
}

/// Computes window rectangles as a pure function of:
/// (layout_kind, windows, monitor_work_area, layout_params) -> Vec<(WindowId, Rect)>
pub fn compute_layout(
    kind: LayoutKind,
    windows: &[WindowId],
    work_area: Rect,
    params: &LayoutParams,
) -> Vec<(WindowId, Rect)> {
    if windows.is_empty() {
        return Vec::new();
    }

    let usable = work_area.shrink(params.outer_gap);
    if usable.is_empty() {
        return windows
            .iter()
            .map(|&w| (w, Rect::new(work_area.x, work_area.y, 1, 1)))
            .collect();
    }

    if windows.len() == 1 || kind == LayoutKind::Monocle {
        return windows.iter().map(|&w| (w, usable)).collect();
    }

    match kind {
        LayoutKind::Dwindle => compute_dwindle(windows, usable, params),
        LayoutKind::MasterStack => compute_master_stack(windows, usable, params),
        LayoutKind::Scrolling => compute_scrolling(windows, usable, params),
        LayoutKind::Monocle => windows.iter().map(|&w| (w, usable)).collect(),
        LayoutKind::Floating => Vec::new(),
    }
}

/// Binary Space Partitioning (BSP) / Dwindle tiling algorithm.
/// Alternates splits based on aspect ratio, allocating inner gaps cleanly.
fn compute_dwindle(
    windows: &[WindowId],
    usable: Rect,
    params: &LayoutParams,
) -> Vec<(WindowId, Rect)> {
    let mut results = Vec::with_capacity(windows.len());
    let mut current_rect = usable;
    let ratio = params.master_ratio.clamp(0.1, 0.9);

    for (i, &win) in windows.iter().enumerate() {
        if i == windows.len() - 1 {
            results.push((win, current_rect));
            break;
        }

        let split_ratio = if i == 0 { ratio } else { 0.5 };
        let split_horizontal = if current_rect.width >= current_rect.height {
            current_rect.width >= 2 || current_rect.height < 2
        } else {
            current_rect.height < 2 && current_rect.width >= 2
        };

        if split_horizontal {
            let inner_gap = if current_rect.width >= params.inner_gap + 2 {
                params.inner_gap
            } else {
                0
            };
            let avail_w = current_rect.width.saturating_sub(inner_gap);
            let w1 = if avail_w <= 1 {
                1
            } else {
                (((avail_w as f32) * split_ratio) as u32).clamp(1, avail_w - 1)
            };
            let w2 = avail_w.saturating_sub(w1).max(1);

            let win_rect = Rect::new(current_rect.x, current_rect.y, w1, current_rect.height);
            results.push((win, win_rect));

            current_rect = Rect::new(
                current_rect.x + (w1 + inner_gap) as i32,
                current_rect.y,
                w2,
                current_rect.height,
            );
        } else {
            let inner_gap = if current_rect.height >= params.inner_gap + 2 {
                params.inner_gap
            } else {
                0
            };
            let avail_h = current_rect.height.saturating_sub(inner_gap);
            let h1 = if avail_h <= 1 {
                1
            } else {
                (((avail_h as f32) * split_ratio) as u32).clamp(1, avail_h - 1)
            };
            let h2 = avail_h.saturating_sub(h1).max(1);

            let win_rect = Rect::new(current_rect.x, current_rect.y, current_rect.width, h1);
            results.push((win, win_rect));

            current_rect = Rect::new(
                current_rect.x,
                current_rect.y + (h1 + inner_gap) as i32,
                current_rect.width,
                h2,
            );
        }
    }

    results
}

/// Master-and-stack tiling layout algorithm.
/// Divides work area into a primary master pane on the left and a vertical stack on the right.
fn compute_master_stack(
    windows: &[WindowId],
    usable: Rect,
    params: &LayoutParams,
) -> Vec<(WindowId, Rect)> {
    let count = windows.len();
    let mut results = Vec::with_capacity(count);

    let master_count = params.master_count.clamp(1, count - 1);
    let stack_count = count - master_count;
    let ratio = params.master_ratio.clamp(0.1, 0.9);

    let master_w = if stack_count > 0 {
        let raw_w = (usable.width as f32 * ratio) - (params.inner_gap as f32 / 2.0);
        (raw_w as u32).clamp(1, usable.width.saturating_sub(params.inner_gap + 1))
    } else {
        usable.width
    };

    let stack_w = if stack_count > 0 {
        usable
            .width
            .saturating_sub(master_w)
            .saturating_sub(params.inner_gap)
            .max(1)
    } else {
        0
    };

    // Layout master column
    let master_slots = split_vertical(
        usable.x,
        usable.y,
        master_w,
        usable.height,
        master_count,
        params.inner_gap,
    );
    for (i, &win) in windows.iter().take(master_count).enumerate() {
        results.push((win, master_slots[i]));
    }

    // Layout stack column
    if stack_count > 0 {
        let stack_x = usable.x + (master_w + params.inner_gap) as i32;
        let stack_slots = split_vertical(
            stack_x,
            usable.y,
            stack_w,
            usable.height,
            stack_count,
            params.inner_gap,
        );
        for (i, &win) in windows.iter().skip(master_count).enumerate() {
            results.push((win, stack_slots[i]));
        }
    }

    results
}

/// Evenly splits a vertical column of `count` windows, respecting `inner_gap`
/// and distributing rounding remainders cleanly to avoid gaps or overlaps.
fn split_vertical(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    count: usize,
    inner_gap: u32,
) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![Rect::new(x, y, width, height)];
    }

    let total_gaps = inner_gap.saturating_mul((count - 1) as u32);
    let avail_h = height.saturating_sub(total_gaps);
    let base_slot_h = (avail_h / count as u32).max(1);
    let remainder = avail_h % count as u32;

    let mut slots = Vec::with_capacity(count);
    let mut current_y = y;

    for i in 0..count {
        let extra = if (i as u32) < remainder { 1 } else { 0 };
        let h = if i == count - 1 {
            // Fill any remaining vertical space accurately
            (y + height as i32 - current_y).max(1) as u32
        } else {
            base_slot_h + extra
        };

        slots.push(Rect::new(x, current_y, width, h));
        current_y += (h + inner_gap) as i32;
    }

    slots
}

/// Niri-style horizontal scrolling ribbon layout.
/// Columns are laid out side-by-side with inner gaps, centering the active window in view.
fn compute_scrolling(
    windows: &[WindowId],
    usable: Rect,
    params: &LayoutParams,
) -> Vec<(WindowId, Rect)> {
    let count = windows.len();
    let col_w = if params.scroll_column_width > 0 {
        params.scroll_column_width.min(usable.width)
    } else {
        ((usable.width as f32 * params.master_ratio.clamp(0.2, 0.9)) as u32)
            .clamp(100, usable.width)
    };

    let active_idx = params.scroll_active_index.min(count.saturating_sub(1));
    let center_x = usable.x + ((usable.width as i32 - col_w as i32) / 2);

    windows
        .iter()
        .enumerate()
        .map(|(i, &win)| {
            let offset_cols = i as i32 - active_idx as i32;
            let col_step = (col_w + params.inner_gap) as i32;
            let win_x = center_x + (offset_cols * col_step);
            (win, Rect::new(win_x, usable.y, col_w, usable.height))
        })
        .collect()
}
