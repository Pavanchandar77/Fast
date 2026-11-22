//! GPUI views for the Nexus shell (the Dynamic Island lives in `island_view`): macOS-style Dock,
//! Mission Control, and Spotlight.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    div, img, px, App, Context, FontWeight, Hsla, ImageSource, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, RenderImage, StatefulInteractiveElement, Styled, Window,
};
use nx_ui::gpui_theme::{mono_font_family, sf_font_family, window_chrome, ThemeStyledExt};
use nx_ui::theme::Theme;
use nx_ui::{glyph_icon, GlyphIcon};

use crate::dock::{magnification_target, running_exe_ids, taskbar_buttons, DockModel};
use crate::expose::ExposeModel;
use crate::menu_bar::{bar_height, BarSlot, MenuBarModel, MENU_TITLES};
use crate::spotlight::{ProviderKind, Spotlight};

fn theme_of(cx: &App) -> Theme {
    if cx.has_global::<Theme>() {
        *cx.global::<Theme>()
    } else {
        Theme::stealth()
    }
}

/// Host-installed callback so Spotlight and the dock can launch or focus an
/// app without depending on the session crate.
type ShellLaunchFn = Arc<dyn Fn(&str, &mut App)>;

#[derive(Default)]
pub struct ShellLaunch {
    handler: Option<ShellLaunchFn>,
}

impl gpui::Global for ShellLaunch {}

impl ShellLaunch {
    pub fn new(handler: impl Fn(&str, &mut App) + 'static) -> Self {
        Self {
            handler: Some(Arc::new(handler)),
        }
    }
}

/// Fire the host launch hook, if the session installed one.
fn launch_dock_target(id: &str, launch: &str, running: bool, cx: &mut App) {
    // A running app comes forward (restored if minimized) instead of
    // starting a second copy.
    if running && crate::dock::focus_app(id) {
        return;
    }
    if id == "trash" {
        #[cfg(windows)]
        shell_open("shell:RecycleBinFolder");
        return;
    }
    let path = if launch.is_empty() { id } else { launch };
    let external = path.contains('\\')
        || path.contains('/')
        || path.to_ascii_lowercase().ends_with(".lnk")
        || path.to_ascii_lowercase().ends_with(".exe");
    if external {
        #[cfg(windows)]
        {
            // ShellExecute follows the shortcut the user pinned (arguments,
            // working folder, run-as) without flashing a console window.
            shell_open(path);
        }
        #[cfg(not(windows))]
        {
            let _ = path;
        }
        return;
    }
    request_shell_launch(cx, id);
}

#[cfg(windows)]
fn shell_open(path: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path = path.to_string();
    let code = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if code.0 as isize <= 32 {
        tracing::warn!("dock: ShellExecute failed ({}) for {path}", code.0 as isize);
    }
}

/// Open a file, shortcut, folder or URI the way Explorer would.
pub fn open_target(path: &str) {
    #[cfg(windows)]
    shell_open(path);
    #[cfg(not(windows))]
    tracing::info!("open_target (preview only): {path}");
}

pub fn request_shell_launch(cx: &mut App, id: &str) {
    let handler = cx
        .try_global::<ShellLaunch>()
        .and_then(|sink| sink.handler.clone());
    if let Some(handler) = handler {
        handler(id, cx);
    }
}

/// Retained for unit tests and host tooling; the session no longer shows a menu bar.
pub struct MenuBarView {
    pub model: MenuBarModel,
}

impl MenuBarView {
    pub fn new(model: MenuBarModel) -> Self {
        Self { model }
    }
}

impl Render for MenuBarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme_of(cx);
        let height = bar_height(theme.scale);
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(height))
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_b_1()
            .border_color(Hsla::from(theme.palette.separator))
            .font_family(sf_font_family())
            .text_size(px(13.0 * theme.scale))
            .text_color(theme.text_primary_hsla())
            .px(px(12.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0 * theme.scale))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(self.model.app_name.clone()),
                    )
                    .children(MENU_TITLES.iter().enumerate().map(|(idx, title)| {
                        let is_open = self.model.open_menu == Some(BarSlot::Menu(idx));
                        div()
                            .px(px(6.0 * theme.scale))
                            .py(px(2.0 * theme.scale))
                            .rounded(px(theme.radii.sm))
                            .when(is_open, |d| d.bg(theme.selection_hsla()))
                            .hover(|s| s.bg(theme.hover_hsla()))
                            .child(*title)
                    })),
            )
            .child(
                div()
                    .font_family(mono_font_family())
                    .text_size(px(12.0 * theme.scale))
                    .child(
                        self.model
                            .clock
                            .format(self.model.hour24)
                            .to_ascii_uppercase(),
                    ),
            )
    }
}

/// macOS-style bottom dock: translucent rounded shelf, magnification, running dots, trash separator.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct DockView {
    pub model: DockModel,
    pub hovered_index: Option<usize>,
    /// Per-icon magnification springs, so icons swell and settle instead of
    /// snapping between sizes.
    mag_springs: Vec<nx_ui::Spring>,
    /// When each icon last started a launch bounce, for the running-dot pulse.
    attention: Vec<Option<Instant>>,
    last_frame: Option<Instant>,
    tracking: bool,
    /// Resting dock origin in logical pixels, captured once.
    home: Option<(f32, f32)>,
    last_procs: Instant,
    last_sample: Option<Instant>,
    /// `Some(true)` while the hidden dock lets clicks pass through.
    click_through: Option<bool>,
    /// Pointer is on the physical bottom edge this frame.
    hold_open: bool,
    /// Pointer x in the same space as the icon slots, while it is over the dock.
    /// Magnification follows this continuously, the way the Windows 11 taskbar
    /// zoom mod does, instead of snapping to one hovered icon.
    cursor_axis: Option<f32>,
    /// Real app icons, keyed by the launch path. Small BGRA frames only.
    icons: HashMap<String, Arc<RenderImage>>,
    icon_miss: HashSet<String>,
    icon_pending: HashSet<String>,
    icon_tx: Sender<IconMsg>,
    icon_rx: Receiver<IconMsg>,
    /// Right-click menu or the recent-items stack, drawn above the shelf.
    popup: Option<DockPopup>,
    /// The pop-up window showing `popup`, while it is open.
    popup_window: Option<gpui::AnyWindowHandle>,
    /// Drag-to-reorder in progress.
    drag: Option<DockDrag>,
    /// Swallow the click that ends a drag.
    suppress_click_until: Option<Instant>,
}

