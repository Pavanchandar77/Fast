use crate::physics::{SpringF32, SpringRect};
use crate::telemetry::{GovernorProfile, SystemTelemetry};
use crate::theme::NexusTheme;
use egui::{Align2, Color32, Context, FontId, Id, Key, Pos2, Rect, Sense, Vec2};
use std::time::Instant;

#[derive(Clone)]
pub struct TiledSurface {
    pub id: usize,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub tag: &'static str,
    pub accent: Color32,
    pub metrics: Vec<(&'static str, String)>,
}

pub struct NexusShell {
    pub telemetry: SystemTelemetry,
    pub active_workspace: usize,
    pub focused_window: usize,
    pub focus_border: SpringRect,
    pub island_expansion: SpringF32,
    pub island_hovered: bool,
    pub command_bar_open: bool,
    pub command_bar_scale: SpringF32,
    pub control_center_open: bool,
    pub search_query: String,
    pub selected_result: usize,
    pub dock_hover_x: Option<f32>,
    pub surfaces: Vec<TiledSurface>,
    last_frame: Instant,
}

impl NexusShell {
    pub fn new() -> Self {
        let initial_rect = Rect::from_min_size(Pos2::new(24.0, 52.0), Vec2::new(600.0, 400.0));
        Self {
            telemetry: SystemTelemetry::new(),
            active_workspace: 0,
            focused_window: 0,
            focus_border: SpringRect::new(initial_rect, 340.0, 32.0),
            island_expansion: SpringF32::new(0.0, 320.0, 28.0),
            island_hovered: false,
            command_bar_open: true,
            command_bar_scale: SpringF32::new(1.0, 360.0, 30.0),
            control_center_open: false,
            search_query: String::new(),
            selected_result: 0,
            dock_hover_x: None,
            surfaces: vec![
                TiledSurface {
                    id: 0,
                    title: "nexus-compositor // kernel_loop.rs",
                    subtitle: "120Hz Symplectic Euler • DirectComposition Surface",
                    tag: "RUST ENGINE",
                    accent: NexusTheme::CYAN_ELECTRIC,
                    metrics: vec![
                        ("Frame Pacing", "0.42 ms (240 fps cap)".into()),
                        ("Swapchain Alpha", "PreMultiplied + DirectComposition".into()),
                        ("DWM Redirection", "Bypassed (WS_EX_NOREDIRECTIONBITMAP)".into()),
                    ],
                },
                TiledSurface {
                    id: 1,
                    title: "os-governor // working_set_trimmer",
                    subtitle: "JobObject Rate Cap • 0.5ms Timer Resolution Locked",
                    tag: "GOVERNOR",
                    accent: NexusTheme::ORANGE_FIRE,
                    metrics: vec![
                        ("Background Throttle", "Active (Eco Cap 4% CPU)".into()),
                        ("Reclaimed Standby", "1.84 GB Purged".into()),
                        ("Foreground Boost", "HIGH_PRIORITY_CLASS".into()),
                    ],
                },
                TiledSurface {
                    id: 2,
                    title: "os-vfs // mft_radix_index",
                    subtitle: "NTFS USN Journal Live Stream • Zero-Allocation Lookup",
                    tag: "INSTANT VFS",
                    accent: NexusTheme::EMERALD_OK,
                    metrics: vec![
                        ("Indexed Nodes", "412,890 paths in 14.2 MB RAM".into()),
                        ("Query Latency", "0.08 ms p99".into()),
                        ("Journal State", "USN_REASON_FILE_CREATE Synced".into()),
                    ],
                },
            ],
            last_frame: Instant::now(),
        }
    }

    pub fn update(&mut self, ctx: &Context) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;

        self.telemetry.tick();

        // Global Hotkeys inside the shell
        ctx.input(|i| {
            if (i.modifiers.alt || i.modifiers.ctrl) && i.key_pressed(Key::Space)
                || (i.modifiers.ctrl && i.key_pressed(Key::K))
            {
                self.command_bar_open = !self.command_bar_open;
                self.command_bar_scale
                    .set_target(if self.command_bar_open { 1.0 } else { 0.0 });
            }
            if i.key_pressed(Key::Escape) {
                if self.command_bar_open {
                    self.command_bar_open = false;
                    self.command_bar_scale.set_target(0.0);
                } else if self.control_center_open {
                    self.control_center_open = false;
                }
            }
            if i.key_pressed(Key::Tab) && !self.command_bar_open {
                self.focused_window = (self.focused_window + 1) % self.surfaces.len();
            }
        });

        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());

