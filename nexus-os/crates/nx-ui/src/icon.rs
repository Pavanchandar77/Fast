//! Monochrome vector icons drawn with GPUI paths (no external SVG files).

use gpui::{
    canvas, point, px, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window,
};

/// Dock / chrome glyph set used by the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphIcon {
    Folder,
    Search,
    Shield,
    Terminal,
    Gear,
    Trash,
    Wifi,
    Bluetooth,
    Speaker,
    SpeakerMuted,
    Moon,
    Globe,
    Sun,
    Cpu,
    Memory,
    Tray,
    Clipboard,
}

impl GlyphIcon {
    /// Map a dock item id to a monochrome glyph.
    pub fn for_dock_id(id: &str) -> Self {
        match id {
            "explorer" | "files" => Self::Folder,
            "spotlight" | "search" => Self::Search,
            "privacy" | "shield" => Self::Shield,
            "terminal" | "console" => Self::Terminal,
            "settings" | "prefs" => Self::Gear,
            "trash" | "bin" => Self::Trash,
            _ => Self::Folder,
        }
    }
}

/// A square canvas that paints `kind` in `color`.
pub fn glyph_icon(kind: GlyphIcon, color: Hsla, size: f32) -> impl IntoElement {
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _cx| {
            paint_glyph(window, bounds, kind, color);
        },
    )
    .w(px(size))
    .h(px(size))
    .flex_shrink_0()
}

fn map(bounds: Bounds<Pixels>, x: f32, y: f32) -> Point<Pixels> {
    let w = f32::from(bounds.size.width).max(1.0);
    let h = f32::from(bounds.size.height).max(1.0);
    point(
        bounds.origin.x + px(x / 24.0 * w),
        bounds.origin.y + px(y / 24.0 * h),
    )
}

fn stroke(window: &mut Window, color: Hsla, width: f32, build: impl FnOnce(&mut PathBuilder)) {
    let mut path = PathBuilder::stroke(px(width));
    build(&mut path);
    if let Ok(built) = path.build() {
        window.paint_path(built, color);
    }
}

fn fill(window: &mut Window, color: Hsla, build: impl FnOnce(&mut PathBuilder)) {
    let mut path = PathBuilder::fill();
    build(&mut path);
    if let Ok(built) = path.build() {
        window.paint_path(built, color);
    }
}

fn scale_r(b: Bounds<Pixels>, r: f32) -> Pixels {
    px(r * f32::from(b.size.width) / 24.0)
}

/// Full circle centred at (cx, cy) with radius r (icon units).
fn circle_path(p: &mut PathBuilder, b: Bounds<Pixels>, cx: f32, cy: f32, r: f32) {
    let rr = scale_r(b, r);
    p.move_to(map(b, cx + r, cy));
    p.arc_to(point(rr, rr), px(0.0), true, true, map(b, cx - r, cy));
    p.arc_to(point(rr, rr), px(0.0), true, true, map(b, cx + r, cy));
}

/// Arc on a circle centred at (cx, cy) from angle a0 to a1 (degrees,
/// clockwise on screen).
fn arc_path(p: &mut PathBuilder, b: Bounds<Pixels>, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
    let (s0, c0) = a0.to_radians().sin_cos();
    let (s1, c1) = a1.to_radians().sin_cos();
    let rr = scale_r(b, r);
    p.move_to(map(b, cx + r * c0, cy + r * s0));
    p.arc_to(
        point(rr, rr),
        px(0.0),
        (a1 - a0).abs() > 180.0,
        true,
        map(b, cx + r * c1, cy + r * s1),
    );
}

fn polyline(p: &mut PathBuilder, b: Bounds<Pixels>, pts: &[(f32, f32)], close: bool) {
    if let Some((x, y)) = pts.first() {
        p.move_to(map(b, *x, *y));
    }
    for (x, y) in pts.iter().skip(1) {
        p.line_to(map(b, *x, *y));
    }
    if close {
        p.close();
    }
}

