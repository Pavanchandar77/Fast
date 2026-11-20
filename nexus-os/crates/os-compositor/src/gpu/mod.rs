//! Nexus-OS GPU Shell Rendering Engine

pub mod border_overlay;
pub mod context;
pub mod shaders;
pub mod window_surface;

pub use border_overlay::{create_border_overlay_window, BorderOverlay};
pub use context::GpuContext;
pub use shaders::{SquirclePipeline, SquircleUniforms};
pub use window_surface::ShellSurface;
