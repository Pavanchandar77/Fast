STATUS: DONE

Leap never-brick (audit L3) is on branch `leap/never-brick`. Nexus records every system mutation before applying it and puts Windows back on a clean exit, a panic, a fail-safe, a hung shell, and the next startup when a stale journal is still on disk.

## Merge with nexus-int

Normal merge of `nexus-int` at `4759b123335ced75f50d9f0efe753ab331d1e694` (`Merge leap/power-features`) into `leap/never-brick`. That integration commit already contains testable-core, privacy-shield, nx-ui, privacy-dashboard, and power-features.

Both sides stay:

- `nexus-model` keeps `Platform`, `FakePlatform`, hotkeys, layout, spring, and the governor policy types. `Win32Platform` stays in `os-core` behind `cfg(windows)`. Linux builds use the unsupported platform stub.
- Workspace members are the union: `nexus-journal`, `nexus-model`, `os-core`, `os-config`, `os-governor`, `os-compositor`, `os-vfs`, `os-shield`, `os-hal`, `os-session`, `nx-fs`, `nx-explorer`, `nx-privacy`, `nx-ui`, `nx-privacy-dash`, `nx-power`.
- `os-config` re-exports `nexus_model::config`. Safe governor defaults live on `GovernorConfig`: `app_nap_enabled = false`, `allow_nt_suspend = false`, `dynamic_priority_boost = false`, foreground `AboveNormal`, background `Normal`, empty `priority_whitelist`.
- Win32-only dependencies in `os-compositor` and `os-session` stay under `cfg(windows)`. Session binaries compile on Linux as a stub `main`.
- The journal still wraps the live path: startup restores a stale file, `RestoreOnDrop` and the panic hook restore on the way out, a failed keyboard hook aborts startup and restores, the fail-safe hook only sets an atomic and posts `WM_APP + 0x15`, the watchdog uses `RegisterHotKey` plus `Local\NexusOsHeartbeat` (15s), foreign `SetWindowPos` uses `SWP_ASYNCWINDOWPOS`, and `--dry-run` logs without recording or applying. Minimize trims memory and does not nap. `NtSuspendProcess` runs only from `ProcessHandle::suspend` after `suspend_if_allowed` sees `allow_nt_suspend`.
- `os-shield` re-exports `nx-privacy` and `attach()` still logs that this build does not install WFP filters.

## What landed

Commits, oldest first:

- `c37556e` feat(journal): add a crash-safe record of system mutations
- `862903b` feat(config): keep governor defaults from bricking Windows
- `41f63cf` feat(core): journal Win32 changes and pulse a main-thread heartbeat
- `a88a612` feat(governor): record priority and nap changes before applying them
- `6270301` feat(compositor): keep foreign moves async and the fail-safe hook tiny
- `9d88d87` feat(session): restore state on exit, panic, and a hung shell

`nexus-journal` is a pure crate (no `windows` dependency). The on-disk format is version 1 (`NXJR` / `NXR1`), CRC32-checked, rewritten by an fsynced temp file and an atomic rename. A torn tail or a bad record is skipped. An unknown version is left untouched. `restore_all` reverts newest-first and shrinks the file after each step.

`SystemMutator` is the small apply/revert trait. `FakeSystem` is the test double. This leap left `Platform` to testable-core. The merge keeps `nexus-model` and records shell mutations through `recovery::commit` and `place_foreign_window`.

Mutations covered: process priority, job CPU cap, suspend, taskbar hide, app bar, timer resolution, window cloak, window hide, window position. Position changes coalesce to one journal entry per hwnd (the original rectangle) so a 120 Hz move does not fsync every frame. The move itself uses `SWP_ASYNCWINDOWPOS`.

Wiring:

