STATUS: DONE

Leap never-brick (audit L3) is on branch `leap/never-brick`. Nexus records every system mutation before applying it and puts Windows back on a clean exit, a panic, a fail-safe, a hung shell, and the next startup when a stale journal is still on disk.

## Merge with nexus-int (`af01e1a`)

Normal merge of `nexus-int` at `af01e1a735d64259080b9c9c48dfad76ea76a28f` (`Merge branch 'leap/wm-core' into nexus-int`) into `leap/never-brick` at `36462ed` (which already contained `nexus-int` `4759b12`). No rebase, no reset, no force. Second parent of this merge is `af01e1a`. That tip already contains smart-governor (`nx-governor-policy`, merge `7aa457c`) and wm-core (`nx-wm`, merge `af01e1a`).

Conflicts were only in the six files named for this leap: `nexus-model/src/config.rs`, `os-compositor/src/hotkeys.rs`, `os-compositor/src/manager.rs`, `os-governor/src/governor.rs`, `os-governor/src/job_object.rs`, `os-session/src/main.rs`. Both sides stay.

Re-read after the resolution: `/workspace/Fast/AUDIT_chief.md` L3 (journal, abort-if-hook-fails, tiny fail-safe plus watchdog `RegisterHotKey`, heartbeat, `SWP_ASYNCWINDOWPOS`, safe defaults, `--dry-run`), L4 (ordered tree, real layouts, workspace cloak/restore, rules, config keybindings), L7 (EcoQoS, ProBalance with restore, one-class foreground boost, game mode, process-exit cleanup, and "all mutations are journaled"). Also `PROGRESS_smart-governor.md` and `PROGRESS_wm-core.md` from the incoming tip.

### Never-brick (L3) kept

- Startup still opens the journal and restores a stale file before the governor, taskbar, or GPU start. `RestoreOnDrop` restores on every return from `main`. The panic hook restores without taking the journal file lock a second time.
- If `SetWindowsHookExW` fails, `GlobalKeyboardHook::start_with_config` returns `Err`. Session `main` logs and returns `Ok(())` so the watchdog launches Explorer instead of crash-looping. The drop guard restores the journal.
- The low-level hook's fail-safe path only stores an atomic and posts `WM_APP + 0x15` to the main thread, then calls `CallNextHookEx`. It does not allocate, restore, or call `process::exit`. The main thread pumps messages and pulses `Local\NexusOsHeartbeat` every frame.
- The watchdog still registers the same chord with `RegisterHotKey` (`fail_safe_hotkey`, default Ctrl+Alt+Shift+Esc) and kills the shell after 15 seconds without a heartbeat, then reverts the journal and starts Explorer.
- Foreign window moves use `WindowHandle::set_rect`, which journals the original rectangle once and calls `SetWindowPos` with `SWP_ASYNCWINDOWPOS`.
- `--dry-run` logs and does not record or apply. Governor plans also honor `GovernorConfig.dry_run` and `MutationPlan.is_dry_run`.
- Safe defaults stay: `app_nap_enabled = false`, `allow_nt_suspend = false`, `dynamic_priority_boost = false`, foreground `AboveNormal`, background `Normal`, empty `priority_whitelist`. Below-normal and idle are clamped away. High and realtime require a whitelist. `NtSuspendProcess` runs only from `ProcessHandle::suspend` after `suspend_if_allowed` sees `allow_nt_suspend`.

### Smart-governor (L7) kept

`nx-governor-policy` is unchanged. Its Linux tests still see ProBalance emit `BelowNormal` and a one-class boost. The Win32 adapter is what enforces L3:

- `GovernorConfig` is the union. New fields (`dry_run`, `whitelist_high_priority`, `eco_qos_enabled` default true, `pro_balance_enabled` default true, `allow_suspend` default false) sit next to `allow_nt_suspend`, `priority_whitelist`, and `dynamic_priority_boost`. `config/system.toml` ships the same values.
- `to_policy_config` sets `boost_foreground` from `dynamic_priority_boost` (off in the shell), `max_boost_class` to `AboveNormal`, and the HIGH whitelist to the union of `priority_whitelist` and `whitelist_high_priority`. `allow_suspend` is `cfg.allow_suspend && cfg.allow_nt_suspend`, so the engine cannot suspend by default.
- `apply_plan` routes every policy mutation through the journal: priority via `journal_priority` (`clamp_priority`, skip a demotion whose request is below Normal so a Normal process is not forced down), EcoQoS via `Mutation::EcoQos` (kind tag 10, journal version stays 1), CPU caps via `JobThrottleManager::throttle_process` (`Mutation::JobCap`), and `ReleaseJob` via `on_process_exit` (`undo_last`).
- EcoQoS apply/revert lives in `os-core` `effects.rs` (`SetProcessInformation` / `ProcessPowerThrottling`). A dead pid is success.
- Foreground changes call `engine.on_foreground_changed`. The policy worker passes `engine.current_foreground_pid()` into the snapshot so a tick does not look like focus loss. The snapshot is still a placeholder (15% CPU, no process samples), so ProBalance and fullscreen game-mode detection wait until real samples exist. Focus changes and process-exit cleanup do run.
- Minimize stays the never-brick path: `GovernorPolicy::evaluate_minimize` with `app_nap_enabled: false`, trim only. `Drop` stops the worker and `execute_plan`s the engine reset, then `RestoreOnDrop` reverts the journal.
- The incoming Win32 job-object (`HANDLE`, `CreateJobObjectW`) was not kept. `os-core` already owns named jobs `Local\NexusOsJob-{pid}` and the journal applies the cap. `job_object.rs` remains the journaled `throttle_process(pid, percent)` wrapper.

