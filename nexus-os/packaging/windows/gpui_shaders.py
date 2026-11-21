#!/usr/bin/env python3
"""Precompile GPUI's DirectX shaders for a Linux -> Windows cross build.

gpui 0.2.2 only runs fxc.exe in its build script when the *host* is Windows.
Cross-compiling from Linux, a release build then fails with a missing
`shaders_bytes.rs`, and a debug build reads the .hlsl files from the build
machine's cargo registry at runtime, so it cannot render on another PC.

This script runs Microsoft's fxc.exe (from the Windows SDK NuGet package,
under wine) with the exact flags gpui's build.rs uses and writes the same
`shaders_bytes.rs` into gpui's OUT_DIR.

usage: gpui_shaders.py <gpui-src-dir> <out-dir> <fxc.exe>
"""
import os
import subprocess
import sys
import tempfile

MODULES = [
    "quad",
    "shadow",
    "path_rasterization",
    "path_sprite",
    "underline",
    "monochrome_sprite",
    "polychrome_sprite",
]


def winpath(p):
    return "Z:" + os.path.abspath(p).replace("/", "\\")


def compile_one(fxc, src, entry, target, tmp):
    out = os.path.join(tmp, f"{entry}.cso")
    cmd = ["wine", fxc, "/nologo", "/T", target, "/E", entry, "/O3",
           "/Fo", winpath(out), winpath(src)]
    env = dict(os.environ, WINEDEBUG="-all")
    r = subprocess.run(cmd, capture_output=True, text=True, env=env)
    if r.returncode != 0 or not os.path.exists(out):
        sys.exit(f"fxc failed for {entry}:\n{r.stdout}\n{r.stderr}")
    with open(out, "rb") as f:
        return f.read()


def main():
    gpui, out_dir, fxc = sys.argv[1:4]
    shaders = os.path.join(gpui, "src/platform/windows/shaders.hlsl")
    emoji = os.path.join(gpui, "src/platform/windows/color_text_raster.hlsl")
    jobs = [(m, shaders) for m in MODULES] + [("emoji_rasterization", emoji)]
    lines = []
    with tempfile.TemporaryDirectory() as tmp:
        for module, src in jobs:
            for stage, target, suffix in (("vertex", "vs_4_1", "VERTEX"),
                                          ("fragment", "ps_4_1", "FRAGMENT")):
                blob = compile_one(fxc, src, f"{module}_{stage}", target, tmp)
                body = ", ".join(f"0x{b:02x}" for b in blob)
                lines.append(f"const {module.upper()}_{suffix}_BYTES: &[u8] = &[{body}];\n")
                print(f"  {module}_{stage}: {len(blob)} bytes")
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "shaders_bytes.rs"), "w") as f:
        f.writelines(lines)


if __name__ == "__main__":
    main()
