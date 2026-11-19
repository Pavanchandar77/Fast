//! # nx-explorer-ui — Finder-quality file window view-model and renderer for Nexus-OS
//!
//! Renders `nx-explorer`'s model with `nx-ui` design tokens, virtualized views over 100k+
//! rows, live folder watcher diffs, and crash-safe on-disk index persistence.

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod context_menu;
pub mod drag_drop;
pub mod gpui_view;
pub mod host;
pub mod icons;
pub mod index_cache;
pub mod ops_progress;
pub mod preview;
pub mod rubber_band;
pub mod sidebar;
pub mod split;
pub mod status_bar;
pub mod tabs;
pub mod toolbar;
pub mod view_model;
pub mod views;
pub mod watcher;

pub use context_menu::{ContextItem, ExplorerContextMenu};
pub use gpui_view::{format_bytes, ExplorerView, FileRowSnapshot};
pub use drag_drop::{DragDropState, DropAction};
#[cfg(windows)]
pub use host::ExplorerApp;
pub use index_cache::PersistentIndexManager;
pub use ops_progress::{ActiveOp, OpsProgressView};
pub use preview::PreviewPane;
pub use rubber_band::RubberBand;
pub use sidebar::{ExplorerSidebar, SidebarEntry, SidebarSection};
pub use split::SplitLayout;
pub use status_bar::StatusBar;
pub use tabs::{TabBar, TabItem};
pub use toolbar::{ToolbarAction, UnifiedToolbar};
pub use view_model::ExplorerWindow;
pub use watcher::{FolderWatcher, RawWatcherEvent, WatcherDebouncer};
