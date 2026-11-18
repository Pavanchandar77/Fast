//! Dashboard page as a pure view-model with deterministic layout tree generation.
//!
//! Provides sections, rows, indicator chips, permission matrix, network stats, and audit log.

use crate::clipboard::ClipboardPrivacyStore;
use crate::consent_store::ConsentStoreSnapshot;
use crate::indicator::MenuBarPrivacyIndicators;
use crate::kill_switch::{KillSwitchKind, KillSwitchStore};
use crate::layout::{LayoutNode, Rect, WidgetKind};
use crate::network::{NetworkPerAppViewModel, TimeWindow};
use crate::permissions::{PermissionCapability, PermissionStore};
use serde::{Deserialize, Serialize};

/// Active dashboard navigation tab.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum DashboardTab {
    #[default]
    Overview,
    Permissions,
    Network,
    Clipboard,
    AuditLog,
}

impl DashboardTab {
    pub fn all() -> [Self; 5] {
        [
            Self::Overview,
            Self::Permissions,
            Self::Network,
            Self::Clipboard,
            Self::AuditLog,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Permissions => "Permissions",
            Self::Network => "Network Monitor",
            Self::Clipboard => "Clipboard Guard",
            Self::AuditLog => "Audit Log",
        }
    }
}

/// A privacy audit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub category: String,
    pub actor: String,
    pub description: String,
    pub is_critical: bool,
}

/// The complete privacy dashboard pure view-model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardViewModel {
    pub active_tab: DashboardTab,
    pub indicators: MenuBarPrivacyIndicators,
    pub kill_switches: KillSwitchStore,
    pub permissions: PermissionStore,
    pub clipboard: ClipboardPrivacyStore,
    pub network: NetworkPerAppViewModel,
    pub audit_log: Vec<AuditLogEntry>,
    next_log_id: u64,
}

impl Default for DashboardViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardViewModel {
    pub fn new() -> Self {
        Self {
            active_tab: DashboardTab::Overview,
            indicators: MenuBarPrivacyIndicators::new(),
            kill_switches: KillSwitchStore::new(),
            permissions: PermissionStore::new(),
            clipboard: ClipboardPrivacyStore::new(),
            network: NetworkPerAppViewModel::new(),
            audit_log: Vec::new(),
            next_log_id: 1,
        }
    }

    /// Select active dashboard tab.
    pub fn set_tab(&mut self, tab: DashboardTab) {
        self.active_tab = tab;
    }

    /// Append an event to the privacy audit log.
    pub fn log_event(
        &mut self,
        category: impl Into<String>,
        actor: impl Into<String>,
        description: impl Into<String>,
        is_critical: bool,
        now_ms: u64,
    ) {
        let entry = AuditLogEntry {
            id: self.next_log_id,
            timestamp_ms: now_ms,
            category: category.into(),
            actor: actor.into(),
            description: description.into(),
            is_critical,
        };
        self.next_log_id += 1;
        self.audit_log.push(entry);
    }

    /// Update view-model state from a fresh ConsentStore snapshot.
    pub fn update_from_consent_store(&mut self, snapshot: &ConsentStoreSnapshot, now_ms: u64) {
        let events = self.indicators.update_snapshot(snapshot, now_ms);
        for ev in events {
            self.log_event(
                "Hardware",
                "ConsentStore",
                format!("{:?}", ev),
                false,
                now_ms,
            );
        }
        self.permissions.ingest_consent_snapshot(snapshot, now_ms);
    }

    /// Advance time ticks across all stores and indicators.
    pub fn tick(&mut self, now_ms: u64) {
        let ind_events = self.indicators.tick(now_ms);
        for ev in ind_events {
            self.log_event("Hardware", "Indicator", format!("{:?}", ev), false, now_ms);
        }

        if let Some(clear_cmd) = self.clipboard.check_auto_clear(now_ms) {
            self.log_event(
                "Clipboard",
                "AutoClear",
                clear_cmd.reason.clone(),
                true,
                now_ms,
            );
        }
    }

