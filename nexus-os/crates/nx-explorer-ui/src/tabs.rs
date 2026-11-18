//! Multi-tab bar with spring-animated active tab indicator.

use crate::icons::{ICON_CLOSE, ICON_FOLDER, ICON_PLUS, ICON_SEARCH};
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::spring::{Spring, SpringConfig};
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::Frame;

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

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect, tabs: &[TabItem]) {
        let theme = frame.theme;

        // Background
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.window_bg),
            CornerRadii::all(0.0),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        let n = tabs.len().max(1);
        let avail_w = (bounds.w - 36.0).max(0.0);
        let tab_w = (avail_w / n as f32).clamp(Self::MIN_TAB_WIDTH, Self::MAX_TAB_WIDTH);

        // Active tab spring indicator pill
        let ind_x = bounds.x + self.indicator.position * tab_w + 3.0;
        let ind_rect = Rect::new(ind_x, bounds.y + 3.0, tab_w - 6.0, bounds.h - 6.0);
        frame.panel(
            Scene::root(),
            ind_rect,
            Paint::Solid(theme.palette.control),
            CornerRadii::all(theme.radii.sm),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            theme.elevation.sm,
            Layer::Background,
        );

        let style = TextStyle::new(
            theme.type_scale.callout.size,
            theme.type_scale.callout.line,
            Weight::Medium,
        );

        for (i, tab) in tabs.iter().enumerate() {
            let tab_x = bounds.x + i as f32 * tab_w;
            let tab_rect = Rect::new(tab_x, bounds.y, tab_w, bounds.h);

            // Icon
            let icon = if tab.is_search {
                ICON_SEARCH
            } else {
                ICON_FOLDER
            };
            let icon_px = 16.0;
            let icon_x = tab_rect.x + 10.0;
            let icon_y = tab_rect.y + (tab_rect.h - icon_px) * 0.5;
            let icon_color = if i == self.active_tab {
                theme.palette.accent
            } else {
                theme.palette.text_secondary
            };
            frame.icon(
                Scene::root(),
                Point::new(icon_x, icon_y),
                icon,
                icon_px,
                icon_color,
            );

            // Label
            let text_color = if i == self.active_tab {
                theme.palette.text_primary
            } else {
                theme.palette.text_secondary
            };
            let text_x = icon_x + icon_px + 6.0;
            let text_y = tab_rect.y + (tab_rect.h - style.line) * 0.5;
            let _max_title_w = tab_w - 54.0;
            frame.label(
                Scene::root(),
                Point::new(text_x, text_y),
                &tab.title,
                style,
                text_color,
            );

            // Close button × (visible when active or hovered)
            if i == self.active_tab || self.hover_tab == Some(i) {
                let close_x = tab_rect.right() - 20.0;
                let close_y = tab_rect.y + (tab_rect.h - 12.0) * 0.5;
                let close_color = if self.hover_close == Some(i) {
                    theme.palette.text_primary
                } else {
                    theme.palette.text_tertiary
                };
                frame.icon(
                    Scene::root(),
                    Point::new(close_x, close_y),
                    ICON_CLOSE,
                    12.0,
                    close_color,
                );
            }
        }

        // Plus button +
        let plus_x = bounds.x + tab_w * n as f32 + 6.0;
        let plus_y = bounds.y + (bounds.h - 16.0) * 0.5;
        let plus_color = if self.hover_plus {
            theme.palette.text_primary
        } else {
            theme.palette.text_secondary
        };
        frame.icon(
            Scene::root(),
            Point::new(plus_x, plus_y),
            ICON_PLUS,
            16.0,
            plus_color,
        );
    }
}
