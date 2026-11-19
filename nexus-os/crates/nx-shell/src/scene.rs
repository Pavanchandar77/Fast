//! macOS-grade scene building for menu bar, dock, Mission Control / Exposé,
//! and Spotlight launcher.
//!
//! Emits retained [`nx_ui::scene::Scene`] nodes using design system tokens,
//! Inter typography, vibrancy materials, and continuous squircle corners.
//! Fully platform-free and testable on Linux.

use nx_ui::color::Color;
use nx_ui::draw::{Border, Layer, Paint};
use nx_ui::geom::{CornerRadii, Point, Rect};
use nx_ui::scene::{NodeId, Scene};
use nx_ui::tokens::Shadow;
use nx_ui::widgets::{style_of, Frame};

use crate::backdrop::BackdropChoice;
use crate::dock::{DockEdge, DockLayout, DockModel, JumpEntry};
use crate::expose::ExposeModel;
use crate::menu_bar::{bar_height, BarSlot, MenuBarLayout, MenuBarModel, StatusKind};
use crate::spotlight::{ProviderKind, Spotlight, SpotlightLayout};

/// Build scene for the macOS menu bar.
pub fn render_menu_bar(
    frame: &mut Frame<'_>,
    model: &MenuBarModel,
    backdrop: &BackdropChoice,
    width: f32,
    scale: f32,
) -> MenuBarLayout {
    let height = bar_height(scale);
    let pad_x = 12.0 * scale;
    let gap = 10.0 * scale;

    let headline_style = style_of(frame.theme.type_scale.headline);
    let body_style = style_of(frame.theme.type_scale.body);
    let caption_style = style_of(frame.theme.type_scale.caption1);

    // 1. Measure leading items
    let mut leading = Vec::new();
    // Apple / Nexus Mark
    leading.push(crate::menu_bar::MeasuredSlot {
        slot: BarSlot::Mark,
        label: "●".to_string(),
        width: 18.0 * scale,
    });

    // App name
    let app_prep = frame
        .text
        .prepare(frame.atlas, &model.app_name, headline_style);
    leading.push(crate::menu_bar::MeasuredSlot {
        slot: BarSlot::App,
        label: model.app_name.clone(),
        width: (app_prep.width + 12.0 * scale).max(24.0),
    });

    // Standard menu titles
    for (i, title) in crate::menu_bar::MENU_TITLES.iter().enumerate() {
        let prep = frame.text.prepare(frame.atlas, title, body_style);
        leading.push(crate::menu_bar::MeasuredSlot {
            slot: BarSlot::Menu(i),
            label: (*title).to_string(),
            width: prep.width + 10.0 * scale,
        });
    }

    // 2. Measure trailing items
    let mut trailing = Vec::new();
    let widgets = model.status_widgets();
    for (i, widget) in widgets.iter().enumerate() {
        let prep = frame
            .text
            .prepare(frame.atlas, &widget.label, caption_style);
        trailing.push(crate::menu_bar::MeasuredSlot {
            slot: BarSlot::Status(i),
            label: widget.label.clone(),
            width: prep.width + 16.0 * scale,
        });
    }

    // Clock
    let clock_str = model.clock_label();
    let clock_prep = frame.text.prepare(frame.atlas, &clock_str, body_style);
    trailing.push(crate::menu_bar::MeasuredSlot {
        slot: BarSlot::Clock,
        label: clock_str.clone(),
        width: clock_prep.width + 10.0 * scale,
    });

    // 3. Layout slots
    let layout = crate::menu_bar::layout_menu_bar(width, height, pad_x, gap, &leading, &trailing);

    // 4. Paint background
    let root = Scene::root();
    if backdrop.uses_dwm() {
        // Subtle translucent fill on Windows 11 DWM Mica
        frame.panel(
            root,
            layout.bounds,
            Paint::Solid(frame.theme.palette.window_bg.with_alpha(0.7)),
            CornerRadii::all(0.0),
            Border::default(),
            Shadow::none(),
            Layer::Background,
        );
    } else {
        // Vibrancy material for custom Gaussian blur fallback
        frame.material(
            root,
            layout.bounds,
            CornerRadii::all(0.0),
            backdrop.material,
        );
    }

    // Bottom hairline separator
    frame.panel(
        root,
        layout.separator,
        Paint::Solid(frame.theme.palette.separator),
        CornerRadii::all(0.0),
        Border::default(),
        Shadow::none(),
        Layer::Foreground,
    );

    // 5. Paint slots
    for slot in &layout.slots {
        let is_open = model.open_menu == Some(slot.slot);
        let pill_bg = if is_open {
            Some(frame.theme.palette.accent.with_alpha(0.25))
        } else {
            None
        };

        match slot.slot {
            BarSlot::Mark => {
                let center = slot.rect.center();
                let mark_size = 12.0 * scale;
                let mark_rect = Rect::new(
                    center.x - mark_size * 0.5,
                    center.y - mark_size * 0.5,
                    mark_size,
                    mark_size,
                );
                frame.panel(
                    root,
                    mark_rect,
                    Paint::Solid(frame.theme.palette.text_primary),
                    CornerRadii::all(mark_size * 0.5),
                    Border::default(),
                    Shadow::none(),
                    Layer::Foreground,
                );
            }
            BarSlot::App => {
                if let Some(bg) = pill_bg {
                    let pill_rect = slot.rect.inset(nx_ui::geom::Insets::symmetric(4.0, 2.0));
                    frame.panel(
                        root,
                        pill_rect,
                        Paint::Solid(bg),
                        CornerRadii::all(4.0),
                        Border::default(),
                        Shadow::none(),
                        Layer::Foreground,
                    );
                }
                let text_origin = Point::new(slot.rect.x + 6.0 * scale, slot.rect.y + 6.0 * scale);
                frame.label(
                    root,
                    text_origin,
                    &model.app_name,
                    headline_style,
                    frame.theme.palette.text_primary,
                );
            }
            BarSlot::Menu(_) => {
                if let Some(bg) = pill_bg {
                    let pill_rect = slot.rect.inset(nx_ui::geom::Insets::symmetric(4.0, 2.0));
                    frame.panel(
                        root,
                        pill_rect,
                        Paint::Solid(bg),
                        CornerRadii::all(4.0),
                        Border::default(),
                        Shadow::none(),
                        Layer::Foreground,
                    );
                }
                let text_origin = Point::new(slot.rect.x + 5.0 * scale, slot.rect.y + 6.0 * scale);
                frame.label(
                    root,
                    text_origin,
                    &slot.label,
                    body_style,
                    frame.theme.palette.text_primary,
                );
            }
            BarSlot::Status(idx) => {
                if let Some(widget) = widgets.get(idx) {
                    let pill_rect = slot.rect.inset(nx_ui::geom::Insets::symmetric(3.0, 3.0));
                    let bg_color = match widget.kind {
                        StatusKind::Privacy => widget.color.with_alpha(0.2),
                        StatusKind::Volume => frame.theme.palette.accent.with_alpha(0.15),
                        StatusKind::Clipboard => Color::hex(0xbf5af2).with_alpha(0.18),
                        StatusKind::GameMode => Color::hex(0x30d158).with_alpha(0.2),
                    };
                    frame.panel(
                        root,
                        pill_rect,
                        Paint::Solid(bg_color),
                        CornerRadii::all(pill_rect.h * 0.5),
                        Border::default(),
                        Shadow::none(),
                        Layer::Foreground,
                    );

                    let dot_size = 6.0 * scale;
                    let dot_rect = Rect::new(
                        pill_rect.x + 6.0 * scale,
                        pill_rect.center().y - dot_size * 0.5,
                        dot_size,
                        dot_size,
                    );
                    frame.panel(
                        root,
                        dot_rect,
                        Paint::Solid(widget.color),
                        CornerRadii::all(dot_size * 0.5),
                        Border::default(),
                        Shadow::none(),
                        Layer::Foreground,
                    );

                    let label_x = dot_rect.right() + 4.0 * scale;
                    let text_origin = Point::new(label_x, pill_rect.y + 3.0 * scale);
                    frame.label(
                        root,
                        text_origin,
                        &widget.label,
                        caption_style,
                        frame.theme.palette.text_primary,
                    );
                }
            }
            BarSlot::Clock => {
                let text_origin = Point::new(slot.rect.x + 4.0 * scale, slot.rect.y + 6.0 * scale);
                frame.label(
                    root,
                    text_origin,
                    &clock_str,
                    body_style,
                    frame.theme.palette.text_primary,
                );
            }
        }
    }

    // 6. Paint open dropdown menu popover if visible
    if model.menu_visible() {
        if let Some(open_slot) = model.open_menu {
            if let Some(slot) = layout.slots.iter().find(|s| s.slot == open_slot) {
                let entries = match open_slot {
                    BarSlot::App => model.app_menu(),
                    BarSlot::Menu(i) => model.menu_entries(i),
                    _ => Vec::new(),
                };
                if !entries.is_empty() {
                    paint_dropdown_menu(
                        frame,
                        root,
                        slot.rect.x,
                        slot.rect.bottom() + 4.0,
                        &entries,
                        model.open.position,
                    );
                }
            }
        }
    }

    layout
}

