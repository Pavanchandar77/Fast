//! Drag and drop state, drop target hit-testing, and floating badge rendering.

use nx_ui::geom::Point;
use nx_ui::input::Input;
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
}
