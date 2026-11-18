//! # nx-explorer — the Nexus-OS file manager model
//!
//! A pure state machine (no rendering, no I/O): the UI sends [`Msg`]s and
//! performs the returned [`Effect`]s using `nx-fs`. Keeps what people love
//! about Windows Explorer (shortcuts, Recycle Bin, breadcrumbs, details view)
//! and adds tabs, split panes, Miller columns, Quick Look, type-to-select and
//! a command palette.
//!
//! ```
//! use nx_explorer::{Explorer, Effect, Msg};
//! let (mut ex, fx) = Explorer::new("/home/me".into());
//! let Some(Effect::ListDir { request, .. }) = fx.first().cloned() else { panic!() };
//! ex.update(Msg::ListingDone { request });
//! assert!(ex.pane().is_some());
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod breadcrumb;
pub mod explorer;
pub mod history;
pub mod input;
pub mod keymap;
pub mod palette;
pub mod pane;
pub mod sidebar;
pub mod tab;

pub use action::{Action, CursorMove, DirDiff, Effect, Msg, PaneId, ViewKind};
pub use explorer::{Explorer, Focus, Prompt};
pub use keymap::{Chord, Key, Keymap, Mods};
pub use pane::{Location, Pane};

#[cfg(test)]
mod tests;