    /// Generate deterministic layout tree for current view-model state within bounds.
    pub fn compute_layout(&self, bounds: Rect, now_ms: u64) -> LayoutNode {
        let mut root = LayoutNode::new("dashboard_window", WidgetKind::Window, bounds)
            .with_label("Nexus Privacy Dashboard");

        let padding = 16.0;
        let content_w = bounds.width - (2.0 * padding);

        // Header bar (y = bounds.y + padding, height = 56)
        let header_rect = Rect::new(bounds.x + padding, bounds.y + padding, content_w, 56.0);
        let mut header = LayoutNode::new("header", WidgetKind::Header, header_rect)
            .with_label("Privacy & Security Hub");

        // Hardware indicator chips in header
        let chips = self.indicators.all_chips(now_ms);
        let chip_w = 140.0;
        let chip_h = 36.0;
        let chip_y = header_rect.y + 10.0;

        for (i, chip) in chips.iter().enumerate() {
            let chip_x = header_rect.right() - ((chips.len() - i) as f32 * (chip_w + 8.0));
            let chip_rect = Rect::new(chip_x, chip_y, chip_w, chip_h);
            let chip_node = LayoutNode::new(
                format!("chip_{:?}", chip.capability),
                WidgetKind::IndicatorChip,
                chip_rect,
            )
            .with_label(&chip.label)
            .with_attr("status", format!("{:?}", chip.status))
            .with_attr("color", chip.color_hex);

            header.add_child(chip_node);
        }

        root.add_child(header);

        // Tab bar (height = 40)
        let tab_bar_y = header_rect.bottom() + 8.0;
        let tab_bar_rect = Rect::new(bounds.x + padding, tab_bar_y, content_w, 40.0);
        let mut tab_bar = LayoutNode::new("tab_bar", WidgetKind::TabBar, tab_bar_rect);

        let tab_btn_w = 130.0;
        for (idx, tab) in DashboardTab::all().iter().enumerate() {
            let btn_rect = Rect::new(
                tab_bar_rect.x + (idx as f32 * (tab_btn_w + 4.0)),
                tab_bar_rect.y + 4.0,
                tab_btn_w,
                32.0,
            );
            let btn = LayoutNode::new(
                format!("tab_button_{:?}", tab),
                WidgetKind::TabButton,
                btn_rect,
            )
            .with_label(tab.title())
            .with_attr(
                "selected",
                if *tab == self.active_tab {
                    "true"
                } else {
                    "false"
                },
            );

            tab_bar.add_child(btn);
        }

        root.add_child(tab_bar);

        // Main content area
        let content_y = tab_bar_rect.bottom() + 12.0;
        let content_h = bounds.bottom() - content_y - padding;
        let content_rect = Rect::new(bounds.x + padding, content_y, content_w, content_h);

        let content_node = match self.active_tab {
            DashboardTab::Overview => self.layout_overview(content_rect, now_ms),
            DashboardTab::Permissions => self.layout_permissions(content_rect, now_ms),
            DashboardTab::Network => self.layout_network(content_rect, now_ms),
            DashboardTab::Clipboard => self.layout_clipboard(content_rect, now_ms),
            DashboardTab::AuditLog => self.layout_audit_log(content_rect),
        };

        root.add_child(content_node);
        root
    }

