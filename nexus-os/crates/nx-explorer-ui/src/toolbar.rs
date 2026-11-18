//! Unified Finder/Explorer toolbar with navigation, breadcrumbs / address bar, view switcher, and search.

use crate::icons::{
    ICON_BACK, ICON_CHEVRON_RIGHT, ICON_EYE, ICON_FORWARD, ICON_MOON, ICON_REFRESH, ICON_SPLIT,
    ICON_UP,
};
use nx_explorer::action::ViewKind;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::input::Input;
use nx_ui::scene::Scene;
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::{Button, Frame, SearchField, Segmented};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarAction {
    Back,
    Forward,
    Up,
    Refresh,
    NavigateBreadcrumb(usize),
    EnterAddressBar,
    CommitAddress(String),
    CancelAddress,
    SetView(ViewKind),
    ToggleSplit,
    ToggleQuickLook,
    NewFolder,
    ToggleTheme,
    SearchChanged(String),
}

pub struct UnifiedToolbar {
    back: Button,
    forward: Button,
    up: Button,
    refresh: Button,
    split_btn: Button,
    preview_btn: Button,
    theme_btn: Button,
    segmented: Segmented,
    search: SearchField,
    address_mode: bool,
    address_text: String,
    address_cursor: usize,
    breadcrumbs: Vec<(String, PathBuf)>,
    hover_crumb: Option<usize>,
}

impl Default for UnifiedToolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedToolbar {
    pub const HEIGHT: f32 = 48.0;

    pub fn new() -> Self {
        Self {
            back: Button::icon_only(ICON_BACK),
            forward: Button::icon_only(ICON_FORWARD),
            up: Button::icon_only(ICON_UP),
            refresh: Button::icon_only(ICON_REFRESH),
            split_btn: Button::icon_only(ICON_SPLIT),
            preview_btn: Button::icon_only(ICON_EYE),
            theme_btn: Button::icon_only(ICON_MOON),
            segmented: Segmented::new(&["Icons", "List", "Columns", "Details"]),
            search: SearchField::new(),
            address_mode: false,
            address_text: String::new(),
            address_cursor: 0,
            breadcrumbs: Vec::new(),
            hover_crumb: None,
        }
    }

    pub fn is_address_mode(&self) -> bool {
        self.address_mode
    }

    pub fn animating(&self) -> bool {
        self.back.animating()
            || self.forward.animating()
            || self.up.animating()
            || self.refresh.animating()
            || self.split_btn.animating()
            || self.preview_btn.animating()
            || self.theme_btn.animating()
            || self.segmented.animating()
            || self.search.animating()
    }

    pub fn set_path(&mut self, current: &Path) {
        if self.address_mode {
            return;
        }
        self.breadcrumbs.clear();
        let mut accum = PathBuf::new();
        for comp in current.components() {
            let seg_str = comp.as_os_str().to_string_lossy().to_string();
            accum.push(comp);
            let display = if seg_str.ends_with('\\') || seg_str.ends_with('/') {
                seg_str.trim_end_matches(['\\', '/']).to_string()
            } else {
                seg_str
            };
            let label = if display.is_empty() {
                "/".to_string()
            } else {
                display
            };
            self.breadcrumbs.push((label, accum.clone()));
        }
        self.address_text = current.to_string_lossy().to_string();
        self.address_cursor = self.address_text.len();
    }

    pub fn set_view(&mut self, view: ViewKind) {
        let idx = match view {
            ViewKind::Grid => 0,
            ViewKind::List => 1,
            ViewKind::Columns => 2,
            ViewKind::Details => 3,
        };
        self.segmented.set_index(idx);
    }

    pub fn start_address_edit(&mut self) {
        self.address_mode = true;
        self.address_cursor = self.address_text.len();
    }

    pub fn cancel_address_edit(&mut self) {
        self.address_mode = false;
    }