### wm-core (L4) kept

`nx-wm` is unchanged. `os-compositor` still drives `nx_wm::WindowManagerModel`.

- `apply_plan` journals cloak (`Mutation::WindowCloak`, `undo_last` on uncloak) and visibility (`Mutation::WindowHide`, `undo_last` on show). Positions update the spring target; `update_animations` calls `set_rect` so the move is journaled and asynchronous.
- Exit, the posted fail-safe, and `HotkeyAction::PanicFailSafe` call `restore_all_on_exit()` (nx-wm exit plan) and then return so `RestoreOnDrop` reverts the rest of the journal. The compositor daemon does the same and does not pulse the watchdog heartbeat.
- `start_with_config` arms the configurable fail-safe chord and loads `nx_wm::KeybindingMap` from `KeybindingsConfig`. A matched binding is swallowed and Win-key-up is masked. Unbound chords, including unbound Windows reserved shortcuts, fall through. Config reload calls `update_keybindings` and retiles the active workspace.
- The dwindle tiny-screen overlap fix in `nexus-model` layout came in with the auto-merge.

Workspace members are the union, including `nexus-journal`, `nx-governor-policy`, and `nx-wm`. Win32-only dependencies stay under `cfg(windows)`.

## Gates

From `nexus-os/`, after the conflict resolution and `cargo fmt --all`. Toolchain `PATH=$HOME/.cargo/bin:$PATH`, rustc 1.100.0 (0ef1bc386 2026-11-09). Profiles `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`. Tests used `--offline`. Run on Wed Nov 18 2026 around 23:09 UTC.

`cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`

```
Finished `dev` profile [unoptimized] target(s) in 27.60s
```

Exit 0. No errors.

`cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`

```
Finished `dev` profile [unoptimized] target(s) in 2.82s
```

Exit 0. No warnings.

`cargo fmt --all -- --check`

Exit 0. No diff.

Linux `cargo test --workspace --offline` three times. Each run: 71 suites, 594 passed, 0 failed, 0 ignored, exit 0.

```
TEST1_EXIT:0
test crash_then_restart_restores_original ... ok
test set_window_pos_on_foreign_windows_is_async ... ok
test nt_suspend_process_is_not_reachable_by_default ... ok
test schema::tests::governor_defaults_cannot_brick ... ok
test schema::tests::shipped_system_toml_uses_safe_governor_defaults ... ok

TEST2:0
TEST3:0
```

The crash/restart proptest is one test with 64 cases. The suspend-guard tests scan the workspace: `NtSuspendProcess` stays behind `suspend_if_allowed` (`if !allow_nt_suspend`), and every `SetWindowPos` passes `SWP_ASYNCWINDOWPOS`. The schema tests lock `allow_nt_suspend: false`, `app_nap_enabled: false`, and `dynamic_priority_boost: false`. `nx-governor-policy` ProBalance tests still expect a `BelowNormal` demotion inside the pure engine; the adapter refuses to apply that class.

## Previous integration (`4759b12`)

The earlier merge of `nexus-int` `4759b12` (`Merge leap/power-features`) is `36462ed`. It brought testable-core, privacy-shield, nx-ui, privacy-dashboard, and power-features. First-leap commits, oldest first: `c37556e`, `862903b`, `41f63cf`, `a88a612`, `6270301`, `9d88d87`. That gate run (around 20:29 UTC the same day) was 58 suites, 485 passed, 0 failed, with the same crash/restart and suspend-guard tests. This merge's 594 count adds the smart-governor and wm-core suites on top of that tree.

## Remaining issues

- Linux `cargo test --workspace` compiles every crate because Win32 modules are `#[cfg(windows)]`. The hook, heartbeat, `RegisterHotKey`, and a real restore were not executed on Windows here.
- The policy worker has no real CPU or process sampler. ProBalance and fullscreen game-mode detection do not run until samples exist. Foreground changes and process-exit cleanup do.
- `dynamic_priority_boost` ships off, so the one-class boost is opt-in. The pure engine can still record a `BelowNormal` ProBalance demotion; the adapter does not apply it. A later restore sees the live class and no-ops when nothing changed.
- `os-shield::attach` still logs that this build does not install WFP filters.
- The journal lock is exclusive. `nexus-os.exe` and `nexus-governor.exe` cannot hold `state.nxjr` at the same time. The in-process governor is the supported path.
- Only `nexus-os.exe` creates `Local\NexusOsHeartbeat`. The compositor and governor daemons restore their own journal on exit and on the next open, and they do not pulse the watchdog.
- A pid that exits and is reused before restore could receive a replayed priority, EcoQoS flag, or cap. Restore treats a dead pid as success and drops the entry. Startup and the watchdog both restore as soon as the shell is gone.
- Shipped `timer_resolution_0_5ms` stays true. The request is journaled and cleared on restore (`SetResolution = FALSE` when `previous_100ns` is 0).
- `nexus-int` moved to `2764e17` (`Merge branch 'leap/explorer-ui' into nexus-int`) after this merge was started. That commit is not a parent of this merge. This commit's second parent is `af01e1a`, the tip named for the merge.