        // 0. Deep Obsidian Canvas Background with Subtle Radial Vignette
        painter.rect_filled(screen, 0.0, NexusTheme::BG_VOID);

        // 1. Render Dwindle Tiled Stage (Master Left + Stacked Right)
        self.render_dwindle_stage(ctx, &painter, screen, dt);

        // 2. Render 28px Top Bar + Morphing Dynamic Island
        self.render_top_bar(ctx, &painter, screen, dt);

        // 3. Render Floating iOS Physics Dock
        self.render_physics_dock(ctx, &painter, screen);

        // 4. Render Spotlight Command Bar (Ctrl+K / Alt+Space)
        self.render_command_bar(ctx, screen, dt);

        // Request continuous 120Hz repaint while any spring is in motion
        ctx.request_repaint();
    }

    fn render_top_bar(&mut self, ctx: &Context, painter: &egui::Painter, screen: Rect, dt: f32) {
        let bar_rect = Rect::from_min_size(
            Pos2::new(screen.min.x + 12.0, screen.min.y + 8.0),
            Vec2::new(screen.width() - 24.0, 32.0),
        );

        NexusTheme::draw_glass_squircle(painter, bar_rect, 16.0, NexusTheme::GLASS_BASE, None);

        // Left: Workspace Pills [1..5]
        let mut x_cursor = bar_rect.min.x + 10.0;
        for ws in 0..5 {
            let is_active = self.active_workspace == ws;
            let pill_w = if is_active { 34.0 } else { 22.0 };
            let pill_rect = Rect::from_center_size(
                Pos2::new(x_cursor + pill_w * 0.5, bar_rect.center().y),
                Vec2::new(pill_w, 20.0),
            );

            let resp = ctx.interact(
                pill_rect,
                Id::new(("ws_pill", ws)),
                Sense::click(),
            );
            if resp.clicked() {
                self.active_workspace = ws;
            }

            let fill = if is_active {
                Color32::from_rgba_unmultiplied(0, 255, 255, 38)
            } else if resp.hovered() {
                Color32::from_white_alpha(20)
            } else {
                Color32::from_white_alpha(8)
            };

            NexusTheme::draw_glass_squircle(
                painter,
                pill_rect,
                10.0,
                fill,
                if is_active {
                    Some(NexusTheme::CYAN_ELECTRIC)
                } else {
                    None
                },
            );

            painter.text(
                pill_rect.center(),
                Align2::CENTER_CENTER,
                format!("{}", ws + 1),
                FontId::proportional(11.5),
                if is_active {
                    NexusTheme::CYAN_ELECTRIC
                } else {
                    NexusTheme::TEXT_SECONDARY
                },
            );

            x_cursor += pill_w + 6.0;
        }

        // Center: Dynamic Island Telemetry Pill (Expands smoothly on hover)
        let island_t = self.island_expansion.step(dt);
        let island_w = 240.0 + island_t * 140.0;
        let island_h = 24.0 + island_t * 6.0;
        let island_rect = Rect::from_center_size(
            Pos2::new(bar_rect.center().x, bar_rect.center().y),
            Vec2::new(island_w, island_h),
        );

        let island_resp = ctx.interact(island_rect, Id::new("dynamic_island"), Sense::click());
        self.island_hovered = island_resp.hovered();
        self.island_expansion
            .set_target(if self.island_hovered { 1.0 } else { 0.0 });
        if island_resp.clicked() {
            self.command_bar_open = !self.command_bar_open;
            self.command_bar_scale
                .set_target(if self.command_bar_open { 1.0 } else { 0.0 });
        }

        NexusTheme::draw_glass_squircle(
            painter,
            island_rect,
            island_h * 0.5,
            Color32::from_rgb(10, 12, 18),
            if self.island_hovered {
                Some(NexusTheme::CYAN_ELECTRIC)
            } else {
                None
            },
        );

        let island_text = if self.island_hovered {
            format!(
                "CPU {:>4.1}%  •  RAM {:.1}/{:.0} GB  •  Click for Command Bar",
                self.telemetry.cpu_usage, self.telemetry.ram_used_gb, self.telemetry.ram_total_gb
            )
        } else {
            format!(
                "●  CPU {:>4.1}%   |   RAM {:.1} GB",
                self.telemetry.cpu_usage, self.telemetry.ram_used_gb
            )
        };

        painter.text(
            island_rect.center(),
            Align2::CENTER_CENTER,
            island_text,
            FontId::proportional(11.5),
            NexusTheme::TEXT_PRIMARY,
        );

        // Right: Governor Power Pill & Close Button
        let close_rect = Rect::from_center_size(
            Pos2::new(bar_rect.max.x - 20.0, bar_rect.center().y),
            Vec2::new(22.0, 22.0),
        );
        let close_resp = ctx.interact(close_rect, Id::new("close_btn"), Sense::click());
        if close_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        NexusTheme::draw_glass_squircle(
            painter,
            close_rect,
            11.0,
            if close_resp.hovered() {
                NexusTheme::ORANGE_FIRE
            } else {
                Color32::from_white_alpha(15)
            },
            None,
        );
        painter.text(
            close_rect.center(),
            Align2::CENTER_CENTER,
            "×",
            FontId::proportional(14.0),
            NexusTheme::TEXT_PRIMARY,
        );

        // Governor Mode Toggle Pill
        let gov_rect = Rect::from_center_size(
            Pos2::new(bar_rect.max.x - 98.0, bar_rect.center().y),
            Vec2::new(112.0, 22.0),
        );
        let gov_resp = ctx.interact(gov_rect, Id::new("gov_pill"), Sense::click());
        if gov_resp.clicked() {
            self.telemetry.profile = self.telemetry.profile.next();
        }
        let gov_accent = match self.telemetry.profile {
            GovernorProfile::EcoSilent => NexusTheme::EMERALD_OK,
            GovernorProfile::Balanced => NexusTheme::CYAN_ELECTRIC,
            GovernorProfile::MaxCompute => NexusTheme::ORANGE_FIRE,
        };
        NexusTheme::draw_glass_squircle(
            painter,
            gov_rect,
            11.0,
             NexusTheme::GLASS_PILL,
            Some(gov_accent),
        );
        painter.text(
            gov_rect.center(),
            Align2::CENTER_CENTER,
            self.telemetry.profile.label(),
            FontId::proportional(10.5),
            gov_accent,
        );
    }

    fn render_dwindle_stage(
        &mut self,
        ctx: &Context,
        painter: &egui::Painter,
        screen: Rect,
        dt: f32,
    ) {
        let stage = Rect::from_min_max(
            Pos2::new(screen.min.x + 16.0, screen.min.y + 50.0),
            Pos2::new(screen.max.x - 16.0, screen.max.y - 76.0),
        );

        let gap = 12.0;
        let master_w = (stage.width() - gap) * 0.56;
        let stack_w = stage.width() - gap - master_w;
        let stack_h = (stage.height() - gap) * 0.5;

        let rects = [
            Rect::from_min_size(stage.min, Vec2::new(master_w, stage.height())),
            Rect::from_min_size(
                Pos2::new(stage.min.x + master_w + gap, stage.min.y),
                Vec2::new(stack_w, stack_h),
            ),
            Rect::from_min_size(
                Pos2::new(stage.min.x + master_w + gap, stage.min.y + stack_h + gap),
                Vec2::new(stack_w, stack_h),
            ),
        ];

        // Update spring target to currently focused window
        self.focus_border.set_target(rects[self.focused_window]);
        let animated_focus_rect = self.focus_border.step(dt);

        for (idx, tile_rect) in rects.iter().enumerate() {
            let surface = &self.surfaces[idx];
            let is_focused = self.focused_window == idx;

            let resp = ctx.interact(*tile_rect, Id::new(("tile", idx)), Sense::click());
            if resp.clicked() && !self.command_bar_open {
                self.focused_window = idx;
            }

            NexusTheme::draw_glass_squircle(
                painter,
                *tile_rect,
                20.0,
                if is_focused {
                    NexusTheme::GLASS_ELEVATED
                } else {
                    NexusTheme::GLASS_BASE
                },
                None,
            );

            // Header Tag Pill
            let tag_rect = Rect::from_min_size(
                Pos2::new(tile_rect.min.x + 18.0, tile_rect.min.y + 18.0),
                Vec2::new(96.0, 20.0),
            );
            NexusTheme::draw_glass_squircle(
                painter,
                tag_rect,
                10.0,
                Color32::from_white_alpha(12),
                Some(surface.accent),
            );
            painter.text(
                tag_rect.center(),
                Align2::CENTER_CENTER,
                surface.tag,
                FontId::proportional(10.0),
                surface.accent,
            );

            // Window Title & Subtitle
            painter.text(
                Pos2::new(tile_rect.min.x + 18.0, tile_rect.min.y + 52.0),
                Align2::LEFT_TOP,
                surface.title,
                FontId::proportional(16.0),
                NexusTheme::TEXT_PRIMARY,
            );
            painter.text(
                Pos2::new(tile_rect.min.x + 18.0, tile_rect.min.y + 74.0),
                Align2::LEFT_TOP,
                surface.subtitle,
                FontId::proportional(12.0),
                NexusTheme::TEXT_SECONDARY,
            );

            // Metric Rows
            let mut row_y = tile_rect.min.y + 112.0;
            for (k, v) in &surface.metrics {
                let row_rect = Rect::from_min_size(
                    Pos2::new(tile_rect.min.x + 18.0, row_y),
                    Vec2::new(tile_rect.width() - 36.0, 34.0),
                );
                NexusTheme::draw_glass_squircle(
                    painter,
                    row_rect,
                    10.0,
                    Color32::from_white_alpha(8),
                    None,
                );
                painter.text(
                    Pos2::new(row_rect.min.x + 12.0, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    *k,
                    FontId::proportional(12.0),
                    NexusTheme::TEXT_SECONDARY,
                );
                painter.text(
                    Pos2::new(row_rect.max.x - 12.0, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    v,
                    FontId::monospace(11.5),
                    surface.accent,
                );
                row_y += 42.0;
            }

            // Extra live process table inside tile 0
            if idx == 0 {
                painter.text(
                    Pos2::new(tile_rect.min.x + 18.0, row_y + 10.0),
                    Align2::LEFT_TOP,
                    "LIVE WORKING-SET TELEMETRY (TOP PROCESSES)",
                    FontId::proportional(11.0),
                    NexusTheme::TEXT_MUTED,
                );
                let mut py = row_y + 32.0;
                for proc in self.telemetry.top_processes.iter().take(5) {
                    if py + 28.0 > tile_rect.max.y - 16.0 {
                        break;
                    }
                    painter.text(
                        Pos2::new(tile_rect.min.x + 22.0, py),
                        Align2::LEFT_TOP,
                        format!("{} (PID {})", proc.name, proc.pid),
                        FontId::monospace(12.0),
                        NexusTheme::TEXT_PRIMARY,
                    );
                    painter.text(
                        Pos2::new(tile_rect.max.x - 22.0, py),
                        Align2::RIGHT_TOP,
                        format!("{} MB", proc.memory_mb),
                        FontId::monospace(12.0),
                        NexusTheme::CYAN_ELECTRIC,
                    );
                    py += 24.0;
                }
            }
        }

        // Gliding 120Hz Spring Focus Squircle Border (#00FFFF)
        let active_accent = self.surfaces[self.focused_window].accent;
        NexusTheme::draw_glass_squircle(
            painter,
            animated_focus_rect,
            20.0,
            Color32::TRANSPARENT,
            Some(active_accent),
        );
    }

    fn render_physics_dock(&mut self, ctx: &Context, painter: &egui::Painter, screen: Rect) {
        let items = [
            ("⚡", "Launcher", NexusTheme::CYAN_ELECTRIC),
            ("⌘", "Compositor", NexusTheme::TEXT_PRIMARY),
            ("🔥", "Governor", NexusTheme::ORANGE_FIRE),
            ("📂", "VFS Index", NexusTheme::EMERALD_OK),
            ("🛡", "Shield WFP", NexusTheme::CYAN_ELECTRIC),
        ];

        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let dock_w = 340.0;
        let dock_h = 54.0;
        let dock_rect = Rect::from_center_size(
            Pos2::new(screen.center().x, screen.max.y - 36.0),
            Vec2::new(dock_w, dock_h),
        );

        NexusTheme::draw_glass_squircle(painter, dock_rect, 24.0, NexusTheme::GLASS_ELEVATED, None);

        let spacing = dock_w / (items.len() as f32);
        for (i, (glyph, _label, color)) in items.iter().enumerate() {
            let center_x = dock_rect.min.x + spacing * (i as f32 + 0.5);
            let center_y = dock_rect.center().y;

            // Gaussian proximity magnification
            let scale = if let Some(p) = pointer_pos {
                if dock_rect.expand(24.0).contains(p) {
                    let dist = (p.x - center_x).abs();
                    1.0 + 0.38 * (-((dist / 48.0).powi(2))).exp()
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let size = 36.0 * scale;
            let icon_rect = Rect::from_center_size(
                Pos2::new(center_x, center_y - (size - 36.0) * 0.45),
                Vec2::splat(size),
            );

            let resp = ctx.interact(icon_rect, Id::new(("dock_icon", i)), Sense::click());
            if resp.clicked() {
                if i == 0 {
                    self.command_bar_open = !self.command_bar_open;
                    self.command_bar_scale
                        .set_target(if self.command_bar_open { 1.0 } else { 0.0 });
                } else if i <= 3 {
                    self.focused_window = (i - 1).min(self.surfaces.len() - 1);
                }
            }

            NexusTheme::draw_glass_squircle(
                painter,
                icon_rect,
                size * 0.32,
                NexusTheme::GLASS_PILL,
                if resp.hovered() { Some(*color) } else { None },
            );

            painter.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                *glyph,
                FontId::proportional(16.0 * scale),
                *color,
            );
        }
    }

    fn render_command_bar(&mut self, ctx: &Context, screen: Rect, dt: f32) {
        let scale = self.command_bar_scale.step(dt);
        if scale < 0.02 {
            return;
        }

        let fg = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("cmd_bar_layer"),
        ));

        // Backdrop Dimmer
        fg.rect_filled(
            screen,
            0.0,
            Color32::from_black_alpha((140.0 * scale.clamp(0.0, 1.0)) as u8),
        );

        let w = 640.0 * (0.92 + 0.08 * scale);
        let h = 360.0 * (0.92 + 0.08 * scale);
        let modal_rect = Rect::from_center_size(
            Pos2::new(screen.center().x, screen.min.y + screen.height() * 0.36),
            Vec2::new(w, h),
        );

        NexusTheme::draw_glass_squircle(
            &fg,
            modal_rect,
            24.0,
            NexusTheme::GLASS_ELEVATED,
            Some(NexusTheme::CYAN_ELECTRIC),
        );

        // Search Input Area
        egui::Area::new(Id::new("command_bar_input"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(modal_rect.min.x + 20.0, modal_rect.min.y + 18.0))
            .show(ctx, |ui| {
                ui.set_width(modal_rect.width() - 40.0);
                ui.horizontal(|ui| {
                    ui.colored_label(NexusTheme::CYAN_ELECTRIC, "⌘");
                    let edit = egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search MFT files, run governor commands, or type math (Ctrl+K to toggle)...")
                        .frame(false)
                        .font(FontId::proportional(16.0))
                        .text_color(NexusTheme::TEXT_PRIMARY)
                        .desired_width(modal_rect.width() - 80.0);
                    let resp = ui.add(edit);
                    if self.command_bar_open {
                        resp.request_focus();
                    }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                let q = self.search_query.to_lowercase();
                let commands = [
                    ("⚡ Purge Standby & Working Sets", "os-governor :: K32EmptyWorkingSet", "ACTION"),
                    ("🔥 Switch Governor -> MaxCompute", "0.5ms Timer Lock + High Priority Boost", "PROFILE"),
                    ("🧊 Switch Governor -> EcoSilent", "JobObject 4% Background CPU Rate Cap", "PROFILE"),
                    ("📐 Toggle DWM Extended Frame Bounds", "Strip 7px invisible resize borders", "COMPOSITOR"),
                    ("🔍 C:\\Fast\\nexus-os\\crates\\os-compositor\\src\\main.rs", "USN MFT Instant Match • 0.04ms", "FILE"),
                ];

                for (title, desc, badge) in commands.iter().filter(|(t, d, _)| {
                    q.is_empty() || t.to_lowercase().contains(&q) || d.to_lowercase().contains(&q)
                }) {
                    let row = ui.allocate_response(Vec2::new(modal_rect.width() - 40.0, 44.0), Sense::click());
                    if row.hovered() {
                        ui.painter().rect_filled(
                            row.rect,
                            10.0,
                            Color32::from_rgba_unmultiplied(0, 255, 255, 24),
                        );
                    }
                    if row.clicked() {
                        if title.contains("MaxCompute") {
                            self.telemetry.profile = GovernorProfile::MaxCompute;
                        } else if title.contains("EcoSilent") {
                            self.telemetry.profile = GovernorProfile::EcoSilent;
                        }
                        self.command_bar_open = false;
                        self.command_bar_scale.set_target(0.0);
                    }

                    ui.painter().text(
                        Pos2::new(row.rect.min.x + 12.0, row.rect.min.y + 14.0),
                        Align2::LEFT_CENTER,
                        *title,
                        FontId::proportional(13.5),
                        NexusTheme::TEXT_PRIMARY,
                    );
                    ui.painter().text(
                        Pos2::new(row.rect.min.x + 12.0, row.rect.min.y + 31.0),
                        Align2::LEFT_CENTER,
                        *desc,
                        FontId::proportional(11.0),
                        NexusTheme::TEXT_SECONDARY,
                    );
                    ui.painter().text(
                        Pos2::new(row.rect.max.x - 12.0, row.rect.center().y),
                        Align2::RIGHT_CENTER,
                        *badge,
                        FontId::monospace(10.5),
                        NexusTheme::CYAN_ELECTRIC,
                    );
                }
            });
    }
}