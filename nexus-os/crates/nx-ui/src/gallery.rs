//! Gallery model. The example is a thin winit shell around this type, so tests
//! can drive every widget without a window.

use crate::atlas::Atlas;
use crate::geom::{Rect, Size};
use crate::icon::{IconCache, ICON_FOLDER, ICON_GEAR, ICON_MOON, ICON_SIDEBAR, ICON_SUN};
use crate::image::RgbaImage;
use crate::input::Input;
use crate::layout::layout_shell;
use crate::scene::Scene;
use crate::text::TextEngine;
use crate::theme::{Accent, Theme, ThemeMode};
use crate::widgets::{
    Button, ContextMenu, Frame, SearchField, Segmented, Sidebar, SidebarItem, Toolbar, Tooltip,
    VirtualTable,
};

const MENU: [&str; 3] = ["Appearance", "Next Accent", "Close"];

pub struct Gallery {
    pub theme: Theme,
    pub viewport: Size,
    pub last_action: &'static str,
    sidebar: Sidebar,
    toolbar: Toolbar,
    search: SearchField,
    segmented: Segmented,
    table: VirtualTable,
    action: Button,
    theme_button: Button,
    toggle: Button,
    menu: ContextMenu,
    tooltip: Tooltip,
    image: Option<[f32; 4]>,
}

impl Gallery {
    pub fn new(rows: u64) -> Self {
        let theme = Theme::light(Accent::Blue);
        let mut theme_button = Button::icon_only(ICON_MOON);
        theme_button.set_icon(ICON_MOON);
        Self {
            theme,
            viewport: Size::new(1280.0, 800.0),
            last_action: "",
            sidebar: Sidebar::new(vec![
                SidebarItem {
                    label: "Recents",
                    icon: ICON_FOLDER,
                },
                SidebarItem {
                    label: "Desktop",
                    icon: ICON_FOLDER,
                },
                SidebarItem {
                    label: "Documents",
                    icon: ICON_FOLDER,
                },
                SidebarItem {
                    label: "Downloads",
                    icon: ICON_FOLDER,
                },
                SidebarItem {
                    label: "Settings",
                    icon: ICON_GEAR,
                },
            ]),
            toolbar: Toolbar::new("Nexus"),
            search: SearchField::new(),
            segmented: Segmented::new(&["All", "Files", "Images"]),
            table: VirtualTable::new(rows),
            action: Button::accent("New"),
            theme_button,
            toggle: Button::icon_only(ICON_SIDEBAR),
            menu: ContextMenu::new(&MENU),
            tooltip: Tooltip::new("Toggle sidebar"),
            image: None,
        }
    }

    pub fn animating(&self) -> bool {
        self.sidebar.animating()
            || self.search.animating()
            || self.segmented.animating()
            || self.table.animating()
            || self.action.animating()
            || self.theme_button.animating()
            || self.toggle.animating()
            || self.menu.animating()
            || self.tooltip.animating()
    }

    pub fn search_query(&self) -> &str {
        self.search.query()
    }

    pub fn update(&mut self, input: &Input, dt: f32) {
        let shell = layout_shell(
            self.viewport,
            self.sidebar.width(),
            56.0 * self.theme.scale.max(0.5),
        );
        let slots = Toolbar::slots(shell.toolbar);
        let menu_was_open = self.menu.is_open();
        if input.secondary_pressed {
            self.menu.open_at(input.pointer);
        }
        let mut widgets = *input;
        if menu_was_open {
            widgets.primary_pressed = false;
            widgets.primary_released = false;
        }
        if self.toggle.update(&widgets, slots.toggle, dt) {
            self.sidebar.toggle();
            self.last_action = "Toggle sidebar";
        }
        if self.theme_button.update(&widgets, slots.theme, dt) {
            self.set_mode(self.theme.mode.toggled());
            self.last_action = "Appearance";
        }
        if self.action.update(&widgets, slots.action, dt) {
            self.last_action = "New";
        }
        self.search.update(&widgets, slots.search, dt);
        self.segmented.update(&widgets, slots.segmented, dt);
        self.sidebar.update(&widgets, shell.sidebar, dt);
        let kind = match self.segmented.index() {
            1 => Some("File"),
            2 => Some("Image"),
            _ => None,
        };
        self.table.set_kind(kind);
        self.table.set_query(self.search.query());
        self.table.update(&widgets, shell.body, dt);
        self.tooltip.update(input, slots.toggle, dt);
        if let Some(index) = self.menu.update(input, self.viewport, dt) {
            self.last_action = MENU[index];
            match index {
                0 => self.set_mode(self.theme.mode.toggled()),
                1 => self.set_accent(next_accent(self.theme.accent)),
                _ => {}
            }
        }
    }

    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextEngine,
        icons: &mut IconCache,
        atlas: &mut Atlas,
    ) {
        scene.set_smoothing(self.theme.corner_smoothing);
        scene.begin_frame();
        self.ensure_image(atlas);
        let theme = self.theme;
        let mut frame = Frame {
            scene,
            text,
            icons,
            atlas,
            theme,
        };
        let shell = layout_shell(
            self.viewport,
            self.sidebar.width(),
            56.0 * theme.scale.max(0.5),
        );
        frame.panel(
            Scene::root(),
            Rect::new(0.0, 0.0, self.viewport.w, self.viewport.h),
            crate::draw::Paint::Solid(theme.palette.window_bg),
            crate::geom::CornerRadii::all(0.0),
            crate::draw::Border::default(),
            crate::tokens::Shadow::none(),
            crate::draw::Layer::Background,
        );
        self.sidebar.paint(&mut frame, shell.sidebar);
        if let Some(src) = self.image {
            let rect = Rect::new(
                shell.sidebar.x + 16.0,
                shell.sidebar.bottom() - 48.0,
                32.0,
                32.0,
            );
            frame.image(
                Scene::root(),
                rect,
                src,
                crate::geom::CornerRadii::all(theme.radii.sm),
            );
        }
        self.toolbar.paint(&mut frame, shell.toolbar);
        let slots = Toolbar::slots(shell.toolbar);
        self.toggle.paint(&mut frame, slots.toggle);
        self.search.paint(&mut frame, slots.search);
        self.segmented.paint(&mut frame, slots.segmented);
        self.action.paint(&mut frame, slots.action);
        self.theme_button.paint(&mut frame, slots.theme);
        self.table.paint(&mut frame, shell.body);
        self.menu.paint(&mut frame, self.viewport);
        self.tooltip.paint(&mut frame, self.viewport);
    }

    fn ensure_image(&mut self, atlas: &mut Atlas) {
        if self.image.is_some() {
            return;
        }
        let image = RgbaImage::gradient(32, 32);
        if let Ok(rect) = image.upload(atlas) {
            self.image = Some(rect.to_array());
        }
    }

    fn set_mode(&mut self, mode: ThemeMode) {
        let accent = self.theme.accent;
        let scale = self.theme.scale;
        self.theme = match mode {
            ThemeMode::Light => Theme::light(accent),
            ThemeMode::Dark => Theme::dark(accent),
        }
        .at_scale(scale);
        self.theme_button
            .set_icon(if mode.is_dark() { ICON_SUN } else { ICON_MOON });
    }

    fn set_accent(&mut self, accent: Accent) {
        let mode = self.theme.mode;
        let scale = self.theme.scale;
        self.theme = match mode {
            ThemeMode::Light => Theme::light(accent),
            ThemeMode::Dark => Theme::dark(accent),
        }
        .at_scale(scale);
    }
}

