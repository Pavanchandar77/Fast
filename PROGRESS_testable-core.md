# Leap testable-core

STATUS: DONE

Date: 18 Nov 2026
Worktree: `/workspace/nx-wt-testable-core`
Branch: `leap/testable-core` (continued from `17cc4b2`, not rewritten)
Toolchain: Rust 1.100.0, target `x86_64-pc-windows-gnu`
Audit: `/workspace/Fast/AUDIT_chief.md` sections 3 and 5 (L2)

## What landed

The previous session on this branch already added the pure crate and the Windows seam:

- `72a2442` `nexus-model`: layout, spring, config validation, hotkey parser, governor policy, workspace state machine, `Platform` + `FakePlatform`
- `ce6ebb4` `Win32Platform` in `os-core`, Win32 APIs behind `#[cfg(windows)]`
- `06304aa` `os-config`, `os-compositor`, and `os-governor` call into `nexus-model`
- `17cc4b2` session binaries and `os-vfs` gated so the workspace builds off Windows

This session finished the gaps those commits left:

- Layout no longer panics on a tight work area. `clamp(20, width - gap - 20)` panicked when the upper bound dropped below 20, and a short area used to drop every window on the same rect. Splits now stay inside the usable rect, use a full inner gap or none, and the gap check only flags direct neighbors. Master-stack and dwindle proptest ranges include small screens. A saved proptest seed is in `nexus-os/crates/nexus-model/proptest-regressions/layout.txt`.
- `Platform::set_window_cloaked` is part of the trait. `FakePlatform` records it. `Win32Platform` sets `DWMWA_CLOAK`. `AboveNormal` maps to `ABOVE_NORMAL_PRIORITY_CLASS`.
- `WindowMutation::apply` / `apply_mutations` run state-machine output on any `Platform`. A test switches workspaces on `FakePlatform` and checks show/hide.
- `os-compositor` and `os-session` keep Win32, wgpu, and the shell crates in `[target.'cfg(windows)'.dependencies]`. Linux `cargo check --workspace --all-targets` compiles every workspace crate with no warnings.

`nexus-model` has no `windows` dependency. Hot paths in that crate do not use `unwrap`, `expect`, or `panic`. Test code still uses `unwrap` / `expect`.

## Gates

Commands were run from `nexus-os/` with `PATH=$HOME/.cargo/bin:$PATH`.

### cargo test -p nexus-model (Linux)

```
running 88 tests
test result: ok. 88 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

Doc-tests nexus_model
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

88 is above the 80-test accept line. The count includes:

- layout proptest `proptest_master_stack_no_overlap_and_inside_bounds` (100 cases): no overlaps, rects inside the usable work area, gaps respected
- layout proptest `proptest_dwindle_no_overlap_and_inside_bounds` (100 cases): same three properties
- `proptest_arbitrary_toml_parse_never_panics`
- `proptest_randomized_config_validation_never_panics`
- `proptest_hotkey_parser_never_panics`
- `proptest_action_parser_never_panics`

### cargo test --workspace (Linux)

`nexus-model`: `ok. 88 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s`

Every other lib and bin (`os-compositor`, `os-config`, `os-core`, `os-governor`, `os-hal`, `os-shield`, `os-vfs`, `nexus-os`, `nexus-compositor`, `nexus-governor`, `nexus-vfs`, `nexus-watchdog`) and every doc-test target:

```
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Workspace exit code 0.

### cargo check --workspace --all-targets --target x86_64-pc-windows-gnu

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.85s
```

Exit code 0. 0 errors.

### cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.17s
```

Exit code 0. No warnings.

### cargo fmt --all -- --check

Exit code 0. No diff.

### cargo check --workspace --all-targets (Linux host)

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.94s
```

Exit code 0. No unused-dependency warnings after the `cfg(windows)` dep move.

## Remaining issues

- Scrolling layout is niri-style: columns keep a fixed width and can extend past the right edge of the work area. The inside-bounds proptest covers master-stack and dwindle. Monocle places every window on the same rect on purpose.
- `Win32Platform::set_job_cpu_rate_cap` leaves the job handle open so the cap survives, and `remove_job_cpu_rate_cap` does not clear it. The live governor uses `JobThrottleManager`, which updates the cap in place. Nothing in the shell calls `Win32Platform` yet.
- `os-compositor` `WindowManager` still stores windows in a `HashMap` and calls `LayoutEngine` directly. `WindowManagerStateMachine` is the ordered pure model and is tested, including apply onto `FakePlatform`. Driving the Win32 manager from that machine is the L4 WM leap.
- Governor policy still decides BelowNormal demotion, a 1% hard CPU cap, and working-set purge. Those decisions are pure and tested. Safe defaults and a journal of the mutations are L3 and L7. This leap does not change runtime governor behavior.
- `os-hal`, `os-shield`, and the VFS indexer are still stubs. Out of scope for L2. `nexus-journal`, `nx-ui`, and `nx-explorer` were not created.
- `scripts/purge-history.sh` was not run. History rewrite needs Pavan's OK.
