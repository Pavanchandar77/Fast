//! Crash-safe journal of every system mutation Nexus applies.
//!
//! The crate is pure Rust: it does not link `windows`. A later `Platform`
//! trait (nexus-model, separate branch) can wrap [`SystemMutator`]; this
//! trait stays small on purpose.
//!
//! On-disk format (version 1), rewritten atomically on every record:
//! a temp file in the same directory is `fsync`ed and then renamed over
//! the journal. A torn rename leaves the previous complete file. Records
//! carry CRC32s. A bad tail or a bit-flipped record is skipped; an unknown
//! version is refused so a newer journal is never overwritten.

mod crc;
mod fake;
mod journal;
mod mutation;
mod mutator;
mod policy;
mod session;

pub use fake::FakeSystem;
pub use journal::{Journal, JournalError, RestoreReport};
pub use mutation::{Mutation, PriorityClass, Rect};
pub use mutator::SystemMutator;
pub use policy::{clamp_priority, parse_priority, PriorityRole};
pub use session::Session;
