//! os-core: Kernel-level NT API abstractions, Win32 wrappers, and telemetry primitives.

pub(crate) mod effects;
pub mod error;
pub mod heartbeat;
pub mod hollow_shell;
pub(crate) mod jobs;
pub mod ntdll;
pub mod process;
pub mod recovery;
pub mod window;

pub use error::{CoreError, CoreResult};
pub use nexus_journal::{
    clamp_priority, parse_priority, Mutation, PriorityClass, PriorityRole, Rect,
};
