//! Translucent sidebar with Favorites, Locations (drives), and Tags.

use crate::icons::{ICON_DRIVE, ICON_FOLDER, ICON_HOME, ICON_TAG};
use nx_ui::color::Color;
use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::spring::{Spring, SpringConfig};
use std::path::PathBuf;

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

#[derive(Debug, Clone)]
pub struct SidebarEntry {
    pub label: String,
    pub path: PathBuf,
    pub icon: &'static str,
    pub tag_color: Option<Color>,
}

#[derive(Debug, Clone)]
pub struct SidebarSection {
    pub title: &'static str,
    pub entries: Vec<SidebarEntry>,
}

pub struct ExplorerSidebar {
    sections: Vec<SidebarSection>,
    selected_path: Option<PathBuf>,
    collapsed: bool,
    width: Spring,
    hover_item: Option<(usize, usize)>,
}

impl Default for ExplorerSidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl ExplorerSidebar {
    pub const EXPANDED: f32 = 210.0;
    pub const COLLAPSED: f32 = 64.0;
    pub const ITEM_H: f32 = 30.0;
    pub const HEADER_H: f32 = 24.0;

    pub fn new() -> Self {
        let mut sidebar = Self {
            sections: Vec::new(),
            selected_path: None,
            collapsed: false,
            width: Spring::new(Self::EXPANDED, cfg(0.34)),
            hover_item: None,
        };
        sidebar.populate_defaults();
        sidebar
    }

    pub fn populate_defaults(&mut self) {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| "/workspace".into());
        let home_path = PathBuf::from(&home);

        // 1. Favorites
        let favorites = vec![
            SidebarEntry {
                label: "Home".into(),
                path: home_path.clone(),
                icon: ICON_HOME,
                tag_color: None,
            },
            SidebarEntry {
                label: "Desktop".into(),
                path: home_path.join("Desktop"),
                icon: ICON_FOLDER,
                tag_color: None,
            },
            SidebarEntry {
                label: "Documents".into(),
                path: home_path.join("Documents"),
                icon: ICON_FOLDER,
                tag_color: None,
            },
            SidebarEntry {
                label: "Downloads".into(),
                path: home_path.join("Downloads"),
                icon: ICON_FOLDER,
                tag_color: None,
            },
            SidebarEntry {
                label: "Pictures".into(),
                path: home_path.join("Pictures"),
                icon: ICON_FOLDER,
                tag_color: None,
            },
        ];

        // 2. Locations / Drives
        let locations = vec![
            SidebarEntry {
                label: "Nexus (C:)".into(),
                path: PathBuf::from("C:\\"),
                icon: ICON_DRIVE,
                tag_color: None,
            },
            SidebarEntry {
                label: "Data (D:)".into(),
                path: PathBuf::from("D:\\"),
                icon: ICON_DRIVE,
                tag_color: None,
            },
        ];

        // 3. Tags
        let tags = vec![
            SidebarEntry {
                label: "Red".into(),
                path: home_path.join(".tags/red"),
                icon: ICON_TAG,
                tag_color: Some(Color::hex(0xF44336)),
            },
            SidebarEntry {
                label: "Blue".into(),
                path: home_path.join(".tags/blue"),
                icon: ICON_TAG,
                tag_color: Some(Color::hex(0x2196F3)),
            },
            SidebarEntry {
                label: "Green".into(),
                path: home_path.join(".tags/green"),
                icon: ICON_TAG,
                tag_color: Some(Color::hex(0x4CAF50)),
            },
            SidebarEntry {
                label: "Purple".into(),
                path: home_path.join(".tags/purple"),
                icon: ICON_TAG,
                tag_color: Some(Color::hex(0x9C27B0)),
            },
        ];

        self.sections = vec![
            SidebarSection {
                title: "FAVORITES",
                entries: favorites,
            },
            SidebarSection {
                title: "LOCATIONS",
                entries: locations,
            },
            SidebarSection {
                title: "TAGS",
                entries: tags,
            },
        ];
    }

    pub fn width(&self) -> f32 {
        self.width.position
    }

    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    pub fn animating(&self) -> bool {
        !self.width.settled()
    }

    pub fn toggle(&mut self) {
        self.collapsed = !self.collapsed;
        self.width.set_target(if self.collapsed {
            Self::COLLAPSED
        } else {
            Self::EXPANDED
        });
    }

    pub fn set_selected(&mut self, path: Option<PathBuf>) {
        self.selected_path = path;
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) -> Option<PathBuf> {
        self.width.set_target(if self.collapsed {
            Self::COLLAPSED
        } else {
            Self::EXPANDED
        });
        self.width.step(dt);

        let mut clicked_path = None;
        self.hover_item = None;

        if bounds.contains(input.pointer) {
            let mut y = bounds.y + 12.0;
            for (sec_idx, sec) in self.sections.iter().enumerate() {
                y += Self::HEADER_H;
                for (item_idx, entry) in sec.entries.iter().enumerate() {
                    let item_rect = Rect::new(bounds.x + 8.0, y, bounds.w - 16.0, Self::ITEM_H);
                    if item_rect.contains(input.pointer) {
                        self.hover_item = Some((sec_idx, item_idx));
                        if input.primary_released {
                            clicked_path = Some(entry.path.clone());
                            self.selected_path = Some(entry.path.clone());
                        }
                    }
                    y += Self::ITEM_H;
                }
                y += 8.0;
            }
        }

        clicked_path
    }
}
