# Nexus-OS

Nexus is a macOS-like shell for Windows: monochrome, rounded, and built in
Rust on [GPUI](https://www.gpui.rs/). It gives Windows a dock, Spotlight,
Mission Control, hot corners, Files, and a Privacy Shield, and it is meant
to feel fast and stay privacy-first.

It runs as a normal app on top of Windows. It does **not** replace
`explorer.exe` or change Winlogon.

> **Status:** Preview software (**v0.3.0-preview**). Several components
> modify windows, the taskbar, hooks, power behaviour, and networking. Use
> it only on a system where those changes are acceptable. This build has
> not been confirmed on a real Windows PC by the people who packaged it; if
> something misbehaves, see [Logs and bug reports](#logs-and-bug-reports).

## Install on Windows

No Rust, no toolchains, no extra downloads.

1. Download **`Nexus-Setup-0.3.0-preview.exe`** from the
   [v0.3.0-preview release](https://github.com/Pavanchandar77/Fast/releases/tag/v0.3.0-preview)
   (about 402 MB; it includes the offline assistant model).
2. Double-click it. Windows SmartScreen may say *"Windows protected your PC"*
   because the installer is not code-signed yet: click **More info**, then
   **Run anyway**.
3. Click through the installer and leave **Launch Nexus** ticked.

What it does:

- Installs for your user only, with no admin prompt, to
  `%LOCALAPPDATA%\Programs\Nexus`.
- Adds Nexus to the Start Menu, optionally to the desktop, and optionally
  starts it when you sign in (off by default).
- While it runs it hides the Windows taskbar (the dock and Dynamic Island
  take over) and puts the taskbar back when it quits.
- Quit Nexus with **Ctrl+Alt+Shift+Esc**. If Nexus was ever force-closed and
  the taskbar stays hidden, run **Start > Nexus > Restore Windows desktop**.
- Uninstall from **Settings > Apps > Installed apps > Nexus**. Settings and
  assistant history in `%LOCALAPPDATA%\Nexus` and `%LOCALAPPDATA%\nexus-os`
  are kept.

Start Menu shortcuts:

- **Nexus** — normal launch
- **Nexus (diagnostic)** — console window and verbose logging (`--diag`)
- **Nexus (self-test)** — opens, hides, shows and toggles the shell surfaces,
  then writes a pass/fail report (`--selftest`)
- **Restore Windows desktop** — puts the taskbar back (`--restore`)

Requirements: 64-bit Windows 10 (1709 or newer) or Windows 11, and a GPU
with DirectX 11. The bundled assistant model is Qwen3-0.6B (Apache-2.0);
see `THIRD-PARTY-NOTICES.txt` in the install folder.

## Features

- **Dock** with magnification, drag-to-reorder, a right-click menu, and a
  Recent Items pop-up. Trash opens the Recycle Bin.
- **Spotlight** (`Alt+Space`): Start Menu apps, recent documents, and
  Windows Settings. Type to search; arrows, Tab, Home, End, and Enter
  work. Esc clears the query first, then closes. Clicking outside dismisses
  it.
- **Hot corners** on every monitor (corners shared with another display are
  skipped): top-left Mission Control, top-right Spotlight, bottom-right
  Show Desktop. They stay off for fullscreen apps.
- **Mission Control**: click a tile to focus that window, Esc or a backdrop
  click closes it, arrows and Enter move and pick, hover highlights a tile.
- **Clipboard history** (`Ctrl+Alt+V`, or type `cb` in Spotlight): text
  only, kept in memory for the session. Password-manager clips are skipped.
  Enter copies an entry back to the clipboard.
- **Dynamic Island** with live activities, including camera, microphone, and
  location-in-use indicators, plus the on-device assistant.
- **Files**: live search (type to filter, Ctrl+F, Esc clears), list and
  grid views, and keyboard navigation (arrows, Enter, Backspace, Alt+Left
  / Alt+Right).
- **Privacy Shield**: clickable ENGAGE / DISENGAGE kill switches and
  per-app permission pills, with risk updates and an audit log (in-app
  state).
- **Window chrome**: monochrome traffic lights at the top-left, an edge
  resize band, minimise into the dock, and hide-on-close for Files and
  Privacy (the dock brings the same window back).
- Live reload of `system.toml`. Your own file keeps its keybindings and
  picks up any shipped ones it is missing (including `Alt+Space` and
  `Ctrl+Alt+V`).

On a Linux preview (not on real Windows), a release build of v0.2.1 measured
roughly 2% of one core at idle and about 600 ms to start. Those numbers were
not re-checked on Windows.

## Flags

| Flag | What it does |
| --- | --- |
| `--safe` | No motion overlay, no foreign-window effects, no global keyboard hook, no window-manager tiling. The crash message box is also skipped. `NEXUS_SAFE=1` is the same. |
| `--diag` | Opens a console, turns on verbose logging and per-window startup times, and waits for Enter when Nexus stops. |
| `--selftest` | Runs the built-in checks, writes `selftest.txt`, shows the result, and exits. Also opens a console. |
| `--restore` | Puts back anything a killed session left changed (for example a hidden taskbar) and exits. |

## Logs and bug reports

Release builds have no console, so traces go to
`%LOCALAPPDATA%\Nexus\logs\`:

- `nexus.log` — the session log (rolls over to `nexus.old.log` past 4 MB)
- `crash-*.txt` — written on a panic or native crash, with the reason and a
  backtrace
- `selftest.txt` — the `--selftest` report, with pass/fail and timings

When something is wrong, run **Start > Nexus > Nexus (self-test)** and send
`selftest.txt`. If the test fails or Nexus will not start, send `nexus.log`
and any `crash-*.txt` from the same folder as well.

## Known limits

- This is a preview. The v0.3.0-preview installer has not been confirmed on
  a real Windows PC by the people who packaged it.
- Dock auto-hide logic is in the tree and tested, but it is not turned on
  visually, so the dock does not slide away on its own.
- Edge resize is a thin band around Files and Privacy, not a full system
  frame.
- Focusing a window from Mission Control can be blocked by Windows'
  foreground-window rules.
- Files has no thumbnails; the grid view uses generic icons.
- Clipboard history is text only and is not written to disk.

## Build from source

The released installer is cross-compiled from Linux for
`x86_64-pc-windows-gnu`. The UI is GPUI 0.2.2, with a vendored fork in
[`patches/gpui`](patches/gpui).

On Debian or Ubuntu:

```bash
sudo apt install nsis mingw-w64 wine64 unzip python3
rustup target add x86_64-pc-windows-gnu

packaging/windows/build-installer.sh 0.3.0-preview
# -> dist/Nexus-Setup-0.3.0-preview.exe (+ .sha256)
```

`build-installer.sh` compiles `nexus-os.exe` with a static C runtime,
precompiles GPUI's HLSL shaders with `fxc` under wine when the host is not
Windows, embeds the application manifest (Common Controls 6, PerMonitorV2
DPI, asInvoker, Windows 10/11), and runs
[`packaging/windows/check_exe.sh`](packaging/windows/check_exe.sh)
on the exe. That check fails if a non-system DLL is imported, if a
comctl32 v6-only function is imported statically, or if the manifest is
missing. Details are in [`packaging/windows`](packaging/windows/README.md).

The running app looks for `config/system.toml` next to the exe (the
installer puts it there), then in the working directory, then in
`%USERPROFILE%\.config\nexus-os\system.toml`. The file in this tree is
[`config/system.toml`](config/system.toml). The session watches it and
applies supported changes without a restart.

`RUST_LOG` still works (`debug`, and so on). `--diag` is the easier way to
get a verbose console on a GUI build.

## License and credits

Licensed under either of:

- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](https://opensource.org/licenses/MIT)

at your option.

Third-party code and patterns (PowerToys, RetroBar, the bundled Qwen
model, and others) are listed in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). The installer also
ships `THIRD-PARTY-NOTICES.txt` next to `nexus-os.exe`.
