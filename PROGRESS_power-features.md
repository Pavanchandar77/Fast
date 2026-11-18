STATUS: DONE

# PROGRESS — leap `power-features` (Audit L10)

Branch `leap/power-features` based on `nexus-int` at `251354b` (foundation + nx-explorer merged).
New crate only: `crates/nx-power` (Nexus-OS power-user features: pure stores and reversible mutation planners).
Shared file modifications were kept minimal: exactly 1 member line added to `nexus-os/Cargo.toml` (`"crates/nx-power"`) and corresponding `Cargo.lock` entry. No Windows dependencies required for pure model testing on Linux; Windows backend calls are strictly gated behind `#[cfg(windows)]`.

---

## 1. Accomplishments & Architecture

Created `crates/nx-power` delivering pure stores, state machines, and dry-run/apply/revert mutation plans across all power-user subsystems:

### 1. Clipboard History (`clipboard.rs`)
- **Ring buffer:** Configurable capacity ring buffer with eviction of oldest unpinned items.
- **Pinning protection:** Pinned items survive buffer overflow eviction; unpinning safely re-enforces capacity.
- **Deduplication:** Repeated identical clips update timestamps rather than creating duplicates.
- **Privacy exclusions:** Rejects and avoids storing sensitive data matching configurable and built-in rules:
  - Source app filtering: excludes password managers (`1password`, `bitwarden`, `keepass`, `lastpass`, `dashlane`, `enpass`, `authenticator`).
  - Sensitive flag honoring: `CF_CLIPBOARD_VIEWER_IGNORE` / `CanIncludeInClipboardHistory = false`.
  - Content pattern regexes: PEM private keys (`-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----`), credit card numbers (13-19 digits), JWT tokens, API keys, bearer tokens.
- **Querying:** Text search across content and source apps, filtering by kind (PlainText, RichText, Image, FileList) and pinned status.

### 2. Always-on-top Pin (`always_on_top.rs` & `windows_backend.rs`)
- **State store:** `AlwaysOnTopStore` tracking pinned `WindowId`s.
- **Reversible mutation plan:** `AlwaysOnTopPlan` supporting `dry_run()`, `apply()`, and `AlwaysOnTopRevertPlan::revert()`.
- **Identity property:** `apply` followed by `revert` returns the store to its exact original state.
- **Backend:** `#[cfg(windows)]` implementation using `SetWindowPos` (`HWND_TOPMOST` / `HWND_NOTOPMOST`, `SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS`) without blocking foreign windows.

### 3. Window Snapping Layouts (`snapping.rs`)
- **Named zone sets:** Built-in presets for "Split 50/50", "3-Column Ultrawide", "2x2 Grid", "Priority Grid" (60/40), and "Focus + Sidecars" (20/60/20).
- **Normalized geometry:** `RectF` normalized coordinates (0.0..=1.0) projected into monitor work area bounds with customizable padding margins and inter-zone gaps (`SnapGeometry`).
- **Nearest zone solver:** Spatial distance calculation finding the target zone for a drag cursor position respecting distance thresholds.
- **Snap groups:** Multi-window layout grouping associating collections of windows with assigned zones for coordinated switching.
- **Reversible plan:** `SnapPlan` with `dry_run()`, `apply()`, and `UnsnapPlan::revert()` which restores the window's un-snapped original bounds.

### 4. Quick Look Preview Session (`quick_look.rs`)
- **Session state machine:** `QuickLookSession` tracking states (`Closed`, `Opening`, `Active`, `Pinned`).
- **Content models:** Text (lines, truncation, encoding), Code (syntax-highlighted lines, line count), Image metadata (dimensions, aspect ratio, file size), Directory summary, and Binary summaries.
- **`nx-fs` integration:** Seamless conversion constructor `QuickLookContent::from_fs_preview(&nx_fs::preview::Preview, file_size)` without duplicating the file-system engine.
- **Navigation & Controls:** Multi-file playlist navigation (`next_file`, `prev_file` with wrap-around), zoom controls (0.25x to 4.0x with clamping), pinning overlay to survive blur/close, and toggle behavior.

### 5. Color Picker & Converter (`color.rs`)
- **Pure color model:** `Color` (RGBA u8) with full parsing and conversion:
  - Hex: `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`.
  - CSS strings: `rgb(...)`, `rgba(...)`, `hsl(...)`, `hsla(...)`, `cmyk(...)`.
  - Color spaces: HSL, HSV/HSB, CMYK.
  - Accessibility: WCAG 2.1 relative luminance and contrast ratio calculations.
- **History & Palettes:** `ColorHistoryStore` with recent color ring buffer (deduplication, move to front), custom named palettes, and favorite swatches.