#[derive(Clone, Debug)]
enum DockPopup {
    Menu(usize),
    Stack(Vec<(String, String)>),
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
struct DockDrag {
    index: usize,
    start_x: Option<f32>,
    moved: bool,
}

type IconMsg = (String, Option<(u32, u32, Vec<u8>)>);

impl DockView {
    pub fn new(model: DockModel) -> Self {
        let (icon_tx, icon_rx) = mpsc::channel();
        Self {
            model,
            hovered_index: None,
            mag_springs: Vec::new(),
            attention: Vec::new(),
            last_frame: None,
            tracking: false,
            home: None,
            last_procs: Instant::now(),
            last_sample: None,
            click_through: None,
            hold_open: false,
            cursor_axis: None,
            icons: HashMap::new(),
            icon_miss: HashSet::new(),
            popup: None,
            popup_window: None,
            drag: None,
            suppress_click_until: None,
            icon_pending: HashSet::new(),
            icon_tx,
            icon_rx,
        }
    }

    /// Launch feedback: the icon hops and its running dot pulses.
    pub fn launch_bounce(&mut self, index: usize) {
        self.model.bounce_index(index);
    }

    /// Step the dock springs. Returns true while anything is still moving.
    fn step_motion(&mut self, count: usize) -> bool {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(1.0 / 120.0)
            .min(0.05);
        self.mag_springs.resize_with(count, || {
            let mut s = nx_ui::Spring::new(1.0, nx_ui::SpringConfig::macos(0.24, 0.78));
            s.config.rest_pos = 0.002;
            s.config.rest_vel = 0.02;
            s
        });
        self.attention.resize(count, None);
        let gap = self.model.mag.gap;
        let base = self.model.mag.rest;
        let slot = base + gap;
        for (idx, spring) in self.mag_springs.iter_mut().enumerate() {
            // Icon center vs the live pointer. Neighbors fall off with distance,
            // same shape as Beej's Windows 11 taskbar zoom (150%, bottom origin).
            let center = idx as f32 * slot + base * 0.5;
            let target = match self.cursor_axis {
                Some(axis) => magnification_target(center, axis, self.model.mag),
                None => 1.0,
            };
            spring.set_target(target);
            spring.step(dt);
        }
        self.model.step(dt);
        for (idx, slot) in self.attention.iter_mut().enumerate() {
            let bouncing = self.model.motion_bounce(idx).abs() > 0.5;
            match (*slot, bouncing) {
                (None, true) => *slot = Some(now),
                (Some(t), false) if (now - t).as_secs_f32() > 1.2 => *slot = None,
                _ => {}
            }
        }
        let moving = self.mag_springs.iter().any(|s| !s.settled())
            || self.model.animating()
            || self.attention.iter().any(|a| a.is_some());
        self.last_frame = moving.then_some(now);
        moving
    }
}

impl DockView {
    fn ensure_tracking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tracking {
            return;
        }
        self.tracking = true;
        let b = window.bounds();
        self.home = Some((f32::from(b.origin.x), f32::from(b.origin.y)));
        cx.spawn_in(window, async move |this, cx| {
            // 120 Hz while the pointer is on the dock or something moves;
            // otherwise a cheap 25 Hz edge check. Redraw only on change, so
            // an idle dock costs no frames at all.
            let mut delay = Duration::from_millis(8);
            loop {
                cx.background_executor().timer(delay).await;
                let alive = this.update_in(cx, |this, window, cx| {
                    let before = this.signature();
                    let pass = this.sample_cursor(window);
                    let busy = this.model.animating()
                        || this.mag_springs.iter().any(|s| !s.settled())
                        || this.attention.iter().any(|a| a.is_some());
                    if busy || this.signature() != before {
                        cx.notify();
                    }
                    let hot = busy
                        || this.hovered_index.is_some()
                        || this.drag.is_some()
                        || this.popup.is_some()
                        || this.hold_open;
                    (pass, hot)
                });
                let Ok((pass, hot)) = alive else {
                    break;
                };
                delay = Duration::from_millis(if hot { 8 } else { 40 });
                // Applied after the view borrow ends. Doing it inside the update
                // re-enters the window procedure and the process exits.
                #[cfg(windows)]
                if let Some((raw, on)) = pass.click {
                    crate::capture::set_click_through(raw, on);
                }
                for path in pass.missing {
                    let tx = pass.icon_tx.clone();
                    std::thread::spawn(move || {
                        let image = crate::icons::app_icon_rgba(&path);
                        let _ = tx.send((path, image));
                    });
                }
            }
        })
        .detach();
    }

