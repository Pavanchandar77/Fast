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

use crate::util::{finite_f32, smoothstep};

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
            margin: 6.0,
            reveal_px: 8.0,
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
    let mut reveal = Spring::new(1.0, SpringConfig::macos(0.34, 1.0));
    reveal.config.rest_pos = 0.01;
    reveal.config.rest_vel = 0.04;
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
    let t = smoothstep(1.0 - distance / influence);
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
            pointer: None,
            context: None,
            last_command: None,
        }
    }

    pub fn items(&self) -> &[DockApp] {
        &self.items
    }

    pub fn pin(&mut self, id: impl Into<String>, label: impl Into<String>) {
        let id = id.into();
        let label = label.into();
        if let Some(existing) = self.items.iter_mut().find(|item| item.id == id) {
            existing.pinned = true;
            if existing.label.is_empty() {
                existing.label = label;
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
            });
        }
        self.reconcile_motion();
        self.retarget();
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
                id,
                label,
                pinned: false,
                running: true,
                frontmost: is_front,
                windows: wins,
                jumps: Vec::new(),
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
        if self.context.is_some() {
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
                point.y >= bounds.bottom() - reach && point.y <= bounds.bottom() + 2.0
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
}
