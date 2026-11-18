//! Split pane geometry and divider interaction.

use nx_explorer::action::PaneId;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Rect};
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::widgets::Frame;

pub struct SplitLayout {
    split_enabled: bool,
    active_pane: PaneId,
    ratio: f32, // 0.1 to 0.9, default 0.5
    dragging_divider: bool,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitLayout {
    pub const DIVIDER_W: f32 = 4.0;

    pub fn new() -> Self {
        Self {
            split_enabled: false,
            active_pane: 1,
            ratio: 0.5,
            dragging_divider: false,
        }
    }

    pub fn is_split(&self) -> bool {
        self.split_enabled
    }

    pub fn toggle(&mut self) {
        self.split_enabled = !self.split_enabled;
    }

    pub fn set_split(&mut self, enabled: bool) {
        self.split_enabled = enabled;
    }

    pub fn active_pane(&self) -> PaneId {
        self.active_pane
    }

    pub fn set_active_pane(&mut self, id: PaneId) {
        self.active_pane = id;
    }

    pub fn compute_rects(&self, content_bounds: Rect) -> (Rect, Option<Rect>, Option<Rect>) {
        if !self.split_enabled {
            return (content_bounds, None, None);
        }

        let total_w = content_bounds.w - Self::DIVIDER_W;
        let left_w = (total_w * self.ratio).max(100.0).min(total_w - 100.0);
        let right_w = total_w - left_w;

        let left_rect = Rect::new(content_bounds.x, content_bounds.y, left_w, content_bounds.h);
        let divider_rect = Rect::new(
            content_bounds.x + left_w,
            content_bounds.y,
            Self::DIVIDER_W,
            content_bounds.h,
        );
        let right_rect = Rect::new(
            divider_rect.right(),
            content_bounds.y,
            right_w,
            content_bounds.h,
        );

        (left_rect, Some(right_rect), Some(divider_rect))
    }

    pub fn update(&mut self, input: &Input, content_bounds: Rect) {
        if !self.split_enabled {
            self.dragging_divider = false;
            return;
        }

        let (_, _, Some(divider_rect)) = self.compute_rects(content_bounds) else {
            return;
        };

        if input.primary_pressed && divider_rect.contains(input.pointer) {
            self.dragging_divider = true;
        }

        if input.primary_released {
            self.dragging_divider = false;
        }

        if self.dragging_divider && input.primary_down {
            let offset_x = input.pointer.x - content_bounds.x;
            let ratio = offset_x / content_bounds.w.max(1.0);
            self.ratio = ratio.clamp(0.2, 0.8);
        }
    }

    pub fn paint_divider(&self, frame: &mut Frame<'_>, divider_rect: Rect) {
        let theme = frame.theme;
        frame.panel(
            Scene::root(),
            divider_rect,
            Paint::Solid(theme.palette.separator),
            CornerRadii::all(0.0),
            Border::default(),
            nx_ui::tokens::Shadow::none(),
            Layer::Foreground,
        );
    }
}
