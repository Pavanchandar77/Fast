//! One file pane: location, listing, sort/filter, selection, cursor,
//! type-to-select and viewport.

use crate::action::{CursorMove, PaneId, ViewKind};
use crate::history::History;
use nx_fs::index::fold;
use nx_fs::sort::{compare_entries, sort_indices, SortKey, SortSpec};
use nx_fs::DirEntry;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where a pane is pointed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    Dir(PathBuf),
    Search {
        query: String,
        scope: Option<PathBuf>,
    },
}

impl Location {
    pub fn dir(&self) -> Option<&Path> {
        match self {
            Location::Dir(p) => Some(p),
            Location::Search { scope, .. } => scope.as_deref(),
        }
    }

    /// Tab title.
    pub fn title(&self) -> String {
        match self {
            Location::Dir(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            Location::Search { query, .. } => format!("Search: {query}"),
        }
    }
}

/// Loading state of the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Idle,
    Loading { request: u64 },
    Failed { error: String },
}

/// Type-to-select buffer (Explorer/Finder: typing jumps to a name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeSelect {
    buffer: String,
    last_ms: u64,
}

/// Reset type-to-select after this much idle time.
pub const TYPE_SELECT_TIMEOUT_MS: u64 = 1000;

/// A file pane.
#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub location: Location,
    pub history: History<Location>,
    pub view: ViewKind,
    pub sort: SortSpec,
    pub show_hidden: bool,
    pub filter: String,
    pub load: LoadState,
    entries: Vec<DirEntry>,
    /// Full paths of entries for search results (parallel to `entries`).
    entry_paths: Option<Vec<PathBuf>>,
    /// Indices into `entries`, filtered + sorted.
    visible: Vec<usize>,
    selected: HashSet<OsString>,
    cursor: Option<usize>,
    anchor: Option<usize>,
    /// Name to select once it shows up (after Back/Up, rename, new folder).
    pending_select: Option<OsString>,
    type_select: TypeSelect,
    pub first_row: usize,
    pub rows: usize,
    pub grid_columns: usize,
}

impl Pane {
    pub fn new(id: PaneId, location: Location) -> Self {
        Pane {
            id,
            location,
            history: History::new(100),
            view: ViewKind::default(),
            sort: SortSpec::default(),
            show_hidden: false,
            filter: String::new(),
            load: LoadState::Idle,
            entries: Vec::new(),
            entry_paths: None,
            visible: Vec::new(),
            selected: HashSet::new(),
            cursor: None,
            anchor: None,
            pending_select: None,
            type_select: TypeSelect::default(),
            first_row: 0,
            rows: 30,
            grid_columns: 6,
        }
    }

    // ------------------------------------------------------------ listing

    /// Start a fresh listing (clears entries; keeps selection names so a
    /// refresh restores them).
    pub fn begin_load(&mut self, request: u64) {
        self.load = LoadState::Loading { request };
        if self.pending_select.is_none() {
            self.pending_select = self.cursor_entry().map(|e| e.name.clone());
        }
        self.entries.clear();
        self.entry_paths = None;
        self.visible.clear();
        self.cursor = None;
        self.anchor = None;
        self.first_row = 0;
    }

    pub fn request(&self) -> Option<u64> {
        match self.load {
            LoadState::Loading { request } => Some(request),
            _ => None,
        }
    }

    /// Append a batch: the batch is sorted and merged into the visible
    /// order (O(n) per batch, so 100k entries stay smooth while loading).
    pub fn add_batch(&mut self, batch: Vec<DirEntry>) {
        let start = self.entries.len();
        self.entries.extend(batch);
        let mut fresh: Vec<usize> = (start..self.entries.len())
            .filter(|&i| self.passes(&self.entries[i]))
            .collect();
        let spec = self.sort;
        let entries = &self.entries;
        sort_indices(entries, &mut fresh, spec);
        let old = std::mem::take(&mut self.visible);
        self.visible = merge(old, fresh, |a, b| cmp_idx(entries, a, b, spec));
        self.restore_pending();
        self.clamp_cursor();
    }

    pub fn finish_load(&mut self) {
        self.load = LoadState::Idle;
        // A pending name that never appeared is dropped.
        self.restore_pending();
        self.pending_select = None;
        self.selected
            .retain(|n| self.entries.iter().any(|e| &e.name == n));
        if self.cursor.is_none() && !self.visible.is_empty() {
            self.cursor = Some(0);
        }
    }

