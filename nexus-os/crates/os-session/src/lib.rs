//! Session library: product startup orchestration for Nexus-OS.

pub mod startup;

pub use startup::{
    open_gpui_windows, start_session, start_session_with, ComponentStatus, GpuiSessionWindows,
    SessionComponent, SessionHandle,
};
