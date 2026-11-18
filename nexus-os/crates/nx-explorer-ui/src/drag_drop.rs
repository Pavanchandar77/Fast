//! Drag and drop state, drop target hit-testing, and floating badge rendering.

use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::Frame;
use std::path::PathBuf;

const DRAG_THRESHOLD: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAction {
    Move,
    Copy,
}

#[derive(Debug, Clone, Default)]
pub struct DragDropState {
    active: bool,
    pending: bool,
    start_pos: Point,
    current_pos: Point,
    paths: Vec<PathBuf>,
    action: Option<DropAction>,
    target_path: Option<PathBuf>,
}

impl DragDropState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn action(&self) -> Option<DropAction> {
        self.action
    }

    pub fn target(&self) -> Option<&PathBuf> {
        self.target_path.as_ref()
    }

    pub fn start_pending(&mut self, pos: Point, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.pending = true;
        self.active = false;
        self.start_pos = pos;
        self.current_pos = pos;
        self.paths = paths;
        self.action = None;
        self.target_path = None;
    }

    pub fn update(&mut self, input: &Input, copy_modifier: bool) {
        if self.pending && !self.active {
            let dx = input.pointer.x - self.start_pos.x;
            let dy = input.pointer.y - self.start_pos.y;
            if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                self.active = true;
                self.pending = false;
            }
        }

        if self.active {
            self.current_pos = input.pointer;
            self.action = Some(if copy_modifier {
                DropAction::Copy
            } else {
                DropAction::Move
            });
        }
    }

    pub fn set_target(&mut self, target: Option<PathBuf>) {
        self.target_path = target;
    }

    pub fn finish(&mut self) -> Option<(Vec<PathBuf>, PathBuf, DropAction)> {
        if !self.active {
            self.cancel();
            return None;
        }
        let paths = std::mem::take(&mut self.paths);
        let target = self.target_path.take();
        let action = self.action.unwrap_or(DropAction::Move);
        self.cancel();

        target.map(|t| (paths, t, action))
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.pending = false;
        self.paths.clear();
        self.action = None;
        self.target_path = None;
    }

    pub fn paint(&self, frame: &mut Frame<'_>) {
        if !self.active || self.paths.is_empty() {
            return;
        }

        let theme = frame.theme;
        let style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Medium,
        );

        let mut buf = [0u8; 32];
        let label = if self.paths.len() == 1 {
            self.paths[0]
                .file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_else(|| "1 item".into())
        } else {
            let count = self.paths.len();
            let text = format!("{count} items");
            let len = text.len().min(buf.len());
            buf[..len].copy_from_slice(&text.as_bytes()[..len]);
            std::str::from_utf8(&buf[..len]).unwrap_or("Items").into()
        };

        let prepared = frame.text.prepare(frame.atlas, &label, style);
        let pad = theme.spacing.s8;
        let pill_w = prepared.width + pad * 2.0 + 20.0;
        let pill_h = style.line + pad;

        let origin = Point::new(self.current_pos.x + 12.0, self.current_pos.y + 12.0);
        let rect = Rect::new(origin.x, origin.y, pill_w, pill_h);

        frame.panel(
            Scene::root(),
            rect,
            Paint::Solid(theme.palette.control),
            CornerRadii::all(theme.radii.pill),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            theme.elevation.md,
            Layer::Foreground,
        );

        let badge_text_x = rect.x + pad;
        let badge_text_y = rect.y + (rect.h - prepared.height) * 0.5;
        frame.label(
            Scene::root(),
            Point::new(badge_text_x, badge_text_y),
            &label,
            style,
            theme.palette.text_primary,
        );
    }
}
