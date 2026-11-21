//! GPUI View for Nexus-OS Privacy Dashboard and Security Shield.
//!
//! Provides a macOS/Nexus-styled privacy dashboard with Mica/Acrylic backdrop,
//! SF typography, live hardware indicators (Camera, Mic, Location), per-app
//! permission audit matrix, network traffic monitor, clipboard guard, and
//! master kill switches.

use gpui::prelude::*;
use gpui::{
    div, px, Context, FontWeight, Hsla, IntoElement, ParentElement, Render, Styled, Window,
};
use nx_ui::gpui_theme::{rounded_window_frame, sf_font_family, window_chrome};
use nx_ui::theme::Theme;

use crate::consent_store::CapabilityKind;
use crate::indicator::IndicatorStatusKind;
use crate::kill_switch::KillSwitchKind;
use crate::network::TimeWindow;
use crate::permissions::{PermissionCapability, RiskLevel};
use crate::view_model::{DashboardTab, DashboardViewModel};

/// Helper to set alpha on an `Hsla` value.
pub fn with_alpha(mut hsla: Hsla, a: f32) -> Hsla {
    hsla.a = a;
    hsla
}

/// Formats a byte count into a human-readable size string.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes == 0 {
        "0 B".to_string()
    } else if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    }
}

/// Map risk to monochrome tone. Critical/High invert to primary ink.
pub fn risk_color(risk: RiskLevel) -> Hsla {
    risk_color_on(&Theme::stealth(), risk)
}

pub fn risk_color_on(theme: &Theme, risk: RiskLevel) -> Hsla {
    match risk {
        RiskLevel::Critical | RiskLevel::High => theme.text_primary_hsla(),
        RiskLevel::Medium => theme.text_secondary_hsla(),
        RiskLevel::Low => theme.text_tertiary_hsla(),
    }
}

/// Map indicator status to monochrome tone.
pub fn chip_status_color(status: IndicatorStatusKind) -> Hsla {
    chip_status_color_on(&Theme::stealth(), status)
}

pub fn chip_status_color_on(theme: &Theme, status: IndicatorStatusKind) -> Hsla {
    match status {
        IndicatorStatusKind::InUse | IndicatorStatusKind::Blocked => theme.text_primary_hsla(),
        IndicatorStatusKind::RecentlyUsed => theme.text_secondary_hsla(),
        IndicatorStatusKind::Idle => theme.text_tertiary_hsla(),
    }
}

/// GPUI View for the Nexus-OS Privacy Dashboard window.
pub struct PrivacyDashboardView {
    pub view_model: DashboardViewModel,
    pub current_time_ms: u64,
}

impl PrivacyDashboardView {
    pub fn new(view_model: DashboardViewModel) -> Self {
        Self {
            view_model,
            current_time_ms: 1_700_000_000_000,
        }
    }

    pub fn with_time(view_model: DashboardViewModel, current_time_ms: u64) -> Self {
        Self {
            view_model,
            current_time_ms,
        }
    }

    pub fn set_tab(&mut self, tab: DashboardTab) {
        self.view_model.set_tab(tab);
    }

    pub fn toggle_kill_switch(&mut self, kind: KillSwitchKind) {
        let current = self.view_model.kill_switches.is_engaged(kind);
        let req = self.view_model.kill_switches.create_request(
            kind,
            !current,
            "User UI Action",
            self.current_time_ms,
        );
        self.view_model.kill_switches.apply_request(&req);
        self.view_model.log_event(
            "KillSwitch",
            "UserInterface",
            format!(
                "Toggled {} to {}",
                kind,
                if !current { "Engaged" } else { "Disengaged" }
            ),
            true,
            self.current_time_ms,
        );
    }