- Startup opens the journal and restores a stale file before the governor, taskbar, or GPU start.
- `RestoreOnDrop` restores on every return from `main`. The panic hook restores without taking the journal file lock a second time, so a panic while the lock is held cannot deadlock.
- If `SetWindowsHookExW` fails, startup logs the error, restores, and returns success so the watchdog launches Explorer instead of crash-looping.
- The low-level hook's fail-safe path only sets an atomic and posts `WM_APP + 0x15` to the main thread, then calls `CallNextHookEx`. The main thread pumps messages every frame.
- The watchdog registers the same chord with `RegisterHotKey` on its own thread (`fail_safe_hotkey`, default Ctrl+Alt+Shift+Esc) and waits on the shell process plus `Local\NexusOsHeartbeat`. A missing pulse for 15 seconds, or the hotkey, kills the shell, reverts the journal, and starts Explorer. Config and missing-binary failures restore a stale journal before Explorer as well.
- `--dry-run` logs mutations and does not record or apply them. The watchdog forwards only that flag.

Safe defaults: `app_nap_enabled = false`, `allow_nt_suspend = false`, `dynamic_priority_boost = false`, foreground `AboveNormal`, background `Normal`, empty priority whitelist. Below-normal and idle are clamped away. High and realtime require the whitelist. Boost only raises a process. Hiding a window trims memory and does not nap it. `NtSuspendProcess` runs only from `ProcessHandle::suspend`, and only after `suspend_if_allowed` sees `allow_nt_suspend`.

`os-shield::attach` logs that this build does not install WFP filters. The stub installer is not called.

## Gates

From `nexus-os/`, after the merge resolution and `cargo fmt --all`. Toolchain `PATH=$HOME/.cargo/bin:$PATH`, rustc 1.100.0 (0ef1bc386 2026-11-09), `--offline`. Run on Wed Nov 18 2026 around 20:29 UTC.

`cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.83s
```

Exit 0. No errors.

`cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.39s
```

Exit 0. No warnings.

`cargo fmt --all -- --check`

Exit 0. No diff.

Linux `cargo test --workspace` three times, so the journal proptest is not a one-off. Each run: 58 suites, 485 passed, 0 failed, exit 0.

```
TEST 1 exit 0
test crash_then_restart_restores_original ... ok
    finished in 0.48s
test set_window_pos_on_foreign_windows_is_async ... ok
test nt_suspend_process_is_not_reachable_by_default ... ok
test schema::tests::governor_defaults_cannot_brick ... ok
test schema::tests::shipped_system_toml_uses_safe_governor_defaults ... ok

TEST 2 exit 0
test crash_then_restart_restores_original ... ok
    finished in 0.44s

TEST 3 exit 0
test crash_then_restart_restores_original ... ok
    finished in 0.39s
```

The crash/restart proptest is one test with 64 cases. The suspend-guard tests scan the workspace: `NtSuspendProcess` stays behind `suspend_if_allowed`, and every `SetWindowPos` passes `SWP_ASYNCWINDOWPOS`. `nexus-model` unit tests (88), `nx-ui` (84), `nx-privacy-dash` (37 + 7), and the session binaries (Linux stubs) are included in the 485.

Hot paths do not use `unwrap`, `expect`, or `panic!`. Those calls are in unit tests only. The fail-safe hook does not call `process::exit`.

## Remaining issues

- Linux `cargo test --workspace` compiles every crate because Win32 modules are `#[cfg(windows)]`. The hook, heartbeat, `RegisterHotKey`, and a real restore were not executed on Windows here.
- `os-shield` still does not install telemetry filters. `attach` reports that honestly.
- The journal lock is exclusive. `nexus-os.exe` and `nexus-governor.exe` cannot hold `state.nxjr` at the same time. The in-process governor is the supported path.
- Only `nexus-os.exe` creates `Local\NexusOsHeartbeat`. The watchdog supervises that binary. The compositor and governor daemons restore their own journal on exit and on the next open, and they do not pulse the watchdog.
- `WindowCloak` and `WindowHide` round-trip through the journal and the Win32 mutator. The shell does not cloak or hide foreign windows itself, so those entries are written only if a later caller records them.
- A pid that exits and is reused before restore could receive a replayed priority or cap. Restore treats a dead pid as success and drops the entry. Startup and the watchdog both restore as soon as the shell is gone, which keeps that window short.
- Shipped `timer_resolution_0_5ms` stays true. The request is journaled and cleared on restore (`SetResolution = FALSE` when `previous_100ns` is 0).
