# Nexus-OS

Nexus-OS is an experimental Windows subsidiary operating environment written
in Rust. It combines tiling window management, compositor effects, process
governance, filesystem indexing, privacy controls, and hardware power
management behind a single configurable session daemon.

> **Status:** Early-stage software for development and experimentation. Several
> components modify Windows processes, windows, power behavior, and networking.
> Use it only on a system where those changes are acceptable.

## Install on Windows

No Rust, no toolchains, no extra downloads.

1. Download **`Nexus-Setup-0.1.0-preview.exe`** from the
   [v0.1.0-preview release](https://github.com/Pavanchandar77/Fast/releases/tag/v0.1.0-preview)
   (about 400 MB; it includes the offline assistant model).
2. Double-click it. Windows SmartScreen may say *"Windows protected your PC"*
   because the installer is not code-signed yet: click **More info**, then
   **Run anyway**.
3. Click through the installer and leave **Launch Nexus** ticked.

What it does:

- Installs for your user only, with no admin prompt, to
  `%LOCALAPPDATA%\Programs\Nexus`.
- Adds Nexus to the Start Menu, optionally to the desktop, and optionally
  starts it when you sign in (off by default).
- Nexus runs as a normal app on top of Windows. It does **not** replace
  `explorer.exe` or change Winlogon. While it runs it hides the Windows
  taskbar (its dock and Dynamic Island take over) and puts it back when it
  quits.
- Quit Nexus with **Ctrl+Alt+Shift+Esc**. If Nexus was ever force-closed and
  the taskbar stays hidden, run **Start > Nexus > Restore Windows desktop**.
- Uninstall from **Settings > Apps > Installed apps > Nexus**. Settings and
  assistant history in `%LOCALAPPDATA%\Nexus` and `%LOCALAPPDATA%\nexus-os`
  are kept.

Requirements: 64-bit Windows 10 (1709 or newer) or Windows 11, and a GPU with
DirectX 11. The bundled assistant model is Qwen3-0.6B (Apache-2.0); see
`THIRD-PARTY-NOTICES.txt` in the install folder.

To build the installer yourself on Linux, see
[`packaging/windows`](packaging/windows/README.md).

## Features

- Tiling window compositor with spring-based animations and GPU rendering
- Global hotkeys, workspaces, floating windows, and command-bar integration
- Foreground process priority boosting and background resource management
- MFT-based filesystem indexing and QuickLook-style previews
- Privacy and behavioral protection rules
- Power, standby, and Bluetooth policy controls
- Live TOML configuration reloads
- Emergency fail-safe hotkey: `Ctrl+Alt+Shift+Esc`

## Requirements

- Windows 10 or later
- Rust toolchain with Cargo
- A GPU and graphics drivers compatible with `wgpu`
- Administrator privileges may be required for some Windows hooks and system
  controls

## Build and run

From this directory:

```powershell
cargo check
cargo build --release
.\target\release\nexus-os.exe
```

The default session daemon reads `config/system.toml`. If that file is not
present in the working directory, it also checks:

```text
%USERPROFILE%\.config\nexus-os\system.toml
```

You can select a different logging level with `RUST_LOG`:

```powershell
$env:RUST_LOG = "debug"
.\target\release\nexus-os.exe
```

## Daemons

The `os-session` crate provides these binaries:

| Binary | Purpose |
| --- | --- |
| `nexus-os` | Starts the integrated session and event loop |
| `nexus-watchdog` | Watchdog process |
| `nexus-governor` | Process and resource governance |
| `nexus-compositor` | Window management and compositor services |
| `nexus-vfs` | Filesystem indexing and preview services |

Individual binaries can be built with:

```powershell
cargo build --release --bin nexus-compositor
```

## Configuration

Edit [`config/system.toml`](config/system.toml) to configure:

- compositor layout, gaps, decorations, top bar, dock, and command bar
- process priorities, RAM trimming, app nap, and game mode
- VFS volumes and preview limits
- privacy shield and protected paths
- power profile and hardware policies
- keybindings

The running session watches the file and applies supported changes without a
restart.

## Workspace layout

```text
crates/
├── os-core        Shared Windows handles, process, and window primitives
├── os-config      TOML schema and live configuration watcher
├── os-compositor  Window manager, hooks, hotkeys, and GPU compositor
├── os-governor    Process priority and resource governance
├── os-vfs         Filesystem indexing and QuickLook support
├── os-shield      Privacy and behavioral protection
├── os-hal         Power, audio, and hardware controls
└── os-session     Integrated daemon and standalone service binaries
```

## License

Licensed under either of:

- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](https://opensource.org/licenses/MIT)

at your option.
