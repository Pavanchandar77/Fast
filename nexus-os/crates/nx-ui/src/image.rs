//! RGBA images and PNG decode. Pixels are straight (not premultiplied) sRGB.

use crate::atlas::{Atlas, PixelRect};
use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = width as usize * height as usize * 4;
        if width == 0 || height == 0 || pixels.len() < expected {
            return Err(Error::Image(
                "rgba buffer does not match the declared size".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            pixels.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    /// A small gradient used by the gallery so the image primitive is visible without a file.
    pub fn gradient(width: u32, height: u32) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                let u = if width <= 1 {
                    0
                } else {
                    (x * 255) / (width - 1)
                };
                let v = if height <= 1 {
                    0
                } else {
                    (y * 255) / (height - 1)
                };
                pixels.extend_from_slice(&[u as u8, 90, v as u8, 255]);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn decode_png(bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder
            .read_info()
            .map_err(|err| Error::Image(err.to_string()))?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|err| Error::Image(err.to_string()))?;
        let width = info.width;
        let height = info.height;
        let color = info.color_type;
        let samples = match color {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Grayscale => 1,
            other => {
                return Err(Error::Image(format!(
                    "unsupported png color type {other:?}"
                )))
            }
        };
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        let row_stride = info.line_size;
        for y in 0..height as usize {
            let row = &buf[y * row_stride..];
            for x in 0..width as usize {
                let s = &row[x * samples..];
                let (r, g, b, a) = match samples {
                    4 => (s[0], s[1], s[2], s[3]),
                    3 => (s[0], s[1], s[2], 255),
                    2 => (s[0], s[0], s[0], s[1]),
                    _ => (s[0], s[0], s[0], 255),
                };
                pixels.extend_from_slice(&[r, g, b, a]);
            }
        }
        Self::from_rgba(width, height, pixels)
    }

    /// Upload into the atlas as premultiplied RGBA so the sprite shader can tint with white.
    pub fn upload(&self, atlas: &mut Atlas) -> Result<PixelRect> {
        let mut premul = vec![0u8; self.width as usize * self.height as usize * 4];
        for i in 0..(self.width as usize * self.height as usize) {
            let o = i * 4;
            let a = self.pixels[o + 3] as u16;
            premul[o] = ((self.pixels[o] as u16 * a) / 255) as u8;
            premul[o + 1] = ((self.pixels[o + 1] as u16 * a) / 255) as u8;
            premul[o + 2] = ((self.pixels[o + 2] as u16 * a) / 255) as u8;
            premul[o + 3] = self.pixels[o + 3];
        }
        atlas
            .insert(self.width, self.height, &premul)
            .ok_or_else(|| Error::Image("image did not fit in the atlas".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip_of_a_2x2() {
        // 2x2 RGBA: red, green / blue, white.
        const PNG: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2,
            8, 6, 0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 240, 31, 12, 129, 52, 24, 0, 0, 73, 200, 9, 247, 249, 171, 182, 13, 0, 0, 0, 0,
            73, 69, 78, 68, 174, 66, 96, 130,
        ];
        let image = RgbaImage::decode_png(PNG).expect("png");
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 2);
        assert_eq!(&image.pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&image.pixels[4..8], &[0, 255, 0, 255]);
        assert_eq!(&image.pixels[8..12], &[0, 0, 255, 255]);
        assert_eq!(&image.pixels[12..16], &[255, 255, 255, 255]);
    }
}
