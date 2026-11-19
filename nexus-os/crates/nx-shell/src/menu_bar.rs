//! Menu bar view-model.
//!
//! The frontmost app comes from the focused window in [`nx_wm::WindowManagerModel`].
//! Status widgets read the existing privacy, volume, clipboard, and governor
//! models. They are not copied into a second store of truth: each `sync_*`
//! call snapshots only what the bar paints.

use nx_governor_policy::GovernorEngine;
use nx_power::{AudioSession, ClipboardStore, VolumeStore};
use nx_privacy_dash::{
    AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus, ConsentStoreSnapshot, FileTime,
    IndicatorChip, MenuBarPrivacyIndicators,
};
use nx_ui::geom::{Point, Rect};
use nx_ui::spring::{Spring, SpringConfig};
use nx_ui::Color;
use nx_wm::{WindowId, WindowManagerModel};

use crate::clock::ClockSample;
use crate::util::truncate_chars;

pub const MENU_TITLES: [&str; 5] = ["File", "Edit", "View", "Window", "Help"];

/// Logical height of the bar at scale 1. macOS is ~24pt; 28pt leaves room for
/// 13pt Inter plus the status pills.
pub fn bar_height(scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (28.0 * scale).max(20.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    About,
    Settings,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    NewWindow,
    CloseWindow,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    MissionControl,
    ShowDesktop,
    Minimize,
    Help,
    Window(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    pub action: Option<MenuAction>,
    pub separator: bool,
}

impl MenuEntry {
    pub fn item(label: impl Into<String>, action: MenuAction) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
            separator: false,
        }
    }

    pub fn separator() -> Self {
        Self {
            label: String::new(),
            action: None,
            separator: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Privacy,
    Volume,
    Clipboard,
    GameMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeWidget {
    pub label: String,
    pub session_id: Option<String>,
    pub level: f32,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardWidget {
    pub count: usize,
    pub recent: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusWidget {
    pub kind: StatusKind,
    pub label: String,
    pub tooltip: String,
    pub color: Color,
    pub badge: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarSlot {
    Mark,
    App,
    Menu(usize),
    Status(usize),
    Clock,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSlot {
    pub slot: BarSlot,
    pub label: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuBarLayout {
    pub bounds: Rect,
    pub slots: Vec<PlacedSlot>,
    pub separator: Rect,
}

#[derive(Clone, Debug)]
pub struct MeasuredSlot {
    pub slot: BarSlot,
    pub label: String,
    pub width: f32,
}

/// Pure geometry. `leading` is the mark, app name, and menus. `trailing` is
/// status widgets then the clock. Trailing stays pinned to the right edge.
/// Leading stops before it would overlap.
pub fn layout_menu_bar(
    width: f32,
    height: f32,
    pad_x: f32,
    gap: f32,
    leading: &[MeasuredSlot],
    trailing: &[MeasuredSlot],
) -> MenuBarLayout {
    let width = if width.is_finite() && width > 0.0 {
        width
    } else {
        1.0
    };
    let height = if height.is_finite() && height > 0.0 {
        height
    } else {
        1.0
    };
    let pad_x = pad_x.max(0.0);
    let gap = gap.max(0.0);
    let bounds = Rect::new(0.0, 0.0, width, height);
    let mut slots = Vec::with_capacity(leading.len() + trailing.len());

    let trailing_width = trailing
        .iter()
        .fold(0.0f32, |sum, slot| sum + slot.width.max(0.0))
        + gap * trailing.len().saturating_sub(1) as f32;
    let trailing_x = (width - pad_x - trailing_width).max(pad_x);
    let mut x = trailing_x;
    for slot in trailing {
        let w = slot.width.max(0.0);
        slots.push(PlacedSlot {
            slot: slot.slot,
            label: slot.label.clone(),
            rect: Rect::new(x, 0.0, w, height),
        });
        x += w + gap;
    }

    let mut x = pad_x;
    let limit = (trailing_x - gap).max(pad_x);
    for slot in leading {
        let w = slot.width.max(0.0);
        if x + w > limit && x > pad_x {
            break;
        }
        if x >= limit {
            break;
        }
        let w = w.min((limit - x).max(0.0));
        slots.push(PlacedSlot {
            slot: slot.slot,
            label: slot.label.clone(),
            rect: Rect::new(x, 0.0, w, height),
        });
        x += w + gap;
    }

    MenuBarLayout {
        bounds,
        slots,
        separator: Rect::new(0.0, (height - 1.0).max(0.0), width, 1.0),
    }
}

pub fn hit_menu_bar(layout: &MenuBarLayout, point: Point) -> Option<BarSlot> {
    if !layout.bounds.contains(point) {
        return None;
    }
    // Later slots are painted on top. Trailing is pushed first, leading after,
    // so a search from the end prefers leading when they were forced to overlap
    // on a tiny width. Walk from the end.
    for slot in layout.slots.iter().rev() {
        if slot.rect.contains(point) {
            return Some(slot.slot);
        }
    }
    None
}

fn app_name_from(process: &str, title: &str) -> String {
    let trimmed = process.trim();
    let base = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    if !base.is_empty() {
        return base.to_string();
    }
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    "Nexus".to_string()
}

fn parse_hex_color(text: &str) -> Color {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return Color::hex(0x8e8e93);
    }
    match u32::from_str_radix(hex, 16) {
        Ok(value) => Color::hex(value),
        Err(_) => Color::hex(0x8e8e93),
    }
}

fn volume_for<'a>(store: &'a VolumeStore, process: &str) -> Option<&'a AudioSession> {
    let want = app_name_from(process, "").to_ascii_lowercase();
    if want.is_empty() || want == "nexus" {
        return None;
    }
    store.list_sessions().into_iter().find(|session| {
        let name = session.name.to_ascii_lowercase();
        name == want || name.trim_end_matches(".exe") == want
    })
}

fn percent(level: f32) -> u8 {
    (level.clamp(0.0, 1.0) * 100.0).round().clamp(0.0, 100.0) as u8
}

/// Menu bar state. Open springs live here so the frame scheduler can see them.
#[derive(Clone, Debug)]
pub struct MenuBarModel {
    pub app_name: String,
    pub window_title: String,
    pub process_name: String,
    pub focused_window: Option<WindowId>,
    pub window_menu: Vec<(WindowId, String)>,
    pub clock: ClockSample,
    pub hour24: bool,
    pub privacy: Vec<IndicatorChip>,
    pub volume: VolumeWidget,
    pub clipboard: ClipboardWidget,
    pub game_mode: bool,
    pub open_menu: Option<BarSlot>,
    pub open: Spring,
    pub last_action: Option<MenuAction>,
}

impl MenuBarModel {
    pub fn new() -> Self {
        let mut open = Spring::new(0.0, SpringConfig::macos(0.22, 1.0));
        open.config.rest_pos = 0.01;
        open.config.rest_vel = 0.05;
        Self {
            app_name: "Nexus".to_string(),
            window_title: String::new(),
            process_name: String::new(),
            focused_window: None,
            window_menu: Vec::new(),
            clock: ClockSample::default(),
            hour24: true,
            privacy: Vec::new(),
            volume: VolumeWidget {
                label: "Volume".to_string(),
                session_id: None,
                level: 1.0,
                muted: false,
            },
            clipboard: ClipboardWidget {
                count: 0,
                recent: Vec::new(),
            },
            game_mode: false,
            open_menu: None,
            open,
            last_action: None,
        }
    }

    pub fn clock_label(&self) -> String {
        self.clock.format(self.hour24)
    }

    pub fn set_clock(&mut self, sample: ClockSample) {
        self.clock = sample;
    }

    pub fn set_hour24(&mut self, hour24: bool) {
        self.hour24 = hour24;
    }

    /// Frontmost app menu, from the focused nx-wm window.
    pub fn sync_wm(&mut self, wm: &WindowManagerModel) {
        self.focused_window = wm.active_window();
        if let Some(id) = self.focused_window {
            if let Some(window) = wm.window(id) {
                self.window_title = window.title.clone();
                self.process_name = window.process_name.clone();
                self.app_name = app_name_from(&window.process_name, &window.title);
            }
        } else {
            self.window_title.clear();
            self.process_name.clear();
            self.app_name = "Nexus".to_string();
        }
        let ws = wm.active_workspace_id();
        self.window_menu.clear();
        if let Some(workspace) = wm.workspace(ws) {
            for id in workspace.tree.iter() {
                if let Some(window) = wm.window(*id) {
                    let title = if window.title.is_empty() {
                        window.process_name.clone()
                    } else {
                        window.title.clone()
                    };
                    self.window_menu.push((*id, title));
                }
            }
        }
    }

    pub fn sync_privacy(&mut self, indicators: &MenuBarPrivacyIndicators, now_ms: u64) {
        self.privacy = indicators.visible_chips(now_ms);
    }

    pub fn sync_volume(&mut self, store: &VolumeStore) {
        if let Some(session) = volume_for(store, &self.process_name) {
            let muted = session.is_muted || store.is_master_muted();
            let level = if muted { 0.0 } else { session.volume };
            self.volume = VolumeWidget {
                label: if muted {
                    format!("{} Muted", session.name)
                } else {
                    format!("{} {}%", session.name, percent(session.volume))
                },
                session_id: Some(session.id.0.clone()),
                level,
                muted,
            };
        } else {
            let muted = store.is_master_muted();
            let level = if muted { 0.0 } else { store.master_volume() };
            self.volume = VolumeWidget {
                label: if muted {
                    "Muted".to_string()
                } else {
                    format!("Volume {}%", percent(level))
                },
                session_id: None,
                level,
                muted,
            };
        }
    }

    pub fn sync_clipboard(&mut self, store: &ClipboardStore) {
        let all = store.search("");
        let mut recent = Vec::new();
        for entry in all.iter().take(5) {
            if entry.content.is_empty() {
                continue;
            }
            recent.push(truncate_chars(&entry.content, 48));
        }
        self.clipboard = ClipboardWidget {
            count: store.len(),
            recent,
        };
    }

    pub fn sync_governor(&mut self, engine: &GovernorEngine) {
        self.game_mode = engine.is_game_mode_active();
    }

    pub fn status_widgets(&self) -> Vec<StatusWidget> {
        let mut widgets = Vec::new();
        for chip in &self.privacy {
            let label = if chip.badge_count > 1 {
                format!("{} {}", chip.label, chip.badge_count)
            } else {
                chip.label.clone()
            };
            widgets.push(StatusWidget {
                kind: StatusKind::Privacy,
                label,
                tooltip: chip.tooltip.clone(),
                color: parse_hex_color(chip.color_hex),
                badge: chip.badge_count,
            });
        }
        widgets.push(StatusWidget {
            kind: StatusKind::Volume,
            label: self.volume.label.clone(),
            tooltip: "Per-app volume".to_string(),
            color: Color::hex(0x0a84ff),
            badge: 0,
        });
        widgets.push(StatusWidget {
            kind: StatusKind::Clipboard,
            label: format!("Clip {}", self.clipboard.count),
            tooltip: self
                .clipboard
                .recent
                .first()
                .cloned()
                .unwrap_or_else(|| "Clipboard history".to_string()),
            color: Color::hex(0xbf5af2),
            badge: self.clipboard.count,
        });
        if self.game_mode {
            widgets.push(StatusWidget {
                kind: StatusKind::GameMode,
                label: "Game Mode".to_string(),
                tooltip: "Governor paused for a fullscreen game".to_string(),
                color: Color::hex(0x30d158),
                badge: 0,
            });
        }
        widgets
    }

    pub fn app_menu(&self) -> Vec<MenuEntry> {
        let name = &self.app_name;
        vec![
            MenuEntry::item(format!("About {name}"), MenuAction::About),
            MenuEntry::separator(),
            MenuEntry::item("Settings…", MenuAction::Settings),
            MenuEntry::separator(),
            MenuEntry::item(format!("Hide {name}"), MenuAction::Hide),
            MenuEntry::item("Hide Others", MenuAction::HideOthers),
            MenuEntry::item("Show All", MenuAction::ShowAll),
            MenuEntry::separator(),
            MenuEntry::item(format!("Quit {name}"), MenuAction::Quit),
        ]
    }

    pub fn menu_entries(&self, index: usize) -> Vec<MenuEntry> {
        match index {
            0 => vec![
                MenuEntry::item("New Window", MenuAction::NewWindow),
                MenuEntry::item("Close Window", MenuAction::CloseWindow),
            ],
            1 => vec![
                MenuEntry::item("Undo", MenuAction::Undo),
                MenuEntry::item("Redo", MenuAction::Redo),
                MenuEntry::separator(),
                MenuEntry::item("Cut", MenuAction::Cut),
                MenuEntry::item("Copy", MenuAction::Copy),
                MenuEntry::item("Paste", MenuAction::Paste),
            ],
            2 => vec![
                MenuEntry::item("Enter Mission Control", MenuAction::MissionControl),
                MenuEntry::item("Show Desktop", MenuAction::ShowDesktop),
            ],
            3 => {
                let mut entries: Vec<MenuEntry> = self
                    .window_menu
                    .iter()
                    .map(|(id, title)| MenuEntry::item(title.clone(), MenuAction::Window(id.0)))
                    .collect();
                entries.push(MenuEntry::separator());
                entries.push(MenuEntry::item("Minimize", MenuAction::Minimize));
                entries
            }
            4 => vec![MenuEntry::item("Nexus Help", MenuAction::Help)],
            _ => Vec::new(),
        }
    }

    pub fn toggle_slot(&mut self, slot: BarSlot) {
        if self.open_menu == Some(slot) {
            self.open_menu = None;
            self.open.set_target(0.0);
        } else {
            self.open_menu = Some(slot);
            self.open.set_target(1.0);
        }
    }

    pub fn close_menu(&mut self) {
        self.open_menu = None;
        self.open.set_target(0.0);
    }

    /// Record the action. Quit, hide, and close are requests for the host.
    /// This method does not touch processes, files, or the journal.
    pub fn perform(&mut self, action: MenuAction) -> MenuAction {
        self.last_action = Some(action);
        self.close_menu();
        action
    }

    pub fn step(&mut self, dt: f32) {
        self.open.step(dt);
        if self.open.settled() && self.open.target <= 0.0 {
            self.open_menu = None;
        }
    }

    pub fn animating(&self) -> bool {
        !self.open.settled()
    }

    pub fn menu_visible(&self) -> bool {
        self.open_menu.is_some() && self.open.position > 0.02
    }
}

impl Default for MenuBarModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a consent record the privacy indicator treats as in use.
pub fn active_consent(capability: CapabilityKind, name: &str, now_ms: u64) -> ConsentRecord {
    ConsentRecord {
        app_id: AppIdentifier::from_package_family_name(name),
        capability,
        status: ConsentStatus::Allow,
        last_used_start: Some(FileTime::from_unix_millis(now_ms.saturating_sub(1_000))),
        last_used_stop: None,
        attribution: None,
    }
}

pub fn privacy_with(records: Vec<ConsentRecord>, now_ms: u64) -> MenuBarPrivacyIndicators {
    let mut indicators = MenuBarPrivacyIndicators::new();
    let snapshot = ConsentStoreSnapshot::new(records);
    indicators.update_snapshot(&snapshot, now_ms);
    indicators
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_governor_policy::GovernorPolicyConfig;
    use nx_power::{AudioSession, ClipboardKind, NewClipboardItem};
    use nx_ui::geom::Point;
    use nx_wm::{MonitorId, Rect as WmRect, WindowId, WindowInfo, WindowManagerModel, WorkspaceId};

    fn wm_with(windows: &[(&str, &str)]) -> WindowManagerModel {
        let mut wm =
            WindowManagerModel::new(WmRect::new(0, 0, 1440, 900), WmRect::new(0, 0, 1440, 860));
        for (index, (title, process)) in windows.iter().enumerate() {
            let info = WindowInfo {
                id: WindowId((index as u64) + 1),
                title: (*title).to_string(),
                class_name: "NxWindow".to_string(),
                process_name: (*process).to_string(),
                rect: WmRect::new(40, 40, 640, 480),
                is_visible: true,
                is_minimized: false,
                is_cloaked: false,
                is_dialog_or_popup: false,
                style: 0,
                ex_style: 0,
            };
            wm.on_window_show(info);
        }
        wm
    }

    fn slot(slot: BarSlot, label: &str, width: f32) -> MeasuredSlot {
        MeasuredSlot {
            slot,
            label: label.to_string(),
            width,
        }
    }

    #[test]
    fn leading_and_trailing_do_not_overlap() {
        let leading = vec![
            slot(BarSlot::Mark, "", 16.0),
            slot(BarSlot::App, "Safari", 70.0),
            slot(BarSlot::Menu(0), "File", 36.0),
            slot(BarSlot::Menu(1), "Edit", 36.0),
            slot(BarSlot::Menu(2), "View", 40.0),
            slot(BarSlot::Menu(3), "Window", 64.0),
            slot(BarSlot::Menu(4), "Help", 40.0),
        ];
        let trailing = vec![
            slot(BarSlot::Status(0), "Camera", 70.0),
            slot(BarSlot::Status(1), "Volume 40%", 90.0),
            slot(BarSlot::Clock, "Fri 18 Nov  21:04", 130.0),
        ];
        let layout = layout_menu_bar(1440.0, 28.0, 12.0, 12.0, &leading, &trailing);
        assert_eq!(layout.bounds, Rect::new(0.0, 0.0, 1440.0, 28.0));
        let mut placed = layout.slots.clone();
        placed.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
        for pair in placed.windows(2) {
            assert!(
                !pair[0].rect.intersects(pair[1].rect),
                "{:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
        let clock = layout
            .slots
            .iter()
            .find(|slot| slot.slot == BarSlot::Clock)
            .expect("clock");
        let app = layout
            .slots
            .iter()
            .find(|slot| slot.slot == BarSlot::App)
            .expect("app");
        assert!(clock.rect.x > app.rect.right());
        assert!(clock.rect.right() <= 1440.0 - 12.0 + 0.1);
    }

    #[test]
    fn narrow_bar_keeps_the_clock_and_drops_menus() {
        let leading = vec![
            slot(BarSlot::Mark, "", 16.0),
            slot(BarSlot::App, "VeryLongApplicationName", 220.0),
            slot(BarSlot::Menu(0), "File", 40.0),
            slot(BarSlot::Menu(1), "Edit", 40.0),
        ];
        let trailing = vec![slot(BarSlot::Clock, "21:04", 48.0)];
        let layout = layout_menu_bar(180.0, 28.0, 8.0, 8.0, &leading, &trailing);
        assert!(layout.slots.iter().any(|slot| slot.slot == BarSlot::Clock));
        assert!(layout
            .slots
            .iter()
            .all(|slot| slot.rect.right() <= 180.0 + 0.1));
        let clock = layout
            .slots
            .iter()
            .find(|slot| slot.slot == BarSlot::Clock)
            .expect("clock");
        for slot in &layout.slots {
            if slot.slot != BarSlot::Clock {
                assert!(!slot.rect.intersects(clock.rect));
            }
        }
    }

    #[test]
    fn frontmost_app_comes_from_the_focused_window() {
        let wm = wm_with(&[("Inbox", "Mail.exe"), ("Start Page", "Safari.exe")]);
        let mut bar = MenuBarModel::new();
        bar.sync_wm(&wm);
        assert_eq!(bar.app_name, "Safari");
        assert_eq!(bar.focused_window, Some(WindowId(2)));
        assert_eq!(bar.window_menu.len(), 2);
        assert_eq!(bar.window_menu[0].1, "Inbox");
        let about = bar.app_menu();
        assert_eq!(about[0].label, "About Safari");
        assert_eq!(
            about.last().map(|entry| entry.action),
            Some(Some(MenuAction::Quit))
        );
    }

    #[test]
    fn focus_change_renames_the_app_menu() {
        let mut wm = wm_with(&[("Inbox", "Mail.exe"), ("Start Page", "Safari.exe")]);
        wm.focus_window(WindowId(1));
        let mut bar = MenuBarModel::new();
        bar.sync_wm(&wm);
        assert_eq!(bar.app_name, "Mail");
        assert_eq!(bar.app_menu()[0].label, "About Mail");
    }

    #[test]
    fn status_widgets_follow_privacy_volume_clipboard_and_game_mode() {
        let now = 1_700_000_000_000;
        let indicators = privacy_with(
            vec![active_consent(CapabilityKind::Webcam, "Zoom", now)],
            now,
        );
        let mut volume = VolumeStore::new();
        volume.upsert_session(AudioSession::new("safari", 42, "Safari", 0.4, false));
        let mut clips = ClipboardStore::with_default_exclusions(25);
        clips
            .push(NewClipboardItem {
                kind: ClipboardKind::PlainText,
                content: "hello from nexus".to_string(),
                timestamp_ms: now,
                source_app: Some("Safari".to_string()),
                sensitive_flag: false,
            })
            .expect("clip");
        let mut governor = GovernorEngine::new(GovernorPolicyConfig::default());
        governor.set_game_mode(true, now);

        let mut bar = MenuBarModel::new();
        bar.process_name = "Safari.exe".to_string();
        bar.app_name = "Safari".to_string();
        bar.sync_privacy(&indicators, now);
        bar.sync_volume(&volume);
        bar.sync_clipboard(&clips);
        bar.sync_governor(&governor);

        let widgets = bar.status_widgets();
        assert!(widgets.iter().any(|widget| {
            widget.kind == StatusKind::Privacy
                && widget.label.to_ascii_lowercase().contains("camera")
        }));
        assert!(widgets
            .iter()
            .any(|widget| { widget.kind == StatusKind::Volume && widget.label.contains("40%") }));
        assert!(widgets
            .iter()
            .any(|widget| widget.kind == StatusKind::Clipboard && widget.badge == 1));
        assert!(widgets
            .iter()
            .any(|widget| widget.kind == StatusKind::GameMode));
        assert_eq!(
            bar.clipboard.recent.first().map(String::as_str),
            Some("hello from nexus")
        );
        assert_eq!(bar.volume.session_id.as_deref(), Some("safari"));
    }

    #[test]
    fn idle_privacy_and_inactive_game_mode_omit_those_chips() {
        let mut bar = MenuBarModel::new();
        bar.sync_privacy(&MenuBarPrivacyIndicators::new(), 0);
        bar.sync_governor(&GovernorEngine::new(GovernorPolicyConfig::default()));
        let widgets = bar.status_widgets();
        assert!(widgets
            .iter()
            .all(|widget| widget.kind != StatusKind::Privacy));
        assert!(widgets
            .iter()
            .all(|widget| widget.kind != StatusKind::GameMode));
        assert!(widgets
            .iter()
            .any(|widget| widget.kind == StatusKind::Volume));
    }

    #[test]
    fn quit_is_recorded_and_does_not_clear_the_window_list() {
        let wm = wm_with(&[("Doc", "Notes.exe")]);
        let mut bar = MenuBarModel::new();
        bar.sync_wm(&wm);
        bar.toggle_slot(BarSlot::App);
        assert!(bar.menu_visible() || bar.open.target > 0.0);
        let action = bar.perform(MenuAction::Quit);
        assert_eq!(action, MenuAction::Quit);
        assert_eq!(bar.last_action, Some(MenuAction::Quit));
        assert_eq!(bar.window_menu.len(), 1);
        assert!(bar.open_menu.is_none());
    }

    #[test]
    fn hit_testing_finds_the_clock() {
        let layout = layout_menu_bar(
            400.0,
            28.0,
            8.0,
            8.0,
            &[slot(BarSlot::App, "Nexus", 50.0)],
            &[slot(BarSlot::Clock, "21:04", 40.0)],
        );
        let clock = layout
            .slots
            .iter()
            .find(|slot| slot.slot == BarSlot::Clock)
            .expect("clock");
        assert_eq!(
            hit_menu_bar(&layout, clock.rect.center()),
            Some(BarSlot::Clock)
        );
        assert_eq!(hit_menu_bar(&layout, Point::new(-4.0, 4.0)), None);
    }

    #[test]
    fn menu_spring_settles_closed() {
        let mut bar = MenuBarModel::new();
        bar.toggle_slot(BarSlot::Menu(0));
        assert!(bar.animating());
        for _ in 0..120 {
            bar.step(1.0 / 60.0);
        }
        assert!(!bar.animating());
        assert!(bar.open.position > 0.9);
        bar.close_menu();
        for _ in 0..120 {
            bar.step(1.0 / 60.0);
        }
        assert!(!bar.animating());
        assert!(bar.open.position < 0.05);
        assert!(bar.open_menu.is_none());
    }

    #[test]
    fn window_ids_stay_on_the_monitor_the_model_reports() {
        // The menu bar does not move windows. This pins the sync to nx-wm ids.
        let wm = wm_with(&[("A", "a.exe")]);
        assert_eq!(wm.active_monitor().id, MonitorId(0));
        assert_eq!(wm.active_workspace_id(), WorkspaceId(1));
        let mut bar = MenuBarModel::new();
        bar.sync_wm(&wm);
        assert_eq!(bar.window_menu[0].0, WindowId(1));
    }
}
