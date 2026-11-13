//! os-core: Kernel-level NT API abstractions, Win32 wrappers, and telemetry primitives.

pub mod error;
pub mod ntdll;
pub mod process;
pub mod window;

pub use error::{CoreError, CoreResult};
