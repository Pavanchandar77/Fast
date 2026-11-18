use serde::{Deserialize, Serialize};

/// Strongly typed identifier for a window (HWND wrapper on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

impl WindowId {
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn from_raw(raw: isize) -> Self {
        Self(raw as u64)
    }

    #[inline]
    pub const fn as_raw(self) -> isize {
        self.0 as isize
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WindowId(0x{:X})", self.0)
    }
}

/// Pixel rectangle defining window or zone geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    #[inline]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[inline]
    pub const fn right(&self) -> i32 {
        self.x.saturating_add(self.width as i32)
    }

    #[inline]
    pub const fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height as i32)
    }

    #[inline]
    pub const fn center(&self) -> (i32, i32) {
        (
            self.x.saturating_add((self.width / 2) as i32),
            self.y.saturating_add((self.height / 2) as i32),
        )
    }

    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Shrinks rect by `margin` on all sides (clamping width/height to at least 1).
    pub fn shrink(&self, margin: u32) -> Self {
        let double_margin = margin.saturating_mul(2);
        let w = self.width.saturating_sub(double_margin).max(1);
        let h = self.height.saturating_sub(double_margin).max(1);
        let x = self.x.saturating_add(margin as i32);
        let y = self.y.saturating_add(margin as i32);
        Self::new(x, y, w, h)
    }

    /// Squared Euclidean distance from point (px, py) to the center of this rect.
    pub fn distance_squared_to_center(&self, px: i32, py: i32) -> u64 {
        let (cx, cy) = self.center();
        let dx = (cx as i64) - (px as i64);
        let dy = (cy as i64) - (py as i64);
        (dx * dx + dy * dy) as u64
    }
}

/// Normalized floating-point rectangle (coordinates and dimensions in 0.0..=1.0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF {
    #[inline]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Projects normalized geometry into a pixel rectangle within a monitor work area.
    pub fn to_pixel_rect(&self, work_area: &Rect, gap: u32) -> Rect {
        let base_x = work_area.x as f32 + (self.x * work_area.width as f32);
        let base_y = work_area.y as f32 + (self.y * work_area.height as f32);
        let raw_w = self.width * work_area.width as f32;
        let raw_h = self.height * work_area.height as f32;

        let half_gap = (gap as f32) / 2.0;
        let px = (base_x + half_gap).round() as i32;
        let py = (base_y + half_gap).round() as i32;
        let pw = (raw_w - gap as f32).max(1.0).round() as u32;
        let ph = (raw_h - gap as f32).max(1.0).round() as u32;

        Rect::new(px, py, pw, ph)
    }
}

/// Status of a reversible mutation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    DryRun,
    Applied,
    Reverted,
}
