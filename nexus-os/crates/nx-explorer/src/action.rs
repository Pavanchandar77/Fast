//! Messages in (from the UI / host) and effects out (to the host).

use nx_fs::index::SearchHit;
use nx_fs::ops::OpRequest;
use nx_fs::preview::Priority;
use nx_fs::sort::SortKey;
use nx_fs::volumes::Volume;
use nx_fs::DirEntry;
use std::path::PathBuf;

/// Identifies a pane across tabs.
pub type PaneId = u32;

/// Layout of a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ViewKind {
    /// Icon grid (Finder icons / Explorer large icons).
    Grid,
    /// Compact list.
    #[default]
    List,
    /// Miller columns (Finder column view).
    Columns,
    /// Table with name / date / size / kind columns.
    Details,
}

/// Cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorMove {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

/// An incremental change to a directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirDiff {
    Created(DirEntry),
    Removed(std::ffi::OsString),
    Updated(DirEntry),
    Renamed {
        from: std::ffi::OsString,
        to: DirEntry,
    },
}

/// User intents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Navigate(PathBuf),
    GoBack,
    GoForward,
    GoUp,
    Refresh,
    NewTab,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    SelectTab(usize),
    ToggleSplit,
    FocusOtherPane,
    SetView(ViewKind),
    /// Sort by a key; repeating flips the direction.
    SortBy(SortKey),
    ToggleHidden,
    SetFilter(String),
    MoveCursor {
        by: CursorMove,
        extend: bool,
    },
    /// Click on a visible row (with Ctrl = toggle, Shift = range).
    ClickRow {
        row: usize,
        ctrl: bool,
        shift: bool,
    },
    SelectAll,
    ClearSelection,
    ToggleSelectCursor,
    OpenSelection,
    QuickLook,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete {
        permanent: bool,
    },
    BeginRename,
    NewFolder,
    Undo,
    CopyPath,
    CopyToOtherPane,
    MoveToOtherPane,
    Properties,
    OpenPalette,
    FocusAddressBar,
    FocusSearch,
    /// Answer to a confirmation prompt.
    Confirm(bool),
    AddFavorite,
    RemoveFavorite(PathBuf),
    /// Activate the n-th sidebar item.
    SidebarActivate(usize),
    FocusSidebar,
    FocusPane,
}

/// Everything the model can receive.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A key press with a monotonic timestamp in ms (for type-to-select).
    Key {
        chord: crate::keymap::Chord,
        now_ms: u64,
    },
    /// Typed text (IME-composed), with timestamp.
    Text {
        text: String,
        now_ms: u64,
    },
    Action(Action),
    ListingBatch {
        request: u64,
        entries: Vec<DirEntry>,
    },
    ListingDone {
        request: u64,
    },
    ListingFailed {
        request: u64,
        error: String,
    },
    SearchResults {
        request: u64,
        hits: Vec<SearchHit>,
    },
    /// Something changed inside `dir` (file op finished, watcher event).
    DirChanged {
        dir: PathBuf,
    },
    /// Live folder watcher update: debounced, coalesced diffs (no full reload).
    DirDiff {
        dir: PathBuf,
        diffs: Vec<DirDiff>,
    },
    /// The pane's viewport: first visible row, row count, grid columns.
    Viewport {
        pane: PaneId,
        first_row: usize,
        rows: usize,
        grid_columns: usize,
    },
    Volumes(Vec<Volume>),
    Favorites(Vec<PathBuf>),
    Recents(Vec<PathBuf>),
}

/// Requests to the host. The model never touches the disk itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    ListDir {
        request: u64,
        pane: PaneId,
        path: PathBuf,
    },
    CancelListing {
        request: u64,
    },
    Search {
        request: u64,
        query: String,
        scope: Option<PathBuf>,
    },
    /// Open a file with its default application.
    Open(PathBuf),
    StartOp(OpRequest),
    Undo,
    RequestPreview {
        path: PathBuf,
        priority: Priority,
    },
    SetVisiblePreviews(Vec<PathBuf>),
    /// Put paths on the system clipboard (`cut` = move on paste).
    SetClipboard {
        paths: Vec<PathBuf>,
        cut: bool,
    },
    /// Put text on the system clipboard.
    CopyText(String),
    RecordOpen(PathBuf),
    ShowProperties(Vec<PathBuf>),
    SaveFavorites(Vec<PathBuf>),
    /// Last tab closed.
    CloseWindow,
}
