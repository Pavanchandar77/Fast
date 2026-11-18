#![deny(clippy::unwrap_used, clippy::expect_used)]

//! # nx-power: Nexus-OS Power User Features
//!
//! Pure, robust state stores and reversible mutation planners for core power-user features:
//! - **Clipboard History:** Ring buffer with pin protection, search, and privacy exclusion rules.
//! - **Always-On-Top:** Reversible mutation plan for window topmost pin (dry-run, apply, revert).
//! - **Window Snapping Layouts:** Named zone sets, snap groups, and bounds restoration.
//! - **Quick Look Session:** File preview state machine across text, code, image metadata, and folders.
//! - **Color Picker:** Color conversions across Hex, RGB, HSL, HSV, CMYK, CSS, and history palettes.
//! - **Batch Rename Tools:** Conflict-free renaming pipelines (regex, numbering, casing, extensions) with undo.
//! - **Keyboard Remapping:** Config-driven remap table protecting stock Windows system shortcuts.
//! - **Per-App Volume:** Session manager with reversible volume/mute mutation plans.
//! - **Quick Notes Store:** Markdown/text notes with full-text search, pinning, tags, and reversible undo.
//! - **Tabs for Any Window:** Grouping arbitrary windows into tab strips with sync bounds.
//! - **Alt-Tab Model:** MRU z-order tracking with thumbnail previews.

pub mod error;
pub mod types;

pub mod alt_tab;
pub mod always_on_top;
pub mod clipboard;
pub mod color;
pub mod notes;
pub mod quick_look;
pub mod remap;
pub mod rename;
pub mod snapping;
pub mod tabs;
pub mod volume;
pub mod windows_backend;

pub use alt_tab::{AltTabState, AltTabSwitcher, AltTabWindow, PlaceholderThumbnail};
pub use always_on_top::{
    AlwaysOnTopAction, AlwaysOnTopDryRun, AlwaysOnTopPlan, AlwaysOnTopRevertPlan, AlwaysOnTopStore,
};
pub use clipboard::{
    ClipboardEntry, ClipboardFilter, ClipboardKind, ClipboardKindFilter, ClipboardStore,
    ExclusionRule, NewClipboardItem,
};
pub use color::{Color, ColorHistoryStore};
pub use error::{PowerError, PowerResult};
pub use notes::{Note, NoteColor, NoteId, NoteRevertPlan, NotesStore};
pub use quick_look::{QuickLookContent, QuickLookSession, QuickLookStatus};
pub use remap::{KeyChord, KeyCode, KeyModifiers, RemapRule, RemapTable, RemapTarget};
pub use rename::{
    CaseType, RenameAction, RenameConflict, RenameDryRunReport, RenamePlan, RenamePlanner,
    RenameRule, RenameUndoPlan,
};
pub use snapping::{
    SnapDryRun, SnapGeometry, SnapGroup, SnapPlan, SnapStore, SnapZone, UnsnapPlan, ZoneSet,
};
pub use tabs::{
    TabGroup, TabGroupId, TabGroupManager, TabSwitchDryRun, TabSwitchPlan, TabSwitchRevertPlan,
    WindowTab,
};
pub use types::{PlanStatus, Rect, RectF, WindowId};
pub use volume::{
    AudioSession, AudioSessionId, MuteDryRun, RevertMutePlan, RevertVolumePlan, SetMutePlan,
    SetVolumePlan, VolumeDryRun, VolumeStore,
};
