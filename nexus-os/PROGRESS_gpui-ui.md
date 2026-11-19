# GPUI UI Migration Progress

## Target Platform & Build Toolchain
- **Official Target:** `x86_64-pc-windows-gnu` (Windows primary target for Nexus-OS)
- **Host / Verification Target:** `x86_64-unknown-linux-gnu`
- **GPUI Version:** `gpui = "0.2.2"` (pinned in Cargo.toml workspace dependencies and Cargo.lock)
- **Compatibility Notes:**
  - `gpui 0.2.2` builds successfully for `x86_64-pc-windows-gnu` with zero errors.
  - A transient transitive dependency `xattr 0.2.3` (from `zed-async-tar 0.5.0-zed`) required a minor patch for newer `libc` (mapping Linux ENOATTR to ENODATA) vendor-patched cleanly under `patches/xattr`.
  - Windows GNU build compiles cleanly.

## Crates Porting Status
- [x] Step 1: Add GPUI 0.2.2 to workspace & verify x86_64-pc-windows-gnu / linux build.
- [ ] Step 2: Rewrite `nx-ui` design tokens / theme as GPUI theme module (Mica/Acrylic backdrop, typography, smooth corners, spring animations).
- [ ] Step 3: Rewrite `nx-shell` views (menu bar, dock with magnification, Mission Control, Spotlight) as GPUI Views/Elements.
- [ ] Step 4: Rewrite `nx-explorer-ui` views (virtualized file list via `gpui::uniform_list` for 100k+ items) as GPUI Views/Elements.
- [ ] Step 5: Rewrite `nx-privacy-dash` views as GPUI Views/Elements.
- [ ] Step 6: Remove legacy wgpu renderer in `nx-ui` and wire session startup to open GPUI windows.
- [ ] Step 7: Verification tests & gates (cargo check windows-gnu, cargo test linux, clippy, cargo fmt, no html/react/webview check).

STATUS: IN_PROGRESS
