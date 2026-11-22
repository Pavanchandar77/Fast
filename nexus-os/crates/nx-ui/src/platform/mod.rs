//! Platform hooks. Backdrop values are pure; the Win32 calls are `cfg(windows)`.

mod backdrop;

pub use backdrop::{BackdropRequest, CornerPreference, SystemBackdrop};

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{apply_hwnd, present_client_window};
