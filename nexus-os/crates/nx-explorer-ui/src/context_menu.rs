//! Spring-animated Context Menu for file and background operations.

use nx_ui::geom::{Point, Rect, Size};
use nx_ui::input::Input;
use nx_ui::spring::{Spring, SpringConfig};

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextItem {
    Open,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Rename,
    Trash,
    DeletePermanently,
    Properties,
    NewFolder,
}

impl ContextItem {
    pub fn label(self) -> &'static str {
        match self {
            ContextItem::Open => "Open",
            ContextItem::Cut => "Cut",
            ContextItem::Copy => "Copy",
            ContextItem::Paste => "Paste",
            ContextItem::Duplicate => "Duplicate",
            ContextItem::Rename => "Rename",
            ContextItem::Trash => "Move to Trash",
            ContextItem::DeletePermanently => "Delete Permanently",
            ContextItem::Properties => "Properties",
            ContextItem::NewFolder => "New Folder",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            ContextItem::Open => "Enter",
            ContextItem::Cut => "Ctrl+X",
            ContextItem::Copy => "Ctrl+C",
            ContextItem::Paste => "Ctrl+V",
            ContextItem::Duplicate => "Ctrl+D",
            ContextItem::Rename => "F2",
            ContextItem::Trash => "Del",
            ContextItem::DeletePermanently => "Shift+Del",
            ContextItem::Properties => "Alt+Enter",
            ContextItem::NewFolder => "Ctrl+Shift+N",
        }
    }
}

pub struct ExplorerContextMenu {
    items: Vec<ContextItem>,
    open: bool,
    closing: bool,
    origin: Point,
    hover: Option<usize>,
    scale: Spring,
    opacity: Spring,
}

impl Default for ExplorerContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl ExplorerContextMenu {
    pub const ITEM_H: f32 = 28.0;
    pub const WIDTH: f32 = 220.0;

    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            open: false,
            closing: false,
            origin: Point::new(0.0, 0.0),
            hover: None,
            scale: Spring::new(1.0, cfg(0.22)),
            opacity: Spring::new(0.0, cfg(0.16)),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open && (self.opacity.position > 0.01 || !self.closing)
    }

    pub fn animating(&self) -> bool {
        self.open && (!self.scale.settled() || !self.opacity.settled())
    }

    pub fn open_for_selection(&mut self, origin: Point, has_selection: bool, can_paste: bool) {
        self.items.clear();
        if has_selection {
            self.items.push(ContextItem::Open);
            self.items.push(ContextItem::Cut);
            self.items.push(ContextItem::Copy);
            if can_paste {
                self.items.push(ContextItem::Paste);
            }
            self.items.push(ContextItem::Duplicate);
            self.items.push(ContextItem::Rename);
            self.items.push(ContextItem::Trash);
            self.items.push(ContextItem::DeletePermanently);
            self.items.push(ContextItem::Properties);
        } else {
            self.items.push(ContextItem::NewFolder);
            if can_paste {
                self.items.push(ContextItem::Paste);
            }
            self.items.push(ContextItem::Properties);
        }

        self.open = true;
        self.closing = false;
        self.origin = origin;
        self.hover = None;
        self.scale.position = 0.94;
        self.scale.velocity = 0.0;
        self.scale.set_target(1.0);
        self.opacity.position = 0.0;
        self.opacity.velocity = 0.0;
        self.opacity.set_target(1.0);
    }

    pub fn close(&mut self) {
        if !self.open {
            return;
        }
        self.closing = true;
        self.scale.set_target(0.96);
        self.opacity.set_target(0.0);
    }

    pub fn bounds(&self, viewport: Size) -> Rect {
        let h = 8.0 + self.items.len() as f32 * Self::ITEM_H;
        let x = self.origin.x.min((viewport.w - Self::WIDTH).max(0.0));
        let y = self.origin.y.min((viewport.h - h).max(0.0));
        Rect::new(x, y, Self::WIDTH, h)
    }

    pub fn update(&mut self, input: &Input, viewport: Size, dt: f32) -> Option<ContextItem> {
        if !self.open {
            return None;
        }

        self.scale.step(dt);
        self.opacity.step(dt);

        if self.closing && self.opacity.position <= 0.02 {
            self.open = false;
            self.closing = false;
            return None;
        }

        let bounds = self.bounds(viewport);
        let inside = bounds.contains(input.pointer);

        if inside {
            let local_y = input.pointer.y - bounds.y - 4.0;
            if local_y >= 0.0 {
                let idx = (local_y / Self::ITEM_H) as usize;
                self.hover = if idx < self.items.len() {
                    Some(idx)
                } else {
                    None
                };
            } else {
                self.hover = None;
            }
        } else {
            self.hover = None;
            if input.primary_pressed || input.secondary_pressed {
                self.close();
                return None;
            }
        }

        if input.primary_released && inside {
            if let Some(idx) = self.hover {
                let item = self.items.get(idx).copied();
                self.close();
                return item;
            }
        }

        None
    }
}
