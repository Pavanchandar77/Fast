# Leap Foundation: Build Clean, Repo Hygiene & CI Readiness

**Date:** 18 Nov 2026  
**Repository Worktree:** `/workspace/nx-wt-foundation`  
**Branch:** `leap/foundation`  
**Author:** Senior Rust Engineer (Nexus-OS Team)  
**Reference Audit:** `/workspace/Fast/AUDIT_chief.md`

---

## 1. Executive Summary

The **foundation** milestone ("Leap 1") brings the Nexus-OS Rust codebase from an unbuildable state with 8,600+ tracked binary artifacts and API-drift compilation errors to a pristine, CI-ready, warning-free state.

All acceptance criteria have been achieved:
1. **0 compilation errors**: Fixed E0061 constructor errors by properly creating and passing Win32 HWND handles to `TopBar`, `CommandBar`, and `BorderOverlay`.
2. **0 manifest warnings**: Removed 7 unused dependencies (plus cascaded unused deps) and decoupled logging stubs `os-hal` and `os-shield` from `os-session`.
3. **0 Clippy warnings**: Resolved all 15 Clippy warnings across all 8 crates under `-D warnings` for target `x86_64-pc-windows-gnu`.
4. **Clean repository hygiene**: Untracked 8,668 build artifact files under `nexus-os/target/`, deduplicated configuration, added `.gitignore` and `rust-toolchain.toml`, and created `scripts/purge-history.sh` (using `git-filter-repo`, unexecuted per instructions).
5. **CI pipeline**: Added `.github/workflows/ci.yml` providing dual matrix CI on `windows-latest` (build release, test, clippy, fmt) and `ubuntu-latest` (cross-check gnu target, clippy, fmt).
6. **Code formatting**: Formatted entire workspace with `cargo fmt`.

---

## 2. Detailed Technical Actions

### Task 1: Fix E0061 Overlay Constructors (Proper HWND Creation)
* **Locations:**
  * `nexus-os/crates/os-session/src/main.rs:110,111,114`
  * `nexus-os/crates/os-session/src/bin/compositor_daemon.rs:77,78,81`
* **Root Cause:**
  `TopBar::new(gpu, hwnd)`, `CommandBar::new(gpu, hwnd)`, and `BorderOverlay::new(gpu, hwnd, spring_cfg)` were updated to require a parent `HWND`, but callers in `os-session` were invoking them with single arguments without creating the underlying Win32 window surface.
* **Resolution:**
  Imported `create_top_bar_window()`, `create_command_bar_window()`, and `create_border_overlay_window()` from `os_compositor` and created the proper Win32 window handles before constructing the shell surfaces:
  ```rust
  let mut top_bar = gpu.as_ref().and_then(|g| {
      let hwnd = create_top_bar_window().ok()?;
      TopBar::new(g.clone(), hwnd).ok()
  });
  let mut command_bar = gpu.as_ref().and_then(|g| {
      let hwnd = create_command_bar_window().ok()?;
      CommandBar::new(g.clone(), hwnd).ok()
  });
  let mut border_overlay = gpu.as_ref().and_then(|g| {
      let hwnd = create_border_overlay_window().ok()?;
      let spring_cfg = WindowManager::to_spring_config(&config_arc.read().compositor.spring);
      BorderOverlay::new(g.clone(), hwnd, spring_cfg).ok()
  });
  ```

### Task 2: Unused Dependency Resolution & Stub Policy
* **Manifest Audit:**
  `cargo check` flagged 7 manifest unused dependencies:
  1. `os-core` in `crates/os-config/Cargo.toml`
  2. `parking_lot` in `crates/os-core/Cargo.toml`
  3. `os-core` in `crates/os-hal/Cargo.toml`
  4. `os-hal` in `crates/os-session/Cargo.toml`
  5. `os-shield` in `crates/os-session/Cargo.toml`
  6. `os-core` in `crates/os-shield/Cargo.toml`
  7. `os-core` in `crates/os-vfs/Cargo.toml`
  Additionally, `thiserror` in `os-config` and `windows` in `os-hal`/`os-shield` were unused and removed.