### 6. Batch File Rename Tools (`rename.rs`)
- **Transformation pipeline:** `RenamePlanner` supporting:
  - Sequential Numbering with configurable prefix, suffix, start index, step, and zero-padding.
  - Regex replacement with capture group expansion (normalizes `$1`, `$2` safely avoiding named-group ambiguity).
  - Prefix/suffix insertion and substring find/replace (case-sensitive or insensitive).
  - Case changes (Uppercase, Lowercase, TitleCase, CamelCase, SnakeCase, KebabCase).
  - Extension modifications.
- **Conflict detection:** Flags destination collisions within the batch, empty names, and Windows forbidden characters (`< > : " / \ | ? *`).
- **Dry-run & Undo:** `RenamePlan::dry_run()` report; `generate_undo_plan()` producing an exact reverse plan that restores original paths (verified via property tests).

### 7. Keyboard Remapping (`remap.rs`)
- **Table model:** `RemapTable` mapping `KeyChord` (modifiers + `KeyCode`) to `RemapTarget` (Key, Chord, Disabled).
- **Application scoping:** Process-specific remaps take precedence over global mappings.
- **Stock Windows safeguard:** By default protects stock Windows shortcuts (`Win+L`, `Ctrl+Alt+Del`, `Win+X`, `Win+R`, `Win+E`, `Win+D`, `Win+I`, `Alt+Tab`) from being hijacked unless `allow_system_shortcut_override` is explicitly enabled.
- **Cycle detection:** Detects circular remappings (e.g. A -> B and B -> A) and self-loops before adding rules.

### 8. Per-App Volume (`volume.rs`)
- **Session model:** `AudioSession` (id, process id, display name, volume level, mute state, peak meter).
- **Volume store:** Manages master volume/mute and per-app sessions.
- **Reversible mutation plans:** `SetVolumePlan` and `SetMutePlan` with dry-run reports, bounds enforcement (0.0..=1.0), and reversible undo plans restoring exact previous volume/mute states.

### 9. Quick Notes Store (`notes.rs`)
- **Note model:** `Note` (id, title, content, tags, color, pin state, timestamps).
- **Store:** Full CRUD operations, pin toggling, tag filtering, and full-text search across titles, contents, and tags (pinned notes ranked first).
- **Reversible undo:** `NoteRevertPlan` providing restoration of previous title/content and undeletion of deleted notes.

### 10. Tabs for Any Window (`tabs.rs`)
- **Tab strip state machine:** `TabGroupManager` grouping arbitrary foreign or native window IDs into tabbed containers (`TabGroup`).
- **Tab lifecycle:** Creation, adding tabs, moving tabs between groups, reordering tabs (maintaining active index), merging groups, and auto-cleanup of dissolved groups.
- **Visibility & Bounds synchronization:** `TabSwitchPlan` computes which window to activate, which windows to cloak, and syncs container bounds; reversible via `TabSwitchRevertPlan`.

### 11. Alt-Tab Model (`alt_tab.rs`)
- **MRU z-order tracking:** `AltTabSwitcher` maintaining windows in most-recently-used order.
- **State machine:** `AltTabState` (`Idle`, `Active { selected_index }`).
- **Keyboard navigation:** Press Alt+Tab opens overlay selecting MRU index 1; repeated press cycles forward; Shift+Tab cycles backward.
- **Commit & Cancel:** Releasing Alt commits selection, bringing chosen window to MRU index 0; cancelling dismisses overlay preserving prior MRU order.
- **Thumbnails:** Lightweight `PlaceholderThumbnail` metadata decoupled from OS, with thin `#[cfg(windows)]` DWM thumbnail hook points.

---

## 2. Universal Gate Verification

Every universal gate was executed from `nexus-os/` and passed with 0 errors and 0 warnings:

### Gate 1: Cross-Target Windows Check
```bash
cargo check --workspace --all-targets --target x86_64-pc-windows-gnu
```
**Output:**
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```
- Status: **0 errors, 0 warnings**

### Gate 2: Cross-Target Windows Clippy (-D warnings)
```bash
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```
**Output:**
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```
- Status: **Clean (0 warnings)**

### Gate 3: Formatting Check
```bash
cargo fmt --all -- --check
```
**Output:**
```
(clean, exit code 0)
```
- Status: **Clean**