fn paint_dropdown_menu(
    frame: &mut Frame<'_>,
    parent: NodeId,
    x: f32,
    y: f32,
    entries: &[crate::menu_bar::MenuEntry],
    open_amount: f32,
) {
    let scale = frame.theme.scale;
    let width = 200.0 * scale;
    let row_h = 24.0 * scale;
    let sep_h = 7.0 * scale;
    let pad = 6.0 * scale;

    let height = entries.iter().fold(pad * 2.0, |acc, e| {
        acc + if e.separator { sep_h } else { row_h }
    });

    let menu_rect = Rect::new(x, y, width, height);

    // Menu container panel with shadow and rounded corners
    frame.panel(
        parent,
        menu_rect,
        Paint::Solid(
            frame
                .theme
                .palette
                .window_bg
                .with_alpha(open_amount.clamp(0.0, 1.0) * 0.95),
        ),
        CornerRadii::all(frame.theme.radii.md),
        Border {
            color: frame.theme.palette.separator,
            width: 1.0,
        },
        frame.theme.elevation.md,
        Layer::Foreground,
    );

    let mut cursor_y = y + pad;
    let body_style = style_of(frame.theme.type_scale.body);

    for entry in entries {
        if entry.separator {
            let sep_rect = Rect::new(x + 8.0, cursor_y + sep_h * 0.5, width - 16.0, 1.0);
            frame.panel(
                parent,
                sep_rect,
                Paint::Solid(frame.theme.palette.separator),
                CornerRadii::all(0.0),
                Border::default(),
                Shadow::none(),
                Layer::Foreground,
            );
            cursor_y += sep_h;
        } else {
            let text_origin = Point::new(x + 12.0 * scale, cursor_y + 4.0 * scale);
            frame.label(
                parent,
                text_origin,
                &entry.label,
                body_style,
                frame.theme.palette.text_primary,
            );
            cursor_y += row_h;
        }
    }
}

