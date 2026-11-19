//! Nexus-OS design system and GPU UI toolkit.
//!
//! Layout, springs, theming, and the scene graph are platform-free.
//! The wgpu renderer and the `nx-ui-gallery` example run on Windows and Linux.
//! Win32 DWM hooks are `cfg(windows)` and do not outlive the HWND.

#![allow(clippy::too_many_arguments)]

pub mod anim;
pub mod atlas;
pub mod color;
pub mod draw;
pub mod error;
pub mod gallery;
pub mod geom;
pub mod gpui_theme;
pub mod gpu;
pub mod icon;
pub mod image;
pub mod input;
pub mod layout;
pub mod material;
pub mod platform;
pub mod scene;
pub mod sdf;
pub mod spring;
pub mod text;
pub mod theme;
pub mod tokens;
pub mod widgets;

pub use anim::{
    FrameClock, FrameDecision, FIRST_FRAME_BUDGET_MS, FRAME_BUDGET_MS, TARGET_REFRESH_HZ,
};
pub use color::Color;
pub use error::{Error, Result};
pub use geom::{CornerRadii, Insets, Point, Rect, Size};
pub use scene::{NodeId, Scene};
pub use spring::{Spring, SpringConfig};
pub use theme::{Accent, Theme, ThemeMode};
pub use gpui_theme::{sf_font_family, step_spring, ThemeStyledExt};
