#!/usr/bin/env bash
# Build Nexus-Setup-<version>.exe from Linux (Debian/Ubuntu).
#
#   needs: rustup target x86_64-pc-windows-gnu, mingw-w64, nsis (makensis)
#   usage: packaging/windows/build-installer.sh [version]
#
# The assistant model is taken from $NEXUS_MODEL_FILE, else
# ~/.local/share/Nexus/models/, else downloaded once from Hugging Face.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"            # nexus-os workspace
VERSION="${1:-0.1.0-preview}"
TARGET=x86_64-pc-windows-gnu
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT="${OUT_DIR:-$ROOT/dist}"
STAGE="$OUT/stage"

MODEL_NAME=Qwen3-0.6B-Q4_K_M.gguf
MODEL_BYTES=396705472
MODEL_URL="https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/$MODEL_NAME"
MODEL_SHA256=""   # optional pin; set to verify

TOOLS="${NEXUS_TOOLS_DIR:-$HOME/.cache/nexus-packaging}"
export WINEPREFIX="${WINEPREFIX:-$TOOLS/wineprefix}" WINEDEBUG=-all
SDK_VER=10.0.28000.2957
FXC="$TOOLS/fxc/fxc.exe"
if [ ! -f "$FXC" ]; then
  echo "==> Fetching Microsoft fxc.exe (Windows SDK NuGet $SDK_VER)"
  mkdir -p "$TOOLS/fxc"
  curl -L --fail -o "$TOOLS/sdkcpp.nupkg" \
    "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp/$SDK_VER/microsoft.windows.sdk.cpp.$SDK_VER.nupkg"
  unzip -o -j -q "$TOOLS/sdkcpp.nupkg" \
    "c/bin/10.0.28000.0/x86/fxc.exe" "c/bin/10.0.28000.0/x86/d3dcompiler_47.dll" -d "$TOOLS/fxc"
  rm -f "$TOOLS/sdkcpp.nupkg"
fi

build() {
  ( cd "$ROOT" && RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
      cargo build --release --target "$TARGET" -p os-session --bin nexus-os "$@" )
}

echo "==> Building nexus-os.exe (release, static CRT)"
# gpui only compiles its HLSL shaders on a Windows host. The first pass runs
# gpui's build script (and fails at shaders_bytes.rs on a fresh target dir);
# we then precompile the shaders with fxc and build again.
if ! build; then
  echo "==> Precompiling gpui shaders with fxc (wine)"
  GPUI_SRC=$(cd "$ROOT" && cargo metadata --format-version 1 \
    | python3 -c "import json,sys; print([p['manifest_path'] for p in json.load(sys.stdin)['packages'] if p['name']=='gpui'][0].rsplit('/',1)[0])")
  for d in "$CARGO_TARGET_DIR/$TARGET/release/build"/gpui/*/out "$CARGO_TARGET_DIR/$TARGET/release/build"/gpui-*/out; do
    [ -d "$d" ] && python3 "$HERE/gpui_shaders.py" "$GPUI_SRC" "$d" "$FXC"
  done
  build
fi
EXE="$CARGO_TARGET_DIR/$TARGET/release/nexus-os.exe"

echo "==> Checking imports (only Windows system DLLs allowed)"
# All of these ship in System32 on Windows 10 1709+ (icuuc: Windows ICU, ktmw32: KTM).
SYSTEM_DLLS='^(kernel32|user32|gdi32|advapi32|shell32|ole32|oleaut32|ws2_32|ntdll|bcrypt|bcryptprimitives|crypt32|secur32|userenv|dwmapi|d3d11|d3dcompiler_47|dxgi|dcomp|uxtheme|shlwapi|imm32|winmm|version|comctl32|comdlg32|setupapi|cfgmgr32|powrprof|dxva2|psapi|iphlpapi|dwrite|d2d1|windowscodecs|propsys|opengl32|msvcrt|ncrypt|winhttp|wininet|dbghelp|netapi32|wtsapi32|mf|mfplat|mfreadwrite|hid|ktmw32|icuuc|icuin|icu|api-ms-win-.*|ext-ms-win-.*)\.dll$'
BAD=$(x86_64-w64-mingw32-objdump -p "$EXE" | awk '/DLL Name:/{print tolower($3)}' | grep -Ev "$SYSTEM_DLLS" || true)
if [ -n "$BAD" ]; then
  echo "Non-system DLL imports found: $BAD" >&2
  exit 1
fi

echo "==> Staging"
rm -rf "$STAGE"; mkdir -p "$STAGE/config" "$STAGE/models"
cp "$EXE" "$STAGE/"
x86_64-w64-mingw32-strip "$STAGE/nexus-os.exe" || true
cp "$ROOT/config/system.toml" "$STAGE/config/"

MODEL_SRC="${NEXUS_MODEL_FILE:-$HOME/.local/share/Nexus/models/$MODEL_NAME}"
if [ ! -f "$MODEL_SRC" ]; then
  MODEL_SRC="$OUT/$MODEL_NAME"
  [ -f "$MODEL_SRC" ] || curl -L --fail -o "$MODEL_SRC" "$MODEL_URL"
fi
size=$(stat -c %s "$MODEL_SRC")
[ "$size" = "$MODEL_BYTES" ] || { echo "model size $size != $MODEL_BYTES" >&2; exit 1; }
if [ -n "$MODEL_SHA256" ]; then
  echo "$MODEL_SHA256  $MODEL_SRC" | sha256sum -c -
fi
ln -f "$MODEL_SRC" "$STAGE/models/$MODEL_NAME" 2>/dev/null || cp "$MODEL_SRC" "$STAGE/models/"

NUM=$(echo "$VERSION" | grep -oE '^[0-9]+(\.[0-9]+){0,2}' || echo 0.1.0)
while [ "$(echo "$NUM" | tr -cd . | wc -c)" -lt 3 ]; do NUM="$NUM.0"; done

echo "==> makensis"
mkdir -p "$OUT"
( cd "$HERE" && makensis -V2 -DVERSION="$VERSION" -DVERSION_NUM="$NUM" \
    -DSTAGE="$STAGE" -DOUTFILE="$OUT/Nexus-Setup-$VERSION.exe" nexus.nsi )

ls -la "$OUT/Nexus-Setup-$VERSION.exe"
sha256sum "$OUT/Nexus-Setup-$VERSION.exe" | tee "$OUT/Nexus-Setup-$VERSION.exe.sha256"
