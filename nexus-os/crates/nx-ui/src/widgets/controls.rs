//! Buttons, scroll, search, segments, menus, tooltips, sidebar, and toolbar.

use super::{style_of, Frame};
use crate::color::Color;
use crate::draw::{Border, Layer, Paint};
use crate::geom::{CornerRadii, Insets, Point, Rect, Size};
use crate::icon::{ICON_CHECK, ICON_SEARCH};
use crate::input::Input;
use crate::scene::Scene;
use crate::spring::{Spring, SpringConfig};
use crate::text::TextStyle;
use crate::tokens::Weight;

fn cfg(response: f32) -> SpringConfig {
    SpringConfig::macos(response, 1.0)
}

pub struct Button {
    label: String,
    icon: Option<&'static str>,
    accent: bool,
    pressed: bool,
    press: Spring,
    hover: Spring,
}

impl Button {
    pub fn labeled(label: impl Into<String>) -> Self {
        Self::build(label.into(), None, false)
    }

    pub fn accent(label: impl Into<String>) -> Self {
        Self::build(label.into(), None, true)
    }

    pub fn icon_only(icon: &'static str) -> Self {
        Self::build(String::new(), Some(icon), false)
    }

    pub fn set_icon(&mut self, icon: &'static str) {
        self.icon = Some(icon);
    }

    fn build(label: String, icon: Option<&'static str>, accent: bool) -> Self {
        Self {
            label,
            icon,
            accent,
            pressed: false,
            press: Spring::new(0.0, cfg(0.16)),
            hover: Spring::new(0.0, cfg(0.22)),
        }
    }

    pub fn animating(&self) -> bool {
        !self.press.settled() || !self.hover.settled()
    }

    pub fn press_amount(&self) -> f32 {
        self.press.position
    }

    /// Returns true on a release that started and ended inside `bounds`.
    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) -> bool {
        let inside = bounds.contains(input.pointer);
        if input.primary_pressed && inside {
            self.pressed = true;
        }
        let mut clicked = false;
        if input.primary_released {
            clicked = self.pressed && inside;
            self.pressed = false;
        } else if !input.primary_down {
            self.pressed = false;
        }
        self.press.set_target(if self.pressed { 1.0 } else { 0.0 });
        self.hover.set_target(if inside { 1.0 } else { 0.0 });
        self.press.step(dt);
        self.hover.step(dt);
        clicked
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;
        let inset = self.press.position;
        let rect = bounds.inset(Insets::all(inset));
        let fill = if self.accent {
            theme
                .palette
                .accent
                .mix(theme.palette.text_primary, 0.12 * self.press.position)
        } else {
            theme
                .palette
                .control
                .mix(theme.palette.hover, self.hover.position * 0.85)
        };
        let mut shadow = theme.elevation.sm;
        shadow.color = shadow
            .color
            .with_alpha(shadow.color.a * (1.0 - 0.45 * self.press.position));
        frame.panel(
            Scene::root(),
            rect,
            Paint::Solid(fill),
            CornerRadii::all(theme.radii.md).clamped(rect.size()),
            Border {
                color: theme.palette.separator,
                width: if self.accent { 0.0 } else { 1.0 },
            },
            shadow,
            Layer::Foreground,
        );
        let fg = if self.accent {
            theme.palette.on_accent
        } else {
            theme.palette.text_primary
        };
        let pad = theme.spacing.s8;
        let mut text_x = rect.x + pad;
        if let Some(icon) = self.icon {
            let px = 16.0 * theme.scale.max(0.5);
            let y = rect.y + (rect.h - px) * 0.5;
            frame.icon(Scene::root(), Point::new(rect.x + pad, y), icon, px, fg);
            text_x += px + theme.spacing.s4;
        }
        if !self.label.is_empty() {
            let base = style_of(theme.type_scale.subhead);
            let style = TextStyle::new(base.size, base.line, Weight::Medium);
            let prepared = frame.text.prepare(frame.atlas, &self.label, style);
            let y = rect.y + (rect.h - prepared.height) * 0.5;
            frame.label(Scene::root(), Point::new(text_x, y), &self.label, style, fg);
        }
    }
}