    fn layout_overview(&self, bounds: Rect, now_ms: u64) -> LayoutNode {
        let mut container = LayoutNode::new("overview_pane", WidgetKind::Section, bounds);

        // Row 1: Stat Cards (4 cards)
        let card_gap = 12.0;
        let card_w = (bounds.width - (3.0 * card_gap)) / 4.0;
        let card_h = 90.0;

        // Card 1: Hardware Guard
        let card1_rect = Rect::new(bounds.x, bounds.y, card_w, card_h);
        let card1 = LayoutNode::new("card_hardware", WidgetKind::Card, card1_rect)
            .with_label("Active Sensors")
            .with_attr(
                "value",
                if self.indicators.is_any_in_use() {
                    "In Use"
                } else {
                    "Secure (Idle)"
                },
            );
        container.add_child(card1);

        // Card 2: Kill Switches Active
        let active_kill = self
            .kill_switches
            .all_states()
            .values()
            .filter(|&&v| v)
            .count();
        let card2_rect = Rect::new(bounds.x + card_w + card_gap, bounds.y, card_w, card_h);
        let card2 = LayoutNode::new("card_kill_switches", WidgetKind::Card, card2_rect)
            .with_label("Master Kill-Switches")
            .with_attr("value", format!("{}/6 Engaged", active_kill));
        container.add_child(card2);

        // Card 3: Telemetry Block Rate
        let block_pct = self
            .network
            .global_telemetry_blocked_pct(TimeWindow::LastHour, now_ms);
        let card3_rect = Rect::new(
            bounds.x + (2.0 * (card_w + card_gap)),
            bounds.y,
            card_w,
            card_h,
        );
        let card3 = LayoutNode::new("card_telemetry", WidgetKind::Card, card3_rect)
            .with_label("Telemetry Block Rate")
            .with_attr("value", format!("{:.1}%", block_pct));
        container.add_child(card3);

        // Card 4: Clipboard Status
        let card4_rect = Rect::new(
            bounds.x + (3.0 * (card_w + card_gap)),
            bounds.y,
            card_w,
            card_h,
        );
        let clip_status = if self.clipboard.is_sensitive_active() {
            "Protected Copy"
        } else {
            "Clean"
        };
        let card4 = LayoutNode::new("card_clipboard", WidgetKind::Card, card4_rect)
            .with_label("Clipboard Guard")
            .with_attr("value", clip_status);
        container.add_child(card4);

        // Row 2: Master Kill-Switch Grid
        let ks_section_y = bounds.y + card_h + 16.0;
        let ks_section_h = 160.0;
        let ks_rect = Rect::new(bounds.x, ks_section_y, bounds.width, ks_section_h);
        let mut ks_section = LayoutNode::new("section_kill_switches", WidgetKind::Section, ks_rect)
            .with_label("System Kill-Switches");

        let ks_card_w = (bounds.width - (2.0 * card_gap)) / 3.0;
        let ks_card_h = 64.0;

        for (i, kind) in KillSwitchKind::all().iter().enumerate() {
            let col = (i % 3) as f32;
            let row = (i / 3) as f32;
            let item_x = ks_rect.x + (col * (ks_card_w + card_gap));
            let item_y = ks_rect.y + 24.0 + (row * (ks_card_h + 8.0));
            let item_rect = Rect::new(item_x, item_y, ks_card_w, ks_card_h);

            let is_on = self.kill_switches.is_engaged(*kind);
            let toggle = LayoutNode::new(
                format!("ks_toggle_{:?}", kind),
                WidgetKind::ToggleSwitch,
                item_rect,
            )
            .with_label(kind.display_name())
            .with_attr("enabled", if is_on { "true" } else { "false" });

            ks_section.add_child(toggle);
        }

        container.add_child(ks_section);

        // Row 3: Audit Recommendations Preview
        let audit = self.permissions.audit(now_ms);
        let rec_y = ks_rect.bottom() + 16.0;
        let rec_h = bounds.bottom() - rec_y;
        let rec_rect = Rect::new(bounds.x, rec_y, bounds.width, rec_h.max(80.0));
        let mut rec_section =
            LayoutNode::new("section_recommendations", WidgetKind::Section, rec_rect)
                .with_label("Privacy Posture & Recommendations");

        for (i, rec) in audit.recommendations.iter().take(3).enumerate() {
            let row_y = rec_rect.y + 24.0 + (i as f32 * 36.0);
            let row_rect = Rect::new(rec_rect.x, row_y, rec_rect.width, 32.0);
            let row_node =
                LayoutNode::new(format!("rec_row_{}", i), WidgetKind::TableRow, row_rect)
                    .with_label(&rec.title)
                    .with_attr("severity", format!("{:?}", rec.severity))
                    .with_attr("explanation", &rec.explanation);

            rec_section.add_child(row_node);
        }

        container.add_child(rec_section);
        container
    }

    fn layout_permissions(&self, bounds: Rect, _now_ms: u64) -> LayoutNode {
        let mut table = LayoutNode::new("permissions_table", WidgetKind::Table, bounds)
            .with_label("Per-App Permission Matrix");

        // Header row
        let header_h = 32.0;
        let header_rect = Rect::new(bounds.x, bounds.y, bounds.width, header_h);
        let table_header = LayoutNode::new("perm_header", WidgetKind::TableHeader, header_rect)
            .with_label("App | Type | Camera | Mic | Location | Clipboard | Notifications | Risk");
        table.add_child(table_header);

        let row_h = 36.0;
        let mut current_y = bounds.y + header_h + 4.0;

        for (idx, profile) in self.permissions.profiles.values().enumerate() {
            let row_rect = Rect::new(bounds.x, current_y, bounds.width, row_h);
            let mut row =
                LayoutNode::new(format!("perm_row_{}", idx), WidgetKind::TableRow, row_rect)
                    .with_label(profile.app.display_name())
                    .with_attr(
                        "is_packaged",
                        if profile.is_packaged { "true" } else { "false" },
                    )
                    .with_attr("risk", format!("{:?}", profile.risk_level));

            // Permission toggles inside row
            for cap in PermissionCapability::all() {
                let cell_rect = Rect::new(row_rect.x, row_rect.y, 40.0, row_h);
                let allowed = profile.is_allowed(cap);
                let toggle = LayoutNode::new(
                    format!("toggle_{:?}_{}", cap, idx),
                    WidgetKind::ToggleSwitch,
                    cell_rect,
                )
                .with_label(format!("{:?}", cap))
                .with_attr("allowed", if allowed { "true" } else { "false" });

                row.add_child(toggle);
            }

            table.add_child(row);
            current_y += row_h + 2.0;
        }

        table
    }

