//! Shelf atlas for glyphs, icons, and small images.
//!
//! Packing only allocates when the atlas grows. A steady frame that reuses
//! glyphs does not touch the pixel buffer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PixelRect {
    pub fn to_array(self) -> [f32; 4] {
        [self.x as f32, self.y as f32, self.w as f32, self.h as f32]
    }
}

pub struct Atlas {
    size: u32,
    pixels: Vec<u8>,
    x: u32,
    y: u32,
    row_h: u32,
    dirty: bool,
    pub generation: u64,
}

impl Atlas {
    pub fn new(size: u32) -> Self {
        let size = size.max(64).next_power_of_two().min(4096);
        Self {
            size,
            pixels: vec![0; size as usize * size as usize * 4],
            x: 1,
            y: 1,
            row_h: 0,
            dirty: false,
            generation: 1,
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn capacity_pixels(&self) -> usize {
        self.pixels.capacity()
    }

    /// Copy a tightly packed RGBA8 image into the atlas. Returns `None` when it cannot fit.
    pub fn insert(&mut self, width: u32, height: u32, rgba: &[u8]) -> Option<PixelRect> {
        if width == 0 || height == 0 {
            return Some(PixelRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            });
        }
        if rgba.len() < width as usize * height as usize * 4 {
            return None;
        }
        if width + 2 > self.size || height + 2 > self.size {
            return None;
        }
        if !self.reserve(width, height) {
            return None;
        }
        let rect = PixelRect {
            x: self.x,
            y: self.y,
            w: width,
            h: height,
        };
        self.blit(rect, rgba);
        self.x += width + 1;
        self.row_h = self.row_h.max(height);
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
        Some(rect)
    }

    fn reserve(&mut self, width: u32, height: u32) -> bool {
        if self.fits(width, height) {
            return true;
        }
        // New shelf.
        let next_y = self.y + self.row_h + 1;
        if next_y + height < self.size {
            self.x = 1;
            self.y = next_y;
            self.row_h = 0;
            if self.fits(width, height) {
                return true;
            }
        }
        while self.size < 4096 {
            if !self.grow() {
                return false;
            }
            if self.fits(width, height) {
                return true;
            }
            let next_y = self.y + self.row_h + 1;
            if next_y + height < self.size {
                self.x = 1;
                self.y = next_y;
                self.row_h = 0;
                if self.fits(width, height) {
                    return true;
                }
            }
        }
        false
    }

    fn fits(&self, width: u32, height: u32) -> bool {
        self.x + width < self.size && self.y + height < self.size
    }

    fn grow(&mut self) -> bool {
        if self.size >= 4096 {
            return false;
        }
        let next = (self.size * 2).min(4096);
        let mut pixels = vec![0u8; next as usize * next as usize * 4];
        for row in 0..self.size {
            let src = row as usize * self.size as usize * 4;
            let dst = row as usize * next as usize * 4;
            let len = self.size as usize * 4;
            pixels[dst..dst + len].copy_from_slice(&self.pixels[src..src + len]);
        }
        self.pixels = pixels;
        self.size = next;
        self.dirty = true;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    fn blit(&mut self, rect: PixelRect, rgba: &[u8]) {
        for row in 0..rect.h {
            let src = row as usize * rect.w as usize * 4;
            let dst = ((rect.y + row) as usize * self.size as usize + rect.x as usize) * 4;
            let len = rect.w as usize * 4;
            self.pixels[dst..dst + len].copy_from_slice(&rgba[src..src + len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_read_back_pixel() {
        let mut atlas = Atlas::new(64);
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let rect = atlas.insert(2, 2, &rgba).expect("fit");
        assert!(atlas.is_dirty());
        let i = ((rect.y as usize * atlas.size() as usize) + rect.x as usize) * 4;
        assert_eq!(&atlas.pixels()[i..i + 4], &[255, 0, 0, 255]);
    }
}