fn speaker_body(window: &mut Window, b: Bounds<Pixels>, color: Hsla) {
    fill(window, color, |p| {
        polyline(
            p,
            b,
            &[
                (3.5, 9.5),
                (7.5, 9.5),
                (12.5, 5.0),
                (12.5, 19.0),
                (7.5, 14.5),
                (3.5, 14.5),
            ],
            true,
        );
    });
}

fn paint_glyph(window: &mut Window, b: Bounds<Pixels>, kind: GlyphIcon, color: Hsla) {
    let w = (f32::from(b.size.width) / 24.0).clamp(0.8, 2.4);
    let sw = 1.7 * w;
    match kind {
        GlyphIcon::Folder => {
            fill(window, color, |p| {
                p.move_to(map(b, 3.0, 8.0));
                p.line_to(map(b, 3.0, 18.5));
                p.line_to(map(b, 21.0, 18.5));
                p.line_to(map(b, 21.0, 9.5));
                p.line_to(map(b, 12.0, 9.5));
                p.line_to(map(b, 10.0, 6.5));
                p.line_to(map(b, 5.0, 6.5));
                p.line_to(map(b, 3.0, 8.0));
                p.close();
            });
        }
        GlyphIcon::Search => {
            let r = px(5.2 * f32::from(b.size.width) / 24.0);
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 15.7, 10.5));
                p.arc_to(point(r, r), px(0.0), true, true, map(b, 5.3, 10.5));
                p.arc_to(point(r, r), px(0.0), true, true, map(b, 15.7, 10.5));
            });
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 14.6, 14.6));
                p.line_to(map(b, 20.0, 20.0));
            });
        }
        GlyphIcon::Shield => {
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 12.0, 3.5));
                p.line_to(map(b, 19.5, 6.5));
                p.line_to(map(b, 19.5, 13.0));
                p.cubic_bezier_to(map(b, 12.0, 20.5), map(b, 19.5, 17.5), map(b, 15.5, 19.5));
                p.cubic_bezier_to(map(b, 4.5, 13.0), map(b, 8.5, 19.5), map(b, 4.5, 17.5));
                p.line_to(map(b, 4.5, 6.5));
                p.close();
            });
        }
        GlyphIcon::Terminal => {
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 5.5, 5.5));
                p.line_to(map(b, 18.5, 5.5));
                p.line_to(map(b, 18.5, 18.5));
                p.line_to(map(b, 5.5, 18.5));
                p.close();
            });
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 7.5, 10.0));
                p.line_to(map(b, 10.5, 12.0));
                p.line_to(map(b, 7.5, 14.0));
            });
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 12.0, 14.5));
                p.line_to(map(b, 16.5, 14.5));
            });
        }
        GlyphIcon::Gear => {
            // Hub
            let hub = px(3.2 * f32::from(b.size.width) / 24.0);
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 15.2, 12.0));
                p.arc_to(point(hub, hub), px(0.0), true, true, map(b, 8.8, 12.0));
                p.arc_to(point(hub, hub), px(0.0), true, true, map(b, 15.2, 12.0));
            });
            // Eight tick marks
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::PI / 4.0;
                let (sin, cos) = a.sin_cos();
                stroke(window, color, sw, |p| {
                    p.move_to(map(b, 12.0 + cos * 5.4, 12.0 + sin * 5.4));
                    p.line_to(map(b, 12.0 + cos * 8.6, 12.0 + sin * 8.6));
                });
            }
        }
        GlyphIcon::Trash => {
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 8.0, 7.5));
                p.line_to(map(b, 16.0, 7.5));
                p.line_to(map(b, 15.2, 19.0));
                p.line_to(map(b, 8.8, 19.0));
                p.close();
            });
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 6.5, 7.5));
                p.line_to(map(b, 17.5, 7.5));
            });
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 10.0, 5.2));
                p.line_to(map(b, 14.0, 5.2));
                p.line_to(map(b, 14.5, 7.5));
                p.line_to(map(b, 9.5, 7.5));
                p.close();
            });
            stroke(window, color, sw * 0.8, |p| {
                p.move_to(map(b, 11.0, 10.0));
                p.line_to(map(b, 11.0, 16.2));
                p.move_to(map(b, 13.0, 10.0));
                p.line_to(map(b, 13.0, 16.2));
            });
        }
        GlyphIcon::Wifi => {
            for r in [3.6, 7.2, 10.8] {
                stroke(window, color, sw * 1.1, |p| {
                    arc_path(p, b, 12.0, 19.5, r, -135.0, -45.0);
                });
            }
            fill(window, color, |p| circle_path(p, b, 12.0, 19.5, 1.5));
        }
        GlyphIcon::Bluetooth => {
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[
                        (6.5, 7.5),
                        (17.0, 16.5),
                        (12.0, 21.0),
                        (12.0, 3.0),
                        (17.0, 7.5),
                        (6.5, 16.5),
                    ],
                    false,
                );
            });
        }
        GlyphIcon::Speaker => {
            speaker_body(window, b, color);
            for r in [3.5, 7.0] {
                stroke(window, color, sw, |p| {
                    arc_path(p, b, 13.5, 12.0, r, -50.0, 50.0);
                });
            }
        }
        GlyphIcon::SpeakerMuted => {
            speaker_body(window, b, color);
            stroke(window, color, sw, |p| {
                p.move_to(map(b, 15.5, 9.5));
                p.line_to(map(b, 20.5, 14.5));
                p.move_to(map(b, 20.5, 9.5));
                p.line_to(map(b, 15.5, 14.5));
            });
        }
        GlyphIcon::Moon => {
            let big = scale_r(b, 8.8);
            let small = scale_r(b, 8.0);
            fill(window, color, |p| {
                p.move_to(map(b, 15.0, 3.0));
                p.arc_to(point(big, big), px(0.0), true, false, map(b, 21.0, 16.7));
                p.arc_to(point(small, small), px(0.0), false, true, map(b, 15.0, 3.0));
            });
        }
        GlyphIcon::Globe => {
            stroke(window, color, sw, |p| circle_path(p, b, 12.0, 12.0, 8.5));
            let rx = scale_r(b, 3.8);
            let ry = scale_r(b, 8.5);
            stroke(window, color, sw * 0.85, |p| {
                p.move_to(map(b, 12.0, 3.5));
                p.arc_to(point(rx, ry), px(0.0), false, true, map(b, 12.0, 20.5));
                p.arc_to(point(rx, ry), px(0.0), false, true, map(b, 12.0, 3.5));
            });
            stroke(window, color, sw * 0.85, |p| {
                p.move_to(map(b, 3.5, 12.0));
                p.line_to(map(b, 20.5, 12.0));
                p.move_to(map(b, 5.2, 7.8));
                p.line_to(map(b, 18.8, 7.8));
                p.move_to(map(b, 5.2, 16.2));
                p.line_to(map(b, 18.8, 16.2));
            });
        }
        GlyphIcon::Sun => {
            fill(window, color, |p| circle_path(p, b, 12.0, 12.0, 4.2));
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::PI / 4.0;
                let (sin, cos) = a.sin_cos();
                stroke(window, color, sw, |p| {
                    p.move_to(map(b, 12.0 + cos * 6.6, 12.0 + sin * 6.6));
                    p.line_to(map(b, 12.0 + cos * 9.2, 12.0 + sin * 9.2));
                });
            }
        }
        GlyphIcon::Cpu => {
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[(6.5, 6.5), (17.5, 6.5), (17.5, 17.5), (6.5, 17.5)],
                    true,
                );
            });
            fill(window, color, |p| {
                polyline(
                    p,
                    b,
                    &[(9.8, 9.8), (14.2, 9.8), (14.2, 14.2), (9.8, 14.2)],
                    true,
                );
            });
            stroke(window, color, sw * 0.8, |p| {
                for v in [9.5, 12.0, 14.5] {
                    p.move_to(map(b, v, 3.5));
                    p.line_to(map(b, v, 6.5));
                    p.move_to(map(b, v, 17.5));
                    p.line_to(map(b, v, 20.5));
                    p.move_to(map(b, 3.5, v));
                    p.line_to(map(b, 6.5, v));
                    p.move_to(map(b, 17.5, v));
                    p.line_to(map(b, 20.5, v));
                }
            });
        }
        GlyphIcon::Memory => {
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[(3.0, 7.5), (21.0, 7.5), (21.0, 15.5), (3.0, 15.5)],
                    true,
                );
            });
            for x in [5.5, 10.0, 14.5] {
                fill(window, color, |p| {
                    polyline(
                        p,
                        b,
                        &[(x, 9.8), (x + 3.5, 9.8), (x + 3.5, 13.2), (x, 13.2)],
                        true,
                    );
                });
            }
            stroke(window, color, sw * 0.8, |p| {
                for x in [6.0, 9.0, 12.0, 15.0, 18.0] {
                    p.move_to(map(b, x, 15.5));
                    p.line_to(map(b, x, 18.5));
                }
            });
        }
        GlyphIcon::Tray => {
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[
                        (3.5, 13.0),
                        (6.0, 5.5),
                        (18.0, 5.5),
                        (20.5, 13.0),
                        (20.5, 19.0),
                        (3.5, 19.0),
                    ],
                    true,
                );
            });
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[
                        (3.5, 13.0),
                        (8.5, 13.0),
                        (10.0, 15.5),
                        (14.0, 15.5),
                        (15.5, 13.0),
                        (20.5, 13.0),
                    ],
                    false,
                );
            });
        }
        GlyphIcon::Clipboard => {
            stroke(window, color, sw, |p| {
                polyline(
                    p,
                    b,
                    &[(5.5, 5.0), (18.5, 5.0), (18.5, 21.0), (5.5, 21.0)],
                    true,
                );
            });
            fill(window, color, |p| {
                polyline(
                    p,
                    b,
                    &[(9.0, 3.0), (15.0, 3.0), (15.0, 7.0), (9.0, 7.0)],
                    true,
                );
            });
            stroke(window, color, sw * 0.8, |p| {
                p.move_to(map(b, 8.5, 11.5));
                p.line_to(map(b, 15.5, 11.5));
                p.move_to(map(b, 8.5, 15.5));
                p.line_to(map(b, 13.5, 15.5));
            });
        }
    }
}