    fn layout_network(&self, bounds: Rect, now_ms: u64) -> LayoutNode {
        let mut container = LayoutNode::new("network_pane", WidgetKind::Section, bounds);

        // Top sparkline / summary bar
        let stat_h = 60.0;
        let stat_rect = Rect::new(bounds.x, bounds.y, bounds.width, stat_h);
        let sparkline = LayoutNode::new("network_sparkline", WidgetKind::Sparkline, stat_rect)
            .with_label("Recent Traffic (Green = Allowed, Red = Blocked Telemetry)");
        container.add_child(sparkline);

        // Network table
        let table_y = bounds.y + stat_h + 12.0;
        let table_h = bounds.bottom() - table_y;
        let table_rect = Rect::new(bounds.x, table_y, bounds.width, table_h);
        let mut table = LayoutNode::new("network_table", WidgetKind::Table, table_rect)
            .with_label("Network Activity Per App");

        let summaries = self.network.summaries(TimeWindow::LastHour, now_ms);
        let row_h = 32.0;
        let mut cur_y = table_rect.y + 24.0;

        for (idx, sum) in summaries.iter().enumerate() {
            let row_rect = Rect::new(table_rect.x, cur_y, table_rect.width, row_h);
            let row = LayoutNode::new(format!("net_row_{}", idx), WidgetKind::TableRow, row_rect)
                .with_label(sum.app.display_name())
                .with_attr("total_bytes", sum.total_bytes.to_string())
                .with_attr("blocked_bytes", sum.blocked_bytes.to_string())
                .with_attr("blocked_pct", format!("{:.1}%", sum.telemetry_blocked_pct));

            table.add_child(row);
            cur_y += row_h + 2.0;
        }

        container.add_child(table);
        container
    }

    fn layout_clipboard(&self, bounds: Rect, _now_ms: u64) -> LayoutNode {
        let mut container = LayoutNode::new("clipboard_pane", WidgetKind::Section, bounds);

        // Status Card
        let card_h = 70.0;
        let card_rect = Rect::new(bounds.x, bounds.y, bounds.width, card_h);
        let card = LayoutNode::new("clipboard_status_card", WidgetKind::Card, card_rect)
            .with_label("Clipboard Memory State")
            .with_attr(
                "status",
                if self.clipboard.is_sensitive_active() {
                    "Protected Sensitive Content"
                } else {
                    "Empty / Safe"
                },
            );
        container.add_child(card);

        // Alerts Table
        let table_y = bounds.y + card_h + 16.0;
        let table_h = bounds.bottom() - table_y;
        let table_rect = Rect::new(bounds.x, table_y, bounds.width, table_h);
        let mut table = LayoutNode::new("clipboard_alerts_table", WidgetKind::Table, table_rect)
            .with_label("Clipboard Read Alerts");

        let alerts = self.clipboard.alerts();
        let row_h = 32.0;
        let mut cur_y = table_rect.y + 24.0;

        for (i, alert) in alerts.iter().enumerate() {
            let row_rect = Rect::new(table_rect.x, cur_y, table_rect.width, row_h);
            let row = LayoutNode::new(format!("alert_row_{}", i), WidgetKind::TableRow, row_rect)
                .with_label(alert.reading_app.display_name())
                .with_attr("kind", format!("{:?}", alert.sensitive_kind))
                .with_attr("severity", format!("{:?}", alert.severity))
                .with_attr("snippet", &alert.redacted_snippet);

            table.add_child(row);
            cur_y += row_h + 2.0;
        }

        container.add_child(table);
        container
    }

