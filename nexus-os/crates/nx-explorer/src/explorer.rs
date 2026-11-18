//! The Explorer model: `update(Msg) -> Vec<Effect>`.
//!
//! Pure and deterministic: no I/O, no clocks, no threads. The host feeds
//! messages (keys, listing batches, search hits) and performs the effects.

use crate::action::{Action, CursorMove, Effect, Msg, PaneId, ViewKind};
use crate::input::TextInput;
use crate::keymap::{Chord, Key, Keymap};
use crate::palette::Palette;
use crate::pane::{Location, Pane};
use crate::sidebar::Sidebar;
use crate::tab::Tab;
use nx_fs::ops::{ConflictPolicy, OpKind, OpRequest};
use nx_fs::path::{split_name, unique_name, validate_file_name};
use nx_fs::preview::Priority;
use nx_fs::{DirEntry, EntryKind};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where keyboard input goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Focus {
    Pane,
    Sidebar,
    Palette,
    AddressBar,
    Search,
    Rename,
    NewFolder,
}

/// A question the user must answer (Enter/Y = yes, Esc/N = no).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub message: String,
    pub on_yes: OpRequest,
}

/// Internal clipboard mirror (the host also gets [`Effect::SetClipboard`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

/// Max tabs remembered for "reopen closed tab".
pub const MAX_CLOSED_TABS: usize = 20;

/// The whole Explorer window.
#[derive(Debug, Clone)]
pub struct Explorer {
    tabs: Vec<Tab>,
    active_tab: usize,
    closed: Vec<Location>,
    pub sidebar: Sidebar,
    pub palette: Option<Palette>,
    pub keymap: Keymap,
    pub focus: Focus,
    /// Address bar / search / rename / new-folder editor.
    pub input: TextInput,
    pub prompt: Option<Prompt>,
    pub clipboard: Option<Clipboard>,
    /// Quick Look is open (follows the cursor).
    pub quick_look: bool,
    /// One-line status / error message.
    pub status: Option<String>,
    next_request: u64,
    next_pane: PaneId,
}

impl Explorer {
    /// New window at `home`. Returns the initial effects (first listing).
    pub fn new(home: PathBuf) -> (Self, Vec<Effect>) {
        let mut ex = Explorer {
            tabs: Vec::new(),
            active_tab: 0,
            closed: Vec::new(),
            sidebar: Sidebar::default(),
            palette: None,
            keymap: Keymap::default(),
            focus: Focus::Pane,
            input: TextInput::default(),
            prompt: None,
            clipboard: None,
            quick_look: false,
            status: None,
            next_request: 1,
            next_pane: 1,
        };
        let mut fx = Vec::new();
        ex.open_tab(Location::Dir(home), &mut fx);
        (ex, fx)
    }

    // ------------------------------------------------------------ queries

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    pub fn pane(&self) -> Option<&Pane> {
        self.tab().and_then(Tab::pane)
    }

    fn pane_mut(&mut self) -> Option<&mut Pane> {
        self.tabs.get_mut(self.active_tab).and_then(Tab::pane_mut)
    }

