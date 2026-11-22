#!/bin/bash
# Load-time sanity checks for a cross-built Windows exe (run by build-installer.sh).
# Fails if the exe would hit "Entry Point Not Found" / wrong comctl32 at launch:
#  - only Windows system DLLs may be imported;
#  - an application manifest must be embedded with Common-Controls 6.0.0.0,
#    PerMonitorV2 DPI, asInvoker and the Windows 10/11 supportedOS id;
#  - comctl32 v6-only functions may never be imported statically (they are
#    missing from the v5 comctl32 the loader picks whenever the manifest is lost).
set -euo pipefail
EXE="$1"
OBJDUMP=${OBJDUMP:-x86_64-w64-mingw32-objdump}
OBJCOPY=${OBJCOPY:-x86_64-w64-mingw32-objcopy}
fail() { echo "check_exe: $*" >&2; exit 1; }

SYSTEM_DLLS='^(kernel32|user32|gdi32|advapi32|shell32|ole32|oleaut32|ws2_32|ntdll|bcrypt|bcryptprimitives|crypt32|secur32|userenv|dwmapi|d3d11|d3dcompiler_47|dxgi|dcomp|uxtheme|shlwapi|imm32|winmm|version|comctl32|comdlg32|setupapi|cfgmgr32|powrprof|dxva2|psapi|iphlpapi|dwrite|d2d1|windowscodecs|propsys|opengl32|msvcrt|ncrypt|winhttp|wininet|dbghelp|netapi32|wtsapi32|mf|mfplat|mfreadwrite|hid|ktmw32|icuuc|icuin|icu|api-ms-win-.*|ext-ms-win-.*)\.dll$'
DUMP=$("$OBJDUMP" -p "$EXE")
BAD=$(echo "$DUMP" | awk '/DLL Name:/{print tolower($3)}' | grep -Ev "$SYSTEM_DLLS" || true)
[ -z "$BAD" ] || fail "non-system DLL imports: $BAD"

echo "== imports"
echo "$DUMP" | awk '/DLL Name:/{d=tolower($3); n[d]=0} /^\t[0-9a-f]+ +[0-9a-f<]/{if(d!="") n[d]++} END{for(k in n) printf "  %-45s %d\n", k, n[k]}' | sort

V6_ONLY='^(TaskDialog|TaskDialogIndirect|LoadIconMetric|LoadIconWithScaleDown|ImageList_CoCreateInstance|DrawShadowText)$'
STATIC_V6=$(echo "$DUMP" | awk '/DLL Name:/{d=tolower($3)} d=="comctl32.dll" && NF>=3 {print $NF}' | grep -E "$V6_ONLY" || true)
[ -z "$STATIC_V6" ] || fail "static import of comctl32 v6-only function(s): $STATIC_V6 (load them with GetProcAddress)"

TMP=$(mktemp); trap 'rm -f "$TMP"' EXIT
"$OBJCOPY" -O binary --only-section=.rsrc "$EXE" "$TMP" 2>/dev/null || fail "no .rsrc section"
for needle in 'Microsoft.Windows.Common-Controls' 'version="6.0.0.0"' 'PerMonitorV2' 'level="asInvoker"' '8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a'; do
  grep -aqF "$needle" "$TMP" || fail "embedded manifest missing: $needle"
done
echo "== manifest OK (Common-Controls 6, PerMonitorV2, asInvoker, Win10/11)"
