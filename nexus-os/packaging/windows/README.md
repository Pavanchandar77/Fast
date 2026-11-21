# Windows installer

`build-installer.sh` produces one self-contained `Nexus-Setup-<version>.exe`
(NSIS, per-user, no admin) from a Linux box.

```bash
# Debian/Ubuntu prerequisites
sudo apt install nsis mingw-w64 wine64 unzip python3
rustup target add x86_64-pc-windows-gnu

nexus-os/packaging/windows/build-installer.sh 0.1.0-preview
# -> nexus-os/dist/Nexus-Setup-0.1.0-preview.exe (+ .sha256)
```

What the script does:

1. Builds `nexus-os.exe` in release for `x86_64-pc-windows-gnu` with a static
   C runtime, then fails if the exe imports any DLL that is not part of
   Windows (`objdump -p`). Nothing else ships next to the exe.
2. gpui 0.2.2 only compiles its HLSL shaders when the build *host* is
   Windows. On Linux, `gpui_shaders.py` runs Microsoft's `fxc.exe` (fetched
   once from the Windows SDK NuGet package into `~/.cache/nexus-packaging`,
   run under wine, used at build time only and never shipped) with gpui's own
   flags and writes `shaders_bytes.rs` into gpui's `OUT_DIR`.
3. `crates/os-session/build.rs` embeds `nexus.ico`, version info and
   `nexus.manifest.xml` (PerMonitorV2 DPI, Common Controls 6) with `windres`.
4. Stages the exe, `config/system.toml` and the Qwen3-0.6B Q4_K_M model
   (from `$NEXUS_MODEL_FILE`, `~/.local/share/Nexus/models/`, or a one-time
   download), then runs `makensis nexus.nsi`.

At run time the app finds `config\system.toml` and
`models\Qwen3-0.6B-Q4_K_M.gguf` beside `nexus-os.exe`, so the assistant works
offline right after install with no first-run download.

The installer never replaces the Windows shell: no Winlogon `Shell` value, no
`explorer.exe` changes. Autostart is an opt-in `HKCU\...\Run` entry.

Code signing: the installer and exe are unsigned, so SmartScreen shows
"Windows protected your PC" (More info > Run anyway). With a certificate,
sign both with `osslsigncode` before release.