    pub fn fail_load(&mut self, error: String) {
        self.load = LoadState::Failed { error };
        self.pending_select = None;
    }

    /// Replace contents with search hits.
    pub fn set_search_results(&mut self, hits: Vec<(DirEntry, PathBuf)>) {
        let (entries, paths): (Vec<_>, Vec<_>) = hits.into_iter().unzip();
        self.entries = entries;
        self.entry_paths = Some(paths);
        // Search results keep the ranking order: no re-sort.
        self.visible = (0..self.entries.len())
            .filter(|&i| self.passes(&self.entries[i]))
            .collect();
        self.load = LoadState::Idle;
        self.cursor = if self.visible.is_empty() {
            None
        } else {
            Some(0)
        };
        self.anchor = None;
        self.selected.clear();
    }

    fn passes(&self, e: &DirEntry) -> bool {
        if !self.show_hidden && e.is_hidden() {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        fold(&e.display_name).contains(fold(&self.filter).as_ref())
    }

    /// Re-filter and re-sort everything (sort/filter/hidden changed).
    pub fn recompute(&mut self) {
        let keep = self.cursor_entry().map(|e| e.name.clone());
        let mut v: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.passes(&self.entries[i]))
            .collect();
        if self.entry_paths.is_none() {
            let (entries, spec) = (&self.entries, self.sort);
            sort_indices(entries, &mut v, spec);
        }
        self.visible = v;
        self.cursor = keep
            .and_then(|n| self.row_of(&n))
            .or(if self.visible.is_empty() {
                None
            } else {
                Some(0)
            });
        self.anchor = None;
        let names: HashSet<&OsString> = self
            .visible
            .iter()
            .map(|&i| &self.entries[i].name)
            .collect();
        self.selected.retain(|n| names.contains(n));
    }

    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort.key == key {
            self.sort.ascending = !self.sort.ascending;
        } else {
            self.sort.key = key;
            // Dates and sizes are most useful newest/largest first.
            self.sort.ascending = matches!(key, SortKey::Name | SortKey::Kind);
        }
        self.recompute();
    }

    pub fn set_filter(&mut self, f: String) {
        self.filter = f;
        self.recompute();
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.recompute();
    }

    // ---------------------------------------------------------- accessors

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    /// Entry at a visible row.
    pub fn row(&self, row: usize) -> Option<&DirEntry> {
        self.visible.get(row).and_then(|&i| self.entries.get(i))
    }

    /// Visible rows in order.
    pub fn rows_iter(&self) -> impl Iterator<Item = &DirEntry> {
        self.visible.iter().filter_map(|&i| self.entries.get(i))
    }

    pub fn row_of(&self, name: &OsString) -> Option<usize> {
        self.visible
            .iter()
            .position(|&i| &self.entries[i].name == name)
    }

    /// Full path of a visible row.
    pub fn path_of_row(&self, row: usize) -> Option<PathBuf> {
        let &i = self.visible.get(row)?;
        if let Some(paths) = &self.entry_paths {
            return paths.get(i).cloned();
        }
        self.location.dir().map(|d| d.join(&self.entries[i].name))
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn cursor_entry(&self) -> Option<&DirEntry> {
        self.cursor.and_then(|r| self.row(r))
    }

    pub fn is_selected(&self, row: usize) -> bool {
        self.row(row)
            .is_some_and(|e| self.selected.contains(&e.name))
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Selected paths in visible order; falls back to the cursor item.
    pub fn selection_paths(&self) -> Vec<PathBuf> {
        let rows: Vec<usize> = if self.selected.is_empty() {
            self.cursor.into_iter().collect()
        } else {
            (0..self.visible.len())
                .filter(|&r| self.is_selected(r))
                .collect()
        };
        rows.into_iter()
            .filter_map(|r| self.path_of_row(r))
            .collect()
    }

    /// Total bytes of the selected files (status bar).
    pub fn selected_bytes(&self) -> u64 {
        self.rows_iter()
            .filter(|e| self.selected.contains(&e.name))
            .map(|e| e.size)
            .sum()
    }

    /// Forget cursor and pending selection (entering a different folder).
    pub fn reset_position(&mut self) {
        self.cursor = None;
        self.anchor = None;
        self.pending_select = None;
        self.selected.clear();
        self.first_row = 0;
    }

    /// Remember a name to select when it appears in the listing.
    pub fn select_when_listed(&mut self, name: OsString) {
        self.pending_select = Some(name);
    }

    fn restore_pending(&mut self) {
        let Some(name) = self.pending_select.clone() else {
            return;
        };
        if let Some(row) = self.row_of(&name) {
            self.cursor = Some(row);
            self.anchor = Some(row);
            self.pending_select = None;
            self.scroll_to_cursor();
        }
    }

    fn clamp_cursor(&mut self) {
        if let Some(c) = self.cursor {
            if c >= self.visible.len() {
                self.cursor = self.visible.len().checked_sub(1);
            }
        }
    }

    // ---------------------------------------------------------- selection

    pub fn select_all(&mut self) {
        self.selected = self.rows_iter().map(|e| e.name.clone()).collect();
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = self.cursor;
    }

    pub fn toggle_cursor(&mut self) {
        if let Some(name) = self.cursor_entry().map(|e| e.name.clone()) {
            if !self.selected.remove(&name) {
                self.selected.insert(name);
            }
            self.anchor = self.cursor;
        }
    }

    fn select_range(&mut self, a: usize, b: usize) {
        let (lo, hi) = (a.min(b), a.max(b));
        self.selected = (lo..=hi)
            .filter_map(|r| self.row(r).map(|e| e.name.clone()))
            .collect();
    }

    /// Mouse click semantics (Explorer/Finder).
    pub fn click(&mut self, row: usize, ctrl: bool, shift: bool) {
        if row >= self.visible.len() {
            self.selected.clear();
            return;
        }
        if shift {
            let anchor = self.anchor.unwrap_or(row);
            self.select_range(anchor, row);
        } else if ctrl {
            self.cursor = Some(row);
            self.toggle_cursor();
        } else {
            self.selected.clear();
            if let Some(e) = self.row(row) {
                self.selected.insert(e.name.clone());
            }
            self.anchor = Some(row);
        }
        self.cursor = Some(row);
    }

    /// Keyboard cursor movement; `extend` = Shift (range from the anchor).
    pub fn move_cursor(&mut self, by: CursorMove, extend: bool) {
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        let cols = if self.view == ViewKind::Grid {
            self.grid_columns.max(1)
        } else {
            1
        };
        let page = self.rows.max(1) * cols;
        let cur = self.cursor.unwrap_or(0);
        let next = match (by, self.cursor) {
            (_, None) => 0,
            (CursorMove::Up, _) => cur.saturating_sub(cols),
            (CursorMove::Down, _) => (cur + cols).min(n - 1),
            (CursorMove::Left, _) if self.view == ViewKind::Grid => cur.saturating_sub(1),
            (CursorMove::Right, _) if self.view == ViewKind::Grid => (cur + 1).min(n - 1),
            (CursorMove::Left | CursorMove::Right, _) => cur,
            (CursorMove::PageUp, _) => cur.saturating_sub(page),
            (CursorMove::PageDown, _) => (cur + page).min(n - 1),
            (CursorMove::Home, _) => 0,
            (CursorMove::End, _) => n - 1,
        };
        self.cursor = Some(next);
        if extend {
            let anchor = *self.anchor.get_or_insert(cur);
            self.select_range(anchor, next);
        } else {
            self.anchor = Some(next);
            self.selected.clear();
            if let Some(e) = self.row(next) {
                self.selected.insert(e.name.clone());
            }
        }
        self.scroll_to_cursor();
    }

    fn scroll_to_cursor(&mut self) {
        let Some(c) = self.cursor else { return };
        let cols = if self.view == ViewKind::Grid {
            self.grid_columns.max(1)
        } else {
            1
        };
        let row = c / cols;
        let rows = self.rows.max(1);
        if row < self.first_row {
            self.first_row = row;
        } else if row >= self.first_row + rows {
            self.first_row = row + 1 - rows;
        }
    }

    /// Paths currently on screen (for thumbnail priorities).
    pub fn visible_paths(&self) -> Vec<PathBuf> {
        let cols = if self.view == ViewKind::Grid {
            self.grid_columns.max(1)
        } else {
            1
        };
        let start = self.first_row * cols;
        let end = (start + self.rows.max(1) * cols).min(self.visible.len());
        (start..end).filter_map(|r| self.path_of_row(r)).collect()
    }

    // ------------------------------------------------------ type-to-select

    /// Feed typed characters. Returns true if the cursor moved.
    pub fn type_select(&mut self, text: &str, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.type_select.last_ms) > TYPE_SELECT_TIMEOUT_MS {
            self.type_select.buffer.clear();
        }
        self.type_select.last_ms = now_ms;
        self.type_select.buffer.push_str(&fold(text));
        let buf = self.type_select.buffer.clone();
        let n = self.visible.len();
        if n == 0 || buf.is_empty() {
            return false;
        }
        let mut chars = buf.chars();
        let first = chars.next().unwrap_or_default();
        let repeated = chars.all(|c| c == first);
        let starts = |r: usize, prefix: &str| {
            self.row(r)
                .is_some_and(|e| fold(&e.display_name).starts_with(prefix))
        };
        let cur = self.cursor.unwrap_or(0);
        let found = if repeated && buf.chars().count() > 1 {
            // "aaa": cycle through names starting with 'a'.
            let p = first.to_string();
            (1..=n).map(|k| (cur + k) % n).find(|&r| starts(r, &p))
        } else {
            // Prefer staying on the current item if it still matches.
            (0..n).map(|k| (cur + k) % n).find(|&r| starts(r, &buf))
        };
        match found {
            Some(r) => {
                self.cursor = Some(r);
                self.anchor = Some(r);
                self.selected.clear();
                if let Some(e) = self.row(r) {
                    self.selected.insert(e.name.clone());
                }
                self.scroll_to_cursor();
                true
            }
            None => false,
        }
    }
}