    /// Everything the dock draws from, folded into one number, so the poll
    /// loop can skip redraws when nothing changed.
    fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hovered_index.hash(&mut h);
        self.cursor_axis
            .map(|a| (a * 2.0).round() as i32)
            .hash(&mut h);
        ((self.model.reveal() * 1000.0) as i32).hash(&mut h);
        self.hold_open.hash(&mut h);
        self.drag.map(|d| (d.index, d.moved)).hash(&mut h);
        match &self.popup {
            None => 0usize.hash(&mut h),
            Some(DockPopup::Menu(i)) => (1usize, *i).hash(&mut h),
            Some(DockPopup::Stack(v)) => (2usize, v.len()).hash(&mut h),
        }
        self.icons.len().hash(&mut h);
        self.icon_miss.len().hash(&mut h);
        for item in self.model.items() {
            (&item.id, item.pinned, item.running, item.frontmost).hash(&mut h);
        }
        h.finish()
    }

    /// Which icon the pointer is over, in screen space. Slots are centered
    /// in the dock window at the resting size; the spring then grows them.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn hover_from_cursor(&mut self, window: &Window, point: nx_ui::geom::Point) {
        let b = window.bounds();
        let left = f32::from(b.origin.x);
        let top = f32::from(b.origin.y);
        let width = f32::from(b.size.width);
        let height = f32::from(b.size.height);
        let n = self.model.items().len();
        let over = point.x >= left
            && point.x <= left + width
            && point.y >= top
            && point.y <= top + height
            && self.model.reveal() > 0.4;
        if !over || n == 0 {
            self.hovered_index = None;
            self.cursor_axis = None;
            return;
        }
        let slot = self.model.mag.rest + self.model.mag.gap;
        let total = n as f32 * slot - self.model.mag.gap;
        let start = left + (width - total) * 0.5;
        let rel = point.x - start;
        if rel < -slot || rel > total + slot {
            self.hovered_index = None;
            self.cursor_axis = None;
            return;
        }
        let idx = ((rel + self.model.mag.gap * 0.5) / slot)
            .floor()
            .clamp(0.0, (n - 1) as f32) as usize;
        self.hovered_index = Some(idx);
        self.cursor_axis = Some(rel);
    }

    /// Reveal when the pointer is near the bottom edge, including over a
    /// fullscreen app. The shelf slides inside the dock window; the window
    /// itself is not moved.
    fn sample_cursor(&mut self, window: &mut Window) -> CursorPass {
        if self.home.is_none() {
            let b = window.bounds();
            self.home = Some((f32::from(b.origin.x), f32::from(b.origin.y)));
        }
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let mut p = POINT::default();
            if unsafe { GetCursorPos(&mut p) }.is_ok() {
                // GetCursorPos and the screen rect we placed the dock with are
                // both physical pixels on this window. Dividing by the GPUI
                // scale put the pointer above the reveal zone, so the shelf
                // never came up and the icons never grew.
                let _ = window.scale_factor();
                let point = nx_ui::geom::Point::new(p.x as f32, p.y as f32);
                let screen_h = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                        windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
                    )
                } as f32;
                // The physical bottom edge opens the dock (macOS rule), also
                // over a fullscreen app since the dock is topmost.
                let at_bottom = point.y >= screen_h - 3.0;
                let bounds = self.model.placement.bounds;
                let reach = self.model.placement.reveal_px.max(28.0) + self.model.placement.margin;
                let near = point.y >= bounds.bottom() - reach && point.y <= bounds.bottom() + 96.0;
                let over_x = point.x >= bounds.x && point.x <= bounds.right();
                if at_bottom || (near && over_x) || self.hovered_index.is_some() {
                    self.model.pointer_moved(point);
                } else if self.model.auto_hide {
                    self.model.pointer_left();
                }
                self.hover_from_cursor(window, point);
                self.hold_open = at_bottom || self.hovered_index.is_some() || self.popup.is_some();
                self.model.set_hold(self.hold_open);
            }
        }
        while let Ok((path, image)) = self.icon_rx.try_recv() {
            self.icon_pending.remove(&path);
            match image.and_then(|(w, h, bytes)| raster_icon(w, h, bytes)) {
                Some(frame) => {
                    self.icons.insert(path, frame);
                }
                None => {
                    self.icon_miss.insert(path);
                }
            }
        }
        self.poll_drag_and_popup(window);
        let mut missing = Vec::new();
        if self.last_procs.elapsed() > Duration::from_millis(400) {
            self.last_procs = Instant::now();
            let names = running_exe_ids();
            self.model.note_running_exes(&names);
            let buttons = taskbar_buttons();
            self.model.note_taskbar(&buttons);
            let mut wanted: Vec<String> = self
                .model
                .items()
                .iter()
                .map(|i| i.launch.clone())
                .collect();
            if let Some(DockPopup::Stack(entries)) = &self.popup {
                wanted.extend(entries.iter().map(|e| e.1.clone()));
            }
            for path in wanted {
                let looks_like_file = path.contains('\\') || path.contains('/');
                if !looks_like_file
                    || self.icons.contains_key(&path)
                    || self.icon_miss.contains(&path)
                    || self.icon_pending.contains(&path)
                {
                    continue;
                }
                if missing.len() >= 3 {
                    break;
                }
                self.icon_pending.insert(path.clone());
                missing.push(path);
            }
        }
        let now = Instant::now();
        let dt = self
            .last_sample
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 120.0)
            .clamp(1.0 / 240.0, 0.05);
        self.last_sample = Some(now);
        self.model.step(dt);
        if self.hold_open {
            self.model.force_reveal();
        }
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut click = None;
        #[cfg(windows)]
        {
            // Clicks pass through unless the pointer is over the shelf: the
            // dock window spans the screen width, and its transparent sides
            // must not swallow clicks meant for the app underneath.
            let hidden = (self.model.auto_hide && self.model.reveal() < 0.08)
                || (self.hovered_index.is_none()
                    && !self.model.menu_open()
                    && self.popup.is_none()
                    && self.drag.is_none());
            if self.click_through != Some(hidden) {
                self.click_through = Some(hidden);
                let raw = {
                    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    HasWindowHandle::window_handle(window)
                        .ok()
                        .and_then(|handle| match handle.as_raw() {
                            RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                            _ => None,
                        })
                };
                if let Some(raw) = raw {
                    click = Some((raw, hidden));
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = window;
        }
        CursorPass {
            click,
            missing,
            icon_tx: self.icon_tx.clone(),
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
struct CursorPass {
    click: Option<(isize, bool)>,
    missing: Vec<String>,
    icon_tx: Sender<IconMsg>,
}

fn raster_icon(w: u32, h: u32, bytes: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = image::RgbaImage::from_raw(w, h, bytes)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

impl DockView {
    /// Drag-to-reorder and popup auto-dismiss, driven from the cursor poll.
    fn poll_drag_and_popup(&mut self, window: &Window) {
        #[cfg(windows)]
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0;
            let cursor = {
                let mut p = windows::Win32::Foundation::POINT::default();
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }
                    .is_ok()
                    .then_some((p.x as f32, p.y as f32))
            };
            if let (Some(mut drag), Some((x, _))) = (self.drag, cursor) {
                if !down {
                    if drag.moved {
                        crate::dock::save_dock_pins(&self.model.kept());
                        self.suppress_click_until =
                            Some(Instant::now() + Duration::from_millis(400));
                    }
                    self.drag = None;
                } else {
                    let start = *drag.start_x.get_or_insert(x);
                    if (x - start).abs() > 8.0 {
                        drag.moved = true;
                    }
                    if drag.moved {
                        if let Some(target) = self.hovered_index {
                            let kept_target = self
                                .model
                                .items()
                                .get(target)
                                .is_some_and(|i| i.pinned && i.id != "trash");
                            if target != drag.index && kept_target {
                                drag.index = self.model.move_item(drag.index, target);
                            }
                        }
                    }
                    self.drag = Some(drag);
                }
            }
            let _ = window;
        }
        #[cfg(not(windows))]
        {
            let _ = window;
        }
    }

    /// Recent items stack, at the right end of the shelf (before nothing,
    /// after Trash, like the Downloads stack on macOS).
    fn render_stack_tile(
        &self,
        theme: &Theme,
        size: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = matches!(self.popup, Some(DockPopup::Stack(_)));
        div()
            .id("dock-stack")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(size))
                    .h(px(size))
                    .rounded(px(theme.radii.lg))
                    .bg(Hsla::from(theme.palette.elevated))
                    .border_1()
                    .border_color(Hsla::from(theme.palette.separator))
                    .hover(|s| s.bg(theme.selection_hsla()))
                    .child(glyph_icon(
                        GlyphIcon::Folder,
                        theme.text_primary_hsla(),
                        (size * 0.5).clamp(14.0, 30.0),
                    )),
            )
            .child(
                div()
                    .w(px(4.0))
                    .h(px(4.0))
                    .rounded(px(2.0))
                    .when(open, |d| d.bg(theme.text_primary_hsla())),
            )
            .on_click(cx.listener(|this, ev: &gpui::ClickEvent, window, cx| {
                if matches!(this.popup, Some(DockPopup::Stack(_))) {
                    this.close_popup(cx);
                } else {
                    let x = f32::from(ev.position().x);
                    let entries = crate::dock::recent_items(12);
                    this.open_popup(DockPopup::Stack(entries), x, window, cx);
                }
                cx.notify();
            }))
    }

    /// Rows for the right-click menu or the Recent Items stack.
    fn popup_rows(&self, popup: &DockPopup) -> Vec<crate::dock_popup::PopupRow> {
        use crate::dock_popup::{PopupAction, PopupRow};
        let mut rows = Vec::new();
        match popup {
            DockPopup::Menu(index) => {
                let Some(item) = self.model.items().get(*index).cloned() else {
                    return rows;
                };
                rows.push(PopupRow::Header(item.label.clone()));
                if !DockModel::is_builtin(&item.id) {
                    let keep = !item.pinned;
                    rows.push(PopupRow::Item {
                        label: if keep {
                            "Keep in Dock"
                        } else {
                            "Remove from Dock"
                        }
                        .into(),
                        action: PopupAction::Keep(*index, keep),
                        icon_path: None,
                    });
                }
                if let Some(folder) = show_in_files_target(&item.launch) {
                    rows.push(PopupRow::Item {
                        label: "Show in Files".into(),
                        action: PopupAction::ShowInFiles(folder),
                        icon_path: None,
                    });
                }
                if item.running && !DockModel::is_builtin(&item.id) {
                    rows.push(PopupRow::Separator);
                    rows.push(PopupRow::Item {
                        label: "Quit".into(),
                        action: PopupAction::Quit(item.id.clone()),
                        icon_path: None,
                    });
                }
                if rows.len() == 1 {
                    rows.push(PopupRow::Item {
                        label: "Open".into(),
                        action: PopupAction::Open(item.id.clone()),
                        icon_path: None,
                    });
                }
            }
            DockPopup::Stack(entries) => {
                rows.push(PopupRow::Header("Recent Items".into()));
                if entries.is_empty() {
                    rows.push(PopupRow::Note("No recent items".into()));
                }
                for (name, path) in entries {
                    rows.push(PopupRow::Item {
                        label: name.clone(),
                        action: PopupAction::Open(path.clone()),
                        icon_path: Some(path.clone()),
                    });
                }
            }
        }
        rows
    }

    /// Open the menu or stack as its own pop-up window above the shelf,
    /// centered on `anchor_x` (dock-window coordinates).
    fn open_popup(
        &mut self,
        popup: DockPopup,
        anchor_x: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_popup(cx);
        let rows = self.popup_rows(&popup);
        if rows.is_empty() {
            return;
        }
        let stack = matches!(popup, DockPopup::Stack(_));
        let icons: HashMap<String, Arc<RenderImage>> = rows
            .iter()
            .filter_map(|r| match r {
                crate::dock_popup::PopupRow::Item {
                    icon_path: Some(p), ..
                } => self.icons.get(p).map(|i| (p.clone(), i.clone())),
                _ => None,
            })
            .collect();
        let b = window.bounds();
        let (left, top) = (f32::from(b.origin.x), f32::from(b.origin.y));
        let (width, height) = (f32::from(b.size.width), f32::from(b.size.height));
        let mag = self.model.mag;
        let shelf_top = top + height - self.model.placement.margin - (mag.rest + 2.0 * mag.padding);
        let size = crate::dock_popup::panel_size(&rows, stack);
        let (x, y, w, h) = crate::dock_popup::popup_bounds(
            size,
            left + anchor_x,
            shelf_top,
            10.0,
            left,
            left + width,
        );
        self.popup = Some(popup);
        let weak = cx.entity().downgrade();
        let sink: crate::dock_popup::PopupSink = {
            let weak = weak.clone();
            Arc::new(move |action, cx: &mut App| {
                let _ = weak.update(cx, |dock, cx| dock.popup_done(action, cx));
            })
        };
        let theme = theme_of(cx);
        cx.defer(move |cx| {
            let mut opts = theme.shell_window_options(
                gpui::Bounds {
                    origin: gpui::point(px(x), px(y)),
                    size: gpui::size(px(w), px(h)),
                },
                true,
                true,
                true,
            );
            opts.focus = true;
            let hook = cx.try_global::<PopupWindowHook>().map(|h| h.0.clone());
            let handle = cx
                .open_window(opts, move |window, cx| {
                    if let Some(hook) = hook {
                        hook(window);
                    }
                    #[cfg(windows)]
                    crate::capture::raise_topmost(window);
                    cx.new(|_| crate::dock_popup::DockPopupView::new(rows, stack, icons, sink))
                })
                .ok();
            match handle {
                Some(handle) => {
                    let any: gpui::AnyWindowHandle = handle.into();
                    let _ = weak.update(cx, |dock, _| dock.popup_window = Some(any));
                }
                None => {
                    tracing::warn!("dock: pop-up window failed to open");
                    let _ = weak.update(cx, |dock, cx| dock.popup_done(None, cx));
                }
            }
        });
    }

    fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.popup = None;
        if let Some(handle) = self.popup_window.take() {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
    }

    /// The pop-up finished: apply the pick (if any) and forget it.
    fn popup_done(
        &mut self,
        action: Option<crate::dock_popup::PopupAction>,
        cx: &mut Context<Self>,
    ) {
        use crate::dock_popup::PopupAction;
        self.popup = None;
        self.popup_window = None;
        match action {
            Some(PopupAction::Keep(index, keep)) => {
                self.model.set_kept(index, keep);
                crate::dock::save_dock_pins(&self.model.kept());
            }
            Some(PopupAction::ShowInFiles(folder)) => {
                let id = format!("files:{folder}");
                cx.defer(move |cx| request_shell_launch(cx, &id));
            }
            Some(PopupAction::Quit(id)) => {
                crate::dock::quit_app(&id);
            }
            Some(PopupAction::Open(target)) => {
                if target.contains('\\') || target.contains('/') {
                    open_target(&target);
                } else {
                    cx.defer(move |cx| request_shell_launch(cx, &target));
                }
            }
            None => {}
        }
        cx.notify();
    }
}

