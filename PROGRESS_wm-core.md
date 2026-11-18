# PROGRESS — Leap `wm-core`

**STATUS: DONE**

Branch: `leap/wm-core`  
Worktree: `/workspace/nx-wt-wm-core`  
Integrated with: `nexus-int` at commit `4759b12` (testable-core, privacy-shield, nx-ui, privacy-dashboard, power-features)  
Merge commit: `9fb0bd2` (`git merge nexus-int`, no rebase)  
Audit Reference: `/workspace/Fast/AUDIT_chief.md` (§3 Compositor/WM + Hotkeys, L4 wm-core)  
Toolchain: Rust 1.100, host Linux, target `x86_64-pc-windows-gnu`

---

## 0. Integration merge and dwindle overlap fix

`git merge nexus-int` is committed as `9fb0bd2`. Both sides survive:

- **Workspace `Cargo.toml` / `Cargo.lock`**: union of members — `nexus-model`, `os-core`, `os-config`, `os-governor`, `os-compositor`, `os-vfs`, `os-shield`, `os-hal`, `os-session`, `nx-fs`, `nx-explorer`, `nx-wm`, `nx-privacy`, `nx-ui`, `nx-privacy-dash`, `nx-power`.
- **`os-compositor/Cargo.toml`**: `nexus-model` and `nx-wm` stay normal dependencies. Win32 crates (`os-core`, `os-config`, `windows`, `wgpu`, …) stay under `[target.'cfg(windows)'.dependencies]`.
- **`os-compositor` sources**: `manager`, `hooks`, `hotkeys`, and `gpu` stay `#[cfg(windows)]`. The window manager still drives `nx_wm::WindowManagerModel`. `spring` re-exports `nexus_model::spring`, and hotkeys re-export `nexus_model::hotkeys::HotkeyAction`, so the Platform / pure-model seam from testable-core stays in place.

**Dwindle / master-stack overlap (testable-core proptest).** On a tiny work area the BSP splitter handed the last pane to every leftover window. Minimal input `screen_w=131`, `screen_h=384`, `inner_gap=8`, `outer_gap=10`, `top_bar=35`, `master_ratio=0.7746066`, `win_count=8` put the last two tiles on the same `Rect { x: 120, y: 373, w: 1, h: 1 }`. `pack_tiles` now places as many 1px tiles as the longer axis can separate and cloaks the rest as one shared zero-area rect on the area's bottom-right corner. `Rect::intersects` is strict, so that corner does not overlap a tile inside the area, and it still passes the inside-bounds check. Master-stack uses the same packer when the work area is too narrow to split. Covered by `test_tiny_screen_dwindle_and_master_stack_do_not_overlap` and proptest regression seed `9df5745fe273ee143a92e9f5059cf5a92b64046421497450c250eef43a6771e8` (shrinks to the 120×120, 8-window case of the same bug).

---

## 1. Executive Summary

This leap implements **L4: wm-core (a WM that actually works)**. All pure window management logic has been built inside a new pure crate `nx-wm` (`nexus-os/crates/nx-wm`) with zero `windows` dependency, allowing 100% of the window management model, layout algorithms, multi-monitor migrations, workspace cloaking plans, and keybinding parsing to be unit- and property-tested directly on Linux.

Thin call-site wiring has been integrated into `os-compositor` behind Windows target checks, replacing naive tiling and hardcoded hooks with the pure `nx-wm::WindowManagerModel` and config-driven keyboard dispatch with Windows shortcut pass-through.

---

## 2. Audit Issues Addressed & Architecture Implemented

