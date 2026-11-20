//! Visual file operation progress indicator (copy/move/delete with progress, pause/cancel).

use nx_ui::spring::{Spring, SpringConfig};

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

#[derive(Debug, Clone)]
pub struct ActiveOp {
    pub id: u64,
    pub title: String,
    pub progress: f32, // 0.0 to 1.0
    pub bytes_done: u64,
    pub total_bytes: u64,
    pub paused: bool,
}

pub struct OpsProgressView {
    ops: Vec<ActiveOp>,
    opacity: Spring,
}

impl Default for OpsProgressView {
    fn default() -> Self {
        Self::new()
    }
}

impl OpsProgressView {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            opacity: Spring::new(0.0, cfg(0.2)),
        }
    }

    pub fn is_active(&self) -> bool {
        !self.ops.is_empty()
    }

    pub fn animating(&self) -> bool {
        !self.opacity.settled()
    }

    pub fn set_ops(&mut self, ops: Vec<ActiveOp>) {
        self.ops = ops;
        self.opacity
            .set_target(if self.ops.is_empty() { 0.0 } else { 1.0 });
    }

    pub fn update(&mut self, dt: f32) {
        self.opacity.step(dt);
    }
}
