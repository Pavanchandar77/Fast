//! Quick Look: instant previews for the selected / visible files.
//!
//! [`generate`] builds a preview from at most a few KiB of the file (text,
//! code with highlighting, image header info, binary kind), and never opens
//! cloud placeholders (that would silently download them). [`pipeline`]
//! runs generation on workers with visible-first priority, cancellation and
//! a byte-budgeted LRU cache. OS thumbnails come from a
//! [`ThumbnailProvider`] (`IThumbnailCache` on Windows).

pub mod cache;
pub mod code;
pub mod image;
pub mod pipeline;

use crate::entry::Attributes;
use crate::error::{FsError, FsResult, IoContext};
use crate::path::to_native;
use std::io::Read;
use std::path::Path;

pub use pipeline::{PreviewEvent, PreviewPipeline, Priority, Ticket};

/// Decoded thumbnail pixels (top-down RGBA8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One highlighted line of a code preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLine {
    pub text: String,
    pub spans: Vec<code::Span>,
}

/// A generated preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Text {
        text: String,
        lines: usize,
        truncated: bool,
        encoding: &'static str,
    },
    Code {
        language: &'static str,
        lines: Vec<CodeLine>,
        truncated: bool,
    },
    Image {
        info: image::ImageInfo,
        size: u64,
        thumbnail: Option<Thumbnail>,
    },
    Thumbnail {
        thumbnail: Thumbnail,
        size: u64,
    },
    Folder {
        entries: usize,
        more: bool,
        sample: Vec<String>,
    },
    Binary {
        size: u64,
        kind: &'static str,
    },
    /// Online-only cloud file: shown without downloading it.
    Placeholder {
        size: u64,
    },
}

impl Preview {
    /// Approximate memory footprint for cache accounting.
    pub fn approx_bytes(&self) -> usize {
        let base = std::mem::size_of::<Preview>();
        base + match self {
            Preview::Text { text, .. } => text.len(),
            Preview::Code { lines, .. } => lines
                .iter()
                .map(|l| l.text.len() + l.spans.len() * 24 + 48)
                .sum(),
            Preview::Image { thumbnail, .. } => thumbnail.as_ref().map_or(0, |t| t.rgba.len()),
            Preview::Thumbnail { thumbnail, .. } => thumbnail.rgba.len(),
            Preview::Folder { sample, .. } => sample.iter().map(|s| s.len() + 24).sum(),
            Preview::Binary { .. } | Preview::Placeholder { .. } => 0,
        }
    }
}

/// OS thumbnail source.
pub trait ThumbnailProvider: Send + Sync {
    /// A thumbnail at most `size` px on the long edge, or `None` if the
    /// shell has no thumbnail for this file type.
    fn thumbnail(&self, path: &Path, size: u32) -> FsResult<Option<Thumbnail>>;
}

/// Generation limits.
#[derive(Debug, Clone, Copy)]
pub struct PreviewOptions {
    /// Bytes read for text/code previews.
    pub max_text_bytes: usize,
    /// Lines kept for code previews.
    pub max_lines: usize,
    /// Thumbnail edge in px.
    pub thumb_size: u32,
    /// Folder previews count at most this many entries.
    pub max_folder_entries: usize,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        PreviewOptions {
            max_text_bytes: 64 * 1024,
            max_lines: 400,
            thumb_size: 256,
            max_folder_entries: 10_000,
        }
    }
}

/// Classify well-known binary formats by magic bytes.
pub fn binary_kind(head: &[u8]) -> &'static str {
    let at = |off: usize, m: &[u8]| head.get(off..off + m.len()) == Some(m);
    if at(0, b"%PDF") {
        "PDF document"
    } else if at(0, b"PK\x03\x04") {
        "ZIP archive / Office document"
    } else if at(0, b"MZ") {
        "Windows executable"
    } else if at(0, b"\x7fELF") {
        "ELF executable"
    } else if at(0, b"ID3") || at(0, &[0xff, 0xfb]) {
        "MP3 audio"
    } else if at(0, b"fLaC") {
        "FLAC audio"
    } else if at(0, b"OggS") {
        "Ogg media"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "WAV audio"
    } else if at(4, b"ftyp") {
        "MP4 / QuickTime video"
    } else if at(0, &[0x1a, 0x45, 0xdf, 0xa3]) {
        "Matroska video"
    } else if at(0, b"7z\xbc\xaf\x27\x1c") {
        "7-Zip archive"
    } else if at(0, b"Rar!") {
        "RAR archive"
    } else if at(0, &[0x1f, 0x8b]) {
        "gzip archive"
    } else if at(0, b"SQLite format 3\0") {
        "SQLite database"
    } else {
        "Binary file"
    }
}

/// Decode bytes as text if they look like text.
fn decode_text(bytes: &[u8]) -> Option<(String, &'static str)> {
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units = crate::index::usn::le_u16s(rest);
        return Some((String::from_utf16_lossy(&units), "UTF-16LE"));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = crate::index::usn::be_u16s(rest);
        return Some((String::from_utf16_lossy(&units), "UTF-16BE"));
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some((s.to_string(), "UTF-8")),
        // A multi-byte char cut by our read limit is fine.
        Err(e) if e.error_len().is_none() => Some((
            String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned(),
            "UTF-8",
        )),
        Err(_) => {
            // Mostly-printable legacy text (Windows-1252 etc.): show lossily.
            let printable = bytes
                .iter()
                .filter(|&&b| b >= 0x20 || matches!(b, b'\n' | b'\r' | b'\t'))
                .count();
            (printable * 100 >= bytes.len() * 95)
                .then(|| (String::from_utf8_lossy(bytes).into_owned(), "8-bit"))
        }
    }
}

