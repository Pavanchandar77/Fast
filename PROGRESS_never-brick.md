STATUS: DONE

Leap never-brick (audit L3) is on branch `leap/never-brick`. Nexus records every system mutation before applying it and puts Windows back on a clean exit, a panic, a fail-safe, a hung shell, and the next startup when a stale journal is still on disk.

## What landed

Commits, oldest first:

- `c37556e` feat(journal): add a crash-safe record of system mutations
- `862903b` feat(config): keep governor defaults from bricking Windows
- `41f63cf` feat(core): journal Win32 changes and pulse a main-thread heartbeat
- `a88a612` feat(governor): record priority and nap changes before applying them
- `6270301` feat(compositor): keep foreign moves async and the fail-safe hook tiny
- `9d88d87` feat(session): restore state on exit, panic, and a hung shell

`nexus-journal` is a pure crate (no `windows` dependency). The on-disk format is version 1 (`NXJR` / `NXR1`), CRC32-checked, rewritten by an fsynced temp file and an atomic rename. A torn tail or a bad record is skipped. An unknown version is left untouched. `restore_all` reverts newest-first and shrinks the file after each step.

`SystemMutator` is the small apply/revert trait. `FakeSystem` is the test double. A later `Platform` trait can wrap this; `nexus-model` was not created.

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

From `nexus-os/`, toolchain `PATH=$HOME/.cargo/bin:$PATH`, Rust 1.100, `--offline`.

`cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.02s
```

Exit 0. No errors.

`cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.19s
```

Exit 0. No warnings.

`cargo fmt --all -- --check`

Exit 0. No diff.

Linux tests (`cargo test -p nexus-journal -p os-config --offline`):

```
running 14 tests
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 1 test
test crash_then_restart_restores_original ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s

running 2 tests
test set_window_pos_on_foreign_windows_is_async ... ok
test nt_suspend_process_is_not_reachable_by_default ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

That is 22 tests passed and 0 failed. The proptest is one test with 64 cases. The two integration tests are the crash/restart property and the source scan that `NtSuspendProcess` is not reachable by default and every `SetWindowPos` uses `SWP_ASYNCWINDOWPOS`.

Hot paths do not use `unwrap`, `expect`, or `panic!`. Those calls are in unit tests only. The fail-safe hook does not call `process::exit`.

## Remaining issues

- This machine cannot run the Win32 crates as a Linux host target. `cargo test -p os-core` fails with `cannot find Win32 in windows`. Windows-native `cargo test --workspace` is the CI job that executes those crates. The hook, heartbeat, `RegisterHotKey`, and real restore were not executed on Windows here.
- `os-shield` still does not install telemetry filters. `attach` reports that honestly.
- The journal lock is exclusive. `nexus-os.exe` and `nexus-governor.exe` cannot hold `state.nxjr` at the same time. The in-process governor is the supported path.
- Only `nexus-os.exe` creates `Local\NexusOsHeartbeat`. The watchdog supervises that binary. The compositor and governor daemons restore their own journal on exit and on the next open, and they do not pulse the watchdog.
- `WindowCloak` and `WindowHide` round-trip through the journal and the Win32 mutator. The shell does not cloak or hide foreign windows itself, so those entries are written only if a later caller records them.
- A pid that exits and is reused before restore could receive a replayed priority or cap. Restore treats a dead pid as success and drops the entry. Startup and the watchdog both restore as soon as the shell is gone, which keeps that window short.
- Shipped `timer_resolution_0_5ms` stays true. The request is journaled and cleared on restore (`SetResolution = FALSE` when `previous_100ns` is 0).