/// Build scene for the macOS dock.
pub fn render_dock(
    frame: &mut Frame<'_>,
    model: &DockModel,
    backdrop: &BackdropChoice,
) -> DockLayout {
    let layout = model.layout();
    if layout.bounds.w <= 0.0 || layout.icons.is_empty() || layout.reveal <= 0.01 {
        return layout;
    }

    let root = Scene::root();

    // 1. Shelf capsule background
    let capsule_radius = match model.placement.edge {
        DockEdge::Bottom => layout.bounds.h * 0.38,
        DockEdge::Left | DockEdge::Right => layout.bounds.w * 0.38,
    };
    let radii = CornerRadii::all(capsule_radius);

    if backdrop.uses_dwm() {
        frame.panel(
            root,
            layout.bounds,
            Paint::Solid(frame.theme.palette.menu_bg.with_alpha(0.85 * layout.reveal)),
            radii,
            Border {
                color: frame.theme.palette.separator,
                width: 1.0,
            },
            frame.theme.elevation.lg,
            Layer::Background,
        );
    } else {
        frame.material(root, layout.bounds, radii, backdrop.material);
        frame.panel(
            root,
            layout.bounds,
            Paint::Solid(Color::TRANSPARENT),
            radii,
            Border {
                color: frame.theme.palette.separator,
                width: 1.0,
            },
            frame.theme.elevation.lg,
            Layer::Foreground,
        );
    }

    // 2. Icons
    let headline_style = style_of(frame.theme.type_scale.headline);
    for icon in &layout.icons {
        let app = model.items().get(icon.index);
        let icon_radius = icon.rect.w * 0.22;
        let icon_radii = CornerRadii::all(icon_radius);

        let icon_color = match icon.index % 6 {
            0 => Color::hex(0x007aff),
            1 => Color::hex(0x34c759),
            2 => Color::hex(0xff9500),
            3 => Color::hex(0xaf52de),
            4 => Color::hex(0xff2d55),
            _ => Color::hex(0x5856d6),
        };

        // Icon squircle panel
        frame.panel(
            root,
            icon.rect,
            Paint::Solid(icon_color),
            icon_radii,
            Border {
                color: Color::rgba(255, 255, 255, 40),
                width: 1.0,
            },
            frame.theme.elevation.sm,
            Layer::Foreground,
        );

        // App initial letter inside icon
        if let Some(app_item) = app {
            let letter = app_item.label.chars().next().unwrap_or('A').to_string();
            let prep = frame.text.prepare(frame.atlas, &letter, headline_style);
            let text_origin = Point::new(
                icon.rect.center().x - prep.width * 0.5,
                icon.rect.center().y - prep.height * 0.5,
            );
            frame.label(root, text_origin, &letter, headline_style, Color::WHITE);
        }

        // 3. Running indicator dot
        if let Some(dot) = icon.dot {
            frame.panel(
                root,
                dot,
                Paint::Solid(frame.theme.palette.text_secondary),
                CornerRadii::all(dot.w * 0.5),
                Border::default(),
                Shadow::none(),
                Layer::Foreground,
            );
        }
    }

    // 4. Paint context/jump menu if open
    if let Some(menu) = &model.context {
        if let Some(icon) = layout.icons.get(menu.index) {
            paint_jump_menu(frame, root, icon.rect, menu);
        }
    }

    layout
}

