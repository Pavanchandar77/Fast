# Nexus-OS status

Nexus-OS is an experimental Rust shell for Windows. It is meant to feel like one product: a tiling window manager, a macOS-style menu bar and dock, a fast Explorer, a privacy dashboard, power-user tools, and a Process Lasso-style governor, all started together and all reversible if something goes wrong.

This document says what each piece does, what Linux tests actually run, and what is only compile-checked for Windows.

## What Nexus does

**Session (`os-session`).** `nexus-os` is the session daemon. On launch it restores any leftover journal, installs the fail-safe keyboard hook, starts the governor and compositor, then brings up nx-shell, Explorer, the privacy dashboard, and power features. If one of those components fails, Nexus logs the error and keeps the session running. A watchdog watches the heartbeat and the fail-safe hotkey (Ctrl+Alt+Shift+Esc) and can put Explorer back.

**Window manager (`nx-wm`, `os-compositor`).** Ordered-tree tiling with dwindle, master-stack, scrolling, and monocle layouts, per-monitor workspaces, window rules, and config-driven keybindings. Foreign windows are moved asynchronously. Off-workspace windows are cloaked. Fullscreen games are left alone.

**Visual shell (`nx-shell`, `nx-ui`), built with GPUI.** The whole user-facing UI is native GPUI (Zed's GPU-accelerated Rust UI framework): no HTML, React, or webview, and no hand-rolled renderer. `nexus-os` starts a `gpui::Application` and opens the menu bar, dock, Spotlight, Mission Control, Explorer, and privacy dashboard as real GPUI windows, while the compositor, governor, hotkeys, and heartbeat run on a background thread at 120 Hz. The fail-safe hotkey restores the session and quits GPUI cleanly. Menu bar with the frontmost app, clock, privacy chips, volume, and clipboard. Dock with magnification springs, running indicators, and jump lists. Mission Control / Exposé with live-thumbnail slots. Spotlight for apps, settings, files, and a calculator. Frame scheduler idles when nothing is animating.

**Explorer (`nx-fs`, `nx-explorer`, `nx-explorer-ui`).** Dual-pane file manager with tabs, virtualized list/grid/details/column views, live folder watching, and a persistent index. File operations that change disk state go through the Explorer journal.

**Smart governor (`nx-governor-policy`, `os-governor`).** A pure policy engine plus a Win32 adapter. It samples real processes through the Platform trait. ProBalance temporarily demotes CPU hogs by one class and restores them. EcoQoS is used for idle background apps. Fullscreen foreground games enter Game Mode, which pauses demotions. App Nap, RAM purge, and 0.5 ms timer resolution stay off unless the operator turns them on. NtSuspendProcess is off by default.

**Privacy (`nx-privacy`, `nx-privacy-dash`, `os-shield`).** A reversible planner for telemetry kill, tracker hosts, kill switches, and WFP rule specs. The dashboard models camera/mic/location indicators, per-app permissions, clipboard alerts, and a network view. Live apply on Windows goes through the Platform trait and the never-brick journal. This build does not install kernel WFP filters.

**Power features (`nx-power`).** Clipboard history with privacy exclusions, always-on-top, snap zones, Quick Look, color picker, batch rename, key remap, per-app volume, notes, window tabs, and Alt-Tab. Always-on-top on Windows is journaled through Platform.

**Never-brick (`nexus-journal`, `os-core` recovery).** Every system mutation is recorded to disk before it is applied: priority, affinity, job caps, EcoQoS, taskbar, app bars, timer resolution, cloak/hide/move, always-on-top, power plan, registry, services, privacy toggles, and system files. Clean exit, panic, fail-safe, watchdog fallback, and the next startup all restore the journal.

**Update resilience (`nx-compat`).** Windows updates arrive every few days, so Nexus checks the Windows build on every start. When the build changes, it re-runs runtime capability probes and switches any feature whose API changed to a fallback instead of failing. Settings an update reset are re-applied from the journal. If explorer.exe or DWM restarts, Nexus re-attaches its hooks, app bars, and windows without restarting the session. Version-sensitive calls go through nx-compat, not straight to Win32.

**Config (`os-config`, `nexus-model`).** TOML schema with hot reload. Governor defaults cannot brick a session: no App Nap, no NtSuspend, no HIGH unless whitelisted, no below-normal background class from the shell adapter.

**HAL and VFS (`os-hal`, `os-vfs`).** Power-profile requests go through Platform. The VFS crate still holds the MFT/USN/Quick Look shape; live USN enumeration is not a completed Windows runtime path.

## Verified by tests on Linux

These suites run on this Linux host. They do not talk to a real Windows kernel.

- Journal record/restore, crash-then-restart identity, dry-run, and CRC handling.
- Grep tests that NtSuspendProcess is not reachable by default, that foreign `SetWindowPos` uses `SWP_ASYNCWINDOWPOS`, and that registry/service/priority/affinity/hosts/always-on-top mutations are not issued outside the Platform and Win32Mutator adapters.
- FakePlatform process snapshots: ProBalance demotes a sustained CPU hog; a fullscreen foreground game (`eldenring.exe`) enters Game Mode and pauses demotions.
- Pure governor policy: ProBalance restore, EcoQoS, boost one class, game-mode pause, process-exit cleanup, safe defaults.
- Layout engines (dwindle, master-stack, scrolling, monocle), workspace cloak, window rules, keybinding parser.
- Shell view-models: menu bar, dock magnification, Exposé grid, Spotlight ranking and calculator, frame scheduler idle.
- Explorer model, virtualized views, watcher debounce, index cache.
- Privacy planner apply/rollback identity on an in-memory FakeSystem, hosts blocking, WFP rule builder, dashboard indicators.
- Power-feature stores: clipboard exclusions, always-on-top plans, notes, volume, snapping, tabs, Alt-Tab.
- Session startup: all four product surfaces come up; an injected Explorer or privacy failure is logged and the session continues.
- Config defaults and the shipped `config/system.toml`.

Workspace result on this host: **689 tests passed, 0 failed**.

## Compile-checked for Windows, not run on real Windows

The workspace cross-checks with `x86_64-pc-windows-gnu`. That proves the Windows code type-checks. It does not run on a Windows machine, so these paths are not executed here:

- Live process sampling (`CreateToolhelp32Snapshot`, foreground HWND, fullscreen metrics).
- Win32Mutator apply/revert: `SetPriorityClass`, job objects, EcoQoS `SetProcessInformation`, DWM cloak, async `SetWindowPos`, `reg.exe` / `sc.exe` / `schtasks.exe`, hosts-file writes.
- Low-level keyboard hook, WinEvent hooks, fail-safe posting, watchdog `RegisterHotKey` and heartbeat.
- Hollow-shell taskbar hide/restore, AppBar registration, `NtSetTimerResolution`.
- GPU compositor (DX12/Vulkan wgpu), DWM Mica/Acrylic, live thumbnails (`DwmRegisterThumbnail`).
- Explorer HWND host (`winit` + swapchain).
- `WindowsSystem` privacy backend (now a thin Platform wrapper).
- Real Explorer.exe relaunch from the watchdog.

Treat those as implemented and compile-clean, not as soak-tested on hardware.
