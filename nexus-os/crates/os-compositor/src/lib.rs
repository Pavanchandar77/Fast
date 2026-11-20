//! os-compositor: Window management, spring physics solver, hooks, and shell surfaces.

#[cfg(windows)]
pub mod gpu;
#[cfg(windows)]
pub mod hooks;
#[cfg(windows)]
pub mod hotkeys;
#[cfg(windows)]
pub mod manager;
pub mod spring;

#[cfg(windows)]
pub use gpu::{create_border_overlay_window, BorderOverlay, GpuContext};
#[cfg(windows)]
pub use hooks::WinEventListener;
#[cfg(windows)]
pub use hotkeys::{pump_messages, GlobalKeyboardHook};
#[cfg(windows)]
pub use manager::WindowManager;
pub use spring::{SpringConfig, SpringSolver};