fn cmp_idx(entries: &[DirEntry], a: usize, b: usize, spec: SortSpec) -> Ordering {
    compare_entries(&entries[a], &entries[b], spec)
}

/// Stable merge of two sorted index lists.
fn merge(
    a: Vec<usize>,
    b: Vec<usize>,
    mut cmp: impl FnMut(usize, usize) -> Ordering,
) -> Vec<usize> {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if cmp(b[j], a[i]) == Ordering::Less {
            out.push(b[j]);
            j += 1;
        } else {
            out.push(a[i]);
            i += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_fs::EntryKind;
    use proptest::prelude::*;

    fn e(name: &str) -> DirEntry {
        let kind = if name.contains('.') {
            EntryKind::File
        } else {
            EntryKind::Dir
        };
        DirEntry::new(name.into(), kind, name.len() as u64)
    }

    fn loaded(names: &[&str]) -> Pane {
        let mut p = Pane::new(1, Location::Dir("/x".into()));
        p.begin_load(1);
        p.add_batch(names.iter().map(|n| e(n)).collect());
        p.finish_load();
        p
    }

    fn names(p: &Pane) -> Vec<String> {
        p.rows_iter().map(|e| e.display_name.clone()).collect()
    }

    #[test]
    fn batches_merge_in_sorted_order() {
        let mut p = Pane::new(1, Location::Dir("/x".into()));
        p.begin_load(9);
        p.add_batch(vec![e("b.txt"), e("file10.txt"), e("zdir")]);
        p.add_batch(vec![e("a.txt"), e("file2.txt"), e(".hidden")]);
        p.finish_load();
        assert_eq!(
            names(&p),
            vec!["zdir", "a.txt", "b.txt", "file2.txt", "file10.txt"]
        );
        p.toggle_hidden();
        assert_eq!(p.len(), 6);
        p.set_sort(SortKey::Size);
        assert!(!p.sort.ascending, "size sorts largest first");
        p.set_filter("FILE".into());
        assert_eq!(names(&p), vec!["file10.txt", "file2.txt"]);
    }

    #[test]
    fn cursor_selection_and_ranges() {
        let mut p = loaded(&["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"]);
        assert_eq!(p.cursor(), Some(0));
        p.move_cursor(CursorMove::Down, false);
        p.move_cursor(CursorMove::Down, true);
        p.move_cursor(CursorMove::Down, true);
        assert_eq!(p.selected_count(), 3);
        assert_eq!(
            p.selection_paths(),
            vec![
                PathBuf::from("/x/b.txt"),
                PathBuf::from("/x/c.txt"),
                PathBuf::from("/x/d.txt")
            ]
        );
        p.click(0, true, false);
        assert_eq!(p.selected_count(), 4, "ctrl-click adds");
        p.click(4, false, true);
        assert_eq!(p.selected_count(), 5, "shift-click ranges from anchor");
        p.move_cursor(CursorMove::End, false);
        assert_eq!(p.selected_count(), 1);
        p.select_all();
        assert_eq!(p.selected_bytes(), 25);
        p.clear_selection();
        assert_eq!(
            p.selection_paths(),
            vec![PathBuf::from("/x/e.txt")],
            "cursor is the implicit selection"
        );
    }

    #[test]
    fn grid_moves_by_columns_and_scrolls() {
        let names_owned: Vec<String> = (0..50).map(|i| format!("f{i:02}.png")).collect();
        let refs: Vec<&str> = names_owned.iter().map(String::as_str).collect();
        let mut p = loaded(&refs);
        p.view = ViewKind::Grid;
        p.grid_columns = 5;
        p.rows = 3;
        p.move_cursor(CursorMove::Down, false);
        assert_eq!(p.cursor(), Some(5));
        p.move_cursor(CursorMove::Right, false);
        assert_eq!(p.cursor(), Some(6));
        p.move_cursor(CursorMove::PageDown, false);
        assert_eq!(p.cursor(), Some(21));
        assert_eq!(p.first_row, 2);
        assert_eq!(p.visible_paths().len(), 15);
        p.move_cursor(CursorMove::End, false);
        assert_eq!(p.cursor(), Some(49));
    }

    #[test]
    fn type_to_select_with_timeout_and_cycling() {
        let mut p = loaded(&[
            "alpha.txt",
            "beta.txt",
            "bravo.txt",
            "brown.txt",
            "charlie.txt",
        ]);
        assert!(p.type_select("b", 1000));
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("beta.txt")
        );
        assert!(p.type_select("r", 1200));
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("bravo.txt")
        );
        assert!(p.type_select("o", 1300));
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("brown.txt")
        );
        // After the timeout the buffer restarts.
        assert!(p.type_select("c", 5000));
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("charlie.txt")
        );
        // Repeating a letter cycles.
        p.type_select("b", 9000);
        p.type_select("b", 9100);
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("bravo.txt")
        );
        p.type_select("b", 9200);
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("brown.txt")
        );
        assert!(!p.type_select("zzz-none", 20000));
    }

    #[test]
    fn refresh_keeps_cursor_and_selection_by_name() {
        let mut p = loaded(&["a.txt", "b.txt", "c.txt"]);
        p.click(1, false, false);
        p.begin_load(2);
        p.add_batch(vec![e("c.txt"), e("b.txt"), e("new.txt")]);
        p.finish_load();
        assert_eq!(
            p.cursor_entry().map(|e| e.display_name.as_str()),
            Some("b.txt")
        );
        assert!(p.is_selected(p.cursor().unwrap_or(99)));
    }

    proptest! {
        #[test]
        fn invariants_hold(ops in proptest::collection::vec((0u8..9, 0usize..12), 0..80), n in 0usize..12) {
            let names_owned: Vec<String> = (0..n).map(|i| format!("n{i}.txt")).collect();
            let refs: Vec<&str> = names_owned.iter().map(String::as_str).collect();
            let mut p = loaded(&refs);
            for (op, arg) in ops {
                match op {
                    0 => p.move_cursor(CursorMove::Down, arg % 2 == 0),
                    1 => p.move_cursor(CursorMove::Up, arg % 2 == 0),
                    2 => p.click(arg, arg % 3 == 0, arg % 3 == 1),
                    3 => p.set_filter(if arg % 2 == 0 { String::new() } else { format!("{arg}") }),
                    4 => p.set_sort([SortKey::Name, SortKey::Size, SortKey::Kind, SortKey::Modified][arg % 4]),
                    5 => { p.type_select(&format!("n{arg}"), arg as u64 * 700); }
                    6 => p.select_all(),
                    7 => p.toggle_cursor(),
                    _ => p.move_cursor(CursorMove::End, false),
                }
                prop_assert!(p.cursor().is_none_or(|c| c < p.len()));
                let visible: HashSet<&OsString> = p.rows_iter().map(|e| &e.name).collect();
                prop_assert!(p.selected.iter().all(|s| visible.contains(s)) || op == 3 || op == 4);
            }
        }
    }
}
