STATUS: DONE

# PROGRESS — Leap `update-resilience` (Windows Update Resilience, Capability Probing & Self-Healing)

Branch: `leap/update-resilience`  
Workspace: `nexus-os/`  
Toolchain: Rust 1.100.0, target `x86_64-pc-windows-gnu` and host Linux  

---

## 1. Executive Summary & Owner Priority

**Owner Priority**: Nexus must keep working seamlessly through frequent Windows updates (Windows updates every few days). It must never break, crash, or look wrong after an update.

The `update-resilience` leap delivers a complete, resilient compatibility and self-healing subsystem for Nexus-OS via the new workspace crate `nx-compat`:
1. **Windows Build & UBR Detection**: Reads OS version and revision via `RtlGetVersion` and registry (`CurrentBuild`, `UBR`, `DisplayVersion`, `ProductName`). Persists the last-seen build in state dir (`last_seen_build.json`) atomically. Triggers startup self-check on build changes.
2. **Dynamic Capability Probing instead of Version Checks**: Replaced fragile OS version checks and static imports with dynamic runtime probing (`GetProcAddress`, feature tests). 21 version-sensitive features (DWM Mica/Acrylic/rounded corners/backdrop, ntdll process controls, shell hooks, taskbar suppression, registry and service controls) operate behind probes with documented graceful fallbacks. Safe dispatch functions (`safe_nt_*`, `safe_apply_dwm_attributes`, `safe_set_composition_attribute`) ensure safe invocation.
3. **Self-Healing After Updates**: On build upgrades, if a capability regresses or fails a probe, only that specific feature is disabled or degraded; the shell keeps running normally. Non-blocking notices are emitted. When subsequent updates restore capability, features automatically re-enable.
4. **Never-Brick Guarantee & Setting Re-application**: Settings commonly reset by Windows updates (privacy/telemetry toggles, taskbar/explorer auto-hide, default browser/handlers) are verified against the system state. Any reset setting is re-applied through the never-brick journal. If a re-apply fails, changes roll back immediately via the journal, ensuring Windows is never left in an inconsistent or broken state.
5. **Mid-Session Restart Resilience**: Mid-session crashes or restarts of `explorer.exe` (detected via registered `TaskbarCreated` window message) and `dwm.exe` (detected via `WM_DWMCOMPOSITIONCHANGED`) are trapped automatically to re-attach hooks, re-suppress the native taskbar, and restore desktop compositing without user intervention.
6. **Additive & Non-Conflicting Changes**: Designed to respect the concurrent `integration-polish` leap. Edits to `crates/os-core/src/effects.rs` were strictly additive (2 single-line match arms for `Mutation::Setting`), and `crates/os-core/src/platform.rs` and `crates/os-governor/src/governor.rs` were left untouched.

---

## 2. Universal Gate Results

All four required gates pass cleanly from `nexus-os/` with `PATH=$HOME/.cargo/bin:$PATH`:

### Gate 1: Windows Cross-Check
```bash
cargo check --workspace --target x86_64-pc-windows-gnu
```
**Status: PASS (0 errors)**

### Gate 2: Windows Clippy
```bash
cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings
```
**Status: PASS (Clean, 0 warnings)**

### Gate 3: Code Formatting Check
```bash
cargo fmt --check
```
**Status: PASS (Clean, 0 diffs)**

### Gate 4: Linux Workspace Test Suite
```bash
cargo test --workspace
```
**Status: PASS (698 passed; 0 failed; 80 test suites green)**

---

## 3. Architecture & Implementation Details

### A. New Crate: `nx-compat` (`crates/nx-compat`)
- **`build.rs`**: `WindowsBuildInfo` structure capturing `major`, `minor`, `build`, `ubr`, `display_version`, and `edition`. Implements `BuildSource` trait (`LiveBuildSource` reading `RtlGetVersion` + `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`, and `MockBuildSource` for zero-IO testing).
- **`store.rs`**: `BuildHistoryStore` tracking build transitions (`FreshInstall`, `Unchanged`, `BuildChanged { previous, current }`). Uses atomic write (tempfile + sync + rename) to `<state_dir>/last_seen_build.json`.
- **`probe.rs`**: Comprehensive `Capability` enum covering 21 APIs and subsystem integrations. `ProbeManager` maintains cached probe statuses (`Available`, `Degraded { fallback, reason }`, `Unavailable { reason }`). Every capability defines an explicit, documented fallback.
- **`dispatch.rs`**: Safe wrapper functions that verify probe status before invoking dynamically resolved function pointers:
  - `safe_nt_suspend_process` / `safe_nt_resume_process` / `safe_nt_terminate_process`
  - `safe_nt_set_timer_resolution`
  - `safe_apply_dwm_attributes` (handles Windows 11 22H2+ `DWMWA_SYSTEMBACKDROP_TYPE`, Windows 11 21H2 `DWMWA_MICA_EFFECT`, rounded corner preference, and fallback to Acrylic or solid blur-free background)
  - `safe_set_window_cloak` / `safe_set_dwm_transitions` / `safe_set_composition_attribute`
  - `safe_register_shell_hook`
