//! Continuous-corner squircle geometry.
//!
//! The signed distance is [`nx_ui::sdf::sd_round_box`], the same function the
//! shape shader uses. This module samples that field so dock, menus, and
//! Mission Control tiles share one corner definition. `smoothing` 0 is a
//! circular corner. 1 is an iOS-style squircle (n ≈ 4.5).

use nx_ui::geom::{Point, Rect, Size};
use nx_ui::sdf::sd_round_box;
use nx_ui::tokens::CORNER_SMOOTHING;

use crate::util::finite_f32;

/// A rounded rectangle with continuous-corner smoothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squircle {
    pub rect: Rect,
    pub radius: f32,
    pub smoothing: f32,
}

impl Squircle {
    pub fn new(rect: Rect, radius: f32, smoothing: f32) -> Self {
        let radius = finite_f32(radius, 0.0).max(0.0);
        let limit = (rect.w.min(rect.h) * 0.5).max(0.0);
        Self {
            rect,
            radius: radius.min(limit),
            smoothing: finite_f32(smoothing, CORNER_SMOOTHING).clamp(0.0, 1.0),
        }
    }

    /// Dock and menu capsules use the design-system smoothing.
    pub fn capsule(rect: Rect) -> Self {
        let radius = rect.w.min(rect.h) * 0.5;
        Self::new(rect, radius, CORNER_SMOOTHING)
    }

    pub fn signed_distance(self, point: Point) -> f32 {
        let center = self.rect.center();
        let p = Point::new(point.x - center.x, point.y - center.y);
        let half = Size::new(self.rect.w * 0.5, self.rect.h * 0.5);
        sd_round_box(p, half, self.radius, self.smoothing)
    }

    pub fn contains(self, point: Point) -> bool {
        self.signed_distance(point) <= 0.0
    }

    /// Points on the boundary, sampled by binary search along rays from the center.
    pub fn outline(self, samples: u32) -> Vec<Point> {
        let samples = samples.max(4);
        let center = self.rect.center();
        let reach = self.rect.w.hypot(self.rect.h).max(1.0);
        let mut points = Vec::with_capacity(samples as usize);
        for i in 0..samples {
            let angle = (i as f32) / (samples as f32) * std::f32::consts::TAU;
            let dir_x = angle.cos();
            let dir_y = angle.sin();
            let mut lo = 0.0f32;
            let mut hi = reach;
            for _ in 0..18 {
                let mid = (lo + hi) * 0.5;
                let sample = Point::new(center.x + dir_x * mid, center.y + dir_y * mid);
                if self.signed_distance(sample) < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            points.push(Point::new(center.x + dir_x * lo, center.y + dir_y * lo));
        }
        points
    }
}

/// Coverage mask. Each pixel is 255 when its center lies inside any shape.
/// Used as the CPU golden image when lavapipe is not installed.
pub fn coverage_mask(width: u32, height: u32, shapes: &[Squircle]) -> Vec<u8> {
    let width = width.max(1);
    let height = height.max(1);
    let mut pixels = vec![0u8; width as usize * height as usize];
    for y in 0..height {
        for x in 0..width {
            let point = Point::new(x as f32 + 0.5, y as f32 + 0.5);
            let inside = shapes.iter().any(|shape| shape.contains(point));
            if inside {
                pixels[(y * width + x) as usize] = 255;
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_shape(smoothing: f32) -> Squircle {
        Squircle::new(Rect::new(10.0, 10.0, 80.0, 48.0), 16.0, smoothing)
    }

    #[test]
    fn center_is_inside_and_far_corner_is_outside() {
        let shape = box_shape(CORNER_SMOOTHING);
        assert!(shape.contains(shape.rect.center()));
        assert!(shape.signed_distance(shape.rect.center()) < -4.0);
        let corner = Point::new(shape.rect.x, shape.rect.y);
        assert!(!shape.contains(corner));
        assert!(shape.signed_distance(corner) > 0.0);
    }

    #[test]
    fn squircle_corner_is_fuller_than_a_circle() {
        let circle = box_shape(0.0);
        let squircle = box_shape(1.0);
        // A point in the rounded corner, inside the bounding box.
        let probe = Point::new(18.0, 18.0);
        let round_sd = circle.signed_distance(probe);
        let squir_sd = squircle.signed_distance(probe);
        assert!(
            squir_sd < round_sd,
            "squircle sd {squir_sd} should be inside the circular sd {round_sd}"
        );
    }

    #[test]
    fn outline_samples_sit_on_the_boundary() {
        let shape = box_shape(CORNER_SMOOTHING);
        let points = shape.outline(48);
        assert_eq!(points.len(), 48);
        for point in points {
            let sd = shape.signed_distance(point);
            assert!(sd.abs() < 0.75, "outline sd {sd} at {point:?}");
            assert!(point.x >= shape.rect.x - 1.0);
            assert!(point.y >= shape.rect.y - 1.0);
            assert!(point.x <= shape.rect.right() + 1.0);
            assert!(point.y <= shape.rect.bottom() + 1.0);
        }
    }

    #[test]
    fn capsule_corner_pixel_is_clear_and_center_is_covered() {
        let shape = Squircle::capsule(Rect::new(4.0, 4.0, 40.0, 16.0));
        let mask = coverage_mask(48, 24, &[shape]);
        let center = mask[(12 * 48 + 24) as usize];
        assert_eq!(center, 255);
        let corner = mask[0];
        assert_eq!(corner, 0);
    }

    #[test]
    fn radius_clamps_to_half_the_short_side() {
        let shape = Squircle::new(Rect::new(0.0, 0.0, 20.0, 10.0), 100.0, 2.0);
        assert!((shape.radius - 5.0).abs() < 1.0e-4);
        assert!((shape.smoothing - 1.0).abs() < 1.0e-4);
    }
}
