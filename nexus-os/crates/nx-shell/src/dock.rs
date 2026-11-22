//! Dock view-model.
//!
//! Pinned apps stay in the user's order. Running apps that are not pinned
//! append in nx-wm tree order, grouped by process. Magnification, launch
//! bounce, and auto-hide are nx-ui springs. Win+number and the jump menu are
//! the Windows taskbar behaviors kept on top of that.
//!
//! Pin state and jump lists live in memory for the session. They are not
//! written to disk, so exit cannot brick a taskbar layout.

use nx_ui::geom::{Point, Rect};
use nx_ui::spring::{Spring, SpringConfig};
use nx_wm::{MonitorId, WindowId, WindowManagerModel};

use crate::util::finite_f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEdge {
    Bottom,
    Left,
    Right,
}

impl DockEdge {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Bottom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockPlacement {
    pub monitor: MonitorId,
    pub edge: DockEdge,
    pub bounds: Rect,
    pub margin: f32,
    /// Pointer within this distance of the edge reveals an auto-hidden dock.
    pub reveal_px: f32,
}

impl DockPlacement {
    pub fn bottom(monitor: MonitorId, bounds: Rect) -> Self {
        Self {
            monitor,
            edge: DockEdge::Bottom,
            bounds,
            margin: 0.0,
            reveal_px: 48.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnification {
    pub enabled: bool,
    pub rest: f32,
    pub max_scale: f32,
    /// Distance along the dock axis at which scale falls back to 1.
    pub influence: f32,
    pub gap: f32,
    pub padding: f32,
    pub bounce: f32,
}

impl Magnification {
    pub fn macos(scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self {
            enabled: true,
            rest: (54.0 * scale).clamp(36.0, 96.0),
            max_scale: 1.7,
            influence: 148.0 * scale,
            gap: 8.0 * scale,
            padding: 8.0 * scale,
            bounce: 22.0 * scale,
        }
    }
}

impl Default for Magnification {
    fn default() -> Self {
        Self::macos(1.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockApp {
    pub id: String,
    pub label: String,
    pub pinned: bool,
    pub running: bool,
    pub frontmost: bool,
    pub windows: Vec<WindowId>,
    pub jumps: Vec<String>,
    /// What to start when the icon is clicked. Empty means `id`.
    pub launch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockCommand {
    Launch,
    Focus(WindowId),
    Pin,
    Unpin,
    Close(WindowId),
    CloseAll,
    Jump(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JumpEntry {
    Pin,
    Unpin,
    CloseAll,
    Window { window: WindowId, title: String },
    Custom { index: usize, label: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpMenu {
    pub index: usize,
    pub entries: Vec<JumpEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockIcon {
    pub index: usize,
    pub rect: Rect,
    pub dot: Option<Rect>,
    pub scale: f32,
    pub bounce: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DockLayout {
    pub bounds: Rect,
    pub icons: Vec<DockIcon>,
    pub reveal: f32,
}

#[derive(Clone, Debug)]
struct IconMotion {
    id: String,
    scale: Spring,
    bounce: Spring,
}

impl IconMotion {
    fn rest(id: String) -> Self {
        let mut scale = Spring::new(1.0, SpringConfig::macos(0.28, 1.0));
        scale.config.rest_pos = 0.012;
        scale.config.rest_vel = 0.04;
        let mut bounce = Spring::new(0.0, SpringConfig::macos(0.5, 0.36));
        bounce.config.rest_pos = 0.4;
        bounce.config.rest_vel = 6.0;
        Self { id, scale, bounce }
    }
}

fn scale_spring() -> Spring {
    // Short period, a little overshoot: the shelf pops instead of gliding.
    let mut reveal = Spring::new(1.0, SpringConfig::macos(0.16, 0.58));
    reveal.config.rest_pos = 0.008;
    reveal.config.rest_vel = 0.25;
    reveal
}

/// Target scale for one icon. `axis` is the pointer coordinate on the dock axis.
pub fn magnification_target(center: f32, axis: f32, mag: Magnification) -> f32 {
    if !mag.enabled {
        return 1.0;
    }
    let max_scale = mag.max_scale.max(1.0);
    let influence = mag.influence.max(1.0);
    let distance = (center - axis).abs();
    if distance >= influence {
        return 1.0;
    }
    // Cosine falloff, as on macOS: full size under the pointer, easing to
    // rest at `influence` with a zero slope at both ends.
    let t = 0.5 * (1.0 + (std::f32::consts::PI * distance / influence).cos());
    1.0 + (max_scale - 1.0) * t
}

/// Lay icons out from the current scales. Bottom-aligned on a horizontal dock
/// so magnification grows upward, the way the macOS dock does.
pub fn layout_dock(
    scales: &[f32],
    bounces: &[f32],
    running: &[bool],
    placement: DockPlacement,
    mag: Magnification,
    reveal: f32,
) -> DockLayout {
    let n = scales.len();
    let reveal = finite_f32(reveal, 1.0).clamp(0.0, 1.0);
    if n == 0 {
        return DockLayout {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            icons: Vec::new(),
            reveal,
        };
    }
    let sizes: Vec<f32> = scales
        .iter()
        .map(|scale| mag.rest * finite_f32(*scale, 1.0).clamp(0.2, mag.max_scale.max(1.0) + 0.25))
        .collect();
    let bounce_at = |index: usize| bounces.get(index).copied().unwrap_or(0.0);
    let gap = mag.gap.max(0.0);
    let pad = mag.padding.max(0.0);
    let indicator = 6.0;
    let span = sizes.iter().sum::<f32>() + gap * n.saturating_sub(1) as f32;
    let thickness = sizes.iter().copied().fold(0.0f32, f32::max) + pad * 2.0 + indicator;
    let length = span + pad * 2.0;
    let hidden = (1.0 - reveal) * (thickness + placement.margin.max(0.0));
    let bounds = placement.bounds;

    let (bar, along_start, cross_edge) = match placement.edge {
        DockEdge::Bottom => {
            let x = bounds.x + (bounds.w - length) * 0.5;
            let y = bounds.bottom() - placement.margin - thickness + hidden;
            (
                Rect::new(x, y, length, thickness),
                x + pad,
                y + thickness - pad - indicator,
            )
        }
        DockEdge::Left => {
            let y = bounds.y + (bounds.h - length) * 0.5;
            let x = bounds.x + placement.margin - hidden;
            (Rect::new(x, y, thickness, length), y + pad, x + pad)
        }
        DockEdge::Right => {
            let y = bounds.y + (bounds.h - length) * 0.5;
            let x = bounds.right() - placement.margin - thickness + hidden;
            (
                Rect::new(x, y, thickness, length),
                y + pad,
                x + thickness - pad,
            )
        }
    };

    let mut icons = Vec::with_capacity(n);
    let mut cursor = along_start;
    for index in 0..n {
        let size = sizes[index];
        let bounce = bounce_at(index);
        let rect = match placement.edge {
            DockEdge::Bottom => Rect::new(cursor, cross_edge - size + bounce, size, size),
            DockEdge::Left => Rect::new(cross_edge, cursor + bounce, size, size),
            DockEdge::Right => Rect::new(cross_edge - size, cursor + bounce, size, size),
        };
        let dot = if running.get(index).copied().unwrap_or(false) {
            let dot = 4.0;
            Some(match placement.edge {
                DockEdge::Bottom => Rect::new(
                    rect.center().x - dot * 0.5,
                    bar.bottom() - pad - dot,
                    dot,
                    dot,
                ),
                DockEdge::Left => {
                    Rect::new(bar.x + pad * 0.5, rect.center().y - dot * 0.5, dot, dot)
                }
                DockEdge::Right => Rect::new(
                    bar.right() - pad * 0.5 - dot,
                    rect.center().y - dot * 0.5,
                    dot,
                    dot,
                ),
            })
        } else {
            None
        };
        icons.push(DockIcon {
            index,
            rect,
            dot,
            scale: finite_f32(scales[index], 1.0),
            bounce,
        });
        cursor += size + gap;
    }

    DockLayout {
        bounds: bar,
        icons,
        reveal,
    }
}

pub fn hit_dock(layout: &DockLayout, point: Point) -> Option<usize> {
    for icon in layout.icons.iter().rev() {
        if icon.rect.contains(point) {
            return Some(icon.index);
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct DockModel {
    pub placement: DockPlacement,
    pub mag: Magnification,
    pub auto_hide: bool,
    items: Vec<DockApp>,
    motion: Vec<IconMotion>,
    reveal: Spring,
    hold: bool,
    pointer: Option<Point>,
    pub context: Option<JumpMenu>,
    pub last_command: Option<(usize, DockCommand)>,
}

impl DockModel {
    pub fn new(placement: DockPlacement) -> Self {
        Self {
            mag: Magnification::macos(1.0),
            placement,
            auto_hide: false,
            items: Vec::new(),
            motion: Vec::new(),
            reveal: scale_spring(),
            hold: false,
            pointer: None,
            context: None,
            last_command: None,
        }
    }

    pub fn items(&self) -> &[DockApp] {
        &self.items
    }

    pub fn pin(&mut self, id: impl Into<String>, label: impl Into<String>) {
        self.pin_launch(id, label, "");
    }

    /// Pin an app. `launch` is the path started on click (a taskbar shortcut).
    /// `id` is the exe name used to light the running dot.
    pub fn pin_launch(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        launch: impl Into<String>,
    ) {
        let id = id.into();
        let label = label.into();
        let launch = launch.into();
        if let Some(existing) = self.items.iter_mut().find(|item| item.id == id) {
            existing.pinned = true;
            if existing.label.is_empty() {
                existing.label = label;
            }
            if !launch.is_empty() {
                existing.launch = launch;
            }
        } else {
            self.items.push(DockApp {
                id: id.clone(),
                label,
                pinned: true,
                running: false,
                frontmost: false,
                windows: Vec::new(),
                jumps: Vec::new(),
                launch: if launch.is_empty() { id } else { launch },
            });
        }
        self.reconcile_motion();
        self.retarget();
    }

    /// Nexus surfaces only. The user's own apps come from the Windows taskbar.
    pub fn pin_defaults(&mut self) {
        for (id, label) in [
            ("explorer", "Explorer"),
            ("privacy", "Privacy"),
            ("trash", "Trash"),
        ] {
            self.pin(id, label);
        }
    }

    /// Pins the user added to the Windows taskbar, in name order.
    pub fn pin_taskbar(&mut self) {
        for (id, label, launch) in taskbar_pins() {
            self.pin_launch(id, label, launch);
        }
    }

    pub fn reveal(&self) -> f32 {
        self.reveal.position
    }

    /// Light the running dot for pinned apps whose exe is in `names`.
    /// The built-in shelf is left to the session.
    pub fn note_running_exes(&mut self, names: &[String]) {
        for item in &mut self.items {
            if matches!(item.id.as_str(), "explorer" | "privacy" | "trash") {
                continue;
            }
            if item.windows.is_empty() {
                item.running = names.iter().any(|n| n == &item.id);
            }
        }
    }

    /// Show every app currently on the taskbar, pinned or not.
    /// Pinned icons stay put. An app that closes drops off unless it is pinned.
    pub fn note_taskbar(&mut self, buttons: &[(String, String, String)]) {
        let mut live = std::collections::HashSet::new();
        let mut changed = false;
        for (id, label, launch) in buttons {
            live.insert(id.clone());
            if let Some(item) = self.items.iter_mut().find(|item| &item.id == id) {
                if item.windows.is_empty() && !item.running {
                    item.running = true;
                    changed = true;
                }
                if (item.launch.is_empty() || item.launch == item.id) && !launch.is_empty() {
                    item.launch = launch.clone();
                }
            } else {
                self.items.push(DockApp {
                    id: id.clone(),
                    label: label.clone(),
                    pinned: false,
                    running: true,
                    frontmost: false,
                    windows: Vec::new(),
                    jumps: Vec::new(),
                    launch: if launch.is_empty() {
                        id.clone()
                    } else {
                        launch.clone()
                    },
                });
                changed = true;
            }
        }
        let before = self.items.len();
        self.items.retain(|item| {
            item.pinned
                || matches!(item.id.as_str(), "explorer" | "privacy" | "trash")
                || live.contains(&item.id)
        });
        if self.items.len() != before {
            changed = true;
        }
        for item in &mut self.items {
            if item.pinned || matches!(item.id.as_str(), "explorer" | "privacy" | "trash") {
                continue;
            }
            if item.windows.is_empty() && !live.contains(&item.id) && item.running {
                item.running = false;
                changed = true;
            }
        }
        if changed {
            self.reconcile_motion();
            self.retarget();
        }
    }

    pub fn set_running(&mut self, id: &str, running: bool) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.running = running;
        }
    }

    pub fn unpin(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.pinned = false;
        }
        self.items.retain(|item| item.pinned || item.running);
        self.reconcile_motion();
        self.retarget();
    }

    pub fn set_jumps(&mut self, id: &str, jumps: Vec<String>) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.jumps = jumps;
        }
    }

    pub fn set_auto_hide(&mut self, enabled: bool) {
        self.auto_hide = enabled;
        if enabled && self.pointer.is_none() && self.context.is_none() {
            self.reveal.position = 0.0;
            self.reveal.velocity = 0.0;
            self.reveal.set_target(0.0);
        }
        self.retarget();
    }

    pub fn set_placement(&mut self, placement: DockPlacement) {
        self.placement = placement;
        self.retarget();
    }

    /// Group windows on this monitor by process. Pinned order is preserved.
    /// Unpinned running apps append in first-seen window-id order.
    pub fn sync_running(&mut self, wm: &WindowManagerModel) {
        let monitor = self.placement.monitor;
        let mut windows: Vec<_> = wm
            .all_windows()
            .filter(|window| window.monitor_id == monitor && !window.is_cloaked)
            .collect();
        windows.sort_by_key(|window| window.id.0);
        let front = wm.active_window();

        let mut groups: Vec<(String, String, Vec<WindowId>, bool)> = Vec::new();
        for window in windows {
            let id = normalize_id(&window.process_name);
            if id.is_empty() {
                continue;
            }
            let is_front = front == Some(window.id);
            if let Some(group) = groups.iter_mut().find(|group| group.0 == id) {
                group.2.push(window.id);
                group.3 |= is_front;
            } else {
                groups.push((
                    id,
                    display_label(&window.process_name),
                    vec![window.id],
                    is_front,
                ));
            }
        }

        for item in &mut self.items {
            item.running = false;
            item.frontmost = false;
            item.windows.clear();
        }
        for (id, label, wins, is_front) in &groups {
            if let Some(item) = self.items.iter_mut().find(|item| item.id == *id) {
                item.running = true;
                item.frontmost = *is_front;
                item.windows = wins.clone();
                if item.label.is_empty() {
                    item.label = label.clone();
                }
            }
        }
        for (id, label, wins, is_front) in groups {
            if self.items.iter().any(|item| item.id == id) {
                continue;
            }
            self.items.push(DockApp {
                id: id.clone(),
                label,
                pinned: false,
                running: true,
                frontmost: is_front,
                windows: wins,
                jumps: Vec::new(),
                launch: id,
            });
        }
        self.items.retain(|item| item.pinned || item.running);
        // Pinned items stay where the user put them. Running unpinned items
        // keep insertion order, which follows sorted window ids.
        self.reconcile_motion();
        self.retarget();
    }

    pub fn pointer_moved(&mut self, point: Point) {
        self.pointer = Some(point);
        self.retarget();
    }

    pub fn pointer_left(&mut self) {
        self.pointer = None;
        self.retarget();
    }

    /// The pointer is on the physical bottom edge. `retarget` can disagree
    /// with that and park the shelf; this puts the target back to open.
    pub fn force_reveal(&mut self) {
        self.reveal.set_target(1.0);
    }

    /// Keep an auto-hidden dock open while the pointer is on the screen edge
    /// or over the shelf (physical-pixel test done by the view).
    pub fn set_hold(&mut self, hold: bool) {
        if self.hold != hold {
            self.hold = hold;
            self.retarget();
        }
    }

    /// A jump menu is open.
    pub fn menu_open(&self) -> bool {
        self.context.is_some()
    }

    pub fn step(&mut self, dt: f32) {
        self.retarget();
        for motion in &mut self.motion {
            motion.scale.step(dt);
            motion.bounce.step(dt);
        }
        self.reveal.step(dt);
    }

    pub fn animating(&self) -> bool {
        if !self.reveal.settled() {
            return true;
        }
        self.motion
            .iter()
            .any(|motion| !motion.scale.settled() || !motion.bounce.settled())
    }

    pub fn scales(&self) -> Vec<f32> {
        self.motion
            .iter()
            .map(|motion| motion.scale.position)
            .collect()
    }

    pub fn layout(&self) -> DockLayout {
        let scales = self.scales();
        let bounces: Vec<f32> = self
            .motion
            .iter()
            .map(|motion| motion.bounce.position)
            .collect();
        let running: Vec<bool> = self.items.iter().map(|item| item.running).collect();
        layout_dock(
            &scales,
            &bounces,
            &running,
            self.placement,
            self.mag,
            self.reveal.position,
        )
    }

    /// Win+1 .. Win+9. Slot 1 is the leftmost (or top) dock icon.
    /// Running apps focus their first window. Others launch.
    pub fn activate_slot(&mut self, slot: u8) -> Option<DockCommand> {
        if slot == 0 || slot > 9 {
            return None;
        }
        let index = (slot - 1) as usize;
        self.activate_index(index)
    }

    pub fn activate_index(&mut self, index: usize) -> Option<DockCommand> {
        let item = self.items.get(index)?;
        let command = if item.running {
            match item.windows.first().copied() {
                Some(window) => DockCommand::Focus(window),
                None => DockCommand::Launch,
            }
        } else {
            DockCommand::Launch
        };
        self.bounce_index(index);
        self.last_command = Some((index, command));
        self.retarget();
        Some(command)
    }

    pub fn bounce_index(&mut self, index: usize) {
        if let Some(motion) = self.motion.get_mut(index) {
            motion.bounce.position = -self.mag.bounce;
            motion.bounce.velocity = 0.0;
            motion.bounce.set_target(0.0);
        }
    }

    pub fn context_menu(&mut self, index: usize) -> Option<&JumpMenu> {
        let item = self.items.get(index)?;
        let mut entries = Vec::new();
        if item.pinned {
            entries.push(JumpEntry::Unpin);
        } else {
            entries.push(JumpEntry::Pin);
        }
        for (jump_index, label) in item.jumps.iter().enumerate() {
            entries.push(JumpEntry::Custom {
                index: jump_index,
                label: label.clone(),
            });
        }
        for window in &item.windows {
            entries.push(JumpEntry::Window {
                window: *window,
                title: format!("Window {}", window.0),
            });
        }
        if item.running {
            entries.push(JumpEntry::CloseAll);
        }
        self.context = Some(JumpMenu { index, entries });
        self.retarget();
        self.context.as_ref()
    }

    /// Apply the local part of a command (pin / unpin). Focus, launch, and
    /// close are returned for the host to run through nx-wm and, if they
    /// mutate the OS, through the journal. The dock itself does not.
    pub fn perform(&mut self, index: usize, command: DockCommand) -> Option<DockCommand> {
        let id = self.items.get(index).map(|item| item.id.clone())?;
        match command {
            DockCommand::Pin => self.pin(id, ""),
            DockCommand::Unpin => self.unpin(&id),
            DockCommand::Jump(_)
            | DockCommand::Launch
            | DockCommand::Focus(_)
            | DockCommand::Close(_)
            | DockCommand::CloseAll => {}
        }
        self.last_command = Some((index, command));
        self.context = None;
        self.retarget();
        Some(command)
    }

    fn reconcile_motion(&mut self) {
        let mut old = std::mem::take(&mut self.motion);
        for item in &self.items {
            if let Some(pos) = old.iter().position(|motion| motion.id == item.id) {
                self.motion.push(old.swap_remove(pos));
            } else {
                self.motion.push(IconMotion::rest(item.id.clone()));
            }
        }
    }

    fn retarget(&mut self) {
        let rest_centers = rest_centers(self.items.len(), self.placement, self.mag);
        let show_mag = self.mag.enabled && self.reveal.position > 0.35;
        for (index, motion) in self.motion.iter_mut().enumerate() {
            let target = match self.pointer {
                Some(point) if show_mag => {
                    let axis = if self.placement.edge.is_horizontal() {
                        point.x
                    } else {
                        point.y
                    };
                    let center = rest_centers.get(index).copied().unwrap_or(axis);
                    magnification_target(center, axis, self.mag)
                }
                _ => 1.0,
            };
            motion.scale.set_target(target);
        }
        let show = self.wants_visible();
        self.reveal.set_target(if show { 1.0 } else { 0.0 });
    }

    fn wants_visible(&self) -> bool {
        if !self.auto_hide {
            return true;
        }
        if self.context.is_some() || self.hold {
            return true;
        }
        if self.motion.iter().any(|motion| !motion.bounce.settled()) {
            return true;
        }
        match self.pointer {
            Some(point) => self.near_edge(point),
            None => false,
        }
    }

    fn near_edge(&self, point: Point) -> bool {
        let bounds = self.placement.bounds;
        let reach = self.placement.reveal_px.max(0.0) + self.placement.margin;
        match self.placement.edge {
            DockEdge::Bottom => {
                // The pointer is in physical pixels divided by the window scale.
                // The screen edge can sit a few dozen px past the logical bottom
                // (taskbar, DPI). A 2px slop made the shelf stay hidden.
                point.y >= bounds.bottom() - reach && point.y <= bounds.bottom() + 96.0
            }
            DockEdge::Left => point.x <= bounds.x + reach,
            DockEdge::Right => point.x >= bounds.right() - reach,
        }
    }

    pub fn motion_bounce(&self, index: usize) -> f32 {
        self.motion
            .get(index)
            .map(|motion| motion.bounce.position)
            .unwrap_or(0.0)
    }
}

/// Shortcuts the user pinned to the Windows taskbar.
pub fn taskbar_pins() -> Vec<(String, String, String)> {
    #[cfg(windows)]
    {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return Vec::new();
        };
        let dir = std::path::PathBuf::from(appdata)
            .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
        let mut pins = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("lnk"))
            {
                continue;
            }
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            if stem.eq_ignore_ascii_case("File Explorer") {
                continue;
            }
            let id = exe_id_from_lnk(&path).unwrap_or_else(|| stem.to_ascii_lowercase());
            if matches!(id.as_str(), "explorer" | "nexus" | "nexus-os") {
                continue;
            }
            pins.push((id, stem, path.to_string_lossy().to_string()));
        }
        pins.sort_by_key(|a| a.1.to_ascii_lowercase());
        pins
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Apps that currently have a taskbar button: (exe id, window title, exe path).
pub fn taskbar_buttons() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (id, label, launch, _) in taskbar_rows() {
        if seen.insert(id.clone()) {
            out.push((id, label, launch));
        }
    }
    out
}

/// Bring a running app's windows forward: restore minimized ones and focus
/// the first. Returns false when the app has no taskbar window (launch it).
pub fn focus_app(id: &str) -> bool {
    let hwnds: Vec<isize> = taskbar_rows()
        .into_iter()
        .filter(|row| row.0 == id)
        .map(|row| row.3)
        .collect();
    if hwnds.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            IsIconic, SetForegroundWindow, ShowWindow, SwitchToThisWindow, SW_RESTORE,
        };
        unsafe {
            for raw in hwnds.iter().rev() {
                let h = HWND(*raw as *mut std::ffi::c_void);
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
            }
            let first = HWND(hwnds[0] as *mut std::ffi::c_void);
            // The dock does not take focus, so Windows may refuse a plain
            // SetForegroundWindow; Alt+Tab's switch is allowed.
            if !SetForegroundWindow(first).as_bool() {
                SwitchToThisWindow(first, true);
            }
        }
    }
    true
}

/// Every taskbar-style window: (app id, title, exe path, hwnd).
fn taskbar_rows() -> Vec<(String, String, String, isize)> {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };

        struct Row {
            id: String,
            label: String,
            launch: String,
            hwnd: isize,
        }
        let mut rows: Vec<Row> = Vec::new();
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // A panic inside EnumWindows is delivered as a fatal callback
            // exception and the process disappears.
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                each_window(hwnd, lparam)
            }));
            caught.unwrap_or(BOOL(1))
        }
        unsafe fn each_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let rows = &mut *(lparam.0 as *mut Vec<Row>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let owned = GetWindow(hwnd, GW_OWNER).unwrap_or(HWND(std::ptr::null_mut()));
            let tool = ex & WS_EX_TOOLWINDOW.0 != 0;
            let app = ex & WS_EX_APPWINDOW.0 != 0;
            if tool && !app {
                return BOOL(1);
            }
            if !owned.0.is_null() && !app {
                return BOOL(1);
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            if matches!(
                class.as_str(),
                "Shell_TrayWnd"
                    | "Shell_SecondaryTrayWnd"
                    | "Progman"
                    | "WorkerW"
                    | "DV2ControlHost"
                    | "MsgrIMEWindowClass"
                    | "Windows.UI.Core.CoreWindow"
            ) {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return BOOL(1);
            }
            let len = len.min(512);
            let mut title = vec![0u16; len as usize + 1];
            let got = GetWindowTextW(hwnd, &mut title) as usize;
            let label = String::from_utf16_lossy(&title[..got]).trim().to_string();
            if label.is_empty() {
                return BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return BOOL(1);
            }
            let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return BOOL(1);
            };
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let path = if QueryFullProcessImageNameW(
                proc,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &raw mut size,
            )
            .is_ok()
            {
                OsString::from_wide(&buf[..size as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                String::new()
            };
            let _ = CloseHandle(proc);
            let mut id = normalize_id(&path);
            if id.is_empty() {
                id = label.to_ascii_lowercase();
            }
            if matches!(
                id.as_str(),
                "explorer"
                    | "nexus"
                    | "nexus-os"
                    | "searchhost"
                    | "startmenuexperiencehost"
                    | "textinputhost"
                    | "systemsettings"
            ) {
                return BOOL(1);
            }
            // One process hosts every Store app. Keep each window.
            if id == "applicationframehost" {
                id = format!("app-{}", label.to_ascii_lowercase());
            }
            rows.push(Row {
                id,
                label,
                launch: path,
                hwnd: hwnd.0 as isize,
            });
            BOOL(1)
        }
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&raw mut rows as isize));
        }
        rows.into_iter()
            .map(|row| (row.id, row.label, row.launch, row.hwnd))
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Image names of running processes, lowercased and without `.exe`.
pub fn running_exe_ids() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return out;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..PROCESSENTRY32W::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    let id = normalize_id(&name);
                    if !id.is_empty() {
                        out.push(id);
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Best-effort exe stem from a `.lnk`. The shortcut's target is stored as
/// UTF-16 near the `.exe` marker.
#[cfg_attr(not(windows), allow(dead_code))]
fn exe_id_from_lnk(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut i = 0;
    while i + 8 <= bytes.len() {
        let exe = bytes[i] == b'.'
            && bytes[i + 1] == 0
            && bytes[i + 2].eq_ignore_ascii_case(&b'e')
            && bytes[i + 3] == 0
            && bytes[i + 4].eq_ignore_ascii_case(&b'x')
            && bytes[i + 5] == 0
            && bytes[i + 6].eq_ignore_ascii_case(&b'e')
            && bytes[i + 7] == 0;
        if exe {
            let mut start = i;
            while start >= 2 {
                let prev = u16::from_le_bytes([bytes[start - 2], bytes[start - 1]]);
                if prev == 0 || prev < 32 {
                    break;
                }
                start -= 2;
            }
            let mut name_at = start;
            let mut k = start;
            while k + 1 < i {
                let ch = u16::from_le_bytes([bytes[k], bytes[k + 1]]);
                if ch == u16::from(b'\\') || ch == u16::from(b'/') {
                    name_at = k + 2;
                }
                k += 2;
            }
            let mut name = String::new();
            let mut k = name_at;
            while k + 1 < i {
                let ch = u16::from_le_bytes([bytes[k], bytes[k + 1]]);
                if ch == 0 {
                    break;
                }
                name.push(char::from_u32(u32::from(ch)).unwrap_or('?'));
                k += 2;
            }
            if !name.is_empty() {
                return Some(name.to_ascii_lowercase());
            }
        }
        i += 2;
    }
    None
}

fn normalize_id(process: &str) -> String {
    let trimmed = process.trim();
    let base = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    base.to_ascii_lowercase()
}

fn display_label(process: &str) -> String {
    let trimmed = process.trim();
    let base = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .to_string()
}

fn rest_centers(count: usize, placement: DockPlacement, mag: Magnification) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    let rest = mag.rest;
    let gap = mag.gap.max(0.0);
    let pad = mag.padding.max(0.0);
    let span = rest * count as f32 + gap * count.saturating_sub(1) as f32;
    let length = span + pad * 2.0;
    let start = match placement.edge {
        DockEdge::Bottom => placement.bounds.x + (placement.bounds.w - length) * 0.5 + pad,
        DockEdge::Left | DockEdge::Right => {
            placement.bounds.y + (placement.bounds.h - length) * 0.5 + pad
        }
    };
    (0..count)
        .map(|index| start + rest * 0.5 + (rest + gap) * index as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_wm::{Rect as WmRect, WindowId, WindowInfo, WindowManagerModel};

    fn screen() -> Rect {
        Rect::new(0.0, 0.0, 1440.0, 900.0)
    }

    fn placement() -> DockPlacement {
        DockPlacement::bottom(MonitorId(0), screen())
    }

    fn show(wm: &mut WindowManagerModel, id: u64, process: &str, title: &str) {
        wm.on_window_show(WindowInfo {
            id: WindowId(id),
            title: title.to_string(),
            class_name: "Nx".to_string(),
            process_name: process.to_string(),
            rect: WmRect::new(10, 10, 400, 300),
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            is_dialog_or_popup: false,
            style: 0,
            ex_style: 0,
        });
    }

    #[test]
    fn magnification_peaks_under_the_pointer_and_falls_off() {
        let mag = Magnification::macos(1.0);
        let near = magnification_target(100.0, 100.0, mag);
        let mid = magnification_target(100.0, 100.0 + mag.influence * 0.5, mag);
        let far = magnification_target(100.0, 100.0 + mag.influence + 4.0, mag);
        assert!((near - mag.max_scale).abs() < 1.0e-3);
        assert!(mid > 1.0 && mid < near);
        assert!((far - 1.0).abs() < 1.0e-3);
    }

    #[test]
    fn icons_do_not_overlap_and_running_dots_follow_the_flag() {
        let mag = Magnification::macos(1.0);
        let scales = [1.0, 1.7, 1.2, 1.0];
        let running = [true, true, false, true];
        let layout = layout_dock(&scales, &[0.0; 4], &running, placement(), mag, 1.0);
        assert_eq!(layout.icons.len(), 4);
        for pair in layout.icons.windows(2) {
            assert!(!pair[0].rect.intersects(pair[1].rect));
        }
        assert!(layout.icons[0].dot.is_some());
        assert!(layout.icons[2].dot.is_none());
        assert!(layout.icons[1].rect.h > layout.icons[0].rect.h);
        // Bottom aligned: the magnified icon's bottom matches the small one.
        assert!((layout.icons[0].rect.bottom() - layout.icons[1].rect.bottom()).abs() < 0.2);
    }

    #[test]
    fn side_dock_stays_on_the_left_edge() {
        let mut place = placement();
        place.edge = DockEdge::Left;
        let layout = layout_dock(
            &[1.0, 1.0],
            &[0.0, 0.0],
            &[true, false],
            place,
            Magnification::macos(1.0),
            1.0,
        );
        assert!(layout.bounds.x <= place.bounds.x + place.margin + 1.0);
        assert!(layout.icons[0].rect.y < layout.icons[1].rect.y);
        assert!(layout.icons[0].dot.is_some());
    }

    #[test]
    fn pinned_order_survives_running_apps_and_win_number_launches() {
        let mut dock = DockModel::new(placement());
        dock.pin("safari", "Safari");
        dock.pin("mail", "Mail");
        dock.pin("notes", "Notes");
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        show(&mut wm, 3, "Terminal.exe", "zsh");
        show(&mut wm, 1, "Mail.exe", "Inbox");
        dock.sync_running(&wm);
        let labels: Vec<_> = dock
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["Safari", "Mail", "Notes", "Terminal"]);
        assert!(dock.items()[1].running);
        assert!(!dock.items()[0].running);
        assert_eq!(dock.activate_slot(1), Some(DockCommand::Launch));
        assert_eq!(dock.activate_slot(2), Some(DockCommand::Focus(WindowId(1))));
        assert_eq!(dock.activate_slot(0), None);
        assert_eq!(dock.activate_slot(10), None);
        assert!(dock.animating());
    }

    #[test]
    fn jump_menu_pins_and_close_is_a_host_command() {
        let mut dock = DockModel::new(placement());
        dock.pin("code", "Code");
        dock.set_jumps("code", vec!["Recent file".to_string()]);
        let menu = dock.context_menu(0).expect("menu").clone();
        assert!(matches!(menu.entries[0], JumpEntry::Pin | JumpEntry::Unpin));
        assert!(menu
            .entries
            .iter()
            .any(|entry| matches!(entry, JumpEntry::Custom { .. })));
        dock.perform(0, DockCommand::Unpin);
        assert!(dock.items().is_empty());
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        show(&mut wm, 7, "Code.exe", "main.rs");
        dock.sync_running(&wm);
        let menu = dock.context_menu(0).expect("menu").clone();
        assert!(menu.entries.iter().any(
            |entry| matches!(entry, JumpEntry::Window { window, .. } if *window == WindowId(7))
        ));
        let command = dock.perform(0, DockCommand::Close(WindowId(7)));
        assert_eq!(command, Some(DockCommand::Close(WindowId(7))));
        // Close is not applied here. The window is still running in the model.
        assert!(dock.items()[0].running);
    }

    #[test]
    fn hover_magnifies_then_idles_when_the_spring_settles() {
        let mut dock = DockModel::new(placement());
        dock.pin("safari", "Safari");
        dock.pin("mail", "Mail");
        dock.pin("notes", "Notes");
        let centers = {
            let layout = dock.layout();
            layout.icons[1].rect.center()
        };
        dock.pointer_moved(centers);
        assert!(dock.animating());
        for _ in 0..180 {
            dock.step(1.0 / 120.0);
        }
        assert!(
            !dock.animating(),
            "settled magnification must stop requesting frames"
        );
        let scales = dock.scales();
        assert!(scales[1] > scales[0]);
        assert!(scales[1] > 1.4);
        dock.pointer_left();
        for _ in 0..180 {
            dock.step(1.0 / 120.0);
        }
        assert!(!dock.animating());
        assert!(dock
            .scales()
            .iter()
            .all(|scale| (*scale - 1.0).abs() < 0.05));
    }

    #[test]
    fn auto_hide_rests_off_screen_until_the_pointer_reaches_the_edge() {
        let mut dock = DockModel::new(placement());
        dock.pin("safari", "Safari");
        dock.set_auto_hide(true);
        for _ in 0..90 {
            dock.step(1.0 / 60.0);
        }
        assert!(!dock.animating());
        assert!(dock.reveal.position < 0.05);
        let hidden = dock.layout();
        assert!(hidden.bounds.y >= screen().bottom() - 1.0 || hidden.reveal < 0.05);
        dock.pointer_moved(Point::new(700.0, 898.0));
        for _ in 0..90 {
            dock.step(1.0 / 60.0);
        }
        assert!(dock.reveal.position > 0.9);
        assert!(dock.layout().bounds.y < screen().bottom() - 20.0);
    }

    #[test]
    fn per_monitor_filter_ignores_other_monitors() {
        let mut dock = DockModel::new(placement());
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        show(&mut wm, 1, "Safari.exe", "Apple");
        if let Some(window) = wm.window_mut(WindowId(1)) {
            window.monitor_id = MonitorId(1);
        }
        dock.sync_running(&wm);
        assert!(dock.items().is_empty());
        if let Some(window) = wm.window_mut(WindowId(1)) {
            window.monitor_id = MonitorId(0);
        }
        dock.sync_running(&wm);
        assert_eq!(dock.items().len(), 1);
        assert!(dock.items()[0].running);
    }

    #[test]
    fn launch_bounce_leaves_rest_and_returns() {
        let mut dock = DockModel::new(placement());
        dock.pin("safari", "Safari");
        dock.activate_slot(1);
        let start = dock.motion_bounce(0);
        assert!(start < -10.0);
        for _ in 0..240 {
            dock.step(1.0 / 120.0);
        }
        assert!(!dock.animating());
        assert!(dock.motion_bounce(0).abs() < 0.5);
    }

    #[test]
    fn pin_defaults_includes_shelf_apps_and_trash() {
        let mut dock = DockModel::new(placement());
        dock.pin_defaults();
        dock.set_running("explorer", true);
        let ids: Vec<&str> = dock.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["explorer", "privacy", "trash"]);
        assert!(dock.items()[0].running);
        assert!(dock.items().last().is_some_and(|i| i.id == "trash"));
        assert!(dock.items().iter().all(|item| item.id != "terminal"));
    }

    #[test]
    fn auto_hide_starts_parked() {
        let mut dock = DockModel::new(placement());
        dock.pin("chrome", "Chrome");
        dock.set_auto_hide(true);
        assert!(dock.reveal() < 0.05);
        assert!(!dock.wants_visible());
    }

    #[test]
    fn real_taskbar_is_on_the_dock_and_the_shelf_pops_fast() {
        if !cfg!(windows) {
            return;
        }
        let pins = taskbar_pins();
        for (_id, _label, path) in &pins {
            assert!(
                std::path::Path::new(path).is_file(),
                "pinned shortcut missing: {path}"
            );
        }
        let buttons = taskbar_buttons();
        let started = std::time::Instant::now();
        for _ in 0..40 {
            let _ = taskbar_buttons();
            let _ = running_exe_ids();
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "taskbar scan took {:?}",
            started.elapsed()
        );

        let mut dock = DockModel::new(DockPlacement::bottom(
            MonitorId(0),
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
        ));
        dock.pin_defaults();
        dock.pin_taskbar();
        dock.note_taskbar(&buttons);
        dock.set_auto_hide(true);
        let ids: Vec<&str> = dock.items().iter().map(|item| item.id.as_str()).collect();
        assert!(ids.contains(&"explorer"));
        assert!(ids.contains(&"privacy"));
        assert!(ids.contains(&"trash"));
        assert!(!ids.contains(&"terminal"));
        for (id, _label, path) in &pins {
            let item = dock
                .items()
                .iter()
                .find(|item| &item.id == id)
                .unwrap_or_else(|| panic!("pin {id} missing"));
            assert_eq!(&item.launch, path);
        }
        for (id, _label, path) in &buttons {
            let item = dock
                .items()
                .iter()
                .find(|item| &item.id == id)
                .unwrap_or_else(|| panic!("running app {id} missing"));
            assert!(item.running, "{id} has no running dot");
            if !path.is_empty() {
                assert!(std::path::Path::new(path).is_file(), "{path}");
            }
        }
        assert!(dock.reveal() < 0.05);
        dock.pointer_moved(Point::new(960.0, 1076.0));
        let mut shown_at = None;
        for step in 0..30 {
            dock.step(0.008);
            if dock.reveal() > 0.8 {
                shown_at = Some(step);
                break;
            }
        }
        let step = shown_at.expect("dock did not pop");
        assert!(step * 8 <= 200, "dock took {}ms to pop", step * 8);
    }

    #[test]
    fn magnification_uses_a_cosine_falloff() {
        let mag = Magnification::macos(1.0);
        let peak = magnification_target(100.0, 100.0, mag);
        assert!((peak - mag.max_scale).abs() < 1e-4);
        let half = magnification_target(100.0 + mag.influence * 0.5, 100.0, mag);
        let expected = 1.0 + (mag.max_scale - 1.0) * 0.5;
        assert!((half - expected).abs() < 1e-3, "half-way scale {half}");
        assert!((magnification_target(100.0 + mag.influence, 100.0, mag) - 1.0).abs() < 1e-4);
        let mut last = peak;
        for step in 1..=20 {
            let s = magnification_target(100.0 + step as f32 * mag.influence / 20.0, 100.0, mag);
            assert!(s <= last + 1e-5, "falloff must not rise");
            last = s;
        }
    }

    #[test]
    fn auto_hidden_dock_stays_open_while_held() {
        let mut dock = DockModel::new(placement());
        dock.pin_defaults();
        dock.set_auto_hide(true);
        for _ in 0..120 {
            dock.step(1.0 / 60.0);
        }
        assert!(dock.reveal() < 0.05, "parked by default");
        dock.set_hold(true);
        for _ in 0..120 {
            dock.step(1.0 / 60.0);
        }
        assert!(dock.reveal() > 0.95, "edge/hover hold reveals");
        dock.set_hold(false);
        dock.pointer_left();
        for _ in 0..120 {
            dock.step(1.0 / 60.0);
        }
        assert!(dock.reveal() < 0.05, "hides after the pointer leaves");
    }
}
