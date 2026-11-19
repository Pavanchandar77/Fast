//! Nexus-OS macOS-grade shell: menu bar, dock, Mission Control, Spotlight, vsync frame scheduler.

pub mod backdrop;
pub mod clock;
pub mod dock;
pub mod error;
pub mod expose;
pub mod host;
pub mod menu_bar;
pub mod scene;
pub mod scheduler;
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
pub use host::{
    apply_backdrop, dwm_flush, register_thumbnail, unregister_thumbnail, update_thumbnail,
    LiveThumbnailManager,
};
pub use menu_bar::{
    bar_height, hit_menu_bar, layout_menu_bar, BarSlot, ClipboardWidget, MeasuredSlot, MenuAction,
    MenuBarLayout, MenuBarModel, MenuEntry, PlacedSlot, StatusKind, StatusWidget, VolumeWidget,
};
pub use scene::{render_dock, render_expose, render_menu_bar, render_spotlight};
pub use scheduler::{present_pace, FrameScheduler, PresentMode, PresentPace};
pub use spotlight::{
    engine_from_paths, eval_calculator, fuzzy_score, layout_spotlight, ProviderKind, Spotlight,
    SpotlightHit, SpotlightLayout,
};
pub use squircle::{coverage_mask, Squircle};