/// Build a preview for `path` (blocking; call from a worker).
pub fn generate(
    path: &Path,
    opts: &PreviewOptions,
    thumbs: Option<&dyn ThumbnailProvider>,
) -> FsResult<Preview> {
    let md = std::fs::symlink_metadata(to_native(path)).at(path)?;
    if md.is_dir() {
        return folder_preview(path, opts);
    }
    let size = md.len();
    if is_placeholder(&md) {
        return Ok(Preview::Placeholder { size });
    }
    let mut head = Vec::with_capacity(opts.max_text_bytes.min(size as usize + 1));
    std::fs::File::open(to_native(path))
        .at(path)?
        .take(opts.max_text_bytes as u64)
        .read_to_end(&mut head)
        .at(path)?;
    if let Some(info) = image::sniff(&head) {
        let thumbnail = match thumbs {
            Some(t) => t.thumbnail(path, opts.thumb_size).ok().flatten(),
            None => None,
        };
        return Ok(Preview::Image {
            info,
            size,
            thumbnail,
        });
    }
    let truncated = (head.len() as u64) < size;
    if let Some((text, encoding)) = decode_text(&head) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(lang) = code::language_for(&ext) {
            let lines: Vec<CodeLine> = text
                .lines()
                .take(opts.max_lines)
                .map(|l| CodeLine {
                    spans: code::highlight_line(l, &lang),
                    text: l.to_string(),
                })
                .collect();
            let cut = truncated || text.lines().count() > opts.max_lines;
            return Ok(Preview::Code {
                language: lang.name,
                lines,
                truncated: cut,
            });
        }
        let lines = text.lines().count();
        return Ok(Preview::Text {
            text,
            lines,
            truncated,
            encoding,
        });
    }
    if let Some(t) = thumbs {
        if let Ok(Some(thumbnail)) = t.thumbnail(path, opts.thumb_size) {
            return Ok(Preview::Thumbnail { thumbnail, size });
        }
    }
    Ok(Preview::Binary {
        size,
        kind: binary_kind(&head),
    })
}

fn folder_preview(path: &Path, opts: &PreviewOptions) -> FsResult<Preview> {
    let rd = std::fs::read_dir(to_native(path)).at(path)?;
    let mut entries = 0usize;
    let mut sample = Vec::new();
    let mut more = false;
    for e in rd.flatten() {
        if entries >= opts.max_folder_entries {
            more = true;
            break;
        }
        entries += 1;
        if sample.len() < 12 {
            sample.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    sample.sort_by(|a, b| crate::sort::natural_cmp(a, b));
    Ok(Preview::Folder {
        entries,
        more,
        sample,
    })
}

#[cfg(windows)]
fn is_placeholder(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    Attributes(md.file_attributes()).is_cloud_placeholder()
}

#[cfg(not(windows))]
fn is_placeholder(_md: &std::fs::Metadata) -> bool {
    let _ = Attributes::default();
    false
}

/// Error used when a request was cancelled before completion.
pub(crate) fn cancelled() -> FsError {
    FsError::Cancelled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).expect("write");
        p
    }

    #[test]
    fn previews_by_content() {
        let t = tempfile::tempdir().expect("tempdir");
        let opts = PreviewOptions::default();
        let txt = write(t.path(), "a.txt", b"hello\nworld\n");
        assert!(matches!(
            generate(&txt, &opts, None).expect("txt"),
            Preview::Text {
                lines: 2,
                truncated: false,
                ..
            }
        ));
        let rs = write(t.path(), "m.rs", b"fn main() {}\n");
        match generate(&rs, &opts, None).expect("rs") {
            Preview::Code {
                language, lines, ..
            } => {
                assert_eq!(language, "Rust");
                assert_eq!(lines[0].spans[0].kind, code::TokenKind::Keyword);
            }
            other => panic!("unexpected {other:?}"),
        }
        let gif = write(t.path(), "x.bin", b"GIF89a\x02\x00\x03\x00rest");
        assert!(matches!(
            generate(&gif, &opts, None).expect("gif"),
            Preview::Image { .. }
        ));
        let pdf = write(t.path(), "d.pdf", b"%PDF-1.7\x00\x01\x02");
        assert_eq!(
            generate(&pdf, &opts, None).expect("pdf"),
            Preview::Binary {
                size: 11,
                kind: "PDF document"
            }
        );
        let u16 = write(t.path(), "w.txt", &[0xff, 0xfe, b'h', 0, b'i', 0]);
        assert!(matches!(
            generate(&u16, &opts, None).expect("u16"),
            Preview::Text {
                encoding: "UTF-16LE",
                ..
            }
        ));
        std::fs::create_dir(t.path().join("sub")).expect("mkdir");
        assert!(matches!(
            generate(t.path(), &opts, None).expect("dir"),
            Preview::Folder { entries: 6, .. }
        ));
        assert!(matches!(
            generate(&t.path().join("nope"), &opts, None),
            Err(FsError::NotFound { .. })
        ));
    }

    #[test]
    fn truncates_large_text() {
        let t = tempfile::tempdir().expect("tempdir");
        let big = "é line\n".repeat(20_000);
        let p = write(t.path(), "big.log", big.as_bytes());
        let opts = PreviewOptions {
            max_text_bytes: 1001,
            ..Default::default()
        };
        match generate(&p, &opts, None).expect("big") {
            Preview::Text {
                truncated, text, ..
            } => {
                assert!(truncated);
                assert!(text.len() <= 1001);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