fn paint_jump_menu(
    frame: &mut Frame<'_>,
    parent: NodeId,
    icon_rect: Rect,
    menu: &crate::dock::JumpMenu,
) {
    let scale = frame.theme.scale;
    let width = 180.0 * scale;
    let row_h = 24.0 * scale;
    let pad = 6.0 * scale;
    let height = pad * 2.0 + row_h * menu.entries.len() as f32;

    let x = (icon_rect.center().x - width * 0.5).max(10.0);
    let y = (icon_rect.y - height - 8.0).max(10.0);
    let menu_rect = Rect::new(x, y, width, height);

    frame.panel(
        parent,
        menu_rect,
        Paint::Solid(frame.theme.palette.window_bg.with_alpha(0.96)),
        CornerRadii::all(frame.theme.radii.md),
        Border {
            color: frame.theme.palette.separator,
            width: 1.0,
        },
        frame.theme.elevation.lg,
        Layer::Foreground,
    );

    let body_style = style_of(frame.theme.type_scale.body);
    let mut cursor_y = y + pad;
    for entry in &menu.entries {
        let label = match entry {
            JumpEntry::Pin => "Pin to Dock".to_string(),
            JumpEntry::Unpin => "Remove from Dock".to_string(),
            JumpEntry::CloseAll => "Quit".to_string(),
            JumpEntry::Window { title, .. } => title.clone(),
            JumpEntry::Custom { label, .. } => label.clone(),
        };
        let text_origin = Point::new(x + 12.0 * scale, cursor_y + 4.0 * scale);
        frame.label(
            parent,
            text_origin,
            &label,
            body_style,
            frame.theme.palette.text_primary,
        );
        cursor_y += row_h;
    }
}