    fn pane_by_id_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.tabs
            .iter_mut()
            .flat_map(|t| t.panes.iter_mut())
            .find(|p| p.id == id)
    }

    fn pane_by_request_mut(&mut self, request: u64) -> Option<&mut Pane> {
        self.tabs
            .iter_mut()
            .flat_map(|t| t.panes.iter_mut())
            .find(|p| p.request() == Some(request))
    }

    fn current_dir(&self) -> Option<PathBuf> {
        self.pane()
            .and_then(|p| p.location.dir().map(Path::to_path_buf))
    }

    fn other_dir(&self) -> Option<PathBuf> {
        self.tab()
            .and_then(Tab::other)
            .and_then(|p| p.location.dir().map(Path::to_path_buf))
    }

    /// Every pane's invariants (used by tests).
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.tabs.is_empty() {
            return Err("no tabs".into());
        }
        if self.active_tab >= self.tabs.len() {
            return Err("active tab out of range".into());
        }
        for t in &self.tabs {
            if t.panes.is_empty() || t.panes.len() > 2 || t.active >= t.panes.len() {
                return Err("bad pane layout".into());
            }
            for p in &t.panes {
                if p.cursor().is_some_and(|c| c >= p.len()) {
                    return Err("cursor out of range".into());
                }
            }
        }
        if self.focus == Focus::Palette && self.palette.is_none() {
            return Err("palette focus without palette".into());
        }
        Ok(())
    }

    // ------------------------------------------------------------- update

    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        let mut fx = Vec::new();
        match msg {
            Msg::Key { chord, now_ms } => self.on_key(chord, now_ms, &mut fx),
            Msg::Text { text, now_ms } => self.on_text(&text, now_ms, &mut fx),
            Msg::Action(a) => self.apply(a, &mut fx),
            Msg::ListingBatch { request, entries } => {
                if let Some(p) = self.pane_by_request_mut(request) {
                    p.add_batch(entries);
                }
            }
            Msg::ListingDone { request } => {
                if let Some(p) = self.pane_by_request_mut(request) {
                    p.finish_load();
                    fx.push(Effect::SetVisiblePreviews(p.visible_paths()));
                }
            }
            Msg::ListingFailed { request, error } => {
                if let Some(p) = self.pane_by_request_mut(request) {
                    p.fail_load(error.clone());
                    self.status = Some(error);
                }
            }
            Msg::SearchResults { request, hits } => {
                if let Some(p) = self.pane_by_request_mut(request) {
                    let rows = hits
                        .into_iter()
                        .map(|h| {
                            let path = PathBuf::from(&h.path);
                            let name: OsString = path
                                .file_name()
                                .map(|n| n.to_os_string())
                                .unwrap_or_else(|| path.clone().into_os_string());
                            let kind = if h.is_dir {
                                EntryKind::Dir
                            } else {
                                EntryKind::File
                            };
                            (DirEntry::new(name, kind, 0), path)
                        })
                        .collect();
                    p.set_search_results(rows);
                    fx.push(Effect::SetVisiblePreviews(p.visible_paths()));
                }
            }
            Msg::DirChanged { dir } => {
                let ids: Vec<PaneId> = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.panes.iter())
                    .filter(|p| p.location == Location::Dir(dir.clone()))
                    .map(|p| p.id)
                    .collect();
                for id in ids {
                    self.reload(id, &mut fx);
                }
            }
            Msg::Viewport {
                pane,
                first_row,
                rows,
                grid_columns,
            } => {
                if let Some(p) = self.pane_by_id_mut(pane) {
                    p.first_row = first_row;
                    p.rows = rows.max(1);
                    p.grid_columns = grid_columns.max(1);
                    fx.push(Effect::SetVisiblePreviews(p.visible_paths()));
                }
            }
            Msg::Volumes(v) => self.sidebar.set_volumes(v),
            Msg::Favorites(f) => self.sidebar.set_favorites(f),
            Msg::Recents(r) => self.sidebar.set_recents(r),
        }
        fx
    }

    // --------------------------------------------------------------- keys

    fn on_key(&mut self, chord: Chord, now_ms: u64, fx: &mut Vec<Effect>) {
        if self.prompt.is_some() {
            match chord.key {
                Key::Enter | Key::Char('Y') => self.apply(Action::Confirm(true), fx),
                Key::Escape | Key::Char('N') => self.apply(Action::Confirm(false), fx),
                _ => {}
            }
            return;
        }
        match self.focus {
            Focus::Palette
            | Focus::AddressBar
            | Focus::Search
            | Focus::Rename
            | Focus::NewFolder => {
                self.edit_key(chord, fx);
            }
            Focus::Sidebar => match chord.key {
                Key::Up => self.sidebar.move_cursor(-1),
                Key::Down => self.sidebar.move_cursor(1),
                Key::Enter => self.apply(Action::SidebarActivate(self.sidebar.cursor), fx),
                Key::Escape | Key::Tab | Key::Right => self.focus = Focus::Pane,
                _ => {
                    if let Some(a) = self.keymap.lookup(&chord).cloned() {
                        self.apply(a, fx);
                    }
                }
            },
            Focus::Pane => {
                if chord.key == Key::Escape && chord.mods == crate::keymap::Mods::NONE {
                    self.escape_pane();
                    return;
                }
                if let Some(a) = self.keymap.lookup(&chord).cloned() {
                    self.apply(a, fx);
                } else if let (Key::Char(c), false, false) =
                    (chord.key, chord.mods.ctrl, chord.mods.alt)
                {
                    // Unbound printable key: type-to-select.
                    let s = c.to_lowercase().to_string();
                    self.type_select(&s, now_ms, fx);
                }
            }
        }
    }

    fn escape_pane(&mut self) {
        if self.quick_look {
            self.quick_look = false;
            return;
        }
        if let Some(p) = self.pane_mut() {
            if !p.filter.is_empty() {
                p.set_filter(String::new());
            } else {
                p.clear_selection();
            }
        }
    }

    fn type_select(&mut self, text: &str, now_ms: u64, fx: &mut Vec<Effect>) {
        let moved = self.pane_mut().is_some_and(|p| p.type_select(text, now_ms));
        if moved {
            self.after_cursor_move(fx);
        }
    }

    fn on_text(&mut self, text: &str, now_ms: u64, fx: &mut Vec<Effect>) {
        if self.prompt.is_some() {
            return;
        }
        match self.focus {
            Focus::Pane => self.type_select(text, now_ms, fx),
            Focus::Sidebar => {}
            _ => {
                self.input.insert(text);
                self.after_edit();
            }
        }
    }

    fn after_edit(&mut self) {
        if self.focus == Focus::Palette {
            if let Some(p) = self.palette.as_mut() {
                p.input = self.input.clone();
                p.refilter();
            }
        }
    }

    fn edit_key(&mut self, chord: Chord, fx: &mut Vec<Effect>) {
        match chord.key {
            Key::Escape => self.cancel_edit(),
            Key::Enter => self.commit_edit(fx),
            Key::Backspace => {
                self.input.backspace();
                self.after_edit();
            }
            Key::Delete => {
                self.input.delete();
                self.after_edit();
            }
            Key::Left => self.input.left(),
            Key::Right => self.input.right(),
            Key::Home => self.input.home(),
            Key::End => self.input.end(),
            Key::Char('A') if chord.mods.ctrl => self.input.select_all(),
            Key::Up | Key::Down if self.focus == Focus::Palette => {
                if let Some(p) = self.palette.as_mut() {
                    p.move_cursor(if chord.key == Key::Up { -1 } else { 1 });
                }
            }
            _ => {}
        }
    }

    fn cancel_edit(&mut self) {
        self.palette = None;
        self.input.clear();
        self.focus = Focus::Pane;
    }

    fn commit_edit(&mut self, fx: &mut Vec<Effect>) {
        let text = self.input.text().trim().to_string();
        let focus = self.focus;
        match focus {
            Focus::Palette => {
                let action = self
                    .palette
                    .as_ref()
                    .and_then(|p| p.selected())
                    .map(|i| i.action.clone());
                self.cancel_edit();
                if let Some(a) = action {
                    self.apply(a, fx);
                }
            }
            Focus::AddressBar => {
                self.cancel_edit();
                let cleaned = text.trim_matches('"');
                if !cleaned.is_empty() {
                    self.navigate(Location::Dir(PathBuf::from(cleaned)), fx);
                }
            }
            Focus::Search => {
                self.cancel_edit();
                if text.is_empty() {
                    return;
                }
                let scope = self.current_dir();
                self.navigate(Location::Search { query: text, scope }, fx);
            }
            Focus::Rename | Focus::NewFolder => {
                if let Err(why) = validate_file_name(&text) {
                    // Keep editing so the user can fix it.
                    self.status = Some(format!("Invalid name: {why}"));
                    return;
                }
                let Some(dir) = self.current_dir() else {
                    self.cancel_edit();
                    return;
                };
                let req = if focus == Focus::Rename {
                    let Some(src) = self
                        .pane()
                        .and_then(|p| p.cursor().and_then(|c| p.path_of_row(c)))
                    else {
                        self.cancel_edit();
                        return;
                    };
                    if src.file_name().is_some_and(|n| n.to_string_lossy() == text) {
                        self.cancel_edit();
                        return;
                    }
                    OpRequest {
                        kind: OpKind::Rename {
                            new_name: text.clone(),
                        },
                        sources: vec![src],
                        conflict: ConflictPolicy::Ask,
                    }
                } else {
                    OpRequest {
                        kind: OpKind::NewFolder {
                            parent: dir,
                            name: text.clone(),
                        },
                        sources: Vec::new(),
                        conflict: ConflictPolicy::KeepBoth,
                    }
                };
                if let Some(p) = self.pane_mut() {
                    p.select_when_listed(OsString::from(&text));
                }
                self.cancel_edit();
                self.status = None;
                fx.push(Effect::StartOp(req));
            }
            Focus::Pane | Focus::Sidebar => {}
        }
    }

    // ------------------------------------------------------------ actions

    /// Apply one action (from keymap, palette, menu or toolbar).
    pub fn apply(&mut self, action: Action, fx: &mut Vec<Effect>) {
        match action {
            Action::Navigate(p) => self.navigate(Location::Dir(p), fx),
            Action::GoBack | Action::GoForward => self.go_history(action == Action::GoBack, fx),
            Action::GoUp => self.go_up(fx),
            Action::Refresh => {
                if let Some(id) = self.pane().map(|p| p.id) {
                    self.reload(id, fx);
                }
            }
            Action::NewTab => {
                let loc = self
                    .pane()
                    .map(|p| p.location.clone())
                    .unwrap_or(Location::Dir(PathBuf::from("/")));
                self.open_tab(loc, fx);
            }
            Action::CloseTab => self.close_tab(fx),
            Action::ReopenClosedTab => {
                if let Some(loc) = self.closed.pop() {
                    self.open_tab(loc, fx);
                }
            }
            Action::NextTab => self.active_tab = (self.active_tab + 1) % self.tabs.len().max(1),
            Action::PrevTab => {
                let n = self.tabs.len().max(1);
                self.active_tab = (self.active_tab + n - 1) % n;
            }
            Action::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active_tab = i;
                }
            }
            Action::ToggleSplit => self.toggle_split(fx),
            Action::FocusOtherPane => {
                if let Some(t) = self.tabs.get_mut(self.active_tab) {
                    if t.is_split() {
                        t.active = 1 - t.active.min(1);
                    }
                }
                self.focus = Focus::Pane;
            }
            Action::SetView(v) => {
                if let Some(p) = self.pane_mut() {
                    p.view = v;
                }
            }
            Action::SortBy(k) => {
                if let Some(p) = self.pane_mut() {
                    p.set_sort(k);
                }
            }
            Action::ToggleHidden => {
                if let Some(p) = self.pane_mut() {
                    p.toggle_hidden();
                }
            }
            Action::SetFilter(f) => {
                if let Some(p) = self.pane_mut() {
                    p.set_filter(f);
                }
            }
            Action::MoveCursor { by, extend } => self.move_cursor(by, extend, fx),
            Action::ClickRow { row, ctrl, shift } => {
                if let Some(p) = self.pane_mut() {
                    p.click(row, ctrl, shift);
                }
                self.focus = Focus::Pane;
                self.after_cursor_move(fx);
            }
            Action::SelectAll => {
                if let Some(p) = self.pane_mut() {
                    p.select_all();
                }
            }
            Action::ClearSelection => {
                if let Some(p) = self.pane_mut() {
                    p.clear_selection();
                }
            }
            Action::ToggleSelectCursor => {
                if let Some(p) = self.pane_mut() {
                    p.toggle_cursor();
                }
            }
            Action::OpenSelection => self.open_selection(fx),
            Action::QuickLook => {
                self.quick_look = !self.quick_look;
                if self.quick_look {
                    self.request_focused_preview(fx);
                }
            }
            Action::Copy | Action::Cut => {
                let paths = self.selection();
                if !paths.is_empty() {
                    let cut = action == Action::Cut;
                    self.clipboard = Some(Clipboard {
                        paths: paths.clone(),
                        cut,
                    });
                    fx.push(Effect::SetClipboard { paths, cut });
                }
            }
            Action::Paste => self.paste(fx),
            Action::Duplicate => {
                let paths = self.selection();
                if let (false, Some(dir)) = (paths.is_empty(), self.current_dir()) {
                    fx.push(Effect::StartOp(
                        OpRequest::copy(paths, dir).with_conflict(ConflictPolicy::KeepBoth),
                    ));
                }
            }
            Action::Delete { permanent } => self.delete(permanent, fx),
            Action::BeginRename => self.begin_rename(),
            Action::NewFolder => self.begin_new_folder(),
            Action::Undo => fx.push(Effect::Undo),
            Action::CopyPath => {
                let paths = self.selection();
                if !paths.is_empty() {
                    let text = paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\n");
                    fx.push(Effect::CopyText(text));
                }
            }
            Action::CopyToOtherPane | Action::MoveToOtherPane => {
                let paths = self.selection();
                match self.other_dir() {
                    Some(dest) if !paths.is_empty() => {
                        let req = if action == Action::CopyToOtherPane {
                            OpRequest::copy(paths, dest)
                        } else {
                            OpRequest::move_to(paths, dest)
                        };
                        fx.push(Effect::StartOp(req));
                    }
                    None => self.status = Some("Open a split view (F6) first".into()),
                    _ => {}
                }
            }
            Action::Properties => {
                let paths = self.selection();
                let paths = if paths.is_empty() {
                    self.current_dir().into_iter().collect()
                } else {
                    paths
                };
                fx.push(Effect::ShowProperties(paths));
            }
            Action::OpenPalette => {
                let mut places: Vec<PathBuf> = self.sidebar.favorites().to_vec();
                places.extend(
                    self.sidebar
                        .recents()
                        .iter()
                        .filter(|r| !places.contains(r))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                self.palette = Some(Palette::open(&self.keymap, &places));
                self.input.clear();
                self.focus = Focus::Palette;
            }
            Action::FocusAddressBar => {
                let text = self
                    .current_dir()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.input = TextInput::new(&text);
                self.input.select_all();
                self.palette = None;
                self.focus = Focus::AddressBar;
            }
            Action::FocusSearch => {
                let q = match self.pane().map(|p| &p.location) {
                    Some(Location::Search { query, .. }) => query.clone(),
                    _ => String::new(),
                };
                self.input = TextInput::new(&q);
                self.input.select_all();
                self.palette = None;
                self.focus = Focus::Search;
            }
            Action::Confirm(yes) => {
                if let Some(prompt) = self.prompt.take() {
                    if yes {
                        fx.push(Effect::StartOp(prompt.on_yes));
                    }
                }
            }
            Action::AddFavorite => {
                let target = self
                    .pane()
                    .and_then(|p| {
                        p.cursor().filter(|_| p.selected_count() > 0).and_then(|c| {
                            p.row(c)
                                .filter(|e| e.kind.is_dir())
                                .and_then(|_| p.path_of_row(c))
                        })
                    })
                    .or_else(|| self.current_dir());
                if let Some(t) = target {
                    if self.sidebar.add_favorite(t) {
                        fx.push(Effect::SaveFavorites(self.sidebar.favorites().to_vec()));
                    }
                }
            }
            Action::RemoveFavorite(p) => {
                if self.sidebar.remove_favorite(&p) {
                    fx.push(Effect::SaveFavorites(self.sidebar.favorites().to_vec()));
                }
            }
            Action::SidebarActivate(n) => {
                if let Some(p) = self.sidebar.path_at(n) {
                    self.sidebar.cursor = n;
                    self.focus = Focus::Pane;
                    self.navigate(Location::Dir(p), fx);
                }
            }
            Action::FocusSidebar => self.focus = Focus::Sidebar,
            Action::FocusPane => self.focus = Focus::Pane,
        }
    }

    fn selection(&self) -> Vec<PathBuf> {
        self.pane().map(Pane::selection_paths).unwrap_or_default()
    }

    fn move_cursor(&mut self, by: CursorMove, extend: bool, fx: &mut Vec<Effect>) {
        let view = self.pane().map(|p| p.view);
        if view == Some(ViewKind::Columns) && !extend {
            // Miller columns: Left = parent, Right = into the folder.
            match by {
                CursorMove::Left => return self.go_up(fx),
                CursorMove::Right => {
                    let dir = self.pane().and_then(|p| {
                        p.cursor_entry()
                            .filter(|e| e.kind.is_dir())
                            .and_then(|_| p.cursor().and_then(|c| p.path_of_row(c)))
                    });
                    if let Some(d) = dir {
                        self.navigate(Location::Dir(d), fx);
                    }
                    return;
                }
                _ => {}
            }
        }
        if let Some(p) = self.pane_mut() {
            p.move_cursor(by, extend);
        }
        self.after_cursor_move(fx);
    }

    fn after_cursor_move(&mut self, fx: &mut Vec<Effect>) {
        if self.quick_look {
            self.request_focused_preview(fx);
        }
    }

    fn request_focused_preview(&self, fx: &mut Vec<Effect>) {
        if let Some(path) = self
            .pane()
            .and_then(|p| p.cursor().and_then(|c| p.path_of_row(c)))
        {
            fx.push(Effect::RequestPreview {
                path,
                priority: Priority::Focused,
            });
        }
    }

    fn open_selection(&mut self, fx: &mut Vec<Effect>) {
        let Some(p) = self.pane() else { return };
        let rows: Vec<(PathBuf, bool)> = {
            let paths = p.selection_paths();
            paths
                .into_iter()
                .map(|path| {
                    let is_dir = p
                        .rows_iter()
                        .zip(0..)
                        .find(|(_, r)| p.path_of_row(*r).as_deref() == Some(path.as_path()))
                        .is_some_and(|(e, _)| e.kind.is_dir());
                    (path, is_dir)
                })
                .collect()
        };
        let dirs: Vec<PathBuf> = rows.iter().filter(|r| r.1).map(|r| r.0.clone()).collect();
        for (path, is_dir) in &rows {
            if !is_dir {
                fx.push(Effect::Open(path.clone()));
                fx.push(Effect::RecordOpen(path.clone()));
            }
        }
        match dirs.as_slice() {
            [] => {}
            [one] if rows.len() == 1 => self.navigate(Location::Dir(one.clone()), fx),
            many => {
                // Several folders: one tab each (Explorer would open windows).
                for d in many.iter().cloned() {
                    self.open_tab(Location::Dir(d), fx);
                }
            }
        }
    }

    fn paste(&mut self, fx: &mut Vec<Effect>) {
        let (Some(clip), Some(dir)) = (self.clipboard.clone(), self.current_dir()) else {
            return;
        };
        if clip.paths.is_empty() {
            return;
        }
        if clip.cut {
            if clip.paths.iter().all(|p| p.parent() == Some(dir.as_path())) {
                self.status = Some("Already in this folder".into());
                return;
            }
            self.clipboard = None;
            fx.push(Effect::StartOp(OpRequest::move_to(clip.paths, dir)));
        } else {
            fx.push(Effect::StartOp(OpRequest::copy(clip.paths, dir)));
        }
    }

    fn delete(&mut self, permanent: bool, fx: &mut Vec<Effect>) {
        let paths = self.selection();
        if paths.is_empty() {
            return;
        }
        let req = OpRequest::delete(paths.clone(), !permanent);
        if permanent {
            let what = match paths.as_slice() {
                [one] => format!(
                    "\"{}\"",
                    one.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                many => format!("{} items", many.len()),
            };
            self.prompt = Some(Prompt {
                message: format!("Permanently delete {what}? This can't be undone."),
                on_yes: req,
            });
        } else {
            fx.push(Effect::StartOp(req));
        }
    }

    fn begin_rename(&mut self) {
        let Some(name) = self
            .pane()
            .and_then(|p| p.cursor_entry())
            .map(|e| e.display_name.clone())
        else {
            return;
        };
        if self
            .pane()
            .is_some_and(|p| matches!(p.location, Location::Search { .. }))
        {
            self.status = Some("Rename from the containing folder".into());
            return;
        }
        // Select the stem only, like Explorer and Finder.
        let stem = split_name(&name).0.chars().count();
        self.input = TextInput::with_selection(&name, 0, stem);
        self.palette = None;
        self.focus = Focus::Rename;
    }

    fn begin_new_folder(&mut self) {
        let Some(p) = self.pane() else { return };
        if p.location.dir().is_none() || matches!(p.location, Location::Search { .. }) {
            return;
        }
        let name = unique_name("New folder", |n| {
            p.rows_iter()
                .any(|e| e.display_name.eq_ignore_ascii_case(n))
        });
        let len = name.chars().count();
        self.input = TextInput::with_selection(&name, 0, len);
        self.palette = None;
        self.focus = Focus::NewFolder;
    }

    // --------------------------------------------------------- navigation

    fn alloc_request(&mut self) -> u64 {
        let r = self.next_request;
        self.next_request += 1;
        r
    }

    /// (Re)load a pane's current location.
    fn reload(&mut self, id: PaneId, fx: &mut Vec<Effect>) {
        let request = self.alloc_request();
        let Some(p) = self.pane_by_id_mut(id) else {
            return;
        };
        if let Some(old) = p.request() {
            fx.push(Effect::CancelListing { request: old });
        }
        p.begin_load(request);
        match p.location.clone() {
            Location::Dir(path) => fx.push(Effect::ListDir {
                request,
                pane: id,
                path,
            }),
            Location::Search { query, scope } => fx.push(Effect::Search {
                request,
                query,
                scope,
            }),
        }
    }

    fn set_location(
        &mut self,
        loc: Location,
        push_history: bool,
        select: Option<OsString>,
        fx: &mut Vec<Effect>,
    ) {
        let Some(p) = self.pane_mut() else { return };
        if push_history {
            let cur = p.location.clone();
            p.history.push(cur);
        }
        p.location = loc.clone();
        p.filter.clear();
        p.clear_selection();
        // Fresh folder: start at the top unless we know what to highlight.
        p.reset_position();
        if let Some(n) = select {
            p.select_when_listed(n);
        }
        let id = p.id;
        if let Location::Dir(d) = &loc {
            self.sidebar.visit(d);
        }
        self.quick_look = false;
        self.reload(id, fx);
    }

    fn navigate(&mut self, loc: Location, fx: &mut Vec<Effect>) {
        if self.pane().map(|p| &p.location) == Some(&loc) {
            if let Some(id) = self.pane().map(|p| p.id) {
                self.reload(id, fx);
            }
            return;
        }
        self.set_location(loc, true, None, fx);
    }

    fn go_up(&mut self, fx: &mut Vec<Effect>) {
        let Some(dir) = self.current_dir() else {
            return;
        };
        let Some(parent) = dir.parent().map(Path::to_path_buf) else {
            return;
        };
        if matches!(
            self.pane().map(|p| &p.location),
            Some(Location::Search { .. })
        ) {
            // Leaving a search goes back to its scope.
            return self.set_location(Location::Dir(dir), true, None, fx);
        }
        let child = dir.file_name().map(|n| n.to_os_string());
        self.set_location(Location::Dir(parent), true, child, fx);
    }

    fn go_history(&mut self, back: bool, fx: &mut Vec<Effect>) {
        let Some(p) = self.pane_mut() else { return };
        let cur = p.location.clone();
        let dest = if back {
            p.history.back(cur.clone())
        } else {
            p.history.forward(cur.clone())
        };
        let Some(dest) = dest else { return };
        // Coming back out of a subfolder: highlight the folder we left.
        let select = match (&dest, &cur) {
            (Location::Dir(d), Location::Dir(c)) => c
                .strip_prefix(d)
                .ok()
                .and_then(|rest| rest.components().next())
                .map(|c| c.as_os_str().to_os_string()),
            _ => None,
        };
        self.set_location(dest, false, select, fx);
    }

    fn open_tab(&mut self, loc: Location, fx: &mut Vec<Effect>) {
        let id = self.next_pane;
        self.next_pane += 1;
        let insert_at = if self.tabs.is_empty() {
            0
        } else {
            self.active_tab + 1
        };
        self.tabs.insert(insert_at, Tab::new(Pane::new(id, loc)));
        self.active_tab = insert_at;
        self.focus = Focus::Pane;
        self.reload(id, fx);
    }

    fn close_tab(&mut self, fx: &mut Vec<Effect>) {
        if self.tabs.len() <= 1 {
            fx.push(Effect::CloseWindow);
            return;
        }
        let tab = self.tabs.remove(self.active_tab);
        for p in &tab.panes {
            if let Some(r) = p.request() {
                fx.push(Effect::CancelListing { request: r });
            }
        }
        if let Some(p) = tab.pane() {
            self.closed.push(p.location.clone());
            if self.closed.len() > MAX_CLOSED_TABS {
                self.closed.remove(0);
            }
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
    }

    fn toggle_split(&mut self, fx: &mut Vec<Effect>) {
        let id = self.next_pane;
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if tab.is_split() {
            let keep = tab.active.min(1);
            let gone = tab.panes.remove(1 - keep);
            tab.active = 0;
            if let Some(r) = gone.request() {
                fx.push(Effect::CancelListing { request: r });
            }
            return;
        }
        let Some(cur) = tab.pane() else { return };
        let mut pane = Pane::new(id, cur.location.clone());
        pane.view = cur.view;
        pane.sort = cur.sort;
        pane.show_hidden = cur.show_hidden;
        tab.panes.push(pane);
        tab.active = 1;
        self.next_pane += 1;
        self.focus = Focus::Pane;
        self.reload(id, fx);
    }
}
