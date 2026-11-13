use std::path::{Path, PathBuf};
use tracing::info;

pub enum PreviewContent {
    Text(String),
    Image(PathBuf),
    Binary { size: u64, mime: String },
}

pub struct QuickLookViewer {
    pub active_path: Option<PathBuf>,
}

impl QuickLookViewer {
    pub fn new() -> Self {
        Self { active_path: None }
    }

    pub fn inspect_file<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<PreviewContent> {
        let p = path.as_ref();
        info!("QuickLook: Rendering preview for {:?}", p);
        if let Ok(metadata) = std::fs::metadata(p) {
            if metadata.len() < 1024 * 1024 {
                if let Ok(text) = std::fs::read_to_string(p) {
                    return Ok(PreviewContent::Text(text));
                }
            }
            Ok(PreviewContent::Binary {
                size: metadata.len(),
                mime: "application/octet-stream".into(),
            })
        } else {
            anyhow::bail!("Cannot read file metadata")
        }
    }
}
