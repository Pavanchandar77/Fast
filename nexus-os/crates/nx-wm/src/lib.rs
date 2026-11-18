#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod animation;
pub mod error;
pub mod focus;
pub mod geometry;
pub mod keybinding;
pub mod layout;
pub mod monitor;
pub mod plan;
pub mod rules;
pub mod snapping;
pub mod state;
pub mod tree;
pub mod types;
pub mod workspace;

pub use animation::{AnimationReason, LayoutTransition};
pub use error::WmError;
pub use focus::{find_directional_candidate, find_window_at_point};
pub use geometry::{Direction, Point, Rect};
pub use keybinding::{
    parse_action, parse_key_combination, Action, Key, KeyCombination, KeybindingMap, ModKey,
    Modifiers,
};
pub use layout::{compute_layout, LayoutKind, LayoutParams};
pub use monitor::Monitor;
pub use plan::{MutationPlan, WindowMutation, WindowSnapshot};
pub use rules::{RuleAction, RuleEngine, RuleMatcher, WindowRule};
pub use snapping::{compute_snap_rect, SnapZone};
pub use state::WindowManagerModel;
pub use tree::OrderedWindowTree;
pub use types::{MonitorId, WindowId, WindowInfo, WindowState, WorkspaceId};
pub use workspace::Workspace;
