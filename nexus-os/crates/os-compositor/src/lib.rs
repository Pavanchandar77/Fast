//! os-compositor: Window management, spring physics solver, hooks, and shell surfaces.

pub mod hooks;
pub mod hotkeys;
pub mod manager;
pub mod spring;
pub mod gpu;

pub use hooks::WinEventListener;
pub use hotkeys::GlobalKeyboardHook;
pub use manager::WindowManager;
pub use spring::{SpringConfig, SpringSolver};
pub use gpu::{
    create_border_overlay_window, create_command_bar_window, create_top_bar_window,
    BorderOverlay, CommandBar, GpuContext, TopBar,
};
