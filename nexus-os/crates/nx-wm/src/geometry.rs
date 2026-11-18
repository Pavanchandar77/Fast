use serde::{Deserialize, Serialize};

/// 2D cardinal direction for directional focus and movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn opposite(&self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
}

/// 2D integer point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn distance_squared(&self, other: &Point) -> u64 {
        let dx = (self.x as i64) - (other.x as i64);
        let dy = (self.y as i64) - (other.y as i64);
        (dx * dx + dy * dy) as u64
    }
}

/// 2D bounding rectangle in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn zero() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.width as i32)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height as i32)
    }

    pub fn center(&self) -> Point {
        Point::new(
            self.x + (self.width as i32 / 2),
            self.y + (self.height as i32 / 2),
        )
    }

    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.right() <= self.right()
            && other.y >= self.y
            && other.bottom() <= self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());

        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, (x2 - x1) as u32, (y2 - y1) as u32))
        } else {
            None
        }
    }

    pub fn shrink(&self, gap: u32) -> Rect {
        let double_gap = gap.saturating_mul(2);
        if self.width <= double_gap || self.height <= double_gap {
            return *self;
        }
        Rect::new(
            self.x + gap as i32,
            self.y + gap as i32,
            self.width - double_gap,
            self.height - double_gap,
        )
    }

    pub fn shrink_edges(&self, left: u32, top: u32, right: u32, bottom: u32) -> Rect {
        let horiz = left.saturating_add(right);
        let vert = top.saturating_add(bottom);
        if self.width <= horiz || self.height <= vert {
            return *self;
        }
        Rect::new(
            self.x + left as i32,
            self.y + top as i32,
            self.width - horiz,
            self.height - vert,
        )
    }

    pub fn to_f32_array(&self) -> [f32; 4] {
        [
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        ]
    }

    pub fn to_tuple(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.width as i32, self.height as i32)
    }

    pub fn from_tuple(t: (i32, i32, i32, i32)) -> Self {
        Self::new(t.0, t.1, t.2.max(0) as u32, t.3.max(0) as u32)
    }

    /// Computes directional distance from `self` to `other` along `dir`.
    /// Returns None if `other` is not in direction `dir`.
    pub fn directional_distance(&self, other: &Rect, dir: Direction) -> Option<u64> {
        let c_self = self.center();
        let c_other = other.center();

        let (in_direction, primary_dist, perp_dist) = match dir {
            Direction::Left => (
                c_other.x < c_self.x,
                (c_self.x - c_other.x).abs() as u64,
                (c_self.y - c_other.y).abs() as u64,
            ),
            Direction::Right => (
                c_other.x > c_self.x,
                (c_other.x - c_self.x).abs() as u64,
                (c_self.y - c_other.y).abs() as u64,
            ),
            Direction::Up => (
                c_other.y < c_self.y,
                (c_self.y - c_other.y).abs() as u64,
                (c_self.x - c_other.x).abs() as u64,
            ),
            Direction::Down => (
                c_other.y > c_self.y,
                (c_other.y - c_self.y).abs() as u64,
                (c_self.x - c_other.x).abs() as u64,
            ),
        };

        if !in_direction {
            return None;
        }

        // Weighted distance: prioritize alignment along primary direction, with penalty for perpendicular offset
        let overlap = match dir {
            Direction::Left | Direction::Right => {
                let y1 = self.y.max(other.y);
                let y2 = self.bottom().min(other.bottom());
                if y2 > y1 {
                    (y2 - y1) as u64
                } else {
                    0
                }
            }
            Direction::Up | Direction::Down => {
                let x1 = self.x.max(other.x);
                let x2 = self.right().min(other.right());
                if x2 > x1 {
                    (x2 - x1) as u64
                } else {
                    0
                }
            }
        };

        // Score: lower is better. Overlapping projections along perpendicular axis get a substantial bonus
        let penalty = perp_dist * 2;
        let bonus = overlap * 2;
        let score = (primary_dist * primary_dist) + (penalty * penalty);
        Some(score.saturating_sub(bonus))
    }
}