    fn layout_audit_log(&self, bounds: Rect) -> LayoutNode {
        let mut table = LayoutNode::new("audit_log_table", WidgetKind::Table, bounds)
            .with_label("Security & Privacy Event Log");

        let row_h = 30.0;
        let mut cur_y = bounds.y + 20.0;

        for entry in self.audit_log.iter().rev() {
            let row_rect = Rect::new(bounds.x, cur_y, bounds.width, row_h);
            let row = LayoutNode::new(
                format!("audit_entry_{}", entry.id),
                WidgetKind::TableRow,
                row_rect,
            )
            .with_label(format!("[{}] {}", entry.category, entry.description))
            .with_attr("actor", &entry.actor)
            .with_attr("critical", if entry.is_critical { "true" } else { "false" });

            table.add_child(row);
            cur_y += row_h + 2.0;
        }

        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent_store::{AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus};
    use crate::filetime::FileTime;

    #[test]
    fn dashboard_layout_tree_overview_snapshot() {
        let mut vm = DashboardViewModel::new();
        let now_ms = 1_000_000;

        // Populate sample data
        let snap = ConsentStoreSnapshot::new(vec![ConsentRecord {
            app_id: AppIdentifier::from_desktop_path("C:\\Program Files\\Zoom\\bin\\Zoom.exe"),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(now_ms)),
            last_used_stop: None,
            attribution: None,
        }]);
        vm.update_from_consent_store(&snap, now_ms);

        let bounds = Rect::new(0.0, 0.0, 1024.0, 768.0);
        let layout = vm.compute_layout(bounds, now_ms);

        assert_eq!(layout.id, "dashboard_window");
        assert_eq!(layout.kind, WidgetKind::Window);
        assert!(layout.rect.approx_eq(&bounds, 0.01));

        // Verify header and chip
        let header = layout.find_node("header").expect("header node");
        assert_eq!(header.kind, WidgetKind::Header);

        let chip = layout.find_node("chip_Webcam").expect("chip node");
        assert_eq!(chip.kind, WidgetKind::IndicatorChip);
        assert!(chip.label.as_deref().unwrap().contains("Zoom"));

        // Verify tab buttons
        let tab_bar = layout.find_node("tab_bar").expect("tab_bar node");
        assert_eq!(tab_bar.children.len(), 5);

        // Verify formatted tree output is non-empty and well-formed
        let formatted = layout.formatted_tree(0);
        assert!(formatted.contains("- Window #dashboard_window: [0.0, 0.0, 1024.0x768.0]"));
        assert!(formatted.contains("IndicatorChip #chip_Webcam"));
    }

    #[test]
    fn dashboard_layout_tree_deterministic_approx_eq() {
        let vm = DashboardViewModel::new();
        let now_ms = 1_000_000;

        let bounds = Rect::new(0.0, 0.0, 1280.0, 800.0);
        let tree1 = vm.compute_layout(bounds, now_ms);
        let tree2 = vm.compute_layout(bounds, now_ms);

        assert!(tree1.approx_eq(&tree2, 0.001));
    }

    #[test]
    fn dashboard_tab_switching_layouts() {
        let mut vm = DashboardViewModel::new();
        let now_ms = 1_000_000;
        let bounds = Rect::new(0.0, 0.0, 1024.0, 768.0);

        // 1. Overview
        vm.set_tab(DashboardTab::Overview);
        let tree_overview = vm.compute_layout(bounds, now_ms);
        assert!(tree_overview.find_node("overview_pane").is_some());

        // 2. Permissions
        vm.set_tab(DashboardTab::Permissions);
        let tree_perm = vm.compute_layout(bounds, now_ms);
        assert!(tree_perm.find_node("permissions_table").is_some());

        // 3. Network
        vm.set_tab(DashboardTab::Network);
        let tree_net = vm.compute_layout(bounds, now_ms);
        assert!(tree_net.find_node("network_pane").is_some());

        // 4. Clipboard
        vm.set_tab(DashboardTab::Clipboard);
        let tree_clip = vm.compute_layout(bounds, now_ms);
        assert!(tree_clip.find_node("clipboard_pane").is_some());

        // 5. Audit Log
        vm.set_tab(DashboardTab::AuditLog);
        let tree_audit = vm.compute_layout(bounds, now_ms);
        assert!(tree_audit.find_node("audit_log_table").is_some());
    }
}
