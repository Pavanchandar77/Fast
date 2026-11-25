# Third-party notices

Nexus itself is licensed MIT OR Apache-2.0. This file lists code and
patterns borrowed from other projects for the v0.3 window, focus, and dock
work. Only MIT, Apache-2.0, or BSD sources were copied. GPL, AGPL, and
non-OSI licenses were read for behaviour only and no code was taken from them.

The on-device assistant model and the crate list are also described in
`packaging/windows/THIRD-PARTY-NOTICES.txt`, which is shipped in the installer.

## PowerToys (MIT)

- Project: <https://github.com/microsoft/PowerToys>
- License: MIT, Copyright (c) Microsoft Corporation
- What we used: the `WindowHelpers.BringToForeground` sequence in
  `src/common/ManagedCommon/WindowHelpers.cs` — a short `SendInput` nudge
  (with a timeout, so a stuck input stack cannot hang the UI thread), then
  `SetForegroundWindow`, then `AttachThreadInput` if Windows still refused.
- Where it lives: `crates/nx-ui/src/platform/focus.rs`

## RetroBar (Apache-2.0)

- Project: <https://github.com/dremin/RetroBar>
- License: Apache License 2.0
- What we used: the auto-hide policy in `RetroBar/Taskbar.xaml.cs`
  (`ShouldAllowAutoHide`, hide delay, do not hide while a menu is open,
  reveal from the screen edge). Rewritten as a pure state machine.
- Where it lives: `crates/nx-shell/src/dock_autohide.rs`

## Studied, not copied

- **Flow Launcher** (MIT, <https://github.com/Flow-Launcher/Flow.Launcher>) —
  launcher activation and single-instance handling. Our Spotlight already
  had equivalent behaviour; no source was copied.
- **gpui-component** (Apache-2.0, <https://github.com/longbridge/gpui-component>) —
  GPUI widget and window-title patterns. Used as a reference for how a
  polished GPUI app structures chrome, not as a source of pasted code.

## Read for behaviour only (no code copied)

These projects are not OSI-permissive, or are copyleft. They were read so
we could understand Windows shell practices, and nothing from them was
copied into this repository:

- Komorebi (Komorebi License 2.0, not OSI)
- GlazeWM (GPL-3.0)
- ExplorerPatcher (GPL-2.0)
- Windhawk (GPL-3.0)
- Seelen-UI (AGPL-3.0)
- Zed application code (GPL-3.0). Zed's Apache-2.0 crates such as gpui are
  used under their own licenses; we already vendor a gpui patch in
  `patches/gpui`.