### Gate 4: Linux Host Unit & Property Tests
```bash
cargo test -p nx-power
```
**Output:**
```
running 3 tests
test test_alt_tab_window_removal ... ok
test test_alt_tab_lifecycle_and_mru ... ok
test test_alt_tab_cancel ... ok
test result: ok. 3 passed; 0 failed

running 5 tests
test test_always_on_top_apply_and_revert_identity ... ok
test test_always_on_top_toggle ... ok
test test_always_on_top_dry_run ... ok
test test_always_on_top_multi_window ... ok
test test_always_on_top_unpin_and_revert ... ok
test result: ok. 5 passed; 0 failed

running 11 tests
test test_clipboard_delete_and_clear ... ok
test test_clipboard_deduplication ... ok
test test_clipboard_unpin_triggers_capacity ... ok
test test_clipboard_search_and_filter ... ok
test test_clipboard_pin_survives_eviction ... ok
test test_clipboard_ring_eviction ... ok
test test_privacy_exclusion_password_managers ... ok
test test_privacy_exclusion_sensitive_flag ... ok
test test_privacy_exclusion_private_key ... ok
test test_privacy_exclusion_credit_cards ... ok
test test_privacy_exclusion_jwt_tokens ... ok
test result: ok. 11 passed; 0 failed

running 8 tests
test test_color_css_strings ... ok
test test_color_cmyk_conversions ... ok
test test_color_hex_formatting ... ok
test test_color_from_hex ... ok
test test_color_history_and_palettes ... ok
test test_color_hsl_conversions ... ok
test test_color_hsv_conversions ... ok
test test_wcag_luminance_and_contrast ... ok
test result: ok. 8 passed; 0 failed

running 4 tests
test test_notes_delete_and_undelete ... ok
test test_notes_crud_and_pin ... ok
test test_notes_update_and_revert_identity ... ok
test test_notes_search_and_tag_filter ... ok
test result: ok. 4 passed; 0 failed

running 4 tests
test prop_color_hex_roundtrip ... ok
test prop_color_hex_rgba_roundtrip ... ok
test prop_clipboard_capacity_invariant ... ok
test prop_rename_undo_identity ... ok
test result: ok. 4 passed; 0 failed

running 5 tests
test test_quick_look_pinning ... ok
test test_quick_look_session_lifecycle ... ok
test test_quick_look_toggle ... ok
test test_quick_look_zoom_controls ... ok
test test_quick_look_playlist_navigation ... ok
test result: ok. 5 passed; 0 failed

running 4 tests
test test_basic_remap ... ok
test test_app_specific_priority ... ok
test test_circular_remap_detection ... ok
test test_system_shortcut_protection ... ok
test result: ok. 4 passed; 0 failed

running 7 tests
test test_invalid_character_detection ... ok
test test_prefix_suffix_and_extension ... ok
test test_collision_detection_within_batch ... ok
test test_sequential_numbering ... ok
test test_rename_dry_run_and_undo_identity ... ok
test test_case_transformations ... ok
test test_regex_replace ... ok
test result: ok. 7 passed; 0 failed

running 6 tests
test test_nearest_zone_solver ... ok
test test_default_zone_sets ... ok
test test_snap_plan_apply_and_unsnap_identity ... ok
test test_custom_zone_set_registration ... ok
test test_snap_groups ... ok
test test_zone_geometry_projection ... ok
test result: ok. 6 passed; 0 failed

running 4 tests
test test_tab_group_creation_and_tabs ... ok
test test_tab_reordering ... ok
test test_tab_switch_plan_dry_run_and_revert ... ok
test test_tab_group_merge_and_close ... ok
test result: ok. 4 passed; 0 failed

running 4 tests
test test_volume_bounds_validation ... ok
test test_volume_store_sessions ... ok
test test_set_volume_plan_dry_run_and_revert_identity ... ok
test test_set_mute_plan_apply_and_revert ... ok
test result: ok. 4 passed; 0 failed
```
- Total `nx-power` tests: **65 passed; 0 failed** (Requirement was ≥40).

### Workspace Test Suite Total:
- `nx-explorer`: 40 tests + 1 doctest = 41 passed
- `nx-fs`: 88 tests passed
- `nx-power`: 65 tests passed
- **Grand total: 194 tests passed, 0 failures.**

### Hot Path Quality:
- `#![deny(clippy::unwrap_used, clippy::expect_used)]` enforced on `nx-power/src/lib.rs`.
- Zero panics or unwrap/expect in library code.

---

## 3. Remaining Issues & Upstream Integration Notes

- **Concurrence with WM (L4):** Snapping layouts and Tab Groups are maintained in pure state stores ready to feed `nx-wm` upon merge.
- **Live DWM & WASAPI backend binding:** The thin `windows_backend.rs` layer provides SetWindowPos and clipboard listener hooks. Full integration with WASAPI `IAudioSessionManager2` and live thumbnail registrations (`DwmRegisterThumbnail`) can be linked directly when running on a live Windows machine.
- **Zero regressions:** Full workspace check and test pass cleanly.