    pub fn update(
        &mut self,
        input: &Input,
        bounds: Rect,
        can_back: bool,
        can_forward: bool,
        dt: f32,
    ) -> Vec<ToolbarAction> {
        let mut actions = Vec::new();

        let pad_x = 12.0;
        let mut x = bounds.x + pad_x;
        let cy = bounds.y + (bounds.h - 28.0) * 0.5;

        // 1. Back button
        let back_rect = Rect::new(x, cy, 28.0, 28.0);
        if can_back && self.back.update(input, back_rect, dt) {
            actions.push(ToolbarAction::Back);
        }
        x += 32.0;

        // 2. Forward button
        let fwd_rect = Rect::new(x, cy, 28.0, 28.0);
        if can_forward && self.forward.update(input, fwd_rect, dt) {
            actions.push(ToolbarAction::Forward);
        }
        x += 32.0;

        // 3. Up button
        let up_rect = Rect::new(x, cy, 28.0, 28.0);
        if self.up.update(input, up_rect, dt) {
            actions.push(ToolbarAction::Up);
        }
        x += 40.0;

        // 4. View switcher
        let seg_w = 240.0;
        let seg_rect = Rect::new(x, cy, seg_w, 28.0);
        let prev_seg = self.segmented.index();
        self.segmented.update(input, seg_rect, dt);
        if self.segmented.index() != prev_seg {
            let view = match self.segmented.index() {
                0 => ViewKind::Grid,
                1 => ViewKind::List,
                2 => ViewKind::Columns,
                _ => ViewKind::Details,
            };
            actions.push(ToolbarAction::SetView(view));
        }
        x += seg_w + 16.0;

        // Right side tools: Theme, Split, Preview, Search
        let mut rx = bounds.right() - pad_x;

        // Theme button
        rx -= 28.0;
        let theme_rect = Rect::new(rx, cy, 28.0, 28.0);
        if self.theme_btn.update(input, theme_rect, dt) {
            actions.push(ToolbarAction::ToggleTheme);
        }
        rx -= 8.0;

        // Preview button
        rx -= 28.0;
        let prev_rect = Rect::new(rx, cy, 28.0, 28.0);
        if self.preview_btn.update(input, prev_rect, dt) {
            actions.push(ToolbarAction::ToggleQuickLook);
        }
        rx -= 8.0;

        // Split button
        rx -= 28.0;
        let split_rect = Rect::new(rx, cy, 28.0, 28.0);
        if self.split_btn.update(input, split_rect, dt) {
            actions.push(ToolbarAction::ToggleSplit);
        }
        rx -= 16.0;

        // Search field
        let search_w = 180.0;
        rx -= search_w;
        let search_rect = Rect::new(rx, cy, search_w, 28.0);
        let prev_query = self.search.query().to_string();
        self.search.update(input, search_rect, dt);
        if self.search.query() != prev_query {
            actions.push(ToolbarAction::SearchChanged(
                self.search.query().to_string(),
            ));
        }
        rx -= 16.0;

        // Middle space: Breadcrumbs / Address Bar
        let crumb_w = (rx - x).max(60.0);
        let crumb_rect = Rect::new(x, cy, crumb_w, 28.0);

        if self.address_mode {
            // Typing in address bar
            if input.escape {
                self.address_mode = false;
                actions.push(ToolbarAction::CancelAddress);
                return actions;
            } else if input.backspace {
                self.address_text.pop();
            } else {
                for &ch in &input.typed[..input.typed_len] {
                    if ch == '\r' || ch == '\n' {
                        let text = self.address_text.trim().to_string();
                        self.address_mode = false;
                        actions.push(ToolbarAction::CommitAddress(text));
                        return actions;
                    } else if !ch.is_control() {
                        self.address_text.push(ch);
                    }
                }
            }
        } else {
            // Breadcrumbs clicking
            self.hover_crumb = None;
            if crumb_rect.contains(input.pointer) {
                let mut chip_x = crumb_rect.x + 8.0;
                let mut hit_crumb = None;
                for (i, (label, _)) in self.breadcrumbs.iter().enumerate() {
                    let w = (label.len() as f32 * 8.0 + 16.0).max(24.0);
                    let chip_rect = Rect::new(chip_x, crumb_rect.y + 2.0, w, crumb_rect.h - 4.0);
                    if chip_rect.contains(input.pointer) {
                        self.hover_crumb = Some(i);
                        hit_crumb = Some(i);
                        break;
                    }
                    chip_x += w + 16.0;
                }

                if input.primary_released {
                    if let Some(idx) = hit_crumb {
                        actions.push(ToolbarAction::NavigateBreadcrumb(idx));
                    } else {
                        // Click on blank area in address bar opens editor
                        self.start_address_edit();
                        actions.push(ToolbarAction::EnterAddressBar);
                    }
                }
            }
        }

        actions
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;

        // Toolbar background
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

        let pad_x = 12.0;
        let mut x = bounds.x + pad_x;
        let cy = bounds.y + (bounds.h - 28.0) * 0.5;

        // Nav buttons
        self.back.paint(frame, Rect::new(x, cy, 28.0, 28.0));
        x += 32.0;
        self.forward.paint(frame, Rect::new(x, cy, 28.0, 28.0));
        x += 32.0;
        self.up.paint(frame, Rect::new(x, cy, 28.0, 28.0));
        x += 40.0;

        // View switcher
        let seg_w = 240.0;
        self.segmented.paint(frame, Rect::new(x, cy, seg_w, 28.0));
        x += seg_w + 16.0;

        // Right side
        let mut rx = bounds.right() - pad_x;
        rx -= 28.0;
        self.theme_btn.paint(frame, Rect::new(rx, cy, 28.0, 28.0));
        rx -= 8.0;

        rx -= 28.0;
        self.preview_btn.paint(frame, Rect::new(rx, cy, 28.0, 28.0));
        rx -= 8.0;

        rx -= 28.0;
        self.split_btn.paint(frame, Rect::new(rx, cy, 28.0, 28.0));
        rx -= 16.0;

        let search_w = 180.0;
        rx -= search_w;
        self.search.paint(frame, Rect::new(rx, cy, search_w, 28.0));
        rx -= 16.0;

        // Middle: Breadcrumb pill / Address input
        let crumb_w = (rx - x).max(60.0);
        let crumb_rect = Rect::new(x, cy, crumb_w, 28.0);

        frame.panel(
            Scene::root(),
            crumb_rect,
            Paint::Solid(theme.palette.control),
            CornerRadii::all(theme.radii.sm),
            Border {
                color: if self.address_mode {
                    theme.palette.accent
                } else {
                    theme.palette.separator
                },
                width: 1.0,
            },
            theme.elevation.sm,
            Layer::Foreground,
        );

        let style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Medium,
        );