pub struct ScrollView {
    pub offset: f32,
    pub velocity: f32,
    pub content: f32,
    overscroll: Spring,
    decay: f32,
}

impl ScrollView {
    pub fn new() -> Self {
        Self {
            offset: 0.0,
            velocity: 0.0,
            content: 0.0,
            overscroll: Spring::new(0.0, cfg(0.28)),
            decay: 6.5,
        }
    }

    pub fn visual_offset(&self) -> f32 {
        self.offset + self.overscroll.position
    }

    pub fn animating(&self) -> bool {
        self.velocity.abs() > 1.0 || !self.overscroll.settled()
    }

    /// `delta` is added to velocity when `active`. Positive delta increases offset
    /// (content moves up). The edge spring is interruptible.
    pub fn update(&mut self, delta: f32, viewport: f32, active: bool, dt: f32) {
        let max_off = (self.content - viewport).max(0.0);
        if active && delta.is_finite() && delta != 0.0 {
            self.velocity += delta;
        }
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }

        // If currently in overscroll, check if velocity is moving back towards valid range
        if self.overscroll.position < 0.0 && self.velocity > 0.0 {
            let step = self.velocity * dt;
            if self.overscroll.position + step >= 0.0 {
                let remaining_step = self.overscroll.position + step;
                self.overscroll.position = 0.0;
                self.overscroll.velocity = 0.0;
                self.velocity = remaining_step / dt;
            } else {
                self.overscroll.position += step;
                self.velocity = 0.0;
            }
        } else if self.overscroll.position > 0.0 && self.velocity < 0.0 {
            let step = self.velocity * dt;
            if self.overscroll.position + step <= 0.0 {
                let remaining_step = self.overscroll.position + step;
                self.overscroll.position = 0.0;
                self.overscroll.velocity = 0.0;
                self.velocity = remaining_step / dt;
            } else {
                self.overscroll.position += step;
                self.velocity = 0.0;
            }
        }

        let next = self.offset + self.velocity * dt;
        if next < 0.0 {
            self.offset = 0.0;
            self.overscroll.position += next;
            self.overscroll.velocity = self.velocity * 0.35;
            self.velocity = 0.0;
        } else if next > max_off {
            self.offset = max_off;
            self.overscroll.position += next - max_off;
            self.overscroll.velocity = self.velocity * 0.35;
            self.velocity = 0.0;
        } else {
            self.offset = next;
        }

        self.overscroll.set_target(0.0);
        self.overscroll.step(dt);
        self.velocity *= (-self.decay * dt).exp();

        if self.overscroll.settled() {
            self.overscroll.position = 0.0;
            self.overscroll.velocity = 0.0;
        }
        if self.velocity.abs() < 4.0 && self.overscroll.settled() {
            self.velocity = 0.0;
        }
    }
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SearchField {
    query: String,
    focused: bool,
    focus: Spring,
}

impl SearchField {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            focused: false,
            focus: Spring::new(0.0, cfg(0.22)),
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn animating(&self) -> bool {
        !self.focus.settled()
    }

