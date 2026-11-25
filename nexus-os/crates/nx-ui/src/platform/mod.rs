//! Platform hooks. Backdrop values are pure; the Win32 calls are `cfg(windows)`.

mod backdrop;
mod focus;

pub use backdrop::{BackdropRequest, CornerPreference, SystemBackdrop};
pub use focus::{
    bring_to_foreground, hide_async, restore_and_focus, show_async, show_noactivate_async, ShowCmd,
};

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{apply_hwnd, present_client_window};
