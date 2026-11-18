//! SVG icons rasterized with resvg into the shared atlas.

use std::collections::HashMap;

use crate::atlas::{Atlas, PixelRect};
use crate::error::{Error, Result};

pub const ICON_SEARCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6"/><line x1="16.2" y1="16.2" x2="21" y2="21"/></svg>"#;
pub const ICON_FOLDER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="black" d="M3 6.5A2.5 2.5 0 0 1 5.5 4H9l2 2h7.5A2.5 2.5 0 0 1 21 8.5v9A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5v-11z"/></svg>"#;
pub const ICON_GEAR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 3v2.2M12 18.8V21M3 12h2.2M18.8 12H21M5.6 5.6l1.6 1.6M16.8 16.8l1.6 1.6M18.4 5.6l-1.6 1.6M7.2 16.8l-1.6 1.6"/></svg>"#;
pub const ICON_SIDEBAR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><line x1="9" y1="4" x2="9" y2="20"/></svg>"#;
pub const ICON_CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.2 4.2L19 7.5"/></svg>"#;
pub const ICON_SUN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2.2M12 19.3V21.5M2.5 12h2.2M19.3 12H21.5M5 5l1.6 1.6M17.4 17.4L19 19M19 5l-1.6 1.6M6.6 17.4L5 19"/></svg>"#;
pub const ICON_MOON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="black"><path d="M15.5 2.5a8.8 8.8 0 1 0 6 14.2A8 8 0 0 1 15.5 2.5z"/></svg>"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    hash: u64,
    px: u32,
}

#[derive(Clone, Copy, Debug)]
struct IconEntry {
    rect: PixelRect,
}

#[derive(Default)]
pub struct IconCache {
    entries: HashMap<Key, IconEntry>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prepare(&mut self, atlas: &mut Atlas, svg: &str, px: u32) -> Result<[f32; 4]> {
        let px = px.clamp(8, 256);
        let hash = hash_svg(svg);
        let key = Key { hash, px };
        if let Some(entry) = self.entries.get(&key) {
            return Ok(entry.rect.to_array());
        }
        let rect = rasterize(atlas, svg, px)?;
        self.entries.insert(key, IconEntry { rect });
        Ok(rect.to_array())
    }
}

pub fn rasterize(atlas: &mut Atlas, svg: &str, px: u32) -> Result<PixelRect> {
    let options = resvg::usvg::Options::default();
    let tree =
        resvg::usvg::Tree::from_str(svg, &options).map_err(|err| Error::Icon(err.to_string()))?;
    let size = tree.size();
    let sx = px as f32 / size.width();
    let sy = px as f32 / size.height();
    let mut pixmap = tiny_skia::Pixmap::new(px, px)
        .ok_or_else(|| Error::Icon("pixmap allocation failed".into()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    let src = pixmap.data();
    let mut rgba = vec![0u8; px as usize * px as usize * 4];
    for i in 0..(px as usize * px as usize) {
        let a = src[i * 4 + 3];
        let o = i * 4;
        rgba[o] = a;
        rgba[o + 1] = a;
        rgba[o + 2] = a;
        rgba[o + 3] = a;
    }
    atlas
        .insert(px, px, &rgba)
        .ok_or_else(|| Error::Icon("icon did not fit in the atlas".into()))
}

fn hash_svg(svg: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in svg.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_icon_rasterizes_opaque_pixels() {
        let mut atlas = Atlas::new(128);
        let rect = rasterize(&mut atlas, ICON_SEARCH, 24).expect("svg");
        assert_eq!(rect.w, 24);
        let mut opaque = 0u32;
        for y in 0..rect.h {
            for x in 0..rect.w {
                let i = ((rect.y + y) as usize * atlas.size() as usize + (rect.x + x) as usize) * 4;
                if atlas.pixels()[i + 3] > 20 {
                    opaque += 1;
                }
            }
        }
        assert!(opaque > 10, "opaque {opaque}");
    }

    #[test]
    fn broken_svg_is_an_error() {
        let mut atlas = Atlas::new(64);
        let err = rasterize(&mut atlas, "<svg>", 16);
        assert!(err.is_err());
    }
}
