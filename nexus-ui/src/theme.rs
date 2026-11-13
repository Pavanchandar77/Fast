use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, Vec2};

pub struct NexusTheme;

impl NexusTheme {
    // Core Obsidian & Glass Surfaces
    pub const BG_VOID: Color32 = Color32::from_rgb(6, 8, 12);
    pub const GLASS_BASE: Color32 = Color32::from_rgba_premultiplied(14, 18, 26, 232);
    pub const GLASS_ELEVATED: Color32 = Color32::from_rgba_premultiplied(22, 28, 40, 242);
    pub const GLASS_PILL: Color32 = Color32::from_rgba_premultiplied(28, 35, 50, 220);

    // Signature Accents
    pub const CYAN_ELECTRIC: Color32 = Color32::from_rgb(0, 255, 255); // #00FFFF
    pub const ORANGE_FIRE: Color32 = Color32::from_rgb(255, 88, 24);   // #FF5818
    pub const EMERALD_OK: Color32 = Color32::from_rgb(16, 235, 140);

    // Typography Hierarchy
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 247, 252);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 160, 184);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(88, 98, 120);

    /// Draws an iOS/visionOS multi-layered glass panel with ambient shadow,
    /// translucent body, and a 1px top-lit specular highlight rim.
    pub fn draw_glass_squircle(
        painter: &Painter,
        rect: Rect,
        radius: f32,
        fill: Color32,
        accent_glow: Option<Color32>,
    ) {
        let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5) as u8;
        let rounding = CornerRadius::same(r);

        // 1. Deep Ambient Drop Shadow (3 concentric soft passes)
        for i in 1..=3 {
            let spread = (i as f32) * 5.0;
            let alpha = (28 / i) as u8;
            let shadow_rect = rect
                .expand(spread)
                .translate(Vec2::new(0.0, (i as f32) * 3.0));
            painter.rect_filled(
                shadow_rect,
                CornerRadius::same(r.saturating_add(spread as u8)),
                Color32::from_black_alpha(alpha),
            );
        }

        // 2. Optional Active Focus Aura (Electric Cyan or Fire Orange)
        if let Some(glow) = accent_glow {
            let aura = Color32::from_rgba_unmultiplied(glow.r(), glow.g(), glow.b(), 26);
            painter.rect_filled(
                rect.expand(2.5),
                CornerRadius::same(r.saturating_add(2)),
                aura,
            );
        }

        // 3. Matte Frosted Glass Fill
        painter.rect_filled(rect, rounding, fill);

        // 4. Outer 1px Subtle Structural Border
        let outer_border = if let Some(glow) = accent_glow {
            Color32::from_rgba_unmultiplied(glow.r(), glow.g(), glow.b(), 185)
        } else {
            Color32::from_white_alpha(28)
        };
        painter.rect_stroke(
            rect,
            rounding,
            Stroke::new(1.0, outer_border),
            egui::StrokeKind::Inside,
        );

        // 5. iOS Top-Edge Specular Rim (Simulates overhead studio lighting)
        let top_left = Pos2::new(rect.min.x + radius * 0.7, rect.min.y + 0.8);
        let top_right = Pos2::new(rect.max.x - radius * 0.7, rect.min.y + 0.8);
        if top_right.x > top_left.x {
            painter.line_segment(
                [top_left, top_right],
                Stroke::new(1.0, Color32::from_white_alpha(52)),
            );
        }
    }
}