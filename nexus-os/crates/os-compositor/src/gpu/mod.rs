//! Nexus-OS GPU Shell Rendering Engine

pub mod context;
pub mod shaders;
pub mod window_surface;
pub mod border_overlay;
pub mod top_bar;
pub mod command_bar;

pub use context::GpuContext;
pub use shaders::{SquirclePipeline, SquircleUniforms};
pub use window_surface::ShellSurface;
pub use border_overlay::{create_border_overlay_window, BorderOverlay};
pub use top_bar::{create_top_bar_window, TopBar};
pub use command_bar::{create_command_bar_window, CommandBar};