- **`notice.rs`**: Non-blocking `ResilienceNotice` queue with severities (`Info`, `Warning`, `FeatureDegraded`, `SettingRestored`). Allows the UI/compositor to query and display subtle notifications without interrupting the user.
- **`self_heal.rs`**: `SelfHealingEngine` coordinating startup verification and update resilience:
  - Detects build upgrades and evaluates capability regressions.
  - Verifies resilient settings (`ResilienceSetting`: privacy toggles, explorer hiding, default apps) against actual OS state via `Platform`.
  - Re-applies reset settings and records mutations into `Journal`.
  - On re-apply failure, rolls back the journal record immediately, guaranteeing Windows is never bricked.
- **`restart.rs`**: `RestartHandler` monitoring system events:
  - `RegisterWindowMessageW("TaskbarCreated")` to catch `explorer.exe` restarts and re-apply taskbar suppression and AppBar registrations.
  - `WM_DWMCOMPOSITIONCHANGED` to re-probe DWM features and restore backdrops.

### B. Workspace Integration & Routing
- **`nexus-model`**: Extended `Platform` trait with additive registry querying (`get/set_registry_dword`, `get/set_registry_string`), service querying (`get/set_service_status`), and `is_taskbar_visible()`. Implemented on `FakePlatform`.
- **`nexus-journal`**: Added `Mutation::Setting { key, previous, new }` with binary serialization/deserialization for atomic journaling and undo operations.
- **`os-core`**:
  - `ntdll.rs`: Replaced static IAT linkages with dynamically probed calls via `nx_compat::dispatch`.
  - `hollow_shell.rs`: Guarded taskbar hiding with `Capability::TaskbarSuppression`.
  - `window.rs`: Routed DWM window transitions through `safe_set_dwm_transitions`.
  - `effects.rs`: Added 2 minimal match arms for `Mutation::Setting` to avoid conflicts with `integration-polish`.
- **`nx-ui`**: Window backdrop selection in `platform/win.rs` routed through `safe_apply_dwm_attributes` with Acrylic / solid fallbacks.
- **`nx-shell`**: Guarded backdrop rendering and DWM thumbnail registrations with runtime capability probes.
- **`os-compositor`**: Added message hook support in `hotkeys.rs` to route `TaskbarCreated` and `WM_DWMCOMPOSITIONCHANGED` from `pump_messages()`.
- **`os-session`**: Hooked startup self-healing check and runtime restart handlers in `main.rs`.

---

## 4. Capability Probe Matrix & Graceful Fallbacks