        if self.address_mode {
            let text_y = crumb_rect.y + (crumb_rect.h - style.line) * 0.5;
            let cursor_str = format!("{}|", self.address_text);
            frame.label(
                Scene::root(),
                Point::new(crumb_rect.x + 8.0, text_y),
                &cursor_str,
                style,
                theme.palette.text_primary,
            );
        } else {
            let mut chip_x = crumb_rect.x + 8.0;
            let text_y = crumb_rect.y + (crumb_rect.h - style.line) * 0.5;

            for (i, (label, _)) in self.breadcrumbs.iter().enumerate() {
                let prep = frame.text.prepare(frame.atlas, label, style);
                let w = (prep.width + 12.0).max(20.0);

                if self.hover_crumb == Some(i) {
                    let chip_rect = Rect::new(chip_x, crumb_rect.y + 2.0, w, crumb_rect.h - 4.0);
                    frame.panel(
                        Scene::root(),
                        chip_rect,
                        Paint::Solid(theme.palette.hover),
                        CornerRadii::all(theme.radii.xs),
                        Border::default(),
                        nx_ui::tokens::Shadow::none(),
                        Layer::Foreground,
                    );
                }

                frame.label(
                    Scene::root(),
                    Point::new(chip_x + 6.0, text_y),
                    label,
                    style,
                    if i + 1 == self.breadcrumbs.len() {
                        theme.palette.text_primary
                    } else {
                        theme.palette.text_secondary
                    },
                );
                chip_x += w;

                if i + 1 < self.breadcrumbs.len() {
                    frame.icon(
                        Scene::root(),
                        Point::new(chip_x, crumb_rect.y + (crumb_rect.h - 10.0) * 0.5),
                        ICON_CHEVRON_RIGHT,
                        10.0,
                        theme.palette.text_tertiary,
                    );
                    chip_x += 14.0;
                }
            }
        }
    }
}
