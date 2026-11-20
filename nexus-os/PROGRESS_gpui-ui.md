# GPUI UI Migration Progress

STATUS: DONE

## Target Platform & Build Toolchain
- **Official Target:** `x86_64-pc-windows-gnu` (Windows primary target for Nexus-OS)
- **Host / Verification Target:** `x86_64-unknown-linux-gnu`
- **GPUI Version:** `gpui = "0.2.2"` (pinned in Cargo.toml workspace dependencies and Cargo.lock)
- **Compatibility & Build Verification:**
  - `gpui 0.2.2` builds cleanly for `x86_64-pc-windows-gnu` with zero errors.
  - A transient transitive dependency `xattr 0.2.3` (from `zed-async-tar 0.5.0-zed`) required a patch for newer Linux `libc` (mapping Linux ENOATTR to ENODATA), vendor-patched cleanly under `patches/xattr` and referenced via `[patch.crates-io]` in workspace `Cargo.toml`.
  - Windows GNU build compiles with 0 errors across all UI crates and session orchestrators.

## Crates Porting Status
- [x] Step 1: Add GPUI 0.2.2 to workspace & verify `x86_64-pc-windows-gnu` / linux build.
- [x] Step 2: Rewrite `nx-ui` design tokens / theme as GPUI theme module (`nx_ui::gpui_theme`).
- [x] Step 3: Rewrite `nx-shell` views (`crates/nx-shell/src/gpui_views.rs`): MenuBarView, DockView, MissionControlView, SpotlightView.
- [x] Step 4: Rewrite `nx-explorer-ui` views (`crates/nx-explorer-ui/src/gpui_view.rs`): ExplorerView with `gpui::uniform_list`.
- [x] Step 5: Rewrite `nx-privacy-dash` views (`crates/nx-privacy-dash/src/gpui_view.rs`): PrivacyDashboardView.
- [x] Step 6: Wire session startup to open all GPUI windows (`crates/os-session/src/startup.rs` + `main.rs`).
- [x] Step 7: Verification tests & gates.

## Fix round 1

Watchdog: check/clippy/fmt/tests passed, but the shipped binary was not actually running GPUI. Four follow-up fixes:

### Fix 1 — `nexus-os` starts a real GPUI application (`cf4fb3c`)
`crates/os-session/src/main.rs` now calls `gpui::Application::new().run(...)` and `open_gpui_windows`, so the menu bar, dock, Spotlight, Mission Control, explorer, and privacy dashboard are real GPUI windows.

The compositor / governor / hotkey / heartbeat / WinEvent 120Hz solver runs on a background thread (`nexus-compositor-loop`). Fail-safe Ctrl+Alt+Shift+Esc restores the session via `WindowManager::restore_all_on_exit` and then `cx.quit()`. `HotkeyAction::CommandBar` toggles the GPUI `SpotlightView` (window starts hidden; toggle shows/hides it).

### Fix 2 — Explorer host opens `ExplorerView` via GPUI (`1e220a9`)
`crates/nx-explorer-ui/src/host.rs` opens `ExplorerView` with `gpui::Application::new().run`. The winit / `nx_ui` renderer host path and wgpu/winit deps were removed from `nx-explorer-ui`.

### Fix 3 — os-compositor user chrome removed (`61c2058`)
`crates/os-compositor/src/gpu/top_bar.rs` and `command_bar.rs` (raw wgpu top bar and command bar) were deleted. GPUI `MenuBarView` and `SpotlightView` replace them. `main.rs` no longer creates those surfaces.

**`border_overlay` and `window_surface` are compositor effects only**, not user-facing UI chrome:
- `border_overlay.rs` is a fullscreen click-through HWND that draws the L4-norm squircle **focus border** around the active managed window at 120Hz.
- `window_surface.rs` is the Win32 HWND / wgpu `Surface` bridge used by that overlay (and DirectComposition frame setup). It does not draw menus, docks, Spotlight, or any other chrome.

### Fix 4 — nx-ui custom renderer removed (`aae07ce`)
Deleted `crates/nx-ui/src/gpu`, `scene`, `atlas`, `text` engine, and `examples/gallery.rs`. wgpu was dropped from `nx-ui`. What remains is `gpui_theme` plus pure gpui-independent layout/model helpers still used by shell/explorer/privacy.

## Quality Gates Verification (Fix round 1)

PATH=`$HOME/.cargo/bin:$PATH`, cwd `nexus-os`.

### 1. Windows GNU Cargo Check (`x86_64-pc-windows-gnu`)
```
$ cargo check --workspace --all-targets --target x86_64-pc-windows-gnu
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.28s
Exit code: 0
Errors: 0
```

### 2. Windows GNU Cargo Clippy (`x86_64-pc-windows-gnu`)
```
$ cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.50s
Exit code: 0
Warnings treated as errors: none (clean)
```

### 3. Rustfmt Check
```
$ cargo fmt --all --check
Exit code: 0
```

### 4. Linux Unit & GPUI Tests
```
$ cargo test --workspace --no-fail-fast
    Finished `test` profile [unoptimized + debuginfo] target(s) in 45.67s
```
All suites: **698 passed; 0 failed; 0 ignored.**

Notable os-session tests:
- `test_gpui_session_startup_opens_windows`
- `test_command_bar_toggles_gpui_spotlight`
(5 os-session lib tests passed)

### 5. Real GPUI startup path
```
$ rg -n "open_gpui_windows|Application::new" crates/os-session/src/main.rs
198:        gpui::Application::new().run(move |cx| {
201:            let windows = os_session::startup::open_gpui_windows(cx, &mut session);
```

### 6. No custom-renderer / winit user-facing UI
```
$ rg -n "nx_ui::gpu|nx_ui::scene|winit" crates
crates/nx-explorer-ui/src/gpui_view.rs:9:use nx_ui::gpui_theme::sf_font_family;
crates/nx-shell/src/gpui_views.rs:8:use nx_ui::gpui_theme::{sf_font_family, ThemeStyledExt};
crates/nx-privacy-dash/src/gpui_view.rs:12:use nx_ui::gpui_theme::sf_font_family;
```
The only hits are `nx_ui::gpui_theme` (the `nx_ui::gpu` regex is a prefix of `gpui`). No `nx_ui::gpu` renderer, no `nx_ui::scene`, no `winit`. No user-facing UI uses the old custom toolkit.
