STATUS: DONE

Leap `integration-polish` on branch `leap/integration-polish`, created from `nexus-int` (`002d997`). Workspace `nexus-os/`. Toolchain rustc 1.100.0 (0ef1bc386 2026-11-09), host `x86_64-unknown-linux-gnu`, cross target `x86_64-pc-windows-gnu`. Never pushed.

This is the final leap: Nexus now starts as one product. The governor consumes real process samples through the Platform trait, every listed system mutation is journaled, and session startup brings shell, Explorer, privacy dashboard, and power features up with graceful degradation.

## What landed

1. **Smart governor, real samples.** `os-governor` ticks `Platform::get_system_snapshot()` instead of a placeholder 15% CPU snapshot. Windows uses ToolHelp + foreground/fullscreen metrics. Linux tests inject `FakePlatform`. RAM purge also goes through Platform. Tests prove ProBalance demotes `ffmpeg.exe` after two high-CPU ticks, and a fullscreen foreground `eldenring.exe` enters Game Mode and pauses demotions.

2. **Journal + Platform, no bypasses.** Journal variants already covered affinity, always-on-top, power, registry, services, privacy, and system files. Remaining live apply paths (`nx-privacy` `WindowsSystem`, `nx-power` always-on-top, `os-hal` power profiles) now call `Win32Platform`, which records then applies via `Win32Mutator`. `nexus-journal/tests/mutation_bypass.rs` fails the build if later crates call `reg.exe`, `sc.exe`, `SetPriorityClass`, `HWND_TOPMOST`, and related mutation APIs outside the os-core adapters.

3. **Session startup.** `os_session::startup::start_session()` constructs nx-shell (menu bar, dock, Exposé, Spotlight), explorer-ui, the privacy dashboard + shield planner, and power-feature stores. Each starter is panic-caught. One failure logs and continues. `nexus-os` main keeps the handle alive for the session loop.

## Gates

Run from `nexus-os/` with `PATH=$HOME/.cargo/bin:$PATH`. Profiles `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` for the long runs.

### `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`

```
Finished `dev` profile [unoptimized] target(s) in 24.27s
```

Exit 0. 0 errors.

### `cargo clippy --workspace --all-targets -- -D warnings`

```
Finished `dev` profile [unoptimized] target(s) in 6.33s
```

Exit 0. Clean. Also run with `--target x86_64-pc-windows-gnu -- -D warnings`:

```
Finished `dev` profile [unoptimized] target(s) in 3.73s
```

Exit 0.

### `cargo fmt --all -- --check`

Exit 0. No diff.

### `cargo test --workspace`

Exit 0. 80 suites, **689 passed**, 0 failed, 0 ignored.

Includes:

- `fake_platform_probalance_throttles_cpu_hog`
- `fake_platform_fullscreen_foreground_triggers_game_mode`
- `fake_platform_game_mode_pauses_probalance`
- `system_mutations_do_not_bypass_journal_and_platform`
- `start_session_brings_up_all_surfaces`
- `one_component_failing_does_not_take_down_the_session`
- existing never-brick, governor-policy, explorer, privacy, power, shell, and wm suites

## Commits on this branch (`nexus-int..HEAD`)

```
d8aef02 style(os-governor): initialize test GovernorConfig without field reassignment
ec90732 feat(session): start shell, explorer, privacy dashboard, and power features together
cb214d6 feat(never-brick): route remaining system mutations through journal and Platform
ee69d66 style: rustfmt journal, Platform trait, and os-core Win32 adapters
e9b35c3 feat(os-governor): feed real process samples via Platform and test ProBalance
08c6bff feat(os-core): implement extended Platform methods, Win32Mutator handlers, and real process snapshot telemetry
a80afc2 feat(model): extend Platform trait with process sampling, affinity, power, registry, services, privacy, and system file operations
1ed3ca3 feat(journal): add mutation variants for affinity, always-on-top, power, registry, services, privacy, and system files
```

See `STATUS.md` for the product map and the Linux-tested vs Windows-compile-only split.
