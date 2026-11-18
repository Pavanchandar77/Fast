//! os-core: Kernel-level NT API abstractions, Win32 wrappers, and telemetry primitives.

#[cfg(windows)]
pub(crate) mod effects;
pub mod error;
#[cfg(windows)]
pub mod heartbeat;
pub mod hollow_shell;
#[cfg(windows)]
pub(crate) mod jobs;
pub mod ntdll;
pub mod platform;
pub mod process;
pub mod recovery;
pub mod window;

pub use error::{CoreError, CoreResult};
pub use nexus_journal::{
    clamp_priority, parse_priority, Mutation, PriorityClass, PriorityRole, Rect,
};
pub use platform::Win32Platform;
