//! Unified Explorer Window View-Model: combines pure state machine, design system, and virtualized views.

use crate::context_menu::{ContextItem, ExplorerContextMenu};
use crate::drag_drop::DragDropState;
use crate::index_cache::PersistentIndexManager;
use crate::ops_progress::OpsProgressView;
use crate::preview::PreviewPane;
use crate::rubber_band::RubberBand;
use crate::sidebar::ExplorerSidebar;
use crate::split::SplitLayout;
use crate::status_bar::StatusBar;
use crate::tabs::{TabBar, TabItem};
use crate::toolbar::{ToolbarAction, UnifiedToolbar};
use crate::views::columns::ColumnsView;
use crate::views::details::DetailsView;
use crate::views::grid::GridView;
use crate::views::list::ListView;
use crate::watcher::FolderWatcher;

use nx_explorer::action::{Action, Effect, Msg, ViewKind};
use nx_explorer::pane::Location;
use nx_explorer::{Explorer, Focus};
use nx_ui::geom::{Rect, Size};
use nx_ui::input::Input;
use nx_ui::theme::{Accent, Theme, ThemeMode};

use std::path::{Path, PathBuf};

pub struct ExplorerWindow {
    pub explorer: Explorer,
    pub viewport: Size,
    pub theme: Theme,
    pub toolbar: UnifiedToolbar,
    pub sidebar: ExplorerSidebar,
    pub tabs: TabBar,
    pub split: SplitLayout,
    pub status_bar: StatusBar,
    pub preview: PreviewPane,
    pub details_view: DetailsView,
    pub list_view: ListView,
    pub grid_view: GridView,
    pub columns_view: ColumnsView,
    pub rubber_band: RubberBand,
    pub context_menu: ExplorerContextMenu,
    pub drag_drop: DragDropState,
    pub ops_progress: OpsProgressView,
    pub watcher: Option<FolderWatcher>,
    pub index_manager: Option<PersistentIndexManager>,
    last_click_time: Option<std::time::Instant>,
    last_click_row: Option<usize>,
}

impl ExplorerWindow {
    pub fn new(home: PathBuf) -> (Self, Vec<Effect>) {
        let (explorer, initial_fx) = Explorer::new(home.clone());
        let theme = Theme::light(Accent::Blue);
        let mut toolbar = UnifiedToolbar::new();
        toolbar.set_path(&home);

        let mut sidebar = ExplorerSidebar::new();
        sidebar.set_selected(Some(home.clone()));

        let watcher = Some(FolderWatcher::watch(home));

        let mut win = Self {
            explorer,
            viewport: Size::new(1280.0, 800.0),
            theme,
            toolbar,
            sidebar,
            tabs: TabBar::new(),
            split: SplitLayout::new(),
            status_bar: StatusBar::new(),
            preview: PreviewPane::new(),
            details_view: DetailsView::new(),
            list_view: ListView::new(),
            grid_view: GridView::new(),
            columns_view: ColumnsView::new(),
            rubber_band: RubberBand::new(),
            context_menu: ExplorerContextMenu::new(),
            drag_drop: DragDropState::new(),
            ops_progress: OpsProgressView::new(),
            watcher,
            index_manager: None,
            last_click_time: None,
            last_click_row: None,
        };

        win.sync_tabs();
        win.sync_status();

        (win, initial_fx)
    }

    pub fn set_index_manager(&mut self, manager: PersistentIndexManager) {
        self.index_manager = Some(manager);
    }

    pub fn toggle_theme(&mut self) {
        let new_mode = if self.theme.mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.theme = if new_mode.is_dark() {
            Theme::dark(self.theme.accent)
        } else {
            Theme::light(self.theme.accent)
        };
    }

    pub fn animating(&self) -> bool {
        self.toolbar.animating()
            || self.sidebar.animating()
            || self.tabs.animating()
            || self.preview.animating()
            || self.details_view.animating()
            || self.list_view.animating()
            || self.grid_view.animating()
            || self.columns_view.animating()
            || self.context_menu.animating()
            || self.ops_progress.animating()
    }