/// Session hook that gives pop-up windows the same native treatment as the
/// other shell windows (client-only frame on Windows).
#[derive(Clone)]
pub struct PopupWindowHook(pub Arc<dyn Fn(&mut Window)>);

impl gpui::Global for PopupWindowHook {}

/// Folder for "Show in Files": the shortcut's target folder, else the
/// launch path's own folder. `None` for Nexus built-ins.
fn show_in_files_target(launch: &str) -> Option<String> {
    if !(launch.contains('\\') || launch.contains('/')) {
        return None;
    }
    let target = crate::icons::shortcut_target(launch).unwrap_or_else(|| launch.to_string());
    let parent = std::path::Path::new(&target).parent()?;
    Some(parent.to_string_lossy().to_string())
}

impl Render for DockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_tracking(window, cx);
        let theme = theme_of(cx);
        let is_horizontal = self.model.placement.edge.is_horizontal();
        let base_size = self.model.mag.rest;
        let gap = self.model.mag.gap;
        let pad = self.model.mag.padding;
        let items = self.model.items().to_vec();
        let first_unpinned = self.model.first_unpinned();
        if self.step_motion(items.len()) {
            window.request_animation_frame();
        }
        let scales: Vec<f32> = self.mag_springs.iter().map(|s| s.position).collect();
        let bounces: Vec<f32> = (0..items.len())
            .map(|i| self.model.motion_bounce(i))
            .collect();
        let pulse_clock = Instant::now();
        let dots: Vec<f32> = self
            .attention
            .iter()
            .map(|a| match a {
                Some(t) => {
                    nx_ui::motion::running_dot_pulse(0.0, Some((pulse_clock - *t).as_secs_f32()))
                }
                None => 1.0,
            })
            .collect();

        let slide = (1.0 - self.model.reveal().clamp(0.0, 1.0)) * 130.0;
        let shelf = div()
            .id("dock-shelf")
            .mt(px(slide))
            .flex()
            .when(is_horizontal, |d| d.flex_row())
            .when(!is_horizontal, |d| d.flex_col())
            .items_end()
            .justify_center()
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(theme.radii.xl))
            .px(px(pad + 2.0))
            .pt(px(pad))
            .pb(px(2.0))
            .gap(px(gap))
            .shadow_lg()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.hovered_index = None;
                    cx.notify();
                }
            }))
            .children(items.into_iter().enumerate().map(|(idx, item)| {
                let scale = scales.get(idx).copied().unwrap_or(1.0).max(0.5);
                let hop = bounces.get(idx).copied().unwrap_or(0.0);
                let dot_alpha = dots.get(idx).copied().unwrap_or(1.0);
                let icon_size = base_size * scale;
                let _label = &item.label;
                let before_trash = item.id == "trash" || (idx > 0 && first_unpinned == Some(idx));

                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(gap))
                    .when(before_trash, |d| {
                        d.child(
                            div()
                                .w(px(1.0))
                                .h(px(base_size * 0.7))
                                .mb(px(10.0))
                                .bg(Hsla::from(theme.palette.separator)),
                        )
                    })
                    .child(
                        div()
                            .id(("dock-icon", idx))
                            .relative()
                            .top(px(hop))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.0))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                this.hovered_index = if *hovered { Some(idx) } else { None };
                                cx.notify();
                            }))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _, _, _cx| {
                                    this.drag = Some(DockDrag {
                                        index: idx,
                                        start_x: None,
                                        moved: false,
                                    });
                                }),
                            )
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
                                    let x = f32::from(ev.position.x);
                                    this.open_popup(DockPopup::Menu(idx), x, window, cx);
                                    cx.notify();
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let dragged = this.drag.take().map(|d| d.moved).unwrap_or(false);
                                let suppressed = this
                                    .suppress_click_until
                                    .take()
                                    .is_some_and(|t| Instant::now() < t);
                                if dragged || suppressed {
                                    crate::dock::save_dock_pins(&this.model.kept());
                                    cx.notify();
                                    return;
                                }
                                this.close_popup(cx);
                                let item = this.model.items().get(idx).cloned();
                                if let Some(item) = item {
                                    let _ = this.model.activate_index(idx);
                                    let launch = item.launch.clone();
                                    let id = item.id.clone();
                                    let running = item.running;
                                    cx.defer(move |cx| {
                                        launch_dock_target(&id, &launch, running, cx)
                                    });
                                }
                                cx.notify();
                            }))
                            .child({
                                let art = if item.launch.is_empty() {
                                    None
                                } else {
                                    self.icons.get(&item.launch).cloned()
                                };
                                let tile = div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(icon_size))
                                    .h(px(icon_size))
                                    .rounded(px(theme.radii.lg * scale.clamp(0.8, 1.4)))
                                    .when(art.is_none(), |d| {
                                        d.bg(Hsla::from(theme.palette.elevated))
                                            .border_1()
                                            .border_color(Hsla::from(theme.palette.separator))
                                            .shadow_sm()
                                            .hover(|s| s.bg(theme.selection_hsla()))
                                    });
                                if let Some(image) = art {
                                    tile.child(
                                        img(ImageSource::Render(image))
                                            .w(px(icon_size))
                                            .h(px(icon_size)),
                                    )
                                } else {
                                    tile.child(glyph_icon(
                                        GlyphIcon::for_dock_id(&item.id),
                                        theme.text_primary_hsla(),
                                        (icon_size * 0.52).clamp(14.0, 36.0),
                                    ))
                                }
                            })
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded(px(2.0))
                                    .when(item.running, |d| {
                                        let mut c = theme.text_primary_hsla();
                                        c.a *= dot_alpha;
                                        d.bg(c)
                                    })
                                    .when(!item.running, |d| {
                                        d.bg(Hsla::from(nx_ui::Color::TRANSPARENT))
                                    }),
                            ),
                    )
            }))
            .child(self.render_stack_tile(&theme, base_size, cx));

        div()
            .id("dock-root")
            .relative()
            .flex()
            .w_full()
            .h_full()
            .overflow_hidden()
            .items_end()
            .justify_center()
            .child(shelf)
    }
}

