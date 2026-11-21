; Nexus for Windows: one-file, per-user installer (no admin needed).
; Build with packaging/windows/build-installer.sh (runs makensis on Linux).
;
; Nexus runs as a normal app on top of Windows. This installer never replaces
; explorer.exe as the shell and never touches Winlogon.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
SetCompressorDictSize 64

!ifndef VERSION
  !define VERSION "0.1.0-preview"
!endif
!ifndef VERSION_NUM
  !define VERSION_NUM "0.1.0.0"
!endif
!ifndef STAGE
  !define STAGE "stage"
!endif
!ifndef OUTFILE
  !define OUTFILE "Nexus-Setup-${VERSION}.exe"
!endif

!define APP      "Nexus"
!define EXE      "nexus-os.exe"
!define UNINST   "Software\Microsoft\Windows\CurrentVersion\Uninstall\Nexus"
!define RUNKEY   "Software\Microsoft\Windows\CurrentVersion\Run"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Nexus"
InstallDirRegKey HKCU "Software\Nexus" "InstallDir"
RequestExecutionLevel user
BrandingText "Nexus ${VERSION}"

VIProductVersion "${VERSION_NUM}"
VIAddVersionKey "ProductName" "Nexus"
VIAddVersionKey "FileDescription" "Nexus Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!define MUI_ICON   "nexus.ico"
!define MUI_UNICON "nexus.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP   "wizard.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "wizard.bmp"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC

!define MUI_WELCOMEPAGE_TITLE "Install Nexus"
!define MUI_WELCOMEPAGE_TEXT "Nexus runs on top of Windows as a normal app: Dynamic Island, dock, motion, privacy tools and an offline assistant.$\r$\n$\r$\nEverything it needs is inside this installer, including the 400 MB assistant model. No admin rights, no Rust, nothing else to install.$\r$\n$\r$\nQuit Nexus any time with Ctrl+Alt+Shift+Esc."
!insertmacro MUI_PAGE_WELCOME
!define MUI_LICENSEPAGE_TEXT_TOP "Nexus bundles the Qwen3-0.6B model under the Apache License 2.0."
!insertmacro MUI_PAGE_LICENSE "THIRD-PARTY-NOTICES.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Nexus"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Ask a running Nexus to close, then force it, then put back anything it
; changed (hidden taskbar etc.) from its journal.
!macro StopNexus
  nsExec::Exec 'taskkill /IM ${EXE}'
  Sleep 1500
  nsExec::Exec 'taskkill /F /IM ${EXE}'
  Sleep 500
  IfFileExists "$INSTDIR\${EXE}" 0 +2
    nsExec::Exec '"$INSTDIR\${EXE}" --restore'
!macroend

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Nexus needs 64-bit Windows 10 or 11."
    Abort
  ${EndIf}
  SetShellVarContext current
FunctionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "Nexus (required)" SecCore
  SectionIn RO
  !insertmacro StopNexus
  SetOutPath "$INSTDIR"
  File "${STAGE}\${EXE}"
  File "nexus.ico"
  File "THIRD-PARTY-NOTICES.txt"
  File "QWEN3-LICENSE.txt"
  SetOutPath "$INSTDIR\config"
  ; Keep a config the user already edited.
  SetOverwrite off
  File "${STAGE}\config\system.toml"
  SetOverwrite on
  SetOutPath "$INSTDIR\models"
  File "${STAGE}\models\Qwen3-0.6B-Q4_K_M.gguf"
  SetOutPath "$INSTDIR"

  CreateDirectory "$SMPROGRAMS\Nexus"
  CreateShortcut "$SMPROGRAMS\Nexus\Nexus.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\nexus.ico" 0
  CreateShortcut "$SMPROGRAMS\Nexus\Restore Windows desktop.lnk" "$INSTDIR\${EXE}" "--restore" "$INSTDIR\nexus.ico" 0 SW_SHOWNORMAL "" "Put back the Windows taskbar if Nexus was force-closed"
  CreateShortcut "$SMPROGRAMS\Nexus\Uninstall Nexus.lnk" "$INSTDIR\Uninstall.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Nexus" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Nexus"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "Nexus-OS"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\nexus.ico"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "URLInfoAbout" "https://github.com/Pavanchandar77/Fast"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINST}" "EstimatedSize" 430000
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\Nexus.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\nexus.ico" 0
SectionEnd

Section /o "Start Nexus when I sign in" SecAutostart
  WriteRegStr HKCU "${RUNKEY}" "Nexus" '"$INSTDIR\${EXE}"'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore} "Nexus, its settings and the offline assistant model (about 400 MB)."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Put a Nexus shortcut on the desktop."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "Start Nexus on top of Windows each time you sign in. Windows stays your shell; you can turn this off in Task Manager > Startup apps."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro StopNexus
  DeleteRegValue HKCU "${RUNKEY}" "Nexus"
  Delete "$DESKTOP\Nexus.lnk"
  RMDir /r "$SMPROGRAMS\Nexus"

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\nexus.ico"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$INSTDIR\QWEN3-LICENSE.txt"
  Delete "$INSTDIR\models\Qwen3-0.6B-Q4_K_M.gguf"
  RMDir "$INSTDIR\models"
  Delete "$INSTDIR\config\system.toml"
  RMDir "$INSTDIR\config"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "${UNINST}"
  DeleteRegKey HKCU "Software\Nexus"
  ; Settings, journal and assistant history in %LOCALAPPDATA%\Nexus and
  ; %LOCALAPPDATA%\nexus-os are kept, like most apps keep user data.
SectionEnd