/// Build scene for Mission Control / Exposé.
pub fn render_expose(frame: &mut Frame<'_>, model: &ExposeModel) {
    if !model.shown() {
        return;
    }

    let root = Scene::root();
    let open = model.open_amount().clamp(0.0, 1.0);
    let slide_x = model.slide_offset();

    // 1. Scrim overlay
    let scrim_opacity = (open * 130.0).round() as u8;
    frame.panel(
        root,
        model.canvas,
        Paint::Solid(Color::rgba(0, 0, 0, scrim_opacity)),
        CornerRadii::all(0.0),
        Border::default(),
        Shadow::none(),
        Layer::Background,
    );

    // 2. Window cards
    let caption_style = style_of(frame.theme.type_scale.caption1);
    for tile in model.tiles() {
        let card = tile.displayed(open, slide_x);
        let radii = CornerRadii::all(frame.theme.radii.md);

        // Thumbnail surface
        frame.panel(
            root,
            card,
            Paint::Solid(frame.theme.palette.window_bg),
            radii,
            Border {
                color: frame.theme.palette.separator,
                width: 1.5,
            },
            frame.theme.elevation.md,
            Layer::Foreground,
        );

        // Window title badge
        let label_h = 22.0 * frame.theme.scale;
        let label_w = (card.w - 16.0).max(40.0);
        let label_rect = Rect::new(
            card.center().x - label_w * 0.5,
            card.bottom() + 6.0,
            label_w,
            label_h,
        );

        frame.panel(
            root,
            label_rect,
            Paint::Solid(frame.theme.palette.window_bg.with_alpha(0.9)),
            CornerRadii::all(label_h * 0.5),
            Border {
                color: frame.theme.palette.separator,
                width: 1.0,
            },
            frame.theme.elevation.sm,
            Layer::Foreground,
        );

        let prep = frame.text.prepare(frame.atlas, &tile.title, caption_style);
        let text_origin = Point::new(
            label_rect.center().x - prep.width * 0.5,
            label_rect.center().y - prep.height * 0.5,
        );
        frame.label(
            root,
            text_origin,
            &tile.title,
            caption_style,
            frame.theme.palette.text_primary,
        );
    }
}

