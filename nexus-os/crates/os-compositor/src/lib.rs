//! os-compositor: Window management, spring physics solver, hooks, and shell surfaces.

pub mod hooks;
pub mod hotkeys;
pub mod manager;
pub mod spring;

pub use hooks::WinEventListener;
pub use hotkeys::GlobalKeyboardHook;
pub use manager::WindowManager;
pub use spring::{SpringConfig, SpringSolver};