/// Mission Control / Exposé.
pub struct MissionControlView {
    pub model: ExposeModel,
}

impl MissionControlView {
    pub fn new(model: ExposeModel) -> Self {
        Self { model }
    }
}

impl Render for MissionControlView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme_of(cx);
        let tiles = self.model.tiles().to_vec();

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(Hsla::from(theme.palette.window_bg))
            .gap(px(24.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .px(px(28.0))
                    .gap(px(16.0))
                    .h(px(64.0))
                    .w_full()
                    .child(space_chip(&theme, "WORKSPACE 1", true))
                    .child(space_chip(&theme, "WORKSPACE 2", false))
                    .child(space_chip(&theme, "+ SPACE", false)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .items_center()
                    .gap(px(24.0))
                    .px(px(28.0))
                    .flex_1()
                    .children(tiles.into_iter().map(|tile| {
                        div()
                            .theme_card(&theme)
                            .rounded(px(theme.radii.lg))
                            .flex()
                            .flex_col()
                            .w(px(280.0))
                            .h(px(188.0))
                            .overflow_hidden()
                            .shadow_md()
                            .hover(|s| s.border_color(theme.accent_hsla()).shadow_lg())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.0))
                                    .h(px(32.0))
                                    .bg(Hsla::from(theme.palette.sidebar_bg))
                                    .border_b_1()
                                    .border_color(Hsla::from(theme.palette.separator))
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text_primary_hsla())
                                    .child(window_chrome(&theme, false))
                                    .child(tile.title.to_ascii_uppercase()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(Hsla::from(theme.palette.content_bg))
                                    .font_family(mono_font_family())
                                    .text_size(px(11.0))
                                    .text_color(theme.text_tertiary_hsla())
                                    .child("PREVIEW"),
                            )
                    })),
            )
    }
}

