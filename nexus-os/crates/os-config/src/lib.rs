//! os-config: Declarative configuration parser and live hot-reloader.

pub mod defaults;
pub mod hotkey;
pub mod schema;
pub mod watcher;

pub use hotkey::{parse_hotkey, parse_or_default, HotkeyChord};
pub use schema::SystemConfig;
pub use watcher::ConfigWatcher;
