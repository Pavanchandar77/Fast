//! Inter text, shaped with cosmic-text.
//!
//! Measurement and rasterization live in the platform-free core. Glyphs are
//! cached by string and style. A steady frame that repeats the same labels
//! does not shape or allocate.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight, Wrap,
};
use fontdb::Database;

use crate::atlas::{Atlas, PixelRect};
use crate::draw::PlacedGlyph;
use crate::error::{Error, Result};
use crate::tokens::Weight as TokenWeight;

const INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold.ttf");

pub const TEXT_NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub line: f32,
    pub weight: TokenWeight,
}

impl TextStyle {
    pub fn new(size: f32, line: f32, weight: TokenWeight) -> Self {
        Self { size, line, weight }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreparedText {
    pub id: u32,
    pub width: f32,
    pub height: f32,
}

struct Entry {
    _hash: u64,
    style_bits: u64,
    text: String,
    width: f32,
    height: f32,
    glyphs: Vec<PlacedGlyph>,
}

pub struct TextEngine {
    font_system: FontSystem,
    swash: SwashCache,
    buffer: Buffer,
    entries: Vec<Entry>,
    index: HashMap<u64, Vec<u32>>,
}

impl TextEngine {
    pub fn new() -> Result<Self> {
        let mut db = Database::new();
        db.load_font_data(INTER_REGULAR.to_vec());
        db.load_font_data(INTER_MEDIUM.to_vec());
        db.load_font_data(INTER_SEMIBOLD.to_vec());
        db.load_font_data(INTER_BOLD.to_vec());
        db.set_sans_serif_family("Inter");
        db.set_serif_family("Inter");
        let found = db.query(&fontdb::Query {
            families: &[fontdb::Family::Name("Inter")],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        });
        if found.is_none() {
            return Err(Error::Font(
                "bundled Inter did not register a family named Inter".into(),
            ));
        }
        let mut font_system = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let buffer = Buffer::new(&mut font_system, Metrics::new(17.0, 22.0));
        Ok(Self {
            font_system,
            swash: SwashCache::new(),
            buffer,
            entries: Vec::new(),
            index: HashMap::new(),
        })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn prepare(&mut self, atlas: &mut Atlas, text: &str, style: TextStyle) -> PreparedText {
        if text.is_empty() || style.size < 1.0 {
            return PreparedText {
                id: TEXT_NONE,
                width: 0.0,
                height: style.line.max(0.0),
            };
        }
        let style_bits = style_key(style);
        let hash = hash_text(text, style_bits);
        if let Some(id) = self.find(text, style_bits, hash) {
            let entry = &self.entries[id];
            return PreparedText {
                id: id as u32,
                width: entry.width,
                height: entry.height,
            };
        }
        let shaped = self.shape(atlas, text, style);
        let id = self.entries.len() as u32;
        self.index.entry(hash).or_default().push(id);
        self.entries.push(Entry {
            _hash: hash,
            style_bits,
            text: text.to_string(),
            width: shaped.0,
            height: shaped.1,
            glyphs: shaped.2,
        });
        PreparedText {
            id,
            width: shaped.0,
            height: shaped.1,
        }
    }

    pub fn measure(&mut self, atlas: &mut Atlas, text: &str, style: TextStyle) -> (f32, f32) {
        let prepared = self.prepare(atlas, text, style);
        (prepared.width, prepared.height)
    }

    pub fn glyphs(&self, id: u32) -> &[PlacedGlyph] {
        self.entries
            .get(id as usize)
            .map(|entry| entry.glyphs.as_slice())
            .unwrap_or(&[])
    }

    fn find(&self, text: &str, style_bits: u64, hash: u64) -> Option<usize> {
        let bucket = self.index.get(&hash)?;
        for id in bucket {
            let entry = &self.entries[*id as usize];
            if entry.style_bits == style_bits && entry.text == text {
                return Some(*id as usize);
            }
        }
        None
    }

    fn shape(
        &mut self,
        atlas: &mut Atlas,
        text: &str,
        style: TextStyle,
    ) -> (f32, f32, Vec<PlacedGlyph>) {
        let metrics = Metrics::new(style.size.max(1.0), style.line.max(style.size));
        self.buffer
            .set_metrics_and_size(&mut self.font_system, metrics, Some(8192.0), None);
        self.buffer.set_wrap(&mut self.font_system, Wrap::None);
        let attrs = Attrs::new()
            .family(Family::Name("Inter"))
            .weight(map_weight(style.weight));
        self.buffer
            .set_text(&mut self.font_system, text, &attrs, Shaping::Basic);
        self.buffer.shape_until_scroll(&mut self.font_system, true);

        let mut pending = Vec::new();
        let mut width = 0.0f32;
        let mut height = style.line.max(style.size);
        for run in self.buffer.layout_runs() {
            width = width.max(run.line_w);
            height = height.max(run.line_top + run.line_height);
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                pending.push((physical.cache_key, physical.x, physical.y));
            }
        }

        let mut glyphs = Vec::with_capacity(pending.len());
        for (cache_key, x, y) in pending {
            let Some(image) = self
                .swash
                .get_image(&mut self.font_system, cache_key)
                .clone()
            else {
                continue;
            };
            if image.placement.width == 0 || image.placement.height == 0 {
                continue;
            }
            let packed = pack_image(atlas, &image);
            let Some(rect) = packed else { continue };
            glyphs.push(PlacedGlyph {
                dx: x as f32 + image.placement.left as f32,
                dy: y as f32 - image.placement.top as f32,
                w: rect.w as f32,
                h: rect.h as f32,
                src_x: rect.x as f32,
                src_y: rect.y as f32,
                src_w: rect.w as f32,
                src_h: rect.h as f32,
            });
        }
        (width, height, glyphs)
    }
}

fn pack_image(atlas: &mut Atlas, image: &cosmic_text::SwashImage) -> Option<PixelRect> {
    let w = image.placement.width;
    let h = image.placement.height;
    match image.content {
        SwashContent::Mask => {
            let mut rgba = vec![0u8; w as usize * h as usize * 4];
            for (i, alpha) in image.data.iter().take(w as usize * h as usize).enumerate() {
                let o = i * 4;
                rgba[o] = *alpha;
                rgba[o + 1] = *alpha;
                rgba[o + 2] = *alpha;
                rgba[o + 3] = *alpha;
            }
            atlas.insert(w, h, &rgba)
        }
        SwashContent::Color => atlas.insert(w, h, &image.data),
        SwashContent::SubpixelMask => None,
    }
}

fn map_weight(weight: TokenWeight) -> Weight {
    match weight {
        TokenWeight::Regular => Weight::NORMAL,
        TokenWeight::Medium => Weight::MEDIUM,
        TokenWeight::Semibold => Weight::SEMIBOLD,
        TokenWeight::Bold => Weight::BOLD,
    }
}

fn style_key(style: TextStyle) -> u64 {
    let size = (style.size * 4.0).round() as u32 as u64;
    let line = (style.line * 4.0).round() as u32 as u64;
    let weight = match style.weight {
        TokenWeight::Regular => 0,
        TokenWeight::Medium => 1,
        TokenWeight::Semibold => 2,
        TokenWeight::Bold => 3,
    };
    size | (line << 21) | (weight << 42)
}

fn hash_text(text: &str, style_bits: u64) -> u64 {
    // FNV-1a. Stable, no allocation, good enough for a UI label cache.
    let mut hash = 0xcbf29ce484222325u64 ^ style_bits;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Weight;

    #[test]
    fn inter_measures_m_wider_than_i() {
        let mut text = TextEngine::new().expect("inter");
        let mut atlas = Atlas::new(256);
        let style = TextStyle::new(17.0, 22.0, Weight::Regular);
        let (m, _) = text.measure(&mut atlas, "M", style);
        let (i, _) = text.measure(&mut atlas, "i", style);
        assert!(m > i, "M {m} i {i}");
        assert!(m > 8.0);
    }

    #[test]
    fn empty_string_has_zero_width() {
        let mut text = TextEngine::new().expect("inter");
        let mut atlas = Atlas::new(64);
        let (w, h) = text.measure(&mut atlas, "", TextStyle::new(17.0, 22.0, Weight::Regular));
        assert_eq!(w, 0.0);
        assert!((h - 22.0).abs() < 1e-3);
        assert_eq!(text.entry_count(), 0);
    }

    #[test]
    fn line_height_covers_the_body_style() {
        let mut text = TextEngine::new().expect("inter");
        let mut atlas = Atlas::new(512);
        let style = TextStyle::new(17.0, 22.0, Weight::Regular);
        let prepared = text.prepare(&mut atlas, "Nexus", style);
        assert!(prepared.height >= 17.0);
        assert!(prepared.width > 20.0);
        let again = text.prepare(&mut atlas, "Nexus", style);
        assert_eq!(prepared.id, again.id);
        assert_eq!(text.entry_count(), 1);
        assert!(!text.glyphs(prepared.id).is_empty());
    }
}