fn space_chip(theme: &Theme, label: &'static str, active: bool) -> impl IntoElement {
    div()
        .theme_card(theme)
        .rounded(px(theme.radii.pill))
        .px(px(20.0))
        .py(px(10.0))
        .when(active, |d| {
            d.bg(theme.inverted_hsla())
                .text_color(theme.on_inverted_hsla())
                .border_color(theme.inverted_hsla())
        })
        .when(!active, |d| d.text_color(theme.text_secondary_hsla()))
        .font_family(sf_font_family())
        .font_weight(FontWeight::BOLD)
        .text_size(px(12.0))
        .child(label)
}

/// Spotlight launcher: centered rounded panel, large field, live results, keyboard nav.
pub struct SpotlightView {
    pub model: Spotlight,
    pub selected: usize,
    focus: Option<gpui::FocusHandle>,
    want_focus: bool,
    watching: bool,
    last_frame: Option<Instant>,
    icons: HashMap<String, Arc<RenderImage>>,
    icon_pending: HashSet<String>,
}

impl SpotlightView {
    pub fn new(model: Spotlight) -> Self {
        Self {
            model,
            selected: 0,
            focus: None,
            want_focus: true,
            watching: false,
            last_frame: None,
            icons: HashMap::new(),
            icon_pending: HashSet::new(),
        }
    }

    /// Toggle the Spotlight palette. Returns true when it is open afterwards.
    pub fn toggle(&mut self) -> bool {
        self.model.toggle();
        if self.model.is_open() {
            self.selected = 0;
            self.want_focus = true;
            self.last_frame = None;
        }
        self.model.is_open()
    }

    /// Close and hide the palette window (Esc, click outside, launch).
    fn dismiss(&mut self, window: &mut Window) {
        self.model.close_palette();
        self.model.query.clear();
        self.selected = 0;
        hide_palette_window(window);
    }

    /// Real app/file icons for the visible rows, loaded off the UI thread.
    fn request_icons(&mut self, ids: &[String], cx: &mut Context<Self>) {
        for id in ids {
            let Some(target) = crate::catalog::target_of(id) else {
                continue;
            };
            if self.icons.contains_key(&target) || !self.icon_pending.insert(target.clone()) {
                continue;
            }
            let path = target.clone();
            cx.spawn(async move |this, cx| {
                let raster = cx
                    .background_executor()
                    .spawn(async move { crate::icons::app_icon_rgba(&path) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(img) = raster.and_then(|(w, h, b)| raster_icon(w, h, b)) {
                        this.icons.insert(target, img);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    /// Record a launch for the highlighted row, close the palette, and return
    /// the hit id so the host can focus or open the matching window.
    pub fn activate_selected(&mut self) -> Option<String> {
        let hits = self.model.query_str(&self.model.query);
        if hits.is_empty() {
            self.model.close_palette();
            return None;
        }
        let idx = self.selected.min(hits.len() - 1);
        let id = hits[idx].id.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1)
            .max(1);
        self.model.activate(&id, now);
        self.model.close_palette();
        Some(id)
    }
}

fn kind_tag(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Calculator => "CALC",
        ProviderKind::App => "APP",
        ProviderKind::Setting => "SET",
        ProviderKind::File => "FILE",
        ProviderKind::Clipboard => "CLIP",
    }
}

/// Hide the Spotlight window without re-entering its window procedure.
fn hide_palette_window(window: &mut Window) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(w) = handle.as_raw() {
                let hwnd = windows::Win32::Foundation::HWND(w.hwnd.get() as *mut std::ffi::c_void);
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                    );
                }
                return;
            }
        }
    }
    #[cfg(all(unix, not(test)))]
    window.minimize_window();
    let _ = window;
}

