//! os-config: Declarative configuration parser and live hot-reloader.

pub mod schema;
pub mod watcher;

pub use schema::SystemConfig;
pub use watcher::ConfigWatcher;
