# PROGRESS — leap `explorer-ui`

STATUS: DONE

Branch `leap/explorer-ui`. Worktree `/workspace/nx-wt-explorer-ui`, based on `nexus-int 4759b12`.
New crate `crates/nx-explorer-ui` (pure view-model + scene building testable on Linux; window/swapchain host behind `cfg(windows)`).

## What Landed

### 1. Finder-Quality Window & Design System Integration (`nx-explorer-ui`)
- **Unified Toolbar (`src/toolbar.rs`):** Navigation controls (Back, Forward, Up, Refresh), breadcrumb segment bar with interactive click navigation, inline address bar text editor, view switcher segmented control (Icons, List, Columns, Details), split-pane toggle, Quick Look preview toggle, theme mode toggle (light/dark with macOS-grade palettes), and search field.
- **Translucent Sidebar (`src/sidebar.rs`):** Collapsible sidebar with macOS material blur, Favorites (Home, Documents, Downloads, Desktop, Pictures), Locations (Drives C:\, D:\), and Tags with macOS control color dots (Red, Blue, Green, Purple); smooth spring animations on expand/collapse.
- **Tabs (`src/tabs.rs`):** Tab bar with new tab button (`+`), per-tab close buttons, active tab indicator spring pill with macOS physics.
- **Split Pane (`src/split.rs`):** Dual-pane view layout with draggable divider, keyboard shortcuts (F6, Shift+F5/F6) for cross-pane copy/move.
- **Status Bar (`src/status_bar.rs`):** Bottom status bar displaying item count, selected items, selected bytes (formatted as B/KB/MB/GB), and drive free space.
- **Quick Look Rich Preview (`src/preview.rs`):** Slide-in pane with spring animation, displaying large file kind icons, metadata (file type, size, modified timestamp, path), and text snippet preview.
- **Context Menu (`src/context_menu.rs`):** Spring-animated context menu with Open, Cut, Copy, Paste, Duplicate, Rename, Move to Trash, Delete Permanently, Properties, and New Folder.
- **Operation Progress Banner (`src/ops_progress.rs`):** Visual file job indicator with progress bar, active job title, and spring fade-in/out.
- **Drag & Drop Model (`src/drag_drop.rs`):** Pointer drag distance threshold detection, drop intent resolution (Move/Copy), and floating translucent item badge with item count.
- **Rubber-band Selection (`src/rubber_band.rs`):** Visual selection box with translucent accent fill, border, and intersection hit-testing against items.

### 2. Virtualized Views over 100k+ Entries (`src/views/`)
- Powered by `nx-ui` `virtual_window`: O(1) row layout, only visible items constructed per frame.
- **Details Table View (`src/views/details.rs`):** Virtualized rows with sortable headers (Name, Date Modified, Size, Kind), inline rename box with filename stem selection.
- **Compact List View (`src/views/list.rs`):** Virtualized compact file list with icons and sizes.
- **Icon Grid View (`src/views/grid.rs`):** Virtualized icon tile layout with dynamic columns and center-aligned labels.
- **Miller Columns View (`src/views/columns.rs`):** Hierarchical Finder-style column navigation with folder chevron indicators.
- **Interactions:** Single-click selection, Shift-click range selection, Ctrl-click toggle, Ctrl+A select-all, inline rename (commit on Enter, cancel on Esc), type-to-select with cycling.

### 3. Folder Watcher with Debounced Diff Application (`src/watcher.rs`)
- **Windows Backend:** `ReadDirectoryChangesW` on Windows behind `cfg(windows)` tracking directory changes with wide-path semantics.
- **Linux / Portable Backend:** `notify` filesystem event watcher.
- **Debouncing & Coalescing (`WatcherDebouncer`):** Rapid burst coalescing (Create then Delete cancels out, rapid modifications coalesce, renames tracked from->to) into minimal `DirDiff`s.
- **Zero Full Reloads:** Diffs feed `nx-explorer` via `Msg::DirDiff`, inserting and removing rows in-place into `visible` in O(log N) time while preserving selection and scroll position.

### 4. On-Disk Crash-Safe Index Persistence (`nx-fs/src/index/disk.rs` & `src/index_cache.rs`)
- **Disk Format:** Compact binary format storing node arenas, folded names, child maps, and parents.
- **Integrity & Checksum:** Magic `NXIX`, format version 1, payload CRC32 via `crc32fast`.
- **Crash-Safe Write:** Atomic write to temporary file (`.tmp_index_<pid>`), `sync_all` flush, and atomic filesystem rename.
- **Persistent Index Manager (`PersistentIndexManager`):** Loads existing index from disk in < 30 ms; detects bit-flip corruption or truncated files via CRC32 and automatically triggers clean rebuild without crashing. Incrementally applies folder watcher diffs to the index without re-indexing.

### 5. Windows Host & Swapchain (`src/host.rs`)
- Behind `cfg(windows)`: Winit event loop + WGPU swapchain host.
- Connects DWM backdrop hooks (Mica / Acrylic) and dark mode switching via `nx-ui::platform::apply_hwnd`.

---

## Gate Outputs

All required gates passed cleanly:

1. **Windows Target Check:**
   `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`
   -> **0 errors, 0 warnings**

2. **Windows Target Clippy:**
   `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
   -> **Clean (0 errors, 0 warnings)**

3. **Linux Workspace Tests (3 Consecutive Runs):**
   `cargo test --workspace`
   - Run 1: **143 tests passed; 0 failed; 0 flakes**
   - Run 2: **143 tests passed; 0 failed; 0 flakes**
   - Run 3: **143 tests passed; 0 failed; 0 flakes**
   Includes all 14 `nx-explorer-ui` unit, property, and integration tests.

4. **Code Formatting:**
   `cargo fmt --all -- --check`
   -> **Clean (0 diffs)**

5. **Lint Safety:**
   `#![deny(clippy::unwrap_used, clippy::expect_used)]` enforced in `nx-explorer-ui`. No unwrap/expect/panic in non-test hot paths.

---

## Benchmark Numbers

Measured via Criterion (`cargo bench -p nx-explorer-ui --bench explorer_ui_bench`) on release profile:

| Benchmark | Budget | Result | Margin |
|---|---|---|---|
| `first_frame_100k_model_work` | < 16.0 ms | **354 ns** (0.00035 ms) | ~45,000x faster |
| `first_batch_streaming_model_work` | < 16.0 ms | **244 µs** (0.244 ms) | ~65x faster |
| `scroll_frame_building_100k` | < 2.0 ms | **19.07 µs** (0.019 ms) | ~100x faster |
| `search_1m_names` | < 30.0 ms | **1.25 ms** | ~24x faster |
| `allocations per scroll frame` | 0 allocs | **0 allocs** (`!scene.grew` verified) | Exact 0 |

---

## Known Gaps & Future Enhancements
- Thumbnail rendering currently uses vector icon fallbacks for images; integration with Windows `IThumbnailCache` via COM for thumbnail previews in icon grid view can be hooked into the preview scheduler.
- Miller columns currently navigates up to 1 level of sub-hierarchy; multi-level horizontal column scrolling can be further extended.