    fn render_header(&self, theme: &Theme) -> impl IntoElement {
        let chips = self.view_model.indicators.all_chips(self.current_time_ms);

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .px(px(20.0 * theme.scale))
            .py(px(14.0 * theme.scale))
            .bg(theme.window_bg_hsla())
            .border_b_1()
            .border_color(theme.separator_hsla())
            .rounded_t(px(theme.radii.window))
            .overflow_hidden()
            // Title & Window Controls
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(14.0 * theme.scale))
                    .child(window_chrome(theme, theme.scale))
                    // App / Hub Title
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_family(sf_font_family())
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(15.0 * theme.scale))
                                    .text_color(theme.text_primary_hsla())
                                    .child("PRIVACY SHIELD"),
                            )
                            .child(
                                div()
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0 * theme.scale))
                                    .text_color(theme.text_secondary_hsla())
                                    .child(
                                        "CapabilityAccessManager • WFP Tracker Blocker • DiagTrack Shield",
                                    ),
                            ),
                    ),
            )
            // Live Hardware Indicator Chips (Camera, Mic, Location)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0 * theme.scale))
                    .overflow_hidden()
                    .children(chips.into_iter().map(|chip| {
                        let status_col = chip_status_color(chip.status);
                        let tag = match chip.capability {
                            CapabilityKind::Webcam => "CAM",
                            CapabilityKind::Microphone => "MIC",
                            CapabilityKind::Location => "LOC",
                            CapabilityKind::Custom(_) => "SEC",
                        };
                        let state = match chip.status {
                            IndicatorStatusKind::InUse => "LIVE",
                            IndicatorStatusKind::Blocked => "BLOCKED",
                            IndicatorStatusKind::RecentlyUsed => "RECENT",
                            IndicatorStatusKind::Idle => "IDLE",
                        };

                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0 * theme.scale))
                            .px(px(8.0 * theme.scale))
                            .py(px(4.0 * theme.scale))
                            .rounded(theme.corner_radius_px(6.0))
                            .bg(theme.elevated_hsla())
                            .border_1()
                            .border_color(theme.separator_hsla())
                            .child(
                                div()
                                    .w(px(8.0 * theme.scale))
                                    .h(px(8.0 * theme.scale))
                                    .rounded_full()
                                    .bg(status_col),
                            )
                            .child(
                                div()
                                    .font_family(sf_font_family())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(px(11.0 * theme.scale))
                                    .text_color(theme.text_primary_hsla())
                                    .child(format!("{tag}  {state}")),
                            )
                    })),
            )
    }

    fn render_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active_tab = self.view_model.active_tab;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0 * theme.scale))
            .w_full()
            .px(px(20.0 * theme.scale))
            .py(px(8.0 * theme.scale))
            .bg(theme.sidebar_bg_hsla())
            .border_b_1()
            .border_color(theme.separator_hsla())
            .children(DashboardTab::all().into_iter().map(|tab| {
                let is_active = tab == active_tab;
                let (bg_color, text_color) = selected_tab_style(theme, is_active);

                div()
                    .id(("privacy-tab", tab as u32))
                    .px(px(14.0 * theme.scale))
                    .py(px(6.0 * theme.scale))
                    .rounded(theme.corner_radius_px(6.0))
                    .bg(bg_color)
                    .font_family(sf_font_family())
                    .font_weight(if is_active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_size(px(12.5 * theme.scale))
                    .text_color(text_color)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_tab(tab);
                        cx.notify();
                    }))
                    .child(tab.title())
            }))
    }

    fn render_overview(&self, theme: &Theme) -> impl IntoElement {
        let all_switches = KillSwitchKind::all();

        div()
            .flex()
            .flex_col()
            .gap(px(16.0 * theme.scale))
            .w_full()
            .p(px(20.0 * theme.scale))
            // Section Header
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0 * theme.scale))
                            .text_color(theme.text_primary_hsla())
                            .child("Master Kill-Switches & Hardware Protection"),
                    )
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(12.0 * theme.scale))
                            .text_color(theme.text_secondary_hsla())
                            .child(
                                "Instant dry-run verified enforcement against cameras, mics, telemetry, and Recall.",
                            ),
                    ),
            )
            // Grid of Kill Switches
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(12.0 * theme.scale))
                    .children(all_switches.into_iter().map(|kind| {
                        let engaged = self.view_model.kill_switches.is_engaged(kind);
                        let badge_bg = if engaged {
                            theme.inverted_hsla()
                        } else {
                            Hsla::from(nx_ui::Color::TRANSPARENT)
                        };
                        let badge_text = if engaged {
                            theme.on_inverted_hsla()
                        } else {
                            theme.text_secondary_hsla()
                        };

                        div()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .w(px(280.0 * theme.scale))
                            .p(px(12.0 * theme.scale))
                            .rounded(theme.corner_radius_px(10.0))
                            .bg(theme.elevated_hsla())
                            .border_1()
                            .border_color(theme.separator_hsla())
                            // Header of card
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(sf_font_family())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(13.0 * theme.scale))
                                            .text_color(theme.text_primary_hsla())
                                            .child(kind.display_name()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.0 * theme.scale))
                                            .py(px(2.0 * theme.scale))
                                            .rounded(theme.corner_radius_px(4.0))
                                            .bg(badge_bg)
                                            .font_family(sf_font_family())
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_size(px(10.0 * theme.scale))
                                            .text_color(badge_text)
                                            .child(if engaged { "ENGAGED" } else { "IDLE" }),
                                    ),
                            )
                            // Description
                            .child(
                                div()
                                    .my(px(8.0 * theme.scale))
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0 * theme.scale))
                                    .text_color(theme.text_secondary_hsla())
                                    .child(kind.description()),
                            )
                            // Reversible toggle button
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_family(sf_font_family())
                                            .text_size(px(10.0 * theme.scale))
                                            .text_color(theme.text_tertiary_hsla())
                                            .child("Reversible • Dry-run verified"),
                                    )
                                    .child(
                                        div()
                                            .px(px(10.0 * theme.scale))
                                            .py(px(4.0 * theme.scale))
                                            .rounded(theme.corner_radius_px(6.0))
                                            .bg(if engaged {
                                                Hsla::from(nx_ui::Color::TRANSPARENT)
                                            } else {
                                                theme.inverted_hsla()
                                            })
                                            .border_1()
                                            .border_color(theme.separator_hsla())
                                            .font_family(sf_font_family())
                                            .font_weight(FontWeight::BOLD)
                                            .text_size(px(11.0 * theme.scale))
                                            .text_color(if engaged {
                                                theme.text_secondary_hsla()
                                            } else {
                                                theme.on_inverted_hsla()
                                            })
                                            .child(if engaged { "DISENGAGE" } else { "ENGAGE" }),
                                    ),
                            )
                    })),
            )
    }

    fn render_permissions(&self, theme: &Theme) -> impl IntoElement {
        let profiles = &self.view_model.permissions.profiles;

        div()
            .flex()
            .flex_col()
            .gap(px(12.0 * theme.scale))
            .w_full()
            .p(px(20.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0 * theme.scale))
                            .text_color(theme.text_primary_hsla())
                            .child("Per-App Permission Audit & Access Control"),
                    )
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(12.0 * theme.scale))
                            .text_color(theme.text_secondary_hsla())
                            .child(format!(
                                "Audited {} application profiles across hardware capabilities.",
                                profiles.len()
                            )),
                    ),
            )
            .child(div().flex().flex_col().gap(px(8.0 * theme.scale)).children(
                profiles.values().map(|profile| {
                    let risk_col = risk_color(profile.risk_level);
                    let app_name = profile.app.display_name().to_string();

                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .p(px(12.0 * theme.scale))
                        .rounded(theme.corner_radius_px(8.0))
                        .bg(theme.elevated_hsla())
                        .border_1()
                        .border_color(theme.separator_hsla())
                        // Left: App name + Risk badge
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0 * theme.scale))
                                .child(
                                    div()
                                        .px(px(6.0 * theme.scale))
                                        .py(px(2.0 * theme.scale))
                                        .rounded(theme.corner_radius_px(4.0))
                                        .bg(with_alpha(risk_col, 0.2))
                                        .font_family(sf_font_family())
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(10.0 * theme.scale))
                                        .text_color(risk_col)
                                        .child(format!("{:?}", profile.risk_level)),
                                )
                                .child(
                                    div()
                                        .font_family(sf_font_family())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(13.0 * theme.scale))
                                        .text_color(theme.text_primary_hsla())
                                        .child(app_name),
                                ),
                        )
                        // Right: Capabilities pill badges
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0 * theme.scale))
                                .children(PermissionCapability::all().into_iter().map(|cap| {
                                    let allowed = profile.is_allowed(cap);
                                    let (bg_pill, text_pill) = if allowed {
                                        (theme.selection_hsla(), theme.text_primary_hsla())
                                    } else {
                                        (
                                            Hsla::from(theme.palette.fill_secondary),
                                            theme.text_tertiary_hsla(),
                                        )
                                    };

                                    div()
                                        .px(px(6.0 * theme.scale))
                                        .py(px(2.0 * theme.scale))
                                        .rounded(theme.corner_radius_px(4.0))
                                        .bg(bg_pill)
                                        .font_family(sf_font_family())
                                        .text_size(px(10.0 * theme.scale))
                                        .text_color(text_pill)
                                        .child(cap.display_name())
                                })),
                        )
                }),
            ))
    }

    fn render_network(&self, theme: &Theme) -> impl IntoElement {
        let summaries = self
            .view_model
            .network
            .summaries(TimeWindow::LastHour, self.current_time_ms);

        div()
            .flex()
            .flex_col()
            .gap(px(12.0 * theme.scale))
            .w_full()
            .p(px(20.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0 * theme.scale))
                            .text_color(theme.text_primary_hsla())
                            .child("Network Activity & Telemetry Blocker"),
                    )
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(12.0 * theme.scale))
                            .text_color(theme.text_secondary_hsla())
                            .child(format!(
                                "Monitoring {} discrete applications (Window: Last Hour)",
                                summaries.len()
                            )),
                    ),
            )
            .child(div().flex().flex_col().gap(px(8.0 * theme.scale)).children(
                summaries.into_iter().map(|sum| {
                    let total = sum.total_bytes;

                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .p(px(12.0 * theme.scale))
                        .rounded(theme.corner_radius_px(8.0))
                        .bg(theme.elevated_hsla())
                        .border_1()
                        .border_color(theme.separator_hsla())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .font_family(sf_font_family())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(13.0 * theme.scale))
                                        .text_color(theme.text_primary_hsla())
                                        .child(sum.app.display_name().to_string()),
                                )
                                .child(
                                    div()
                                        .font_family(sf_font_family())
                                        .text_size(px(11.0 * theme.scale))
                                        .text_color(theme.text_secondary_hsla())
                                        .child(format!(
                                            "Traffic: {} (Sent: {}, Recv: {})",
                                            format_bytes(total),
                                            format_bytes(sum.total_bytes_sent),
                                            format_bytes(sum.total_bytes_received)
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0 * theme.scale))
                                .child(
                                    div()
                                        .px(px(8.0 * theme.scale))
                                        .py(px(3.0 * theme.scale))
                                        .rounded(theme.corner_radius_px(4.0))
                                        .bg(theme.selection_hsla())
                                        .font_family(sf_font_family())
                                        .text_size(px(10.5 * theme.scale))
                                        .text_color(theme.text_primary_hsla())
                                        .child(format!("{} Allowed", sum.allowed_count)),
                                )
                                .child(
                                    div()
                                        .px(px(8.0 * theme.scale))
                                        .py(px(3.0 * theme.scale))
                                        .rounded(theme.corner_radius_px(4.0))
                                        .bg(theme.selection_hsla())
                                        .font_family(sf_font_family())
                                        .text_size(px(10.5 * theme.scale))
                                        .text_color(theme.text_primary_hsla())
                                        .child(format!("{} Blocked", sum.blocked_count)),
                                ),
                        )
                }),
            ))
    }

    fn render_clipboard(&self, theme: &Theme) -> impl IntoElement {
        let is_sensitive = self.view_model.clipboard.is_sensitive_active();
        let alerts = self.view_model.clipboard.alerts();

        div()
            .flex()
            .flex_col()
            .gap(px(12.0 * theme.scale))
            .w_full()
            .p(px(20.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0 * theme.scale))
                            .text_color(theme.text_primary_hsla())
                            .child("Clipboard Privacy Guard & Sensitive Auto-Clear"),
                    )
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(12.0 * theme.scale))
                            .text_color(theme.text_secondary_hsla())
                            .child(if is_sensitive {
                                "Sensitive token or credentials active on clipboard. Auto-clear timer running."
                            } else {
                                "Clipboard is clean. Automatic credential scrubbing active."
                            }),
                    ),
            )
            // Status Card
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .p(px(14.0 * theme.scale))
                    .rounded(theme.corner_radius_px(8.0))
                    .bg(if is_sensitive {
                        theme.selection_hsla()
                    } else {
                        theme.elevated_hsla()
                    })
                    .border_1()
                    .border_color(theme.separator_hsla())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_family(sf_font_family())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(13.0 * theme.scale))
                                    .text_color(theme.text_primary_hsla())
                                    .child(if is_sensitive {
                                        "⚠️ Sensitive Content On Clipboard"
                                    } else {
                                        "🛡️ Clipboard Guard Standing By"
                                    }),
                            )
                            .child(
                                div()
                                    .font_family(sf_font_family())
                                    .text_size(px(11.0 * theme.scale))
                                    .text_color(theme.text_secondary_hsla())
                                    .child("Whitelisted: 1Password, Bitwarden, KeePassXC, Nexus-OS"),
                            ),
                    ),
            )
            // Recent Alerts List
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0 * theme.scale))
                    .children(alerts.iter().map(|alert| {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .p(px(10.0 * theme.scale))
                            .rounded(theme.corner_radius_px(6.0))
                            .bg(theme.elevated_hsla())
                            .border_1()
                            .border_color(theme.separator_hsla())
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .font_family(sf_font_family())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(12.5 * theme.scale))
                                            .text_color(theme.text_primary_hsla())
                                            .child(format!(
                                                "App '{}' read {}",
                                                alert.reading_app.display_name(),
                                                alert.sensitive_kind
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_family(sf_font_family())
                                            .text_size(px(11.0 * theme.scale))
                                            .text_color(theme.text_secondary_hsla())
                                            .child(format!("Snippet: {}", alert.redacted_snippet)),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(6.0 * theme.scale))
                                    .py(px(2.0 * theme.scale))
                                    .rounded(theme.corner_radius_px(4.0))
                                    .bg(theme.selection_hsla())
                                    .font_family(sf_font_family())
                                    .text_size(px(10.0 * theme.scale))
                                    .text_color(theme.text_primary_hsla())
                                    .child(format!("{:?}", alert.severity)),
                            )
                    })),
            )
    }

    fn render_audit_log(&self, theme: &Theme) -> impl IntoElement {
        let logs = &self.view_model.audit_log;

        div()
            .flex()
            .flex_col()
            .gap(px(12.0 * theme.scale))
            .w_full()
            .p(px(20.0 * theme.scale))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(16.0 * theme.scale))
                            .text_color(theme.text_primary_hsla())
                            .child("Deterministic Privacy & Security Audit Log"),
                    )
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(12.0 * theme.scale))
                            .text_color(theme.text_secondary_hsla())
                            .child(format!("Recorded {} immutable audit events", logs.len())),
                    ),
            )
            .child(div().flex().flex_col().gap(px(6.0 * theme.scale)).children(
                logs.iter().rev().map(|entry| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .p(px(10.0 * theme.scale))
                        .rounded(theme.corner_radius_px(6.0))
                        .bg(theme.elevated_hsla())
                        .border_1()
                        .border_color(theme.separator_hsla())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .font_family(sf_font_family())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(12.5 * theme.scale))
                                        .text_color(theme.text_primary_hsla())
                                        .child(format!(
                                            "[{}] {}",
                                            entry.category, entry.description
                                        )),
                                )
                                .child(
                                    div()
                                        .font_family(sf_font_family())
                                        .text_size(px(10.5 * theme.scale))
                                        .text_color(theme.text_tertiary_hsla())
                                        .child(format!(
                                            "Actor: {} • ID: #{}",
                                            entry.actor, entry.id
                                        )),
                                ),
                        )
                        .when(entry.is_critical, |this| {
                            this.child(
                                div()
                                    .px(px(6.0 * theme.scale))
                                    .py(px(2.0 * theme.scale))
                                    .rounded(theme.corner_radius_px(4.0))
                                    .bg(theme.selection_hsla())
                                    .font_family(sf_font_family())
                                    .text_size(px(10.0 * theme.scale))
                                    .text_color(theme.text_primary_hsla())
                                    .child("CRITICAL"),
                            )
                        })
                }),
            ))
    }
}