/// Legacy SVG snippets kept for tests and documentation. GPUI paints via
/// [`glyph_icon`] rather than these strings.
pub const ICON_SEARCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6"/><line x1="16.2" y1="16.2" x2="21" y2="21"/></svg>"#;
pub const ICON_FOLDER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="black" d="M3 6.5A2.5 2.5 0 0 1 5.5 4H9l2 2h7.5A2.5 2.5 0 0 1 21 8.5v9A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5v-11z"/></svg>"#;
pub const ICON_GEAR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 3v2.2M12 18.8V21M3 12h2.2M18.8 12H21M5.6 5.6l1.6 1.6M16.8 16.8l1.6 1.6M18.4 5.6l-1.6 1.6M7.2 16.8l-1.6 1.6"/></svg>"#;
pub const ICON_SIDEBAR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><line x1="9" y1="4" x2="9" y2="20"/></svg>"#;
pub const ICON_CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.2 4.2L19 7.5"/></svg>"#;
pub const ICON_SUN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.2M12 19.3V21.5M2.5 12h2.2M19.3 12H21.5M5 5l1.6 1.6M17.4 17.4L19 19M19 5l-1.6 1.6M6.6 17.4L5 19"/></svg>"#;
pub const ICON_MOON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="black"><path d="M15.5 2.5a8.8 8.8 0 1 0 6 14.2A8 8 0 0 1 15.5 2.5z"/></svg>"#;

#[cfg(test)]
mod tests {
    use super::GlyphIcon;

    #[test]
    fn dock_ids_map_to_expected_glyphs() {
        assert_eq!(GlyphIcon::for_dock_id("explorer"), GlyphIcon::Folder);
        assert_eq!(GlyphIcon::for_dock_id("spotlight"), GlyphIcon::Search);
        assert_eq!(GlyphIcon::for_dock_id("privacy"), GlyphIcon::Shield);
        assert_eq!(GlyphIcon::for_dock_id("terminal"), GlyphIcon::Terminal);
        assert_eq!(GlyphIcon::for_dock_id("settings"), GlyphIcon::Gear);
        assert_eq!(GlyphIcon::for_dock_id("trash"), GlyphIcon::Trash);
    }
}