fn next_accent(accent: Accent) -> Accent {
    let index = Accent::ALL
        .iter()
        .position(|item| *item == accent)
        .unwrap_or(0);
    Accent::ALL[(index + 1) % Accent::ALL.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{FrameClock, FrameDecision, FIRST_FRAME_BUDGET_MS, TARGET_REFRESH_HZ};

    fn resources() -> (Scene, TextEngine, IconCache, Atlas) {
        (
            Scene::with_capacity(2048, 8192),
            TextEngine::new().expect("inter"),
            IconCache::new(),
            Atlas::new(1024),
        )
    }

    fn paint_of(
        gallery: &mut Gallery,
        scene: &mut Scene,
        text: &mut TextEngine,
        icons: &mut IconCache,
        atlas: &mut Atlas,
    ) {
        gallery.paint(scene, text, icons, atlas);
        scene.flatten(|id| text.glyphs(id));
    }

    #[test]
    fn gallery_keeps_a_hundred_thousand_rows_in_a_small_tree() {
        let mut gallery = Gallery::new(100_000);
        let (mut scene, mut text, mut icons, mut atlas) = resources();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        assert!(scene.node_count() > 40, "nodes {}", scene.node_count());
        assert!(scene.node_count() < 500, "nodes {}", scene.node_count());
        assert!(!scene.draw().materials.is_empty());
        assert!(!scene.draw().sprites.is_empty());
        assert!(!scene.draw().background.is_empty());
    }

    #[test]
    fn steady_frames_do_not_grow_capacity() {
        let mut gallery = Gallery::new(100_000);
        let (mut scene, mut text, mut icons, mut atlas) = resources();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        let caps = scene.capacities();
        let entries = text.entry_count();
        let pixels = atlas.capacity_pixels();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        assert!(!scene.grew, "scene grew on a steady frame");
        assert_eq!(scene.capacities(), caps);
        assert_eq!(text.entry_count(), entries);
        assert_eq!(atlas.capacity_pixels(), pixels);
    }

    #[test]
    fn first_frame_stays_under_the_budget() {
        let mut gallery = Gallery::new(100_000);
        let (mut scene, mut text, mut icons, mut atlas) = resources();
        let started = std::time::Instant::now();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        assert!(ms < FIRST_FRAME_BUDGET_MS, "first frame {ms:.1} ms");
    }

    #[test]
    fn settled_gallery_requests_no_redraws() {
        let mut gallery = Gallery::new(100_000);
        gallery.update(&Input::default(), 1.0 / 120.0);
        assert!(!gallery.animating());
        let mut clock = FrameClock::new(TARGET_REFRESH_HZ);
        for step in 0..12 {
            let decision = clock.begin_frame(step as f64 * 0.05, gallery.animating(), false);
            assert!(matches!(decision, FrameDecision::Idle));
        }
        assert_eq!(clock.redraws, 0);
    }

    #[test]
    fn search_filters_without_growing_the_match_list_twice() {
        let mut gallery = Gallery::new(100_000);
        let shell_search = {
            let shell = layout_shell(gallery.viewport, gallery.sidebar.width(), 56.0);
            Toolbar::slots(shell.toolbar).search
        };
        let mut input = Input {
            pointer: shell_search.center(),
            primary_pressed: true,
            ..Default::default()
        };
        gallery.update(&input, 0.016);
        input.primary_pressed = false;
        input.push_char('0');
        input.push_char('0');
        input.push_char('0');
        input.push_char('0');
        input.push_char('4');
        input.push_char('2');
        gallery.update(&input, 0.016);
        assert_eq!(gallery.search_query(), "000042");
        let (mut scene, mut text, mut icons, mut atlas) = resources();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        let entries = text.entry_count();
        paint_of(&mut gallery, &mut scene, &mut text, &mut icons, &mut atlas);
        assert_eq!(text.entry_count(), entries);
        assert!(
            scene.node_count() < 160,
            "filtered nodes {}",
            scene.node_count()
        );
    }
}
