# PROGRESS — leap `nx-explorer`

Branch `leap/nx-explorer`. New crates only: `crates/nx-fs` (the file-system engine) and
`crates/nx-explorer` (the Explorer model). The only other changes are the two workspace member
lines and `Cargo.lock`. `[workspace.dependencies]` and the other crates are untouched.

## What's done

### nx-fs
- **Robustness:** no `unwrap`/`expect` outside tests (enforced with `#![deny(clippy::unwrap_used, clippy::expect_used)]`). Every error is a typed `FsError` (thiserror) that carries its path. Long paths go through `path::to_native` (`\\?\` and `\\?\UNC\`). Reserved names (CON, NUL, …) and invalid names are rejected before any op runs.
- **Listing:** async and streaming. Batches grow from 128 entries to 4096, so the first rows paint immediately. It runs on a two-lane thread pool (Interactive/Background) with a `CancelToken`.
  - Windows backend: `FindFirstFileExW` with `FindExInfoBasic` and `FIND_FIRST_EX_LARGE_FETCH`. Reparse tags tell symlinks apart from junctions. Cloud placeholders are flagged and never opened.
  - Portable backend: `read_dir`.
- **Index (Everything-style):**
  - `PathIndex` stores 20-byte nodes in a shared byte arena. Folded names are shared when the name is already lowercase. A 64-bit char-mask prefilter and a (parent, hash) child map support resolve, rename, subtree removal and incremental `IndexChange`s.
  - Sources sit behind the `VolumeIndexer` trait:
    - NTFS: `NtfsIndexer`, which enumerates the MFT through `FSCTL_ENUM_USN_DATA` and follows live `FSCTL_READ_USN_JOURNAL` updates.
    - Everything else: a portable parallel walker that detects loops (dev/ino on Unix, canonical path on Windows).
  - USN V2/V3 records are parsed with bounds checks. `MftTree` builds the tree iteratively and survives orphans and cycles.
- **Search:**
  - Query syntax: AND terms, `dir/name` ancestor segments, `ext:`, `type:dir|file`.
  - Ranking bands: exact > prefix > word-boundary > substring > fuzzy, plus a closeness bonus and a frecency boost (7-day half-life). Frecency is stored with atomic writes and can be cleared for privacy.
  - The scan is parallel with a bounded top-K. It skips using the real maximum frecency boost and a single-term upper bound. Ancestor and hidden state is memoised per folder.
  - A proptest checks results against a brute-force reference.
- **Previews:**
  - Text, code (language plus lines), image headers (PNG/JPEG/GIF/BMP/WebP dimensions), binary kind and folders.
  - An LRU cache with a byte budget, validated by (mtime, size).
  - A priority scheduler (Focused < Visible < Near < Background) with dedup across pending and running jobs, `set_visible` reprioritisation and cancellation.
  - `IThumbnailCache` on Windows.
- **Ops engine:**
  - A copy/move/delete/rename/new-folder job queue with progress, pause/resume/cancel, and conflict policies (Ask/Skip/Replace/KeepBoth/ReplaceIfNewer, with apply-to-all).
  - Moves fall back to copy+delete across devices. Folders merge. Copying onto itself creates "name (2)".
  - Writes go to a temp file and are then renamed. A replace keeps a backup until the op commits.
  - **Crash-safe fsynced journal.** On startup, recovery deletes temp files, restores or discards backups and keeps the last undo record.
  - **Undo of the last op.** Undoing a Created step sends it to the trash, never deletes it outright.
  - Recycle Bin via `IFileOperation` (`FOFX_RECYCLEONDELETE`). Restore parses `$I` v1/v2 files.
- **Other:** folder sizes, tags/favourites store, git status badges (from `git status --porcelain`), and volumes (`GetLogicalDrives` or `/proc/mounts`).
- **Natural sort:** a precomputed byte key per entry plus an unstable sort and an in-place permutation. 100k entries sort in about 32 ms (previously 444 ms).

### nx-explorer (pure state machine, no rendering, no I/O)
- `Explorer::update(Msg) -> Vec<Effect>`. The host performs the effects: ListDir, Search, StartOp, RequestPreview, SetClipboard, and so on.
- **Tabs:** new, close, reopen closed, next/prev, select. Closing the last tab emits `CloseWindow`.
- **Split panes** (F6), with copy/move to the other pane (Shift+F5/F6).
- **Views:** Grid, List, Columns (Miller: Left goes up, Right enters a folder) and Details. Sort by name, date, size or type (repeat to flip direction). Show/hide hidden files. Filter.
- **Panes:**
  - Streaming batches are merged in sorted order.
  - Stale listings are ignored by request id.
  - Selection is kept by name across refreshes.
  - Back/Up re-highlights the folder you came from.
  - Ctrl/Shift click and keyboard range selection.
  - The grid moves by column count and the viewport scrolls with the cursor.
- **Keyboard-first:** all familiar Explorer shortcuts (Alt+←/→/↑, Backspace, F2, F5, Ctrl+L/F/E, Del vs Shift+Del, Ctrl+Shift+N, Alt+Enter, …) plus Space for Quick Look and Ctrl+P for the palette. Bindings are remappable.
- **Type-to-select:** 1 s timeout and same-letter cycling.
- **Command palette:** fuzzy-ranked with the nx-fs matcher. It covers commands and favourite/recent places and shows shortcut hints.
- **Breadcrumbs:** drive, UNC, `\\?\` and Unix paths, with collapse.
- **Sidebar:** favourites, recents and drives.
- **Inline editors:** address bar, search, rename (selects the stem; invalid names keep the editor open) and new folder (unique "New folder (2)").
- **Prompts:** permanent delete asks for confirmation.
- Quick Look follows the cursor.

## Tests

| Suite | Tests |
|---|---|
| nx-fs unit + proptest | 88 |
| nx-explorer unit + proptest | 40 |
| nx-explorer doctest | 1 |
| **Total** | **129** (target ≥ 60) |

The proptests cover:
- natural-sort total order
- key vs comparator equivalence
- path round-trips
- unique names
- USN parsing
- search vs brute force
- random pane operations
- random 120-step Explorer sessions keeping invariants

The suite passed 6 consecutive runs with no flakes.

## Acceptance checks
- `cargo check --target x86_64-pc-windows-gnu -p nx-fs -p nx-explorer`: **0 errors, 0 warnings**
- `cargo test -p nx-fs -p nx-explorer`: **129 passed**
- `cargo clippy -p nx-fs -p nx-explorer --all-targets -- -D warnings`: **clean** on Linux and on `x86_64-pc-windows-gnu`
- `cargo fmt -p nx-fs -p nx-explorer --check`: **clean**

## Benchmarks (criterion, real runs)
Environment: Linux box, 8-core Intel Xeon (virtualised), 15 GB RAM, ext4, release profile, page cache warm.

| Benchmark | Result | Target | Status |
|---|---|---|---|
| 100k dir: first batch (sync) | **171 µs** | < 16 ms | ✅ |
| 100k dir: first batch via async pool | **207 µs** | < 16 ms | ✅ |
| 100k dir: full listing, names only | **29.9 ms** | — | |
| 100k dir: full listing + metadata | **89.9 ms** | — | |
| 100k natural sort | **31.7 ms** | — | (was 444 ms before the key-based sort) |
| 1M index: heap | **90.2 MiB** (RSS delta 90.3 MiB, 94.5 B/path) | < 150 MB | ✅ |
| 1M index from MFT records incl. ref map | **95.9 MiB** | < 150 MB | ✅ |
| 1M index build (synthetic / MFT records) | 465 ms / 654 ms | — | |
| Query 1M, prefix "budget" | **1.99 ms** (1 thread 6.8 ms) | < 5 ms | ✅ parallel |
| Query 1M, substring "port_1" | **2.36 ms** (1 thread 8.2 ms) | < 5 ms | ✅ parallel |
| Query 1M, fuzzy "nxexpl" | **1.29 ms** (1 thread 6.2 ms) | < 5 ms | ✅ parallel |
| Query 1M, multi-term "summer photos" | **0.93 ms** (1 thread 2.1 ms) | < 5 ms | ✅ |
| Query 1M, path "src/report" | **2.77 ms** (1 thread 8.0 ms) | < 5 ms | ✅ parallel |
| Query 1M, "ext:rs kernel" | **1.28 ms** (1 thread 4.6 ms) | < 5 ms | ✅ |
| Query 1M, single char "a" | **2.70 ms** (1 thread 9.8 ms) | < 5 ms | ✅ parallel |

Notes:
- Every query meets < 5 ms with the default parallel scan (8 threads). Single-threaded, prefix, substring, fuzzy, path and single-char queries take 6–10 ms. The scan is memory-bound at about 28 MB per pass over 1M nodes. On a 2–4 core machine, expect roughly 3–5 ms.
- A query also allocates a transient 1 byte per node per scan thread for the folder memo (lazily zeroed pages).
- The listing numbers come from Linux `read_dir` on ext4. The `FindFirstFileExW` path is compile-checked but has not been benchmarked on Windows.
- Index data is synthetic: 1M nodes with an average name length of 23.8 bytes.

How to reproduce:
`CARGO_TARGET_DIR=/tmp/t cargo bench -p nx-fs --bench listing|index|index_memory`

## Windows-only parts that are compile-checked but not runtime-tested
- `FindFirstFileExW` listing backend (`platform/windows/listing.rs`)
- `NtfsIndexer`: MFT enumeration and live USN journal (`platform/windows/volume.rs`), plus `list_volumes` / `GetLogicalDrives`. The USN record parsing and `MftTree` build/apply *are* unit-tested on Linux with synthetic records.
- `RecycleBin` via `IFileOperation`, and restore from `$Recycle.Bin`. The `$I` parsing *is* unit-tested.
- `ShellThumbnails` via `IThumbnailCache` + `GetDIBits`

## Known gaps / what's next
1. Run the Windows-only parts on real Windows (NTFS volume, Recycle Bin, thumbnails). Add an integration test that is `#[ignore]`d unless it runs on Windows.
2. Permanent delete of a directory has no per-file progress or cancel; it deletes in one step.
3. Junctions are copied as symlinks rather than recreated as junctions.
4. No index compaction after heavy churn (deleted nodes are tombstoned). No on-disk index snapshot yet, so cold start rebuilds from the MFT (~0.6 s per 1M records).
5. No directory watcher (`ReadDirectoryChangesW` / inotify). The walker indexer relies on fed changes, and the Explorer refreshes on `DirChanged` from the host.
6. Wire nx-explorer to the nx-ui toolkit and add a host loop that performs the effects with nx-fs.
7. Speed up single-thread queries (SIMD mask scan, or splitting the node array by field).
