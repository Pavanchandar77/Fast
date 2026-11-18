//! Points, sizes, rectangles, and insets in a y-down coordinate space.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }
}

impl std::ops::Add for Point {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// Axis-aligned rectangle. `x` and `y` are the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.w, size.h)
    }

    pub fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn center(self) -> Point {
        Point::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
            && self.w > 0.0
            && self.h > 0.0
            && other.w > 0.0
            && other.h > 0.0
    }

    pub fn union(self, other: Self) -> Self {
        if self.w <= 0.0 || self.h <= 0.0 {
            return other;
        }
        if other.w <= 0.0 || other.h <= 0.0 {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let r = self.right().max(other.right());
        let b = self.bottom().max(other.bottom());
        Self::new(x, y, r - x, b - y)
    }

    pub fn inset(self, insets: Insets) -> Self {
        let w = (self.w - insets.left - insets.right).max(0.0);
        let h = (self.h - insets.top - insets.bottom).max(0.0);
        Self::new(self.x + insets.left, self.y + insets.top, w, h)
    }

    pub fn translate(self, delta: Point) -> Self {
        Self::new(self.x + delta.x, self.y + delta.y, self.w, self.h)
    }

    pub fn scale(self, factor: f32) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.w * factor,
            self.h * factor,
        )
    }

    pub fn shrink(self, amount: f32) -> Self {
        self.inset(Insets::all(amount))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self::new(vertical, horizontal, vertical, horizontal)
    }
}

/// Corner radii in the order top-left, top-right, bottom-right, bottom-left.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CornerRadii {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl CornerRadii {
    pub const fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }

    pub fn uniform(self) -> bool {
        self.tl == self.tr && self.tr == self.br && self.br == self.bl
    }

    /// Clamp each radius so adjacent corners cannot overlap.
    pub fn clamped(self, size: Size) -> Self {
        let max_r = (size.w.min(size.h) * 0.5).max(0.0);
        Self {
            tl: self.tl.clamp(0.0, max_r),
            tr: self.tr.clamp(0.0, max_r),
            br: self.br.clamp(0.0, max_r),
            bl: self.bl.clamp(0.0, max_r),
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.tl, self.tr, self.br, self.bl]
    }

    pub fn scale(self, factor: f32) -> Self {
        Self {
            tl: self.tl * factor,
            tr: self.tr * factor,
            br: self.br * factor,
            bl: self.bl * factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_edges() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(Point::new(10.0, 20.0)));
        assert!(r.contains(Point::new(39.9, 59.9)));
        assert!(!r.contains(Point::new(40.0, 30.0)));
        assert!(!r.contains(Point::new(9.9, 20.0)));
    }

    #[test]
    fn rect_intersection_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(8.0, 8.0, 10.0, 4.0);
        let c = Rect::new(20.0, 20.0, 2.0, 2.0);
        assert!(a.intersects(b));
        assert!(!a.intersects(c));
        let u = a.union(b);
        assert_eq!(u, Rect::new(0.0, 0.0, 18.0, 12.0));
    }

    #[test]
    fn insets_deflate_and_clamp() {
        let r = Rect::new(0.0, 0.0, 40.0, 20.0).inset(Insets::symmetric(4.0, 6.0));
        assert_eq!(r, Rect::new(6.0, 4.0, 28.0, 12.0));
        let gone = Rect::new(0.0, 0.0, 4.0, 4.0).inset(Insets::all(8.0));
        assert_eq!(gone.w, 0.0);
        assert_eq!(gone.h, 0.0);
    }

    #[test]
    fn corner_radii_clamp_to_half_min_side() {
        let radii = CornerRadii::all(40.0).clamped(Size::new(20.0, 10.0));
        assert!(radii.tl <= 5.0 + f32::EPSILON);
        assert!(radii.br <= 5.0 + f32::EPSILON);
    }

    #[test]
    fn point_lerp() {
        let p = Point::new(0.0, 10.0).lerp(Point::new(10.0, 0.0), 0.25);
        assert!((p.x - 2.5).abs() < 1e-5);
        assert!((p.y - 7.5).abs() < 1e-5);
    }
}
