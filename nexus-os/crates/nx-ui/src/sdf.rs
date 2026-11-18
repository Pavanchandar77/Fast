//! CPU signed-distance field for rounded rectangles.
//!
//! `sd_round_box` is the reference implementation of the shape shader. The
//! golden-image test compares GPU pixels against a raster of this function
//! outside the anti-aliased edge, where `fwidth` does not matter.

use crate::color::Color;
use crate::geom::{Point, Rect, Size};

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = if (edge1 - edge0).abs() < 1.0e-6 {
        if x < edge0 {
            0.0
        } else {
            1.0
        }
    } else {
        ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0)
    };
    t * t * (3.0 - 2.0 * t)
}

/// `smoothing` is 0 for a circular corner and 1 for a squircle (n ≈ 4.5).
pub fn sd_round_box(p: Point, half: Size, radius: f32, smoothing: f32) -> f32 {
    let limit = half.w.min(half.h).max(0.0);
    let r = radius.clamp(0.0, limit);
    let qx = p.x.abs() - half.w + r;
    let qy = p.y.abs() - half.h + r;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    let n = 2.0 + (4.5 - 2.0) * smoothing.clamp(0.0, 1.0);
    let corner = (ax.powf(n) + ay.powf(n)).powf(1.0 / n);
    qx.max(qy).min(0.0) + corner - r
}

pub fn radius_at(p: Point, radii: [f32; 4]) -> f32 {
    let top = if p.x >= 0.0 { radii[1] } else { radii[0] };
    let bot = if p.x >= 0.0 { radii[2] } else { radii[3] };
    if p.y >= 0.0 {
        bot
    } else {
        top
    }
}

pub fn coverage(sd: f32, aa: f32) -> f32 {
    let aa = aa.max(1.0e-3);
    1.0 - smoothstep(-aa, aa, sd)
}

/// Raster `rect` into an RGBA8 buffer of `width` by `height`. `aa` is the
/// coverage ramp in pixels. Colors are straight sRGB.
pub fn raster_round_rect(
    width: u32,
    height: u32,
    rect: Rect,
    radius: f32,
    smoothing: f32,
    fill: Color,
    clear: Color,
    aa: f32,
) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let center = rect.center();
    let half = Size::new(rect.w * 0.5, rect.h * 0.5);
    for y in 0..height {
        for x in 0..width {
            let sample = Point::new(x as f32 + 0.5, y as f32 + 0.5);
            let p = Point::new(sample.x - center.x, sample.y - center.y);
            let sd = sd_round_box(p, half, radius, smoothing);
            let cov = coverage(sd, aa) * fill.a;
            let color = Color {
                r: clear.r * (1.0 - cov) + fill.r * cov,
                g: clear.g * (1.0 - cov) + fill.g * cov,
                b: clear.b * (1.0 - cov) + fill.b * cov,
                a: clear.a * (1.0 - cov) + cov,
            };
            let i = ((y * width + x) * 4) as usize;
            let bytes = color.to_bytes();
            pixels[i..i + 4].copy_from_slice(&bytes);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_of_a_round_rect_is_inside() {
        let sd = sd_round_box(Point::new(0.0, 0.0), Size::new(40.0, 24.0), 8.0, 0.85);
        assert!(sd < -8.0);
        assert!((coverage(sd, 1.0) - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn outside_corner_is_outside() {
        // Box half-size 20, radius 10. The geometric corner is 10px outside the arc.
        let sd = sd_round_box(Point::new(20.0, 20.0), Size::new(20.0, 20.0), 10.0, 0.0);
        assert!(sd > 1.0, "sd {sd}");
        assert!(coverage(sd, 1.0) < 0.05);
    }

    #[test]
    fn squircle_corner_is_fuller_than_a_circle() {
        let p = Point::new(16.0, 16.0);
        let half = Size::new(20.0, 20.0);
        let round = sd_round_box(p, half, 12.0, 0.0);
        let squircle = sd_round_box(p, half, 12.0, 1.0);
        assert!(squircle < round, "squircle {squircle} round {round}");
    }

    #[test]
    fn raster_center_matches_fill_and_corner_is_clear() {
        let fill = Color::rgb(10, 132, 255);
        let clear = Color::TRANSPARENT;
        let pixels = raster_round_rect(
            64,
            64,
            Rect::new(8.0, 8.0, 48.0, 48.0),
            8.0,
            0.85,
            fill,
            clear,
            1.0,
        );
        let center = pixel(&pixels, 64, 32, 32);
        assert_eq!(center, [10, 132, 255, 255]);
        let outside = pixel(&pixels, 64, 0, 0);
        assert_eq!(outside, [0, 0, 0, 0]);
        let corner = pixel(&pixels, 64, 8, 8);
        assert!(corner[3] < 40, "corner alpha {}", corner[3]);
    }

    fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }
}