/// Selected Overview (and other) tabs use the inverted pill: light fill,
/// dark ink. Using `accent` + `text_primary` is white-on-white in stealth.
pub fn selected_tab_style(theme: &Theme, is_active: bool) -> (Hsla, Hsla) {
    if is_active {
        (theme.inverted_hsla(), theme.on_inverted_hsla())
    } else {
        (
            Hsla::from(nx_ui::Color::TRANSPARENT),
            theme.text_secondary_hsla(),
        )
    }
}

impl Render for PrivacyDashboardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if cx.has_global::<Theme>() {
            *cx.global::<Theme>()
        } else {
            Theme::stealth()
        };

        rounded_window_frame(
            &theme,
            div()
                .flex()
                .flex_col()
                .w_full()
                .h_full()
                .overflow_hidden()
                .font_family(sf_font_family())
                .child(self.render_header(&theme))
                .child(self.render_tabs(&theme, cx))
                .child(match self.view_model.active_tab {
                    DashboardTab::Overview => self.render_overview(&theme).into_any_element(),
                    DashboardTab::Permissions => self.render_permissions(&theme).into_any_element(),
                    DashboardTab::Network => self.render_network(&theme).into_any_element(),
                    DashboardTab::Clipboard => self.render_clipboard(&theme).into_any_element(),
                    DashboardTab::AuditLog => self.render_audit_log(&theme).into_any_element(),
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[gpui::test]
    fn test_privacy_dashboard_view_renders_initial(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let vm = DashboardViewModel::new();
            let entity = cx.new(|_cx| PrivacyDashboardView::new(vm));
            assert_eq!(
                entity.read(cx).view_model.active_tab,
                DashboardTab::Overview
            );
        });
    }

    #[gpui::test]
    fn test_privacy_dashboard_tab_switching(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let vm = DashboardViewModel::new();
            let entity = cx.new(|_cx| PrivacyDashboardView::new(vm));

            entity.update(cx, |this, _cx| {
                this.set_tab(DashboardTab::Permissions);
                assert_eq!(this.view_model.active_tab, DashboardTab::Permissions);

                this.set_tab(DashboardTab::Network);
                assert_eq!(this.view_model.active_tab, DashboardTab::Network);

                this.set_tab(DashboardTab::Clipboard);
                assert_eq!(this.view_model.active_tab, DashboardTab::Clipboard);

                this.set_tab(DashboardTab::AuditLog);
                assert_eq!(this.view_model.active_tab, DashboardTab::AuditLog);
            });
        });
    }

    #[gpui::test]
    fn test_privacy_dashboard_kill_switch_toggle(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let vm = DashboardViewModel::new();
            let entity = cx.new(|_cx| PrivacyDashboardView::new(vm));

            entity.update(cx, |this, _cx| {
                assert!(!this
                    .view_model
                    .kill_switches
                    .is_engaged(KillSwitchKind::CameraMaster));
                this.toggle_kill_switch(KillSwitchKind::CameraMaster);
                assert!(this
                    .view_model
                    .kill_switches
                    .is_engaged(KillSwitchKind::CameraMaster));
                assert_eq!(this.view_model.audit_log.len(), 1);
            });
        });
    }

    #[test]
    fn selected_overview_tab_uses_dark_ink_on_inverted_pill() {
        let theme = Theme::stealth();
        let (bg, fg) = selected_tab_style(&theme, true);
        assert!(
            bg.l > 0.6,
            "selected pill must be inverted/light, got l={}",
            bg.l
        );
        assert!(
            fg.l < 0.3,
            "selected tab label must be dark ink, got l={}",
            fg.l
        );
        let (idle_bg, idle_fg) = selected_tab_style(&theme, false);
        assert!(idle_bg.a < 0.05);
        assert!(idle_fg.l > fg.l);
    }
}
