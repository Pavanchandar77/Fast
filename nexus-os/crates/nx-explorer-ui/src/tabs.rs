//! Multi-tab bar with spring-animated active tab indicator.

use nx_ui::geom::Rect;
use nx_ui::input::Input;
use nx_ui::spring::{Spring, SpringConfig};

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

#[derive(Debug, Clone)]
pub struct TabItem {
    pub title: String,
    pub is_search: bool,
}

pub struct TabBar {
    active_tab: usize,
    tab_count: usize,
    hover_tab: Option<usize>,
    hover_close: Option<usize>,
    hover_plus: bool,
    indicator: Spring,
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl TabBar {
    pub const HEIGHT: f32 = 34.0;
    pub const MAX_TAB_WIDTH: f32 = 200.0;
    pub const MIN_TAB_WIDTH: f32 = 100.0;

    pub fn new() -> Self {
        Self {
            active_tab: 0,
            tab_count: 1,
            hover_tab: None,
            hover_close: None,
            hover_plus: false,
            indicator: Spring::new(0.0, cfg(0.24)),
        }
    }

    pub fn animating(&self) -> bool {
        !self.indicator.settled()
    }

    pub fn set_active(&mut self, active: usize, count: usize) {
        self.active_tab = active;
        self.tab_count = count.max(1);
        self.indicator.set_target(active as f32);
    }

    pub fn update(
        &mut self,
        input: &Input,
        bounds: Rect,
        dt: f32,
    ) -> (Option<usize>, Option<usize>, bool) {
        self.indicator.step(dt);

        let mut select_tab = None;
        let mut close_tab = None;
        let mut add_tab = false;

        let n = self.tab_count;
        let avail_w = (bounds.w - 36.0).max(0.0);
        let tab_w = (avail_w / n as f32).clamp(Self::MIN_TAB_WIDTH, Self::MAX_TAB_WIDTH);

        let inside = bounds.contains(input.pointer);
        self.hover_tab = None;
        self.hover_close = None;
        self.hover_plus = false;

        if inside {
            let local_x = input.pointer.x - bounds.x;
            if local_x < tab_w * n as f32 {
                let idx = (local_x / tab_w) as usize;
                if idx < n {
                    self.hover_tab = Some(idx);
                    let tab_x = bounds.x + idx as f32 * tab_w;
                    let close_rect = Rect::new(
                        tab_x + tab_w - 24.0,
                        bounds.y + (bounds.h - 16.0) * 0.5,
                        16.0,
                        16.0,
                    );
                    if close_rect.contains(input.pointer) {
                        self.hover_close = Some(idx);
                    }
                }
            } else {
                let plus_rect = Rect::new(
                    bounds.x + tab_w * n as f32 + 6.0,
                    bounds.y + (bounds.h - 20.0) * 0.5,
                    20.0,
                    20.0,
                );
                if plus_rect.contains(input.pointer) {
                    self.hover_plus = true;
                }
            }

            if input.primary_released {
                if let Some(idx) = self.hover_close {
                    close_tab = Some(idx);
                } else if let Some(idx) = self.hover_tab {
                    select_tab = Some(idx);
                } else if self.hover_plus {
                    add_tab = true;
                }
            }
        }

        (select_tab, close_tab, add_tab)
    }
}
