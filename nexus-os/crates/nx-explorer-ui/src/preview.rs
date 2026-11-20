//! Quick Look-style rich preview pane with spring slide-in animation.

use nx_fs::DirEntry;
use nx_ui::spring::{Spring, SpringConfig};
use std::path::PathBuf;

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

pub struct PreviewPane {
    open: bool,
    width_spring: Spring,
    current_entry: Option<DirEntry>,
    current_path: Option<PathBuf>,
    text_snippet: Option<String>,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPane {
    pub const TARGET_WIDTH: f32 = 280.0;

    pub fn new() -> Self {
        Self {
            open: false,
            width_spring: Spring::new(0.0, cfg(0.24)),
            current_entry: None,
            current_path: None,
            text_snippet: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn width(&self) -> f32 {
        self.width_spring.position
    }

    pub fn animating(&self) -> bool {
        !self.width_spring.settled()
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.width_spring
            .set_target(if self.open { Self::TARGET_WIDTH } else { 0.0 });
    }

    pub fn set_open(&mut self, open: bool) {
        if self.open != open {
            self.open = open;
            self.width_spring
                .set_target(if self.open { Self::TARGET_WIDTH } else { 0.0 });
        }
    }

    pub fn set_entry(&mut self, entry: Option<DirEntry>, path: Option<PathBuf>) {
        self.current_entry = entry;
        self.current_path = path;
    }

    pub fn set_text_snippet(&mut self, snippet: Option<String>) {
        self.text_snippet = snippet;
    }

    pub fn update(&mut self, dt: f32) {
        self.width_spring.step(dt);
    }
}