| Capability | Probe Method | Available On | Documented Graceful Fallback |
|---|---|---|---|
| `DwmMicaBackdrop` | Probe `DwmSetWindowAttribute` with `DWMWA_SYSTEMBACKDROP_TYPE=2` | Win 11 22H2+ (Build 22621+) | Fallback to `DwmAcrylicBackdrop` or solid blur-free theme |
| `DwmMicaAltBackdrop` | Probe `DwmSetWindowAttribute` with `DWMWA_SYSTEMBACKDROP_TYPE=4` | Win 11 22H2+ (Build 22621+) | Fallback to standard Mica or solid theme |
| `DwmAcrylicBackdrop` | Probe `DwmSetWindowAttribute` with `DWMWA_SYSTEMBACKDROP_TYPE=3` | Win 11 22H2+ | Fallback to solid blur-free theme |
| `DwmMicaLegacy` | Probe `DwmSetWindowAttribute` with `DWMWA_MICA_EFFECT=1029` | Win 11 21H2 (Build 22000) | Fallback to solid theme |
| `DwmRoundedCorners` | Probe `DwmSetWindowAttribute` with `DWMWA_WINDOW_CORNER_PREFERENCE=33` | Win 11+ (Build 22000+) | Standard rectangular borders, no crash |
| `DwmWindowCloak` | Probe `DwmSetWindowAttribute` with `DWMWA_CLOAK=13` | Win 8+ | Fallback to `ShowWindow(SW_HIDE)` |
| `DwmTransitions` | Probe `DwmSetWindowAttribute` with `DWMWA_TRANSITIONS_FORCEDISABLED=3` | Win 7+ | Ignore transition toggle, no crash |
| `DwmThumbnail` | Probe `DwmRegisterThumbnail` in `dwmapi.dll` | Win Vista+ | Omit live preview thumbnail |
| `NtSuspendProcess` | Dynamically resolve `NtSuspendProcess` export in `ntdll.dll` | NTDLL export | Skip process freeze, log notice |
| `NtResumeProcess` | Dynamically resolve `NtResumeProcess` export in `ntdll.dll` | NTDLL export | Skip resume, log notice |
| `NtTerminateProcess` | Dynamically resolve `NtTerminateProcess` export in `ntdll.dll` | NTDLL export | Fallback to Win32 `TerminateProcess` |
| `NtSetTimerResolution` | Dynamically resolve `NtSetTimerResolution` export in `ntdll.dll` | NTDLL export | Fallback to standard multimedia timer |
| `SetWindowCompositionAttribute` | Dynamically resolve `SetWindowCompositionAttribute` in `user32.dll` | Undocumented user32 | Fallback to DWM public attributes or solid styling |
| `ShellHook` | Dynamically resolve `RegisterShellHookWindow` in `user32.dll` | Win32 Shell API | Fallback to active polling of foreground window |
| `TaskbarSuppression` | Probe `FindWindowW("Shell_TrayWnd")` and message responses | Explorer shell | Gracefully skip taskbar suppression |
| `RegistryPolicies` | Probe access to `HKLM\SOFTWARE\Policies\Microsoft\Windows` | Registry hive | Skip policy setting re-application, warn user |
| `ServiceControl` | Probe `Advapi32.dll` Service Manager functions | Service control manager | Skip service reconfiguration, log notice |
| `PrivacyToggles` | Probe telemetry and advertising registry keys | Registry paths | Degrade privacy toggle enforcement |
| `DefaultAppAssociations` | Probe `IApplicationAssociationRegistration` in shell32 | COM Shell interfaces | Notify user with link to Settings app |
| `EcoQosPowerThrottling` | Probe `SetProcessInformation(ProcessPowerThrottling)` | Win 11 / Win 10 2004+ | Fallback to `IDLE_PRIORITY_CLASS` |
| `VirtualDesktopPinning` | Probe `IVirtualDesktopManager` COM interface | Win 10+ | Omit virtual desktop pinning |

---

## 5. Test Coverage & Verification

### Unit and Integration Tests in `nx-compat`
1. **`test_simulated_build_upgrade_workflow`**: Simulates build upgrade from Windows 11 23H2 (22631.3000) to 24H2 (26100.1). Verifies build transition detection, startup self-check execution, and notice generation.
2. **`test_missing_exports_and_failing_probes_graceful_fallbacks`**: Simulates an update removing or regressing `DwmMicaBackdrop` and `NtSuspendProcess`. Verifies probe status degrades to `Degraded`/`Unavailable`, graceful fallbacks are engaged, dispatch safely rejects invalid calls, and zero panics occur.
3. **`test_reset_settings_verified_and_healed_with_journal_consistency`**: Simulates Windows update resetting telemetry to `3` and taskbar to visible. Verifies `SelfHealingEngine` detects reset settings, re-applies desired values through `Platform`, and writes valid entries to the journal.
4. **`test_reapply_failure_rolls_back_journal_never_brick`**: Simulates platform failure when re-applying a setting. Verifies error is caught, journal record is rolled back, non-blocking warning notice is queued, and system state is not bricked.
5. **`test_mid_session_explorer_and_dwm_restarts`**: Simulates `explorer.exe` restart (`TaskbarCreated` message) and `dwm.exe` restart (`WM_DWMCOMPOSITIONCHANGED`). Verifies automatic re-probe and re-application of taskbar suppression without user intervention.
6. **`store::tests::test_build_store_fresh_unchanged_changed_cycle`**: Verifies atomic JSON persistence and transition classification.
7. **`notice::tests::test_notice_queue_lifecycle`**: Verifies severity ordering, queue capacity, and drain semantics.

---

## 6. Verification Summary

| Metric | Target | Result | Status |
|---|---|---|---|
| Windows Compilation (`x86_64-pc-windows-gnu`) | 0 errors | 0 errors | **PASS** |
| Clippy Warnings (`--target x86_64-pc-windows-gnu`) | 0 warnings | 0 warnings | **PASS** |
| Formatting (`cargo fmt --check`) | Clean | Clean | **PASS** |
| Workspace Tests (`cargo test --workspace`) | All passing | 698 passed, 0 failed (80 suites) | **PASS** |
| `nx-compat` Tests | All passing | 18 passed, 0 failed | **PASS** |
| Concurrent Leap Compatibility | Minimal additive edits | 2 lines in `effects.rs`, 0 in `platform.rs`/`governor.rs` | **PASS** |
| Git Branch & Commit Policy | `leap/update-resilience`, never push | Followed strictly | **PASS** |
