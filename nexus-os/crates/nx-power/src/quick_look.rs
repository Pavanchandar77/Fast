use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Status of the Quick Look preview overlay session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuickLookStatus {
    Closed,
    Opening { path: PathBuf },
    Active,
    Pinned,
}

/// Content payload displayed in the Quick Look session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QuickLookContent {
    Text {
        text: String,
        line_count: usize,
        truncated: bool,
        encoding: String,
    },
    Code {
        language: String,
        lines: Vec<String>,
        line_count: usize,
        truncated: bool,
    },
    ImageMetadata {
        format: String,
        width: u32,
        height: u32,
        aspect_ratio: f32,
        file_size: u64,
    },
    Directory {
        entry_count: usize,
        sample: Vec<String>,
    },
    Binary {
        kind: String,
        size: u64,
    },
    Empty,
}

impl QuickLookContent {
    /// Seamlessly converts from nx-fs preview to QuickLookContent.
    pub fn from_fs_preview(preview: &nx_fs::preview::Preview, _file_size: u64) -> Self {
        match preview {
            nx_fs::preview::Preview::Text {
                text,
                lines,
                truncated,
                encoding,
            } => Self::Text {
                text: text.clone(),
                line_count: *lines,
                truncated: *truncated,
                encoding: (*encoding).to_string(),
            },
            nx_fs::preview::Preview::Code {
                language,
                lines,
                truncated,
            } => {
                let code_lines = lines.iter().map(|l| l.text.clone()).collect();
                Self::Code {
                    language: (*language).to_string(),
                    lines: code_lines,
                    line_count: lines.len(),
                    truncated: *truncated,
                }
            }
            nx_fs::preview::Preview::Image { info, size, .. } => {
                let aspect = if info.height > 0 {
                    (info.width as f32) / (info.height as f32)
                } else {
                    1.0
                };
                Self::ImageMetadata {
                    format: format!("{:?}", info.format),
                    width: info.width,
                    height: info.height,
                    aspect_ratio: aspect,
                    file_size: *size,
                }
            }
            nx_fs::preview::Preview::Folder {
                entries, sample, ..
            } => Self::Directory {
                entry_count: *entries,
                sample: sample.clone(),
            },
            nx_fs::preview::Preview::Binary { size, kind } => Self::Binary {
                kind: (*kind).to_string(),
                size: *size,
            },
            nx_fs::preview::Preview::Thumbnail { thumbnail, size } => Self::ImageMetadata {
                format: "Thumbnail".to_string(),
                width: thumbnail.width,
                height: thumbnail.height,
                aspect_ratio: if thumbnail.height > 0 {
                    (thumbnail.width as f32) / (thumbnail.height as f32)
                } else {
                    1.0
                },
                file_size: *size,
            },
            nx_fs::preview::Preview::Placeholder { size } => Self::Binary {
                kind: "Cloud Placeholder".to_string(),
                size: *size,
            },
        }
    }
}

/// State machine for a Quick Look preview session (macOS-style Spacebar preview).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickLookSession {
    status: QuickLookStatus,
    current_path: Option<PathBuf>,
    content: QuickLookContent,
    playlist: Vec<PathBuf>,
    current_index: Option<usize>,
    zoom_level: f32,
    scroll_offset: usize,
    pinned: bool,
}

impl QuickLookSession {
    pub const MIN_ZOOM: f32 = 0.25;
    pub const MAX_ZOOM: f32 = 4.0;
    pub const ZOOM_STEP: f32 = 0.25;

    pub fn new() -> Self {
        Self {
            status: QuickLookStatus::Closed,
            current_path: None,
            content: QuickLookContent::Empty,
            playlist: Vec::new(),
            current_index: None,
            zoom_level: 1.0,
            scroll_offset: 0,
            pinned: false,
        }
    }

    pub fn status(&self) -> &QuickLookStatus {
        &self.status
    }

