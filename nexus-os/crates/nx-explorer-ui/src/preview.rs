//! Quick Look-style rich preview pane with spring slide-in animation.

use crate::icons::{ICON_CODE, ICON_DOCUMENT, ICON_FOLDER, ICON_IMAGE};
use crate::status_bar::format_bytes;
use nx_fs::DirEntry;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::scene::Scene;
use nx_ui::spring::{Spring, SpringConfig};
use nx_ui::text::TextStyle;
use nx_ui::tokens::Weight;
use nx_ui::widgets::Frame;
use std::path::PathBuf;

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

pub struct PreviewPane {
    open: bool,
    width_spring: Spring,
    current_entry: Option<DirEntry>,
    current_path: Option<PathBuf>,
    text_snippet: Option<String>,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPane {
    pub const TARGET_WIDTH: f32 = 280.0;

    pub fn new() -> Self {
        Self {
            open: false,
            width_spring: Spring::new(0.0, cfg(0.24)),
            current_entry: None,
            current_path: None,
            text_snippet: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn width(&self) -> f32 {
        self.width_spring.position
    }

    pub fn animating(&self) -> bool {
        !self.width_spring.settled()
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.width_spring
            .set_target(if self.open { Self::TARGET_WIDTH } else { 0.0 });
    }

    pub fn set_open(&mut self, open: bool) {
        if self.open != open {
            self.open = open;
            self.width_spring
                .set_target(if self.open { Self::TARGET_WIDTH } else { 0.0 });
        }
    }

    pub fn set_entry(&mut self, entry: Option<DirEntry>, path: Option<PathBuf>) {
        self.current_entry = entry;
        self.current_path = path;
    }

    pub fn set_text_snippet(&mut self, snippet: Option<String>) {
        self.text_snippet = snippet;
    }

    pub fn update(&mut self, dt: f32) {
        self.width_spring.step(dt);
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let current_w = self.width_spring.position;
        if current_w <= 2.0 {
            return;
        }

        let theme = frame.theme;
        let pane_rect = Rect::new(bounds.right() - current_w, bounds.y, current_w, bounds.h);

        // Preview pane panel with border
        frame.panel(
            Scene::root(),
            pane_rect,
            Paint::Solid(theme.palette.window_bg),
            CornerRadii::all(0.0),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            nx_ui::tokens::Shadow::none(),
            Layer::Background,
        );

        if current_w < 120.0 {
            return;
        }

        let pad_x = theme.spacing.s16;
        let center_x = pane_rect.center().x;
        let mut y = pane_rect.y + 24.0;

        if let Some(entry) = &self.current_entry {
            // Pick icon
            let icon = if entry.kind.is_dir() {
                ICON_FOLDER
            } else {
                let name = &entry.display_name;
                if name.ends_with(".rs")
                    || name.ends_with(".toml")
                    || name.ends_with(".json")
                    || name.ends_with(".js")
                    || name.ends_with(".ts")
                    || name.ends_with(".py")
                    || name.ends_with(".c")
                    || name.ends_with(".cpp")
                {
                    ICON_CODE
                } else if name.ends_with(".png")
                    || name.ends_with(".jpg")
                    || name.ends_with(".jpeg")
                    || name.ends_with(".gif")
                    || name.ends_with(".webp")
                    || name.ends_with(".bmp")
                {
                    ICON_IMAGE
                } else {
                    ICON_DOCUMENT
                }
            };

            // Large Preview Icon
            let icon_px = 64.0;
            let icon_x = center_x - icon_px * 0.5;
            frame.icon(
                Scene::root(),
                Point::new(icon_x, y),
                icon,
                icon_px,
                theme.palette.accent,
            );
            y += icon_px + 16.0;

            // Filename
            let name_style = TextStyle::new(
                theme.type_scale.headline.size,
                theme.type_scale.headline.line,
                Weight::Semibold,
            );
            let name = &entry.display_name;
            let prep = frame.text.prepare(frame.atlas, name, name_style);
            let name_x = center_x - prep.width * 0.5;
            frame.label(
                Scene::root(),
                Point::new(name_x.max(pane_rect.x + pad_x), y),
                name,
                name_style,
                theme.palette.text_primary,
            );
            y += name_style.line + 8.0;

            // Kind & Size
            let sub_style = TextStyle::new(
                theme.type_scale.subhead.size,
                theme.type_scale.subhead.line,
                Weight::Regular,
            );
            let size_str = if entry.kind.is_dir() {
                "Folder".to_string()
            } else {
                format_bytes(entry.size)
            };
            let sub_prep = frame.text.prepare(frame.atlas, &size_str, sub_style);
            frame.label(
                Scene::root(),
                Point::new(center_x - sub_prep.width * 0.5, y),
                &size_str,
                sub_style,
                theme.palette.text_secondary,
            );
            y += sub_style.line + 20.0;

            // Divider
            let div_rect = Rect::new(pane_rect.x + pad_x, y, pane_rect.w - pad_x * 2.0, 1.0);
            frame.panel(
                Scene::root(),
                div_rect,
                Paint::Solid(theme.palette.separator),
                CornerRadii::all(0.0),
                Border::default(),
                nx_ui::tokens::Shadow::none(),
                Layer::Foreground,
            );
            y += 16.0;

            // Metadata key-values
            let meta_style = TextStyle::new(
                theme.type_scale.caption1.size,
                theme.type_scale.caption1.line,
                Weight::Regular,
            );

            // Path
            if let Some(p) = &self.current_path {
                frame.label(
                    Scene::root(),
                    Point::new(pane_rect.x + pad_x, y),
                    "Path",
                    meta_style,
                    theme.palette.text_tertiary,
                );
                let path_str = p.to_string_lossy();
                frame.label(
                    Scene::root(),
                    Point::new(pane_rect.x + pad_x + 60.0, y),
                    &path_str,
                    meta_style,
                    theme.palette.text_secondary,
                );
                y += meta_style.line + 6.0;
            }

            // Text snippet if available
            if let Some(snippet) = &self.text_snippet {
                y += 12.0;
                let snippet_rect = Rect::new(
                    pane_rect.x + pad_x,
                    y,
                    pane_rect.w - pad_x * 2.0,
                    (pane_rect.bottom() - y - 16.0).max(40.0),
                );

                frame.panel(
                    Scene::root(),
                    snippet_rect,
                    Paint::Solid(theme.palette.control),
                    CornerRadii::all(theme.radii.sm),
                    Border {
                        color: theme.palette.separator,
                        width: 1.0,
                    },
                    nx_ui::tokens::Shadow::none(),
                    Layer::Foreground,
                );

                let code_style = TextStyle::new(11.0, 16.0, Weight::Regular);
                let mut line_y = snippet_rect.y + 8.0;
                for line in snippet.lines().take(12) {
                    frame.label(
                        Scene::root(),
                        Point::new(snippet_rect.x + 8.0, line_y),
                        line,
                        code_style,
                        theme.palette.text_secondary,
                    );
                    line_y += 16.0;
                }
            }
        }
    }
}