/// Vector glyph for a row without a real icon.
fn kind_glyph(kind: ProviderKind) -> Option<GlyphIcon> {
    match kind {
        ProviderKind::Calculator => None,
        ProviderKind::App => Some(GlyphIcon::Tray),
        ProviderKind::Setting => Some(GlyphIcon::Gear),
        ProviderKind::File => Some(GlyphIcon::Folder),
        ProviderKind::Clipboard => Some(GlyphIcon::Clipboard),
    }
}

#[allow(dead_code)]
fn kind_mark(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Calculator => "=",
        ProviderKind::App => "▶",
        ProviderKind::Setting => "⚙",
        ProviderKind::File => "▣",
        ProviderKind::Clipboard => "⧉",
    }
}

impl Render for SpotlightView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme_of(cx);
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if self.want_focus && self.model.is_open() {
            self.want_focus = false;
            window.focus(&focus);
        }
        if !self.watching {
            self.watching = true;
            // Click anywhere else and Spotlight goes away, like macOS.
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && this.model.is_open() {
                    this.dismiss(window);
                    cx.notify();
                }
            })
            .detach();
        }
        // Appear: a short spring fade/lift, stepped with the real frame time.
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 120.0)
            .clamp(1.0 / 240.0, 1.0 / 30.0);
        self.last_frame = Some(now);
        self.model.step(dt);
        if self.model.animating() {
            window.request_animation_frame();
        }
        let appear = self.model.open_amount().clamp(0.0, 1.0);
        let hits = self.model.query_str(&self.model.query);
        let icon_ids: Vec<String> = hits
            .iter()
            .filter(|h| h.id.starts_with("app:") || h.id.starts_with("doc:"))
            .map(|h| h.id.clone())
            .collect();
        self.request_icons(&icon_ids, cx);
        let row_icons: Vec<Option<Arc<RenderImage>>> = hits
            .iter()
            .map(|h| crate::catalog::target_of(&h.id).and_then(|t| self.icons.get(&t).cloned()))
            .collect();
        let hit_count = hits.len();
        if self.selected >= hit_count && hit_count > 0 {
            self.selected = hit_count - 1;
        }
        let selected = self.selected;
        let query_empty = self.model.query.is_empty();
        let field_text = if query_empty {
            "Search apps, settings, files…".to_string()
        } else {
            self.model.query.clone()
        };

        let card = div()
            .id("spotlight")
            .track_focus(&focus)
            .flex()
            .flex_col()
            .w(px(680.0))
            .opacity(0.35 + 0.65 * appear)
            .mt(px((1.0 - appear) * 10.0))
            .bg(Hsla::from(theme.palette.menu_bg))
            .border_1()
            .border_color(Hsla::from(theme.palette.separator))
            .rounded(px(20.0))
            .shadow_lg()
            .overflow_hidden()
            .font_family(sf_font_family())
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let mods = event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();
                // Ctrl+Backspace / Ctrl+U clear the field; other chords pass.
                if mods.control && matches!(key, "backspace" | "u") {
                    this.model.query.clear();
                    this.selected = 0;
                    cx.notify();
                    return;
                }
                if mods.control || mods.alt || mods.platform || mods.function {
                    return;
                }
                let count = this.model.query_str(&this.model.query).len();
                let last = count.saturating_sub(1);
                match key {
                    "up" | "arrowup" => {
                        this.selected = if this.selected == 0 {
                            last
                        } else {
                            this.selected - 1
                        };
                    }
                    "down" | "arrowdown" => {
                        this.selected = if this.selected >= last {
                            0
                        } else {
                            this.selected + 1
                        };
                    }
                    "tab" => {
                        this.selected = if mods.shift {
                            if this.selected == 0 {
                                last
                            } else {
                                this.selected - 1
                            }
                        } else if this.selected >= last {
                            0
                        } else {
                            this.selected + 1
                        };
                    }
                    "home" | "pageup" => this.selected = 0,
                    "end" | "pagedown" => this.selected = last,
                    "enter" | "return" => {
                        if let Some(id) = this.activate_selected() {
                            this.dismiss(window);
                            cx.defer(move |cx| request_shell_launch(cx, &id));
                        }
                    }
                    "escape" => {
                        // First Esc clears the query, the second closes.
                        if this.model.query.is_empty() {
                            this.dismiss(window);
                        } else {
                            this.model.query.clear();
                            this.selected = 0;
                        }
                    }
                    "backspace" => {
                        this.model.query.pop();
                        this.selected = 0;
                    }
                    "space" => {
                        this.model.query.push(' ');
                        this.selected = 0;
                    }
                    _ if event.keystroke.key_char.is_some() => {
                        // key_char honours the keyboard layout and Shift
                        // (symbols, accents), not just ASCII letters.
                        if let Some(text) = event.keystroke.key_char.as_ref() {
                            if this.model.query.len() < 256 {
                                this.model.query.push_str(text);
                            }
                        }
                        this.selected = 0;
                    }
                    k if k.chars().count() == 1 => {
                        let ch = k.chars().next().unwrap_or(' ');
                        let ch = if mods.shift {
                            ch.to_ascii_uppercase()
                        } else {
                            ch
                        };
                        this.model.query.push(ch);
                        this.selected = 0;
                    }
                    _ => {}
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(22.0))
                    .py(px(18.0))
                    .gap(px(14.0))
                    .border_b_1()
                    .border_color(Hsla::from(theme.palette.separator))
                    .rounded_t(px(20.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(32.0))
                            .h(px(32.0))
                            .rounded(px(theme.radii.sm))
                            .bg(Hsla::from(theme.palette.fill))
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0))
                            .text_color(theme.text_primary_hsla())
                            .child("⌕"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(22.0))
                            .font_weight(FontWeight::MEDIUM)
                            .when(query_empty, |d| d.text_color(theme.text_tertiary_hsla()))
                            .when(!query_empty, |d| d.text_color(theme.text_primary_hsla()))
                            .child(field_text),
                    )
                    .child(
                        div()
                            .font_family(mono_font_family())
                            .text_size(px(11.0))
                            .text_color(theme.text_tertiary_hsla())
                            .child("Alt Space"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(10.0))
                    .gap(px(4.0))
                    .max_h(px(420.0))
                    .children(hits.into_iter().enumerate().map(|(idx, hit)| {
                        let row_icon = row_icons.get(idx).cloned().flatten();
                        let is_selected = idx == selected;
                        div()
                            .id(("spotlight-hit", idx))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(12.0))
                            .py(px(10.0))
                            .rounded(px(theme.radii.md))
                            .when(is_selected, |d| {
                                d.bg(theme.inverted_hsla())
                                    .text_color(theme.on_inverted_hsla())
                            })
                            .when(!is_selected, |d| d.hover(|s| s.bg(theme.hover_hsla())))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selected = idx;
                                if let Some(id) = this.activate_selected() {
                                    this.dismiss(window);
                                    cx.defer(move |cx| request_shell_launch(cx, &id));
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .w(px(32.0))
                                            .h(px(32.0))
                                            .rounded(px(theme.radii.sm))
                                            .when(is_selected, |d| {
                                                d.bg(theme.on_inverted_hsla())
                                                    .text_color(theme.inverted_hsla())
                                            })
                                            .when(!is_selected, |d| {
                                                d.bg(Hsla::from(theme.palette.fill))
                                                    .text_color(theme.text_primary_hsla())
                                            })
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(14.0))
                                            .map(|d| {
                                                let fg = if is_selected {
                                                    theme.inverted_hsla()
                                                } else {
                                                    theme.text_primary_hsla()
                                                };
                                                match (row_icon.clone(), kind_glyph(hit.kind)) {
                                                    (Some(image), _) => d.child(
                                                        img(ImageSource::Render(image))
                                                            .w(px(24.0))
                                                            .h(px(24.0)),
                                                    ),
                                                    (None, Some(g)) => {
                                                        d.child(glyph_icon(g, fg, 18.0))
                                                    }
                                                    (None, None) => d.child("="),
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.0))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_size(px(15.0))
                                                    .when(is_selected, |d| {
                                                        d.text_color(theme.on_inverted_hsla())
                                                    })
                                                    .when(!is_selected, |d| {
                                                        d.text_color(theme.text_primary_hsla())
                                                    })
                                                    .child(hit.title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .when(is_selected, |d| {
                                                        d.text_color(theme.on_inverted_hsla())
                                                    })
                                                    .when(!is_selected, |d| {
                                                        d.text_color(theme.text_secondary_hsla())
                                                    })
                                                    .child(hit.subtitle),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .when_some(hit.value, |d, val| {
                                        d.child(
                                            div()
                                                .font_family(mono_font_family())
                                                .font_weight(FontWeight::BOLD)
                                                .text_size(px(16.0))
                                                .child(val),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(theme.radii.pill))
                                            .when(is_selected, |d| {
                                                d.bg(theme.on_inverted_hsla())
                                                    .text_color(theme.inverted_hsla())
                                            })
                                            .when(!is_selected, |d| {
                                                d.bg(Hsla::from(theme.palette.fill))
                                                    .text_color(theme.text_secondary_hsla())
                                            })
                                            .font_family(mono_font_family())
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::BOLD)
                                            .child(kind_tag(hit.kind)),
                                    ),
                            )
                    })),
            );

        div()
            .id("spotlight-root")
            .flex()
            .w_full()
            .h_full()
            .items_start()
            .justify_center()
            .p(px(20.0))
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::DockPlacement;
    use crate::spotlight::Spotlight;
    use gpui::TestAppContext;
    use nx_ui::geom::Rect;
    use nx_wm::MonitorId;

    #[gpui::test]
    fn test_menu_bar_view_renders(cx: &mut TestAppContext) {
        let model = MenuBarModel::new();
        cx.update(|cx| {
            let entity = cx.new(|_cx| MenuBarView::new(model));
            let view = entity.read(cx);
            assert_eq!(view.model.app_name, "Nexus");
        });
    }

    #[gpui::test]
    fn test_dock_view_renders(cx: &mut TestAppContext) {
        let placement = DockPlacement::bottom(MonitorId(1), Rect::new(0.0, 0.0, 1920.0, 1080.0));
        let model = DockModel::new(placement);
        cx.update(|cx| {
            let entity = cx.new(|_cx| DockView::new(model));
            let view = entity.read(cx);
            assert!(view.model.items().is_empty());
        });
    }

    #[gpui::test]
    fn test_spotlight_view_renders(cx: &mut TestAppContext) {
        let mut model = Spotlight::new();
        model.query = "calc".to_string();
        cx.update(|cx| {
            let entity = cx.new(|_cx| SpotlightView::new(model));
            let view = entity.read(cx);
            assert_eq!(view.model.query, "calc");
        });
    }

    #[gpui::test]
    fn test_spotlight_enter_activates_selected_and_closes(cx: &mut TestAppContext) {
        let model = Spotlight::with_builtins();
        cx.update(|cx| {
            let entity = cx.new(|_cx| SpotlightView::new(model));
            entity.update(cx, |this, _cx| {
                assert!(this.toggle());
                this.model.query = "explorer".to_string();
                this.selected = 0;
                let id = this.activate_selected().expect("explorer hit");
                assert_eq!(id, "explorer");
                assert!(!this.model.is_open());
            });
        });
    }

    #[gpui::test]
    fn test_mission_control_view_renders(cx: &mut TestAppContext) {
        let canvas = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let model = ExposeModel::new(canvas);
        cx.update(|cx| {
            let entity = cx.new(|_cx| MissionControlView::new(model));
            let view = entity.read(cx);
            assert!(view.model.tiles().is_empty());
        });
    }
}