/// Build scene for Spotlight palette launcher.
pub fn render_spotlight(frame: &mut Frame<'_>, model: &Spotlight, screen: Rect) -> SpotlightLayout {
    let hits = model.search();
    let row_h = 44.0 * frame.theme.scale;
    let layout = crate::spotlight::layout_spotlight(screen, hits.len().max(1), row_h);

    if !model.is_open() {
        return layout;
    }

    let root = Scene::root();
    let open = model.open_amount().clamp(0.0, 1.0);

    // 1. Scrim
    let scrim_opacity = (open * 90.0).round() as u8;
    frame.panel(
        root,
        screen,
        Paint::Solid(Color::rgba(0, 0, 0, scrim_opacity)),
        CornerRadii::all(0.0),
        Border::default(),
        Shadow::none(),
        Layer::Background,
    );

    // 2. Palette window
    let radii = CornerRadii::all(frame.theme.radii.xl);
    frame.panel(
        root,
        layout.panel,
        Paint::Solid(frame.theme.palette.window_bg.with_alpha(0.95)),
        radii,
        Border {
            color: frame.theme.palette.separator,
            width: 1.0,
        },
        frame.theme.elevation.lg,
        Layer::Foreground,
    );

    // 3. Search query bar
    let headline_style = style_of(frame.theme.type_scale.headline);
    let body_style = style_of(frame.theme.type_scale.body);
    let caption_style = style_of(frame.theme.type_scale.caption1);

    let query_text = if model.query.is_empty() {
        "Spotlight Search"
    } else {
        &model.query
    };
    let query_color = if model.query.is_empty() {
        frame.theme.palette.text_tertiary
    } else {
        frame.theme.palette.text_primary
    };

    let text_origin = Point::new(layout.query.x + 8.0, layout.query.y + 6.0);
    frame.label(root, text_origin, query_text, headline_style, query_color);

    // Hairline divider below search box
    let sep_rect = Rect::new(
        layout.panel.x,
        layout.query.bottom() + 10.0,
        layout.panel.w,
        1.0,
    );
    frame.panel(
        root,
        sep_rect,
        Paint::Solid(frame.theme.palette.separator),
        CornerRadii::all(0.0),
        Border::default(),
        Shadow::none(),
        Layer::Foreground,
    );

    // 4. Rows
    for (i, hit) in hits.iter().enumerate() {
        if let Some(row_rect) = layout.rows.get(i) {
            let is_top = i == 0;
            if is_top {
                frame.panel(
                    root,
                    *row_rect,
                    Paint::Solid(frame.theme.palette.accent.with_alpha(0.18)),
                    CornerRadii::all(frame.theme.radii.sm),
                    Border::default(),
                    Shadow::none(),
                    Layer::Foreground,
                );
            }

            // Kind badge / icon pill
            let badge_w = 48.0 * frame.theme.scale;
            let badge_h = 20.0 * frame.theme.scale;
            let badge_rect = Rect::new(
                row_rect.x + 8.0,
                row_rect.center().y - badge_h * 0.5,
                badge_w,
                badge_h,
            );
            let (badge_text, badge_color) = match hit.kind {
                ProviderKind::Calculator => ("Calc", Color::hex(0xff9500)),
                ProviderKind::App => ("App", Color::hex(0x007aff)),
                ProviderKind::Setting => ("Set", Color::hex(0x8e8e93)),
                ProviderKind::File => ("File", Color::hex(0x34c759)),
            };

            frame.panel(
                root,
                badge_rect,
                Paint::Solid(badge_color.with_alpha(0.2)),
                CornerRadii::all(4.0),
                Border::default(),
                Shadow::none(),
                Layer::Foreground,
            );
            frame.label(
                root,
                Point::new(badge_rect.x + 6.0, badge_rect.y + 2.0),
                badge_text,
                caption_style,
                badge_color,
            );

            // Title & subtitle
            let title_x = badge_rect.right() + 10.0;
            frame.label(
                root,
                Point::new(title_x, row_rect.y + 4.0),
                &hit.title,
                body_style,
                frame.theme.palette.text_primary,
            );
            frame.label(
                root,
                Point::new(title_x, row_rect.y + 22.0),
                &hit.subtitle,
                caption_style,
                frame.theme.palette.text_tertiary,
            );

            // Calculator value on right edge if present
            if let Some(val) = &hit.value {
                let val_prep = frame.text.prepare(frame.atlas, val, headline_style);
                let val_x = row_rect.right() - val_prep.width - 12.0;
                frame.label(
                    root,
                    Point::new(val_x, row_rect.y + 8.0),
                    val,
                    headline_style,
                    frame.theme.palette.accent,
                );
            }
        }
    }

    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backdrop::choose;
    use crate::dock::{DockModel, DockPlacement};
    use crate::expose::ExposeModel;
    use crate::menu_bar::MenuBarModel;
    use crate::spotlight::Spotlight;
    use nx_ui::atlas::Atlas;
    use nx_ui::geom::Rect;
    use nx_ui::icon::IconCache;
    use nx_ui::platform::SystemBackdrop;
    use nx_ui::scene::Scene;
    use nx_ui::text::TextEngine;
    use nx_ui::theme::{Accent, Theme};
    use nx_wm::MonitorId;

    #[test]
    fn render_menu_bar_populates_scene_nodes() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let mut text = TextEngine::new().expect("text");
        let mut icons = IconCache::new();
        let mut atlas = Atlas::new(256);
        let theme = Theme::dark(Accent::Blue);
        let backdrop = choose(&theme, SystemBackdrop::Mica, false);
        let mut model = MenuBarModel::new();
        model.app_name = "Safari".to_string();

        let layout = {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_menu_bar(&mut frame, &model, &backdrop, 1440.0, 1.0)
        };

        assert_eq!(layout.bounds.w, 1440.0);
        assert!(scene.node_count() > 5);
        let draw = scene.flatten(|id| text.glyphs(id));
        assert!(!draw.background.is_empty() || !draw.materials.is_empty());
        assert!(!draw.foreground.is_empty());
    }

    #[test]
    fn render_dock_populates_shelf_and_icons() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let mut text = TextEngine::new().expect("text");
        let mut icons = IconCache::new();
        let mut atlas = Atlas::new(256);
        let theme = Theme::light(Accent::Purple);
        let backdrop = choose(&theme, SystemBackdrop::Acrylic, true);

        let placement = DockPlacement::bottom(MonitorId(0), Rect::new(0.0, 0.0, 1440.0, 900.0));
        let mut model = DockModel::new(placement);
        model.pin("finder", "Finder");
        model.pin("safari", "Safari");
        model.pin("terminal", "Terminal");

        let layout = {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_dock(&mut frame, &model, &backdrop)
        };

        assert_eq!(layout.icons.len(), 3);
        assert!(scene.node_count() >= 3);
        let draw = scene.flatten(|id| text.glyphs(id));
        assert!(!draw.foreground.is_empty());
    }

    #[test]
    fn render_expose_renders_cards_when_shown() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let mut text = TextEngine::new().expect("text");
        let mut icons = IconCache::new();
        let mut atlas = Atlas::new(256);
        let theme = Theme::dark(Accent::Blue);

        let mut model = ExposeModel::new(Rect::new(0.0, 28.0, 1440.0, 872.0));
        model.show_settled();

        {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_expose(&mut frame, &model);
        }
        assert!(scene.node_count() >= 1);
    }

    #[test]
    fn render_spotlight_renders_palette_and_rows() {
        let mut scene = Scene::new();
        scene.begin_frame();
        let mut text = TextEngine::new().expect("text");
        let mut icons = IconCache::new();
        let mut atlas = Atlas::new(256);
        let theme = Theme::dark(Accent::Blue);

        let mut model = Spotlight::with_builtins();
        model.open_palette();
        model.set_query("saf");

        let layout = {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_spotlight(&mut frame, &model, Rect::new(0.0, 0.0, 1440.0, 900.0))
        };
        assert!(layout.panel.w > 0.0);
        assert!(scene.node_count() > 3);
    }

    #[test]
    fn headless_wgpu_golden_images_for_bar_dock_and_expose() {
        // Attempt headless wgpu rendering via Lavapipe on Linux
        let width = 640;
        let height = 360;
        let mut headless = match nx_ui::gpu::Headless::try_new(width, height) {
            Ok(h) => h,
            Err(err) => {
                eprintln!("Skipping headless wgpu rendering (no adapter available: {err})");
                return;
            }
        };

        let mut scene = Scene::new();
        scene.begin_frame();
        let mut text = TextEngine::new().expect("text");
        let mut icons = IconCache::new();
        let mut atlas = Atlas::new(512);
        let theme = Theme::dark(Accent::Blue);
        let backdrop = choose(&theme, SystemBackdrop::Mica, false);

        // 1. Render Menu Bar
        let mut bar_model = MenuBarModel::new();
        bar_model.app_name = "Nexus".to_string();
        {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_menu_bar(&mut frame, &bar_model, &backdrop, width as f32, 1.0);
        }
        let draw_bar = scene.flatten(|id| text.glyphs(id));
        let clear = Color::rgba(20, 20, 24, 255);
        let rgba_bar = headless
            .render_rgba(clear, draw_bar, &atlas)
            .expect("render bar");
        assert_eq!(rgba_bar.len(), (width * height * 4) as usize);

        // 2. Render Dock
        scene.begin_frame();
        let placement = DockPlacement::bottom(
            MonitorId(0),
            Rect::new(0.0, 0.0, width as f32, height as f32),
        );
        let mut dock_model = DockModel::new(placement);
        dock_model.pin("finder", "Finder");
        dock_model.pin("terminal", "Terminal");
        {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_dock(&mut frame, &dock_model, &backdrop);
        }
        let draw_dock = scene.flatten(|id| text.glyphs(id));
        let rgba_dock = headless
            .render_rgba(clear, draw_dock, &atlas)
            .expect("render dock");
        assert_eq!(rgba_dock.len(), (width * height * 4) as usize);

        // 3. Render Expose
        scene.begin_frame();
        let mut expose_model =
            ExposeModel::new(Rect::new(0.0, 28.0, width as f32, (height - 28) as f32));
        expose_model.show_settled();
        {
            let mut frame = Frame {
                scene: &mut scene,
                text: &mut text,
                icons: &mut icons,
                atlas: &mut atlas,
                theme,
            };
            render_expose(&mut frame, &expose_model);
        }
        let draw_expose = scene.flatten(|id| text.glyphs(id));
        let rgba_expose = headless
            .render_rgba(clear, draw_expose, &atlas)
            .expect("render expose");
        assert_eq!(rgba_expose.len(), (width * height * 4) as usize);
    }
}