    pub fn sync_tabs(&mut self) {
        let tabs_meta: Vec<TabItem> = self
            .explorer
            .tabs()
            .iter()
            .map(|t| TabItem {
                title: t.title(),
                is_search: t
                    .pane()
                    .is_some_and(|p| matches!(p.location, Location::Search { .. })),
            })
            .collect();
        self.tabs
            .set_active(self.explorer.active_tab(), tabs_meta.len());
    }

    pub fn sync_status(&mut self) {
        if let Some(pane) = self.explorer.pane() {
            let item_count = pane.len();
            let selected_count = pane.selected_count();
            let selected_bytes = pane.selected_bytes();
            self.status_bar.update(
                item_count,
                selected_count,
                selected_bytes,
                Some("342.5 GB free".to_string()),
            );
        }
    }

    pub fn current_view(&self) -> ViewKind {
        self.explorer
            .pane()
            .map(|p| p.view)
            .unwrap_or(ViewKind::Details)
    }

    pub fn handle_msg(&mut self, msg: Msg) -> Vec<Effect> {
        let fx = self.explorer.update(msg);
        self.sync_tabs();
        self.sync_status();
        fx
    }

    pub fn handle_chord(&mut self, chord: nx_explorer::keymap::Chord, now_ms: u64) -> Vec<Effect> {
        self.handle_msg(Msg::Key { chord, now_ms })
    }

