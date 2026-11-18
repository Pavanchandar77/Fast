//! Visual file operation progress indicator (copy/move/delete with progress, pause/cancel).

use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::scene::Scene;
use nx_ui::spring::{Spring, SpringConfig};
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::Frame;

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

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        if self.opacity.position <= 0.01 || self.ops.is_empty() {
            return;
        }

        let theme = frame.theme;
        let alpha = self.opacity.position.clamp(0.0, 1.0);

        let h = 48.0;
        let w = 280.0_f32.min(bounds.w - 32.0);
        let x = bounds.right() - w - 16.0;
        let y = bounds.bottom() - h - 16.0;
        let rect = Rect::new(x, y, w, h);

        frame.panel(
            Scene::root(),
            rect,
            Paint::Solid(theme.palette.control),
            CornerRadii::all(theme.radii.sm),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            theme.elevation.md,
            Layer::Foreground,
        );

        if let Some(op) = self.ops.first() {
            let style = TextStyle::new(
                theme.type_scale.caption1.size,
                theme.type_scale.caption1.line,
                Weight::Medium,
            );

            let mut text_color = theme.palette.text_primary;
            text_color.a *= alpha;

            let title_y = rect.y + 6.0;
            frame.label(
                Scene::root(),
                Point::new(rect.x + 12.0, title_y),
                &op.title,
                style,
                text_color,
            );

            // Progress bar track
            let bar_x = rect.x + 12.0;
            let bar_y = rect.y + 26.0;
            let bar_w = rect.w - 24.0;
            let bar_h = 6.0;
            let track_rect = Rect::new(bar_x, bar_y, bar_w, bar_h);

            frame.panel(
                Scene::root(),
                track_rect,
                Paint::Solid(theme.palette.fill_secondary),
                CornerRadii::all(3.0),
                Border::default(),
                nx_ui::tokens::Shadow::none(),
                Layer::Foreground,
            );

            // Fill
            let fill_w = (bar_w * op.progress.clamp(0.0, 1.0)).max(2.0);
            let fill_rect = Rect::new(bar_x, bar_y, fill_w, bar_h);

            let mut accent = theme.palette.accent;
            accent.a *= alpha;
            frame.panel(
                Scene::root(),
                fill_rect,
                Paint::Solid(accent),
                CornerRadii::all(3.0),
                Border::default(),
                nx_ui::tokens::Shadow::none(),
                Layer::Foreground,
            );
        }
    }
}
