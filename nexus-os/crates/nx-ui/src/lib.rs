//! Nexus-OS design system and UI toolkit.
//!
//! Layout, springs, theming, and widgets are platform-free.
//! Win32 DWM hooks are `cfg(windows)` and do not outlive the HWND.

#![allow(clippy::too_many_arguments)]

pub mod anim;
pub mod color;
pub mod error;
pub mod geom;
pub mod gpui_theme;
pub mod icon;
pub mod input;
pub mod layout;
pub mod material;
pub mod platform;
pub mod sdf;
pub mod spring;
pub mod theme;
pub mod tokens;
pub mod widgets;

pub use anim::{
    FrameClock, FrameDecision, FIRST_FRAME_BUDGET_MS, FRAME_BUDGET_MS, TARGET_REFRESH_HZ,
};
pub use color::Color;
pub use error::{Error, Result};
pub use geom::{CornerRadii, Insets, Point, Rect, Size};
pub use gpui_theme::{
    mono_font_family, sf_font_family, step_spring, upper_label, window_chrome, ThemeStyledExt,
};
pub use spring::{Spring, SpringConfig};
pub use theme::{Accent, Theme, ThemeMode};