    pub fn update(&mut self, input: &Input, dt: f32) -> Vec<Effect> {
        let mut fx = Vec::new();

        // 1. Poll folder watcher for live debounced diffs
        if let Some(watcher) = &self.watcher {
            if let Some(diffs) = watcher.poll_diffs() {
                let dir = watcher.current_dir().to_path_buf();
                // Incremental diff application to explorer model (no full reload!)
                let diff_fx = self.explorer.update(Msg::DirDiff {
                    dir: dir.clone(),
                    diffs: diffs.clone(),
                });
                fx.extend(diff_fx);

                // Incremental update to search index
                if let Some(manager) = &mut self.index_manager {
                    manager.apply_diffs(&dir, &diffs);
                }
            }
        }

        // Layout calculation
        let tabs_rect = Rect::new(0.0, 0.0, self.viewport.w, TabBar::HEIGHT);
        let toolbar_rect = Rect::new(0.0, TabBar::HEIGHT, self.viewport.w, UnifiedToolbar::HEIGHT);
        let _status_rect = Rect::new(
            0.0,
            self.viewport.h - StatusBar::HEIGHT,
            self.viewport.w,
            StatusBar::HEIGHT,
        );

        let content_top = TabBar::HEIGHT + UnifiedToolbar::HEIGHT;
        let content_h = (self.viewport.h - content_top - StatusBar::HEIGHT).max(0.0);

        let sidebar_w = self.sidebar.width();
        let sidebar_rect = Rect::new(0.0, content_top, sidebar_w, content_h);

        let preview_w = self.preview.width();
        let _preview_rect = Rect::new(
            self.viewport.w - preview_w,
            content_top,
            preview_w,
            content_h,
        );

        let center_x = sidebar_w;
        let center_w = (self.viewport.w - sidebar_w - preview_w).max(0.0);
        let center_rect = Rect::new(center_x, content_top, center_w, content_h);

        // 2. Tab Bar
        let (sel_tab, close_tab, add_tab) = self.tabs.update(input, tabs_rect, dt);
        if let Some(idx) = sel_tab {
            fx.extend(self.explorer.update(Msg::Action(Action::SelectTab(idx))));
            self.sync_tabs();
        } else if let Some(idx) = close_tab {
            if idx == self.explorer.active_tab() {
                fx.extend(self.explorer.update(Msg::Action(Action::CloseTab)));
            }
            self.sync_tabs();
        } else if add_tab {
            fx.extend(self.explorer.update(Msg::Action(Action::NewTab)));
            self.sync_tabs();
        }

        // 3. Toolbar
        let can_back = self.explorer.pane().is_some_and(|p| p.history.can_back());
        let can_forward = self
            .explorer
            .pane()
            .is_some_and(|p| p.history.can_forward());

        let tb_actions = self
            .toolbar
            .update(input, toolbar_rect, can_back, can_forward, dt);
        for act in tb_actions {
            match act {
                ToolbarAction::Back => {
                    fx.extend(self.explorer.update(Msg::Action(Action::GoBack)));
                }
                ToolbarAction::Forward => {
                    fx.extend(self.explorer.update(Msg::Action(Action::GoForward)));
                }
                ToolbarAction::Up => {
                    fx.extend(self.explorer.update(Msg::Action(Action::GoUp)));
                }
                ToolbarAction::Refresh => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Refresh)));
                }
                ToolbarAction::NavigateBreadcrumb(idx) => {
                    if let Some(cur) = self.current_dir() {
                        let comps: Vec<_> = cur.components().collect();
                        if idx < comps.len() {
                            let mut target = PathBuf::new();
                            for c in &comps[..=idx] {
                                target.push(c);
                            }
                            fx.extend(self.explorer.update(Msg::Action(Action::Navigate(target))));
                        }
                    }
                }
                ToolbarAction::EnterAddressBar => {
                    fx.extend(self.explorer.update(Msg::Action(Action::FocusAddressBar)));
                }
                ToolbarAction::CommitAddress(text) => {
                    let path = PathBuf::from(text);
                    fx.extend(self.explorer.update(Msg::Action(Action::Navigate(path))));
                }
                ToolbarAction::CancelAddress => {
                    self.explorer.focus = Focus::Pane;
                }
                ToolbarAction::SetView(view) => {
                    fx.extend(self.explorer.update(Msg::Action(Action::SetView(view))));
                }
                ToolbarAction::ToggleSplit => {
                    self.split.toggle();
                    fx.extend(self.explorer.update(Msg::Action(Action::ToggleSplit)));
                }
                ToolbarAction::ToggleQuickLook => {
                    self.preview.toggle();
                    fx.extend(self.explorer.update(Msg::Action(Action::QuickLook)));
                }
                ToolbarAction::NewFolder => {
                    fx.extend(self.explorer.update(Msg::Action(Action::NewFolder)));
                }
                ToolbarAction::ToggleTheme => {
                    self.toggle_theme();
                }
                ToolbarAction::SearchChanged(query) => {
                    if query.is_empty() {
                        if let Some(cur) = self.current_dir() {
                            fx.extend(self.explorer.update(Msg::Action(Action::Navigate(cur))));
                        }
                    } else if let Some(manager) = &self.index_manager {
                        let hits = manager.search(&query, 100);
                        fx.extend(self.explorer.update(Msg::SearchResults {
                            request: 9999,
                            hits,
                        }));
                    }
                }
            }
        }

        // 4. Sidebar
        if let Some(clicked_path) = self.sidebar.update(input, sidebar_rect, dt) {
            fx.extend(
                self.explorer
                    .update(Msg::Action(Action::Navigate(clicked_path.clone()))),
            );
            self.toolbar.set_path(&clicked_path);
            self.watcher = Some(FolderWatcher::watch(clicked_path));
        }

        // 5. Context Menu
        let context_item = self.context_menu.update(input, self.viewport, dt);
        if let Some(item) = context_item {
            match item {
                ContextItem::Open => {
                    fx.extend(self.explorer.update(Msg::Action(Action::OpenSelection)));
                }
                ContextItem::Cut => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Cut)));
                }
                ContextItem::Copy => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Copy)));
                }
                ContextItem::Paste => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Paste)));
                }
                ContextItem::Duplicate => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Duplicate)));
                }
                ContextItem::Rename => {
                    fx.extend(self.explorer.update(Msg::Action(Action::BeginRename)));
                }
                ContextItem::Trash => {
                    fx.extend(
                        self.explorer
                            .update(Msg::Action(Action::Delete { permanent: false })),
                    );
                }
                ContextItem::DeletePermanently => {
                    fx.extend(
                        self.explorer
                            .update(Msg::Action(Action::Delete { permanent: true })),
                    );
                }
                ContextItem::Properties => {
                    fx.extend(self.explorer.update(Msg::Action(Action::Properties)));
                }
                ContextItem::NewFolder => {
                    fx.extend(self.explorer.update(Msg::Action(Action::NewFolder)));
                }
            }
        }

        // Secondary click opens context menu
        if input.secondary_pressed && center_rect.contains(input.pointer) {
            let has_sel = self.explorer.pane().is_some_and(|p| p.selected_count() > 0);
            let can_paste = self.explorer.clipboard.is_some();
            self.context_menu
                .open_for_selection(input.pointer, has_sel, can_paste);
        }

        // 6. Split divider & Content Panes
        self.split.update(input, center_rect);
        let (primary_rect, _sec_rect, _) = self.split.compute_rects(center_rect);

        // Update active pane view
        let view_kind = self.current_view();
        let (clicked_row, clicked_sort) = if let Some(pane) = self.explorer.pane() {
            match view_kind {
                ViewKind::Details => self.details_view.update(input, primary_rect, pane, dt),
                ViewKind::List => {
                    let clicked = self.list_view.update(input, primary_rect, pane, dt);
                    (clicked, None)
                }
                ViewKind::Grid => {
                    let clicked = self.grid_view.update(input, primary_rect, pane, dt);
                    (clicked, None)
                }
                ViewKind::Columns => {
                    let clicked = self.columns_view.update(input, primary_rect, pane, dt);
                    (clicked, None)
                }
            }
        } else {
            (None, None)
        };

        if let Some(sort_key) = clicked_sort {
            fx.extend(self.explorer.update(Msg::Action(Action::SortBy(sort_key))));
        }

        if let Some(row) = clicked_row {
            // Check double-click
            let is_double_click = if let Some(last_time) = self.last_click_time {
                last_time.elapsed().as_millis() < 400 && self.last_click_row == Some(row)
            } else {
                false
            };
            self.last_click_time = Some(std::time::Instant::now());
            self.last_click_row = Some(row);

            if is_double_click {
                fx.extend(self.explorer.update(Msg::Action(Action::OpenSelection)));
            } else {
                let ctrl = false;
                let shift = false;
                fx.extend(
                    self.explorer
                        .update(Msg::Action(Action::ClickRow { row, ctrl, shift })),
                );
            }
        }

        // Rubber-band selection
        if input.primary_pressed
            && primary_rect.contains(input.pointer)
            && clicked_row.is_none()
            && !self.context_menu.is_open()
        {
            self.rubber_band.start(input.pointer);
        }
        if self.rubber_band.is_active() {
            self.rubber_band.update(input.pointer);
            if input.primary_released {
                if let Some(band_rect) = self.rubber_band.end() {
                    // Multi-select items intersecting the rubber band
                    if let Some(pane) = self.explorer.pane() {
                        let total = pane.len();
                        for i in 0..total {
                            let item_y = primary_rect.y + i as f32 * 28.0;
                            let item_rect = Rect::new(primary_rect.x, item_y, primary_rect.w, 28.0);
                            if band_rect.intersects(item_rect) {
                                fx.extend(self.explorer.update(Msg::Action(Action::ClickRow {
                                    row: i,
                                    ctrl: true,
                                    shift: false,
                                })));
                            }
                        }
                    }
                }
            }
        }

        // Preview Pane update
        self.preview.update(dt);
        if let Some(pane) = self.explorer.pane() {
            let cursor_entry = pane.cursor_entry().cloned();
            let cursor_path = pane.cursor().and_then(|r| pane.path_of_row(r));
            self.preview.set_entry(cursor_entry, cursor_path);
        }

        // Drag Drop update
        self.drag_drop.update(input, false);

        // Progress update
        self.ops_progress.update(dt);

        self.sync_tabs();
        self.sync_status();

        fx
    }

    pub fn current_dir(&self) -> Option<PathBuf> {
        self.explorer
            .pane()
            .and_then(|p| p.location.dir().map(Path::to_path_buf))
    }
}
