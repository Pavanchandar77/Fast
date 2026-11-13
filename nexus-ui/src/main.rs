mod physics;
mod shell;
mod telemetry;
mod theme;

use eframe::egui;
use shell::NexusShell;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Nexus OS — UI Shell Lab")
            .with_inner_size([1360.0, 820.0])
            .with_decorations(false) // Borderless custom desktop frame
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Nexus UI Shell",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            visuals.window_fill = egui::Color32::TRANSPARENT;
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(NexusShell::new()))
        }),
    )
}

impl eframe::App for NexusShell {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update(ctx);
    }
}