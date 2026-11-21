//! Session library: product startup orchestration for Nexus-OS.

#[cfg(windows)]
pub mod motion_bridge;
pub mod motion_plan;
pub mod startup;

pub use startup::{
    open_gpui_windows, start_session, start_session_with, toggle_gpui_spotlight, ComponentStatus,
    GpuiSessionWindows, SessionComponent, SessionHandle,
};
