//! Image header sniffing: format and dimensions without decoding pixels.

/// Recognised image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    WebP,
    Ico,
}

impl ImageFormat {
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Ico => "image/x-icon",
        }
    }
}

/// Format and pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Sniff the first bytes of a file. Returns `None` if not a known image or
/// the header is truncated/corrupt.
pub fn sniff(b: &[u8]) -> Option<ImageInfo> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16) == Some(b"IHDR") {
        return Some(ImageInfo {
            format: ImageFormat::Png,
            width: be32(b, 16)?,
            height: be32(b, 20)?,
        });
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some(ImageInfo {
            format: ImageFormat::Gif,
            width: le16(b, 6)?,
            height: le16(b, 8)?,
        });
    }
    if b.starts_with(b"BM") {
        let w = le32(b, 18)? as i32;
        let h = le32(b, 22)? as i32;
        return Some(ImageInfo {
            format: ImageFormat::Bmp,
            width: w.unsigned_abs(),
            height: h.unsigned_abs(),
        });
    }
    if b.starts_with(&[0, 0, 1, 0]) {
        let w = u32::from(*b.get(6)?);
        let h = u32::from(*b.get(7)?);
        return Some(ImageInfo {
            format: ImageFormat::Ico,
            width: if w == 0 { 256 } else { w },
            height: if h == 0 { 256 } else { h },
        });
    }
    if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return webp(b);
    }
    if b.starts_with(&[0xff, 0xd8]) {
        return jpeg(b);
    }
    None
}

fn webp(b: &[u8]) -> Option<ImageInfo> {
    let (w, h) = match b.get(12..16)? {
        b"VP8 " => (le16(b, 26)? & 0x3fff, le16(b, 28)? & 0x3fff),
        b"VP8L" => {
            let bits = le32(b, 21)?;
            ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
        }
        b"VP8X" => {
            let w = u32::from_le_bytes([*b.get(24)?, *b.get(25)?, *b.get(26)?, 0]) + 1;
            let h = u32::from_le_bytes([*b.get(27)?, *b.get(28)?, *b.get(29)?, 0]) + 1;
            (w, h)
        }
        _ => return None,
    };
    Some(ImageInfo {
        format: ImageFormat::WebP,
        width: w,
        height: h,
    })
}

fn jpeg(b: &[u8]) -> Option<ImageInfo> {
    let mut i = 2usize;
    // Walk segments until a start-of-frame marker.
    for _ in 0..1024 {
        while *b.get(i)? == 0xff && *b.get(i + 1)? == 0xff {
            i += 1;
        }
        if *b.get(i)? != 0xff {
            return None;
        }
        let marker = *b.get(i + 1)?;
        let len = be16(b, i + 2)? as usize;
        let is_sof = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if is_sof {
            return Some(ImageInfo {
                format: ImageFormat::Jpeg,
                height: be16(b, i + 5)?,
                width: be16(b, i + 7)?,
            });
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn sniffs_formats() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(
            sniff(&png),
            Some(ImageInfo {
                format: ImageFormat::Png,
                width: 640,
                height: 480
            })
        );
        let gif = b"GIF89a\x20\x00\x10\x00";
        assert_eq!(sniff(gif).map(|i| (i.width, i.height)), Some((32, 16)));
        // JPEG: SOI, APP0 (len 16), SOF0 with 100x200.
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xc8, 0x00, 0x64]);
        assert_eq!(
            sniff(&jpg),
            Some(ImageInfo {
                format: ImageFormat::Jpeg,
                width: 100,
                height: 200
            })
        );
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend_from_slice(&7i32.to_le_bytes());
        bmp.extend_from_slice(&(-9i32).to_le_bytes());
        assert_eq!(sniff(&bmp).map(|i| (i.width, i.height)), Some((7, 9)));
        assert_eq!(sniff(b"hello world"), None);
        assert_eq!(sniff(&png[..18]), None, "truncated");
    }

    proptest! {
        #[test]
        fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..64), prefix in 0usize..4) {
            let heads: [&[u8]; 4] = [b"\x89PNG\r\n\x1a\n", b"\xff\xd8", b"RIFF\0\0\0\0WEBPVP8X", b"BM"];
            let mut b = heads[prefix].to_vec();
            b.extend(bytes);
            let _ = sniff(&b);
        }
    }
}
