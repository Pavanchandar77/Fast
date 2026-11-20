//! Window host for Nexus Explorer via GPUI with DWM Mica/Acrylic backdrop hooks.

use crate::gpui_view::ExplorerView;
use crate::view_model::ExplorerWindow;
use gpui::AppContext;
use nx_ui::theme::Theme;

/// Explorer application runner that hosts ExplorerView inside a GPUI window.
pub struct ExplorerApp {
    explorer_window: ExplorerWindow,
}

impl ExplorerApp {
    pub fn new(explorer_window: ExplorerWindow) -> Option<Self> {
        Some(Self { explorer_window })
    }

    pub fn run(self) {
        open_explorer(self.explorer_window);
    }
}

/// Open an ExplorerView window via GPUI with Mica/Acrylic backdrop styling.
pub fn open_explorer(window: ExplorerWindow) {
    gpui::Application::new().run(move |cx| {
        let theme = Theme::dark(nx_ui::theme::Accent::Blue);
        if !cx.has_global::<Theme>() {
            cx.set_global(theme);
        }
        let bounds = gpui::Bounds {
            origin: gpui::point(gpui::px(100.0), gpui::px(100.0)),
            size: gpui::size(gpui::px(1024.0), gpui::px(700.0)),
        };
        let _ = cx.open_window(theme.blurred_window_options(bounds), move |_window, cx| {
            cx.new(|_cx| ExplorerView::new(window))
        });
    });
}