    /// Returns true when the query text changed.
    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) -> bool {
        if input.primary_pressed {
            self.focused = bounds.contains(input.pointer);
        }
        let mut changed = false;
        if self.focused {
            if input.escape {
                if self.query.is_empty() {
                    self.focused = false;
                } else {
                    self.query.clear();
                    changed = true;
                }
            }
            if input.backspace && self.query.pop().is_some() {
                changed = true;
            }
            for ch in input.typed.iter().take(input.typed_len) {
                self.query.push(*ch);
                changed = true;
            }
        }
        self.focus.set_target(if self.focused { 1.0 } else { 0.0 });
        self.focus.step(dt);
        changed
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;
        let border = theme
            .palette
            .separator
            .mix(theme.palette.accent, self.focus.position);
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.fill),
            CornerRadii::all(theme.radii.sm).clamped(bounds.size()),
            Border {
                color: border,
                width: 1.0 + self.focus.position,
            },
            theme.elevation.sm,
            Layer::Foreground,
        );
        let px = 14.0 * theme.scale.max(0.5);
        let icon_y = bounds.y + (bounds.h - px) * 0.5;
        frame.icon(
            Scene::root(),
            Point::new(bounds.x + theme.spacing.s8, icon_y),
            ICON_SEARCH,
            px,
            theme.palette.text_secondary,
        );
        let style = style_of(theme.type_scale.subhead);
        let text_x = bounds.x + theme.spacing.s8 + px + theme.spacing.s4;
        let text_y = bounds.y + (bounds.h - style.line) * 0.5;
        if self.query.is_empty() {
            frame.label(
                Scene::root(),
                Point::new(text_x, text_y),
                "Search",
                style,
                theme.palette.text_tertiary,
            );
        } else {
            frame.label(
                Scene::root(),
                Point::new(text_x, text_y),
                &self.query,
                style,
                theme.palette.text_primary,
            );
        }
    }
}

impl Default for SearchField {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Segmented {
    labels: Vec<&'static str>,
    index: usize,
    thumb: Spring,
}

impl Segmented {
    pub fn new(labels: &[&'static str]) -> Self {
        Self {
            labels: labels.to_vec(),
            index: 0,
            thumb: Spring::new(0.0, cfg(0.28)),
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn set_index(&mut self, idx: usize) {
        if !self.labels.is_empty() {
            self.index = idx.min(self.labels.len() - 1);
            self.thumb.set_target(self.index as f32);
        }
    }

    pub fn thumb(&self) -> f32 {
        self.thumb.position
    }

    pub fn animating(&self) -> bool {
        !self.thumb.settled()
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) -> bool {
        let mut changed = false;
        if input.primary_pressed && bounds.contains(input.pointer) && !self.labels.is_empty() {
            let w = (bounds.w / self.labels.len() as f32).max(1.0);
            let idx = ((input.pointer.x - bounds.x) / w).floor() as usize;
            let idx = idx.min(self.labels.len() - 1);
            if idx != self.index {
                self.index = idx;
                changed = true;
            }
        }
        self.thumb.set_target(self.index as f32);
        self.thumb.step(dt);
        changed
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;
        frame.panel(
            Scene::root(),
            bounds,
            Paint::Solid(theme.palette.fill),
            CornerRadii::all(theme.radii.sm).clamped(bounds.size()),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            crate::tokens::Shadow::none(),
            Layer::Foreground,
        );
        if self.labels.is_empty() {
            return;
        }
        let inset = 3.0 * theme.scale.max(0.5);
        let inner = bounds.inset(Insets::all(inset));
        let w = inner.w / self.labels.len() as f32;
        let thumb = Rect::new(inner.x + self.thumb.position * w, inner.y, w, inner.h);
        frame.panel(
            Scene::root(),
            thumb,
            Paint::Solid(theme.palette.control),
            CornerRadii::all(theme.radii.xs).clamped(thumb.size()),
            Border::default(),
            theme.elevation.sm,
            Layer::Foreground,
        );
        let style = TextStyle::new(
            theme.type_scale.caption1.size,
            theme.type_scale.caption1.line,
            Weight::Medium,
        );
        for (i, label) in self.labels.iter().enumerate() {
            let slot = Rect::new(inner.x + i as f32 * w, inner.y, w, inner.h);
            let prepared = frame.text.prepare(frame.atlas, label, style);
            let x = slot.x + (slot.w - prepared.width) * 0.5;
            let y = slot.y + (slot.h - prepared.height) * 0.5;
            let color = if i == self.index {
                theme.palette.text_primary
            } else {
                theme.palette.text_secondary
            };
            frame.label(Scene::root(), Point::new(x, y), label, style, color);
        }
    }
}

pub struct ContextMenu {
    items: Vec<&'static str>,
    open: bool,
    closing: bool,
    origin: Point,
    hover: Option<usize>,
    scale: Spring,
    opacity: Spring,
}

impl ContextMenu {
    pub const ITEM_H: f32 = 28.0;
    pub const WIDTH: f32 = 220.0;

    pub fn new(items: &[&'static str]) -> Self {
        Self {
            items: items.to_vec(),
            open: false,
            closing: false,
            origin: Point::new(0.0, 0.0),
            hover: None,
            scale: Spring::new(1.0, cfg(0.22)),
            opacity: Spring::new(0.0, cfg(0.16)),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn animating(&self) -> bool {
        self.open && (!self.scale.settled() || !self.opacity.settled())
    }

    pub fn open_at(&mut self, origin: Point) {
        self.open = true;
        self.closing = false;
        self.origin = origin;
        self.hover = None;
        self.scale.position = 0.94;
        self.scale.velocity = 0.0;
        self.scale.set_target(1.0);
        self.opacity.position = 0.0;
        self.opacity.velocity = 0.0;
        self.opacity.set_target(1.0);
    }

    pub fn close(&mut self) {
        if !self.open {
            return;
        }
        self.closing = true;
        self.scale.set_target(0.96);
        self.opacity.set_target(0.0);
    }

    pub fn bounds(&self, viewport: Size) -> Rect {
        let h = 8.0 + self.items.len() as f32 * Self::ITEM_H;
        let x = self.origin.x.min((viewport.w - Self::WIDTH).max(0.0));
        let y = self.origin.y.min((viewport.h - h).max(0.0));
        Rect::new(x, y, Self::WIDTH, h)
    }

    pub fn item_rect(&self, viewport: Size, index: usize) -> Option<Rect> {
        if index >= self.items.len() {
            return None;
        }
        let bounds = self.bounds(viewport);
        Some(Rect::new(
            bounds.x + 4.0,
            bounds.y + 4.0 + index as f32 * Self::ITEM_H,
            bounds.w - 8.0,
            Self::ITEM_H,
        ))
    }

    pub fn update(&mut self, input: &Input, viewport: Size, dt: f32) -> Option<usize> {
        if !self.open {
            return None;
        }
        if self.closing {
            self.opacity.step(dt);
            self.scale.step(dt);
            if self.opacity.settled() {
                self.open = false;
                self.closing = false;
                self.opacity.position = 0.0;
            }
            return None;
        }
        let bounds = self.bounds(viewport);
        if input.escape || (input.primary_pressed && !bounds.contains(input.pointer)) {
            self.close();
        }
        self.hover = None;
        if bounds.contains(input.pointer) {
            let local = input.pointer.y - bounds.y - 4.0;
            if local >= 0.0 {
                let idx = (local / Self::ITEM_H) as usize;
                if idx < self.items.len() {
                    self.hover = Some(idx);
                }
            }
        }
        let selected = if input.primary_released {
            self.hover
        } else {
            None
        };
        if selected.is_some() {
            self.close();
        }
        if !self.closing {
            self.scale.set_target(1.0);
            self.opacity.set_target(1.0);
        }
        self.scale.step(dt);
        self.opacity.step(dt);
        selected
    }

    pub fn paint(&self, frame: &mut Frame<'_>, viewport: Size) {
        if !self.open || self.opacity.position <= 0.02 {
            return;
        }
        let theme = frame.theme;
        let bounds = self.bounds(viewport);
        let scale = self.scale.position.clamp(0.9, 1.05);
        let center = bounds.center();
        let scaled = Rect::new(
            center.x - bounds.w * scale * 0.5,
            center.y - bounds.h * scale * 0.5,
            bounds.w * scale,
            bounds.h * scale,
        );
        frame.material(
            Scene::root(),
            scaled,
            CornerRadii::all(theme.radii.md),
            theme.materials.menu,
        );
        // A transparent foreground panel supplies the shadow. Materials do not.
        frame.panel(
            Scene::root(),
            scaled,
            Paint::Solid(Color::TRANSPARENT),
            CornerRadii::all(theme.radii.md),
            Border {
                color: theme.palette.separator,
                width: 1.0,
            },
            theme.elevation.md,
            Layer::Foreground,
        );
        let style = style_of(theme.type_scale.subhead);
        for (i, item) in self.items.iter().enumerate() {
            let row = Rect::new(
                scaled.x + 4.0 * scale,
                scaled.y + (4.0 + i as f32 * Self::ITEM_H) * scale,
                (scaled.w - 8.0 * scale).max(0.0),
                Self::ITEM_H * scale,
            );
            if self.hover == Some(i) {
                frame.panel(
                    Scene::root(),
                    row,
                    Paint::Solid(theme.palette.selection),
                    CornerRadii::all(theme.radii.xs),
                    Border::default(),
                    crate::tokens::Shadow::none(),
                    Layer::Foreground,
                );
            }
            frame.label(
                Scene::root(),
                Point::new(row.x + 12.0 * scale, row.y + (row.h - style.line) * 0.5),
                item,
                style,
                theme.palette.text_primary,
            );
        }
    }
}

pub struct Tooltip {
    text: &'static str,
    delay: f32,
    timer: f32,
    shown: bool,
    anchor: Point,
    opacity: Spring,
}

impl Tooltip {
    pub fn new(text: &'static str) -> Self {
        Self {
            text,
            delay: 0.45,
            timer: 0.0,
            shown: false,
            anchor: Point::new(0.0, 0.0),
            opacity: Spring::new(0.0, cfg(0.16)),
        }
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn opacity(&self) -> f32 {
        self.opacity.position
    }

    pub fn animating(&self) -> bool {
        !self.opacity.settled()
    }

    pub fn update(&mut self, input: &Input, target: Rect, dt: f32) {
        if target.contains(input.pointer) {
            self.timer += dt.max(0.0);
            self.anchor = Point::new(target.x, target.bottom() + 6.0);
            self.shown = self.timer >= self.delay;
        } else {
            self.timer = 0.0;
            self.shown = false;
        }
        self.opacity.set_target(if self.shown { 1.0 } else { 0.0 });
        self.opacity.step(dt);
    }

    pub fn paint(&self, frame: &mut Frame<'_>, viewport: Size) {
        if self.opacity.position <= 0.02 {
            return;
        }
        let theme = frame.theme;
        let style = style_of(theme.type_scale.caption1);
        let prepared = frame.text.prepare(frame.atlas, self.text, style);
        let pad = theme.spacing.s8;
        let w = prepared.width + pad * 2.0;
        let h = style.line + pad;
        let x = self.anchor.x.min((viewport.w - w).max(0.0));
        let y = self.anchor.y.min((viewport.h - h).max(0.0));
        let rect = Rect::new(x, y, w, h);
        frame.material(
            Scene::root(),
            rect,
            CornerRadii::all(theme.radii.sm),
            theme.materials.tooltip,
        );
        frame.label(
            Scene::root(),
            Point::new(x + pad, y + (h - prepared.height) * 0.5),
            self.text,
            style,
            theme.palette.text_primary,
        );
    }
}

pub struct SidebarItem {
    pub label: &'static str,
    pub icon: &'static str,
}

pub struct Sidebar {
    items: Vec<SidebarItem>,
    selected: usize,
    collapsed: bool,
    width: Spring,
}

impl Sidebar {
    pub const EXPANDED: f32 = 220.0;
    pub const COLLAPSED: f32 = 72.0;
    pub const ITEM_H: f32 = 36.0;
    pub const ITEM_Y: f32 = 56.0;

    pub fn new(items: Vec<SidebarItem>) -> Self {
        Self {
            items,
            selected: 0,
            collapsed: false,
            width: Spring::new(Self::EXPANDED, cfg(0.34)),
        }
    }

    pub fn width(&self) -> f32 {
        self.width.position
    }

    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn animating(&self) -> bool {
        !self.width.settled()
    }

    pub fn toggle(&mut self) {
        self.collapsed = !self.collapsed;
        self.width.set_target(if self.collapsed {
            Self::COLLAPSED
        } else {
            Self::EXPANDED
        });
    }

    pub fn update(&mut self, input: &Input, bounds: Rect, dt: f32) -> bool {
        let mut changed = false;
        if input.primary_pressed && bounds.contains(input.pointer) {
            let local = input.pointer.y - (bounds.y + Self::ITEM_Y);
            if local >= 0.0 {
                let idx = (local / Self::ITEM_H) as usize;
                if idx < self.items.len() && idx != self.selected {
                    self.selected = idx;
                    changed = true;
                }
            }
        }
        self.width.set_target(if self.collapsed {
            Self::COLLAPSED
        } else {
            Self::EXPANDED
        });
        self.width.step(dt);
        changed
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;
        frame.material(
            Scene::root(),
            bounds,
            CornerRadii::all(0.0),
            theme.materials.sidebar,
        );
        let expanded = self.width.position > (Self::COLLAPSED + Self::EXPANDED) * 0.5;
        let style = TextStyle::new(
            theme.type_scale.subhead.size,
            theme.type_scale.subhead.line,
            Weight::Medium,
        );
        for (i, item) in self.items.iter().enumerate() {
            let row = Rect::new(
                bounds.x + theme.spacing.s8,
                bounds.y + Self::ITEM_Y + i as f32 * Self::ITEM_H,
                (bounds.w - theme.spacing.s16).max(0.0),
                Self::ITEM_H - 4.0,
            );
            if i == self.selected {
                frame.panel(
                    Scene::root(),
                    row,
                    Paint::Solid(theme.palette.selection),
                    CornerRadii::all(theme.radii.sm),
                    Border::default(),
                    crate::tokens::Shadow::none(),
                    Layer::Foreground,
                );
            }
            let px = 18.0 * theme.scale.max(0.5);
            let tint = if i == self.selected {
                theme.palette.accent
            } else {
                theme.palette.text_secondary
            };
            frame.icon(
                Scene::root(),
                Point::new(row.x + 8.0, row.y + (row.h - px) * 0.5),
                item.icon,
                px,
                tint,
            );
            if expanded {
                frame.label(
                    Scene::root(),
                    Point::new(
                        row.x + 8.0 + px + theme.spacing.s8,
                        row.y + (row.h - style.line) * 0.5,
                    ),
                    item.label,
                    style,
                    theme.palette.text_primary,
                );
                if i == self.selected {
                    frame.icon(
                        Scene::root(),
                        Point::new(row.right() - px - 4.0, row.y + (row.h - px) * 0.5),
                        ICON_CHECK,
                        px * 0.8,
                        theme.palette.accent,
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ToolbarSlots {
    pub toggle: Rect,
    pub search: Rect,
    pub segmented: Rect,
    pub action: Rect,
    pub theme: Rect,
}

pub struct Toolbar {
    title: &'static str,
}

impl Toolbar {
    pub fn new(title: &'static str) -> Self {
        Self { title }
    }

    pub fn animating(&self) -> bool {
        false
    }

    pub fn slots(bounds: Rect) -> ToolbarSlots {
        let h = 32.0_f32.min(bounds.h);
        let y = bounds.y + (bounds.h - h) * 0.5;
        let toggle = Rect::new(bounds.x + 12.0, y, 32.0, h);
        let theme = Rect::new((bounds.right() - 44.0).max(bounds.x), y, 32.0, h);
        let action = Rect::new((theme.x - 112.0).max(bounds.x), y, 100.0, h);
        let segmented = Rect::new((action.x - 200.0).max(bounds.x), y, 188.0, h);
        let search_x = toggle.right() + 132.0;
        let search_w = (segmented.x - 12.0 - search_x).max(80.0);
        let search = Rect::new(search_x, y, search_w, h);
        ToolbarSlots {
            toggle,
            search,
            segmented,
            action,
            theme,
        }
    }

    pub fn paint(&self, frame: &mut Frame<'_>, bounds: Rect) {
        let theme = frame.theme;
        frame.material(
            Scene::root(),
            bounds,
            CornerRadii::all(0.0),
            theme.materials.bar,
        );
        let hair = Rect::new(bounds.x, bounds.bottom() - 1.0, bounds.w, 1.0);
        frame.panel(
            Scene::root(),
            hair,
            Paint::Solid(theme.palette.separator),
            CornerRadii::all(0.0),
            Border::default(),
            crate::tokens::Shadow::none(),
            Layer::Foreground,
        );
        let style = TextStyle::new(
            theme.type_scale.headline.size,
            theme.type_scale.headline.line,
            Weight::Semibold,
        );
        let slots = Self::slots(bounds);
        frame.label(
            Scene::root(),
            Point::new(
                slots.toggle.right() + theme.spacing.s8,
                bounds.y + (bounds.h - style.line) * 0.5,
            ),
            self.title,
            style,
            theme.palette.text_primary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(point: Point) -> Input {
        Input {
            pointer: point,
            primary_pressed: true,
            primary_down: true,
            ..Default::default()
        }
    }

    fn release(point: Point) -> Input {
        Input {
            pointer: point,
            primary_released: true,
            ..Default::default()
        }
    }

    #[test]
    fn button_clicks_only_when_release_stays_inside() {
        let mut button = Button::labeled("New");
        let bounds = Rect::new(10.0, 10.0, 80.0, 28.0);
        assert!(!button.update(&press(Point::new(20.0, 20.0)), bounds, 0.016));
        assert!(button.press_amount() > 0.0);
        assert!(button.update(&release(Point::new(20.0, 20.0)), bounds, 0.016));
        assert!(!button.update(&press(Point::new(20.0, 20.0)), bounds, 0.016));
        assert!(!button.update(&release(Point::new(200.0, 20.0)), bounds, 0.016));
    }

    #[test]
    fn scroll_inertia_moves_then_settles() {
        let mut scroll = ScrollView::new();
        scroll.content = 10_000.0;
        scroll.velocity = 2000.0;
        for _ in 0..30 {
            scroll.update(0.0, 400.0, false, 1.0 / 120.0);
        }
        assert!(scroll.offset > 10.0, "offset {}", scroll.offset);
        for _ in 0..500 {
            scroll.update(0.0, 400.0, false, 1.0 / 120.0);
        }
        assert!(
            !scroll.animating(),
            "vel {} visual {}",
            scroll.velocity,
            scroll.visual_offset()
        );
    }

    #[test]
    fn scroll_rubber_band_returns_to_the_edge() {
        let mut scroll = ScrollView::new();
        scroll.content = 5_000.0;
        scroll.velocity = -4000.0;
        scroll.update(0.0, 400.0, false, 1.0 / 60.0);
        assert!(
            scroll.visual_offset() < -1.0,
            "visual {}",
            scroll.visual_offset()
        );
        for _ in 0..240 {
            scroll.update(0.0, 400.0, false, 1.0 / 120.0);
        }
        assert!(
            scroll.visual_offset().abs() < 2.0,
            "visual {}",
            scroll.visual_offset()
        );
        assert!(!scroll.animating());
    }

    #[test]
    fn search_field_edits_and_escape_clears() {
        let mut field = SearchField::new();
        let bounds = Rect::new(0.0, 0.0, 200.0, 32.0);
        assert!(!field.update(&press(Point::new(8.0, 8.0)), bounds, 0.016));
        assert!(field.focused());
        let mut typed = Input::default();
        typed.push_char('N');
        typed.push_char('x');
        assert!(field.update(&typed, bounds, 0.016));
        assert_eq!(field.query(), "Nx");
        let back = Input {
            backspace: true,
            ..Default::default()
        };
        assert!(field.update(&back, bounds, 0.016));
        assert_eq!(field.query(), "N");
        let esc = Input {
            escape: true,
            ..Default::default()
        };
        assert!(field.update(&esc, bounds, 0.016));
        assert!(field.query().is_empty());
    }

    #[test]
    fn segmented_thumb_springs_toward_the_clicked_index() {
        let mut seg = Segmented::new(&["All", "Files", "Images"]);
        let bounds = Rect::new(0.0, 0.0, 180.0, 32.0);
        assert!(seg.update(&press(Point::new(100.0, 10.0)), bounds, 0.016));
        assert_eq!(seg.index(), 1);
        assert!(seg.thumb() > 0.0);
        for _ in 0..48 {
            seg.update(&Input::default(), bounds, 0.016);
        }
        assert!(seg.thumb() > 0.85, "thumb {}", seg.thumb());
        assert!(!seg.animating());
    }

    #[test]
    fn context_menu_selects_and_escape_closes() {
        let mut menu = ContextMenu::new(&["Appearance", "Next Accent", "Close"]);
        let viewport = Size::new(800.0, 600.0);
        menu.open_at(Point::new(20.0, 20.0));
        let row = menu.item_rect(viewport, 0).expect("item");
        let selected = menu.update(&release(row.center()), viewport, 0.016);
        assert_eq!(selected, Some(0));
        assert!(menu.is_open());
        for _ in 0..80 {
            menu.update(&Input::default(), viewport, 1.0 / 60.0);
        }
        assert!(!menu.is_open());

        menu.open_at(Point::new(20.0, 20.0));
        let esc = Input {
            escape: true,
            ..Default::default()
        };
        assert_eq!(menu.update(&esc, viewport, 0.016), None);
        for _ in 0..80 {
            menu.update(&Input::default(), viewport, 1.0 / 60.0);
        }
        assert!(!menu.is_open());
    }

    #[test]
    fn tooltip_waits_then_shows_and_hides() {
        let mut tip = Tooltip::new("Toggle sidebar");
        let target = Rect::new(0.0, 0.0, 32.0, 32.0);
        let mut input = Input {
            pointer: Point::new(8.0, 8.0),
            ..Default::default()
        };
        for _ in 0..20 {
            tip.update(&input, target, 0.016);
        }
        assert!(!tip.shown());
        for _ in 0..20 {
            tip.update(&input, target, 0.016);
        }
        assert!(tip.shown());
        assert!(tip.opacity() > 0.05);
        input.pointer = Point::new(400.0, 400.0);
        tip.update(&input, target, 0.016);
        assert!(!tip.shown());
    }

    #[test]
    fn sidebar_width_springs_between_expanded_and_collapsed() {
        let mut sidebar = Sidebar::new(vec![SidebarItem {
            label: "Recents",
            icon: ICON_SEARCH,
        }]);
        let bounds = Rect::new(0.0, 0.0, 220.0, 600.0);
        assert!((sidebar.width() - Sidebar::EXPANDED).abs() < 0.1);
        sidebar.toggle();
        for _ in 0..50 {
            sidebar.update(&Input::default(), bounds, 0.016);
        }
        assert!(sidebar.width() < 120.0, "width {}", sidebar.width());
        assert!(sidebar.collapsed());
        assert!(!sidebar.update(
            &press(Point::new(20.0, Sidebar::ITEM_Y + 4.0)),
            bounds,
            0.016
        ));
        assert_eq!(sidebar.selected(), 0);
    }
}
