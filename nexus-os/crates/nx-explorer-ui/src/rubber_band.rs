//! Rubber-band selection rectangle logic and rendering.

use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::scene::Scene;
use nx_ui::tokens::Shadow;
use nx_ui::widgets::Frame;

#[derive(Debug, Clone, Default)]
pub struct RubberBand {
    active: bool,
    origin: Point,
    current: Point,
}

impl RubberBand {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self, origin: Point) {
        self.active = true;
        self.origin = origin;
        self.current = origin;
    }

    pub fn update(&mut self, current: Point) {
        if self.active {
            self.current = current;
        }
    }

    pub fn end(&mut self) -> Option<Rect> {
        if !self.active {
            return None;
        }
        self.active = false;
        let r = self.rect();
        if r.w > 2.0 && r.h > 2.0 {
            Some(r)
        } else {
            None
        }
    }

    pub fn cancel(&mut self) {
        self.active = false;
    }

    pub fn rect(&self) -> Rect {
        let x = self.origin.x.min(self.current.x);
        let y = self.origin.y.min(self.current.y);
        let w = (self.origin.x - self.current.x).abs();
        let h = (self.origin.y - self.current.y).abs();
        Rect::new(x, y, w, h)
    }

    pub fn intersects(&self, target: Rect) -> bool {
        if !self.active {
            return false;
        }
        let r = self.rect();
        r.intersects(target)
    }

    pub fn paint(&self, frame: &mut Frame<'_>) {
        if !self.active {
            return;
        }
        let r = self.rect();
        if r.w < 1.0 || r.h < 1.0 {
            return;
        }

        let theme = frame.theme;
        let mut fill_color = theme.palette.accent;
        fill_color.a = 0.16;

        let mut border_color = theme.palette.accent;
        border_color.a = 0.65;

        frame.panel(
            Scene::root(),
            r,
            Paint::Solid(fill_color),
            CornerRadii::all(theme.radii.xs),
            Border {
                color: border_color,
                width: 1.0,
            },
            Shadow::none(),
            Layer::Foreground,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubber_band_geometry_and_intersection() {
        let mut rb = RubberBand::new();
        assert!(!rb.is_active());

        rb.start(Point::new(10.0, 20.0));
        assert!(rb.is_active());

        rb.update(Point::new(100.0, 150.0));
        let r = rb.rect();
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.w, 90.0);
        assert_eq!(r.h, 130.0);

        // Target inside
        assert!(rb.intersects(Rect::new(20.0, 30.0, 40.0, 40.0)));
        // Target outside
        assert!(!rb.intersects(Rect::new(200.0, 200.0, 10.0, 10.0)));

        // End
        let final_rect = rb.end();
        assert_eq!(final_rect, Some(r));
        assert!(!rb.is_active());
    }
}
