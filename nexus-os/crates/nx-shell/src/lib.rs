//! Nexus-OS shell: Dynamic Island, dock, Mission Control, Spotlight, vsync frame scheduler.

pub mod backdrop;
#[cfg(windows)]
pub mod capture;
pub mod catalog;
pub mod clip_history;
pub mod clock;
pub mod dock;
pub mod dock_autohide;
pub mod dock_popup;
pub mod error;
pub mod expose;
pub mod gpui_views;
pub mod guard;
pub mod host;
pub mod hot_corners;
pub mod icons;
pub mod island;
pub mod island_view;
pub mod live;
pub mod menu_bar;
pub mod motion_overlay;
pub mod scheduler;
pub mod sensors;
pub mod spotlight;
pub mod squircle;
pub mod util;

pub use backdrop::{choose, BackdropChoice, BackdropPath};
pub use clock::{ClockSample, Month, Weekday};
pub use dock::{
    hit_dock, layout_dock, magnification_target, DockApp, DockCommand, DockEdge, DockIcon,
    DockLayout, DockModel, DockPlacement, JumpEntry, JumpMenu, Magnification,
};
pub use error::{ShellError, ShellResult};
pub use expose::{
    grid_shape, layout_expose, thumbnail_slots, ExposeModel, ExposeParams, ExposeTile,
    ExposeWindow, ThumbnailSlot,
};
pub use gpui_views::{
    request_shell_launch, DockView, MenuBarView, MissionControlView, ShellLaunch, SpotlightView,
};
pub use host::{
    apply_backdrop, dwm_flush, register_thumbnail, unregister_thumbnail, update_thumbnail,
    LiveThumbnailManager,
};
pub use island::{
    BatteryAlert, IslandModel, IslandShape, COLLAPSED_HEIGHT, EXPANDED_HEIGHT, ISLAND_WIDTH,
    PEEK_HEIGHT, SLIT_HEIGHT,
};
pub use island_view::{eyes_element, IslandHooks, IslandView};
pub use live::{HudKind, LiveKind, LiveState};
pub use menu_bar::{
    bar_height, hit_menu_bar, layout_menu_bar, BarSlot, ClipboardWidget, MeasuredSlot, MenuAction,
    MenuBarLayout, MenuBarModel, MenuEntry, PlacedSlot, StatusKind, StatusWidget, VolumeWidget,
};
pub use motion_overlay::{dock_icon_rect, MotionOverlayView, OverlayEvent, Snapshot};
pub use scheduler::{present_pace, FrameScheduler, PresentMode, PresentPace};
pub use sensors::NowPlaying;
pub use spotlight::{
    engine_from_paths, eval_calculator, fuzzy_score, layout_spotlight, ProviderKind, Spotlight,
    SpotlightHit, SpotlightLayout,
};
pub use squircle::{coverage_mask, Squircle};