| Issue from Audit / Spec | Previous State | Resolved State in `nx-wm` / `os-compositor` |
|---|---|---|
| **Fake Workspaces** | Workspace switch only changed a number; off-workspace windows remained visible on screen. | Real workspaces: switching workspaces emits a `MutationPlan` that cloaks/hides windows of the outgoing workspace and uncloaks/shows/positions windows on the incoming workspace. `create_exit_plan()` restores all windows across all workspaces upon exit. |
| **New Windows Never Tiled** | `WindowCreated` required visibility (which new windows don't have yet), and `WindowShow` only updated already-registered windows. | Registration on `SHOW` (and on `CREATE` if already visible). Windows newly shown are evaluated against rules, inserted into the workspace tree, and retiled instantly. |
| **Random Tile Order (HashMap)** | Windows were stored in a `HashMap`, causing tile order and master pane to reshuffle randomly. | Implemented `OrderedWindowTree`: deterministic tiling order, stable insertion adjacent to active pane or at tail, and MRU focus stack for seamless fallback upon window destruction. |
| **Naive Single Layout** | Master-stack, dwindle, and scrolling all executed the identical master-stack code. | Implemented real pure layout functions `compute_layout(kind, windows, work_area, params) -> Vec<(WindowId, Rect)>`: <br>• **Dwindle**: recursive BSP with aspect-ratio adaptive splits and inner gap preservation.<br>• **MasterStack**: master pane + stack column with exact vertical gap distribution and remainder absorption.<br>• **Scrolling**: niri-style horizontal ribbon centering active column with configurable column width.<br>• **Monocle**: full usable area minus outer gaps.<br>• **Floating**: preserves floating coordinates clamped to monitor. |
| **Single Monitor & No DPI / Work Area** | Hardcoded `SM_CXSCREEN`/`SM_CYSCREEN`, no multi-monitor or work area accounting. | Multi-monitor engine in `Monitor`: per-monitor bounds, reserved work area (appbars/taskbar/top-bar), DPI scale factor. Support for adding, removing, and repositioning monitors. |
| **Monitor Unplugging** | Unplugging a monitor lost window state and layout. | Dynamic migration: when a monitor is removed, all its workspaces and windows are migrated to the primary surviving monitor without losing any window. |
| **Swallowed Windows Shortcuts** | Hook swallowed `Win+Space`, `Win+Q`, and `Win+1..9`, breaking input language switch, search, and taskbar launch. | Config-driven `KeybindingMap` with strict passthrough for Windows reserved shortcuts (`Win+Space`, `Win+Q`, `Win+1..9`, `Win+Tab`, `Alt+Tab`, `Win+D`, `Win+E`, `Win+Z`). Shortcuts pass through unless explicitly bound by user. |
| **Start Menu Activation on Win Release** | No Win-key-up masking when using Super shortcuts. | Super shortcut triggers emit `MaskWinKeyUp` and synthesize neutral keyboard event (`keybd_event(0,0,0,0)`), preventing the Windows Start Menu from popping up. |
| **Window Rules & Dialogs** | Context menus, splash screens, dialogs were tiled or ignored haphazardly. | `RuleEngine` with declarative matchers (title, class, process name, dialog flag) and actions (`Float`, `Tile`, `AssignWorkspace`, `AutoExclude`, `PiP`, `SetInitialSize`). Dialogs and PiP float by default. Full-screen borderless games are auto-excluded from tiling. |
| **Main Loop Hanging on Foreign Windows** | `SetWindowPos` was called synchronously at 120Hz; hung foreign apps could freeze compositor. | Enforced `SWP_ASYNCWINDOWPOS \| SWP_NOACTIVATE \| SWP_NOZORDER` on foreign window repositioning calls. |
| **Dry-Run & Reversibility** | Mutations directly mutated system state with no rollback plan. | Modeled mutations as pure data `MutationPlan` (`SetPosition`, `SetVisibility`, `SetCloak`, `SetFocus`, `SetFullscreen`, `SetFloating`, `MaskWinKeyUp`). Supports snapshotting and `compute_inverse()` guaranteeing apply/revert identity. |
| **Animation Hooks** | Layout updates jumped coordinates directly without transition metadata. | Layout engine emits `LayoutTransition` containing `from_rects`, `to_rects`, and `AnimationReason` (WindowOpened, WindowClosed, WorkspaceSwitch, FocusChanged, Retile, LayoutChanged, MonitorChanged, Snap, etc.) so downstream spring animators can interpolate smoothly. |

---

## 3. Universal Gates & Exact Command Outputs

All commands from `nexus-os/` with `PATH=$HOME/.cargo/bin:$PATH`. Three `cargo test --workspace` runs each reported 510 passed and 0 failed (pass counts identical across runs).

### Gate 1: `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.25s
```
**Result: 0 errors.**

---

### Gate 2: `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.94s
```
**Result: 0 warnings, clean.**

---

### Gate 3: `cargo fmt --all -- --check`
```
(no diff)
```
**Result: clean.**

---

### Gate 4: `cargo test --workspace` (Linux host, 3 runs)

| Run | Exit | Passed | Failed |
|---|---|---|---|
| 1 | 0 | 510 | 0 |
| 2 | 0 | 510 | 0 |
| 3 | 0 | 510 | 0 |

Non-zero suites (same on every run):

| Crate | Passed |
|---|---|
| nexus-model | 89 |
| nx-fs | 88 |
| nx-ui | 84 |
| nx-power | 65 |
| nx-privacy | 53 |
| nx-wm (`wm_core_tests`) | 46 |
| nx-privacy-dash | 44 |
| nx-explorer | 40 |
| os-vfs | 1 |

`nx-wm` still has 46 tests (target was ≥40), including dwindle / master-stack / scrolling proptests. `nexus-model` includes `test_tiny_screen_dwindle_and_master_stack_do_not_overlap` plus the layout proptests (100 cases each, regression seeds replayed first). Windows-only crates (`os-compositor`, `os-core`, `os-config`, `os-governor`, `os-session`, `os-hal`, `os-shield`) compile on Linux and report 0 host tests.

---

### Gate 5: No unwrap/expect/panic in hot paths
- Enforced with `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]` in `crates/nx-wm/src/lib.rs`.
- Zero unwrap, expect, or panic occurrences in `nx-wm` runtime paths.

---

## 4. Test Suite Coverage Details

| Category | Tests | Description |
|---|---|---|
| **Geometry** | 4 | Bounding box intersection, edge shrinking, gap safety, cardinal directional distance metrics. |
| **Layouts (Unit)** | 6 | Dwindle single window, 2-window split, 4-window spiral; Master-stack master/stack ratio; Scrolling ribbon; Monocle. |
| **Layouts (Proptest)** | 3 | `proptest_dwindle_invariants`, `proptest_master_stack_invariants`, `proptest_scrolling_invariants` verifying for arbitrary window counts (1..15), screen sizes (600..2560 x 400..1440), and gap configurations that all rects remain strictly within work area bounds and have zero overlaps. |
| **Ordered Tree** | 4 | Deterministic insertion order, MRU focus stack, active cycling wrapping, swap/move operations. |
| **Workspaces & Cloaking** | 3 | Workspace switching cloaking/restoring; last-focused window restoration per workspace; moving windows between active and inactive workspaces. |
| **Registration & Rules** | 5 | Register on SHOW bugfix; dialog auto-floating; Picture-in-Picture sizing; per-app workspace matching; exclusive fullscreen game auto-exclusion. |
| **Focus & Navigation** | 4 | 2D directional candidate selection (left/right/up/down); directional window movement; boundary handling when blocked; follow-mouse pointer tracking. |
| **Multi-Monitor** | 3 | Multi-monitor addition; dynamic repositioning/work area resizing; unplugging monitor with workspace/window migration ensuring zero lost windows. |
| **Keybindings & Safety** | 3 | Key combination & action string parsing; native Windows shortcut protection (Win+Space, Win+Q, Win+1-9, Alt+Tab passthrough); Win-key-up masking. |
| **Snapping & Fullscreen** | 3 | FancyZones/Snap compatible zone geometry (halves, thirds, quadrants); fullscreen state machine with saved rect restoration; snap window planning. |
| **Dry-Run & Identity** | 3 | Mutation plan generation without OS mutation; snapshot creation and apply/revert identity; exit plan uncloaking all windows. |
| **Total** | **46** | All green on Linux and cross-check. |

---

## 5. Remaining Issues & Next Steps

1. **Spring Animator Integration (Leap L5 `macos-shell`)**: `LayoutTransition` is cleanly emitted from `nx-wm`. When L5 arrives, the wgpu compositor can consume `transition.from_rects` and `transition.to_rects` to drive the spring interpolation solver without altering `nx-wm`.
2. **Platform seam**: `nexus-model` is on this branch. `os-compositor` re-exports its spring solver and hotkey action. `nx-wm` remains the window-manager model the Windows `WindowManager` applies. A later pass can route `nx-wm` mutations through `nexus_model::Platform` without changing the plans.
3. **Undo Journal Integration (Leap L3 `never-brick`)**: `MutationPlan` is already modeled as pure data with `compute_inverse(&snapshots)`, ready to serialize to the crash-safe journal when L3 lands.