    pub fn is_open(&self) -> bool {
        matches!(
            self.status,
            QuickLookStatus::Active | QuickLookStatus::Opening { .. } | QuickLookStatus::Pinned
        )
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    pub fn content(&self) -> &QuickLookContent {
        &self.content
    }

    pub fn zoom_level(&self) -> f32 {
        self.zoom_level
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Open or update preview for a path within a playlist of navigable sibling files.
    pub fn open(&mut self, path: PathBuf, playlist: Vec<PathBuf>) {
        let index = playlist.iter().position(|p| p == &path);
        self.current_path = Some(path.clone());
        self.playlist = playlist;
        self.current_index = index;
        self.zoom_level = 1.0;
        self.scroll_offset = 0;
        self.content = QuickLookContent::Empty;
        self.status = if self.pinned {
            QuickLookStatus::Pinned
        } else {
            QuickLookStatus::Opening { path }
        };
    }

    /// Toggles preview: closes if already open on same path, opens otherwise.
    pub fn toggle(&mut self, path: PathBuf, playlist: Vec<PathBuf>) {
        if self.is_open() && self.current_path.as_ref() == Some(&path) {
            self.close();
        } else {
            self.open(path, playlist);
        }
    }

    /// Closes the Quick Look session (unless pinned).
    pub fn close(&mut self) {
        if !self.pinned {
            self.status = QuickLookStatus::Closed;
            self.current_path = None;
            self.content = QuickLookContent::Empty;
            self.current_index = None;
        }
    }

    /// Force-closes even if pinned.
    pub fn force_close(&mut self) {
        self.pinned = false;
        self.status = QuickLookStatus::Closed;
        self.current_path = None;
        self.content = QuickLookContent::Empty;
        self.current_index = None;
    }

    /// Populates content once loaded from disk or worker thread.
    pub fn set_content(&mut self, content: QuickLookContent) {
        self.content = content;
        if self.is_open() {
            self.status = if self.pinned {
                QuickLookStatus::Pinned
            } else {
                QuickLookStatus::Active
            };
        }
    }

    /// Navigates to the next file in the playlist.
    pub fn next_file(&mut self) -> Option<&Path> {
        if self.playlist.is_empty() {
            return None;
        }
        let next_idx = match self.current_index {
            Some(i) => (i + 1) % self.playlist.len(),
            None => 0,
        };
        self.current_index = Some(next_idx);
        let path = self.playlist[next_idx].clone();
        self.current_path = Some(path.clone());
        self.content = QuickLookContent::Empty;
        self.zoom_level = 1.0;
        self.scroll_offset = 0;
        self.status = if self.pinned {
            QuickLookStatus::Pinned
        } else {
            QuickLookStatus::Opening { path }
        };
        self.current_path.as_deref()
    }

    /// Navigates to the previous file in the playlist.
    pub fn prev_file(&mut self) -> Option<&Path> {
        if self.playlist.is_empty() {
            return None;
        }
        let prev_idx = match self.current_index {
            Some(0) | None => self.playlist.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.current_index = Some(prev_idx);
        let path = self.playlist[prev_idx].clone();
        self.current_path = Some(path.clone());
        self.content = QuickLookContent::Empty;
        self.zoom_level = 1.0;
        self.scroll_offset = 0;
        self.status = if self.pinned {
            QuickLookStatus::Pinned
        } else {
            QuickLookStatus::Opening { path }
        };
        self.current_path.as_deref()
    }

    pub fn toggle_pinned(&mut self) {
        self.pinned = !self.pinned;
        if self.is_open() {
            self.status = if self.pinned {
                QuickLookStatus::Pinned
            } else {
                QuickLookStatus::Active
            };
        }
    }

    pub fn zoom_in(&mut self) {
        self.zoom_level = (self.zoom_level + Self::ZOOM_STEP).min(Self::MAX_ZOOM);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_level = (self.zoom_level - Self::ZOOM_STEP).max(Self::MIN_ZOOM);
    }

    pub fn reset_zoom(&mut self) {
        self.zoom_level = 1.0;
    }

    pub fn scroll_to(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }
}

impl Default for QuickLookSession {
    fn default() -> Self {
        Self::new()
    }
}