* **Architecture Decision (os-hal & os-shield):**
  Per Chief's audit (§1, §3), `os-shield` and `os-hal` are currently pure stubs containing only logging statements (`info!()` logs claiming dynamic WFP packet filters and Bluetooth A2DP locking). Wiring them into `os-session`'s production boot path at this stage would mislead operators by emitting false assertions of active kernel protections. Real implementations are designated for Leap 8 (privacy-shield) and Leap 10 (power-features). They were decoupled from `os-session/Cargo.toml` so the binary links cleanly without phantom dependencies.

### Task 3: Clippy Warnings Elimination (-D warnings clean)
Fixed all 15 Clippy warnings across workspace crates:
1. `crates/os-vfs/src/mft.rs`: Added `#[derive(Default)]` for `MftIndexer` (`new_without_default`).
2. `crates/os-vfs/src/lock_assassin.rs`: Added comprehensive `# Safety` section to `force_close_remote_handle` doc comment (`missing_safety_doc`).
3. `crates/os-vfs/src/quicklook.rs`: Added `#[derive(Default)]` for `QuickLookViewer` (`new_without_default`).
4. `crates/os-shield/src/etw.rs`: Added `#[derive(Default)]` for `EtwBehavioralGuard` (`new_without_default`).
5. `crates/os-shield/src/wfp.rs`: Implemented `Default` for `WfpFirewall` (`new_without_default`).
6. `crates/os-config/src/schema.rs`: Derived `Default` on `SystemConfig` and removed redundant manual `impl Default` (`derivable_impls`).
7. `crates/os-governor/src/app_nap.rs`: Replaced separate field assignment on `PROCESSENTRY32W` with struct update syntax (`field_reassign_with_default`).
8. `crates/os-governor/src/booster.rs`: Implemented `Default` for `ForegroundBooster` (`new_without_default`).
9. `crates/os-governor/src/job_object.rs`: Derived `Default` for `JobThrottleManager` (`new_without_default`).
10. `crates/os-governor/src/job_object.rs`: Replaced separate field assignment on `JOBOBJECT_CPU_RATE_CONTROL_INFORMATION` in `throttle_process` (`field_reassign_with_default`).
11. `crates/os-governor/src/job_object.rs`: Replaced separate field assignment on `JOBOBJECT_CPU_RATE_CONTROL_INFORMATION` in `unthrottle_process` (`field_reassign_with_default`).
12. `crates/os-compositor/src/manager.rs`: Iterated over `values_mut()` instead of `iter_mut()` with discarded key (`for_kv_map`).
13. `crates/os-compositor/src/gpu/border_overlay.rs`: Replaced separate field assignment on `SquircleUniforms` (`field_reassign_with_default`).
14. `crates/os-session/src/bin/watchdog.rs`: Changed parameter `&PathBuf` to `&Path` in `validate_config` (`ptr_arg`).
15. `crates/os-session/src/bin/watchdog.rs`: Replaced manual candidate loop with `candidates.into_iter().find(...)` (`manual_find`).

### Task 4: Repository Hygiene & Artifact Untracking
* Removed 8,668 tracked files under `nexus-os/target/` from Git cache (`git rm -r --cached nexus-os/target`).
* Removed obsolete serialized duplicate `config/system.toml` from repo root, leaving canonical documented `nexus-os/config/system.toml`.
* Added `.gitignore` covering `target/`, `**/target/`, `*.d`, `*.pdb`, `*.exe`, `*.dll`, IDE, and OS artifacts.
* Added `rust-toolchain.toml` pinned to stable 1.100.0 with targets `x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu`, with `rustfmt` and `clippy`.
* Created `scripts/purge-history.sh` using `git-filter-repo` (executable, protected with interactive confirmation, **NOT executed**).

### Task 5: Dual-OS GitHub Actions CI (.github/workflows/ci.yml)
* Configured workflow for pushes and pull requests across `master`, `main`, and `leap/**`.
* `ubuntu-cross`: Runs `cargo fmt --check`, `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`, and `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`.
* `windows-native`: Runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --release`, and `cargo test --workspace`.

### Task 6: Workspace Formatting
* Installed `rustfmt` component on toolchain.
* Ran `cargo fmt` across all crates in the workspace.

---

## 3. Verification & Exact Command Outputs

### 3.1 Git Tracked Artifact Verification
Command:
```bash
git ls-files | grep target
```
Output:
```text
(exit code: 1, 0 matches)
EMPTY (0 tracked target files)
```

### 3.2 Workspace Cross-Target Compilation Check
Command:
```bash
cargo check --workspace --all-targets --target x86_64-pc-windows-gnu
```
Output:
```text
    Checking os-config v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-config)
    Checking gpu-allocator v0.27.0
    Checking os-core v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-core)
    Checking os-vfs v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-vfs)
    Checking os-shield v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-shield)
    Checking os-hal v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-hal)
    Checking os-governor v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-governor)
    Checking os-compositor v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-compositor)
    Checking os-session v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-session)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.41s
(exit code: 0, 0 errors, 0 warnings)
```

### 3.3 Clippy Strict Verification (-D warnings)
Command:
```bash
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```
Output:
```text
    Checking os-config v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-config)
    Checking os-core v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-core)
    Checking os-vfs v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-vfs)
    Checking os-shield v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-shield)
    Checking os-hal v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-hal)
    Checking os-compositor v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-compositor)
    Checking os-governor v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-governor)
    Checking os-session v0.1.0 (/workspace/nx-wt-foundation/nexus-os/crates/os-session)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.85s
(exit code: 0, 0 errors, 0 warnings)
```

### 3.4 Formatting Verification
Command:
```bash
cargo fmt --check
```
Output:
```text
(exit code: 0, clean)
```

---

## 4. Git Commits on `leap/foundation`

All changes were committed in small logical increments on branch `leap/foundation`:

| Commit Hash | Type | Description | Files Affected |
|-------------|------|-------------|----------------|
| `d7cedf1` | `fix` | Create proper parent HWND for TopBar, CommandBar, and BorderOverlay | `os-session/src/main.rs`, `os-session/src/bin/compositor_daemon.rs`, untracked `nexus-os/target/` and duplicate `config/system.toml` |
| `cd58742` | `chore` | Add .gitignore and rust-toolchain.toml | `.gitignore`, `rust-toolchain.toml` |
| `4ddb420` | `refactor` | Remove unused dependencies across workspace crates | `os-config`, `os-core`, `os-hal`, `os-shield`, `os-vfs`, `os-session` Cargo.tomls, `Cargo.lock` |
| `a3bdec7` | `chore` | Resolve all 15 clippy warnings for windows-gnu target | 12 files across `os-vfs`, `os-shield`, `os-config`, `os-governor`, `os-compositor`, `os-session` |
| `f986524` | `style` | Format workspace crates with rustfmt | 23 source files across all crates |
| `e92bd0c` | `chore` | Add purge-history.sh script using git-filter-repo | `scripts/purge-history.sh` |
| `ea595d6` | `ci` | Add GitHub Actions workflow for Windows and Ubuntu cross-check | `.github/workflows/ci.yml` |

---

## 5. Remaining Subsystem Issues & Handoff to Next Leaps

Per Chief's Audit (`/workspace/Fast/AUDIT_chief.md`), the codebase is now build-clean and stable for downstream leaps:

1. **Leap 2 (testable-core):** Extract pure logic into `nexus-model` (spring physics, layout solver, config parsing, state machine) so tests run natively on Linux hosts without Win32 FFI dependencies.
2. **Leap 3 (never-brick):** Replace irreversible process priority/throttling and headless taskbar suppression with a crash-safe state journal, safe defaults, and watchdog heartbeat.
3. **Leap 4 (wm-core):** Replace naive `HashMap` window store with an ordered tree supporting true BSP and master-stack layouts, SHOW-event registration, and directional focus.
4. **Leap 6 & 8 (vfs & shield):** Replace `os-vfs` stubs with real USN journal / MFT indexing, and replace `os-shield` log stubs with real dynamic WFP sublayers.
5. **History Purge Execution:** `scripts/purge-history.sh` is ready to run once Pavan authorizes the history rewrite and coordinate force-push.
