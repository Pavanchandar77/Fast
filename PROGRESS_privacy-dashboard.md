STATUS: DONE

# PROGRESS — leap `privacy-dashboard` (audit L9: indicators + permissions)

Worktree: `/workspace/nx-wt-privacy-dashboard`
Branch: `leap/privacy-dashboard`

## 1. Overview & Architecture

Leap "privacy-dashboard" builds the complete privacy data and view model for Nexus-OS in a dedicated pure Rust crate: `nexus-os/crates/nx-privacy-dash`.

In adherence to the workspace concurrency boundaries:
- We did **NOT** create `nexus-model`, `nexus-journal`, `nx-ui`, or `nx-privacy`.
- Pure logic is fully decoupled from Windows runtime dependencies; all unit, integration, and property tests run cleanly on Linux.
- Windows readers (`CapabilityAccessManager` `ConsentStore`) are isolated behind `#[cfg(windows)]` in a thin reader module.
- All system mutation requests (permission toggle, kill-switch requests, clipboard auto-clear) are modelled as reversible, dry-run-able command objects with deterministic inverse operations (`reverse()`) that can be handed off to `nx-privacy` and journaled safely.

## 2. Core Capabilities Built

### 1. CapabilityAccessManager ConsentStore Parser & Reader (`consent_store/`, `filetime.rs`)
- Full Windows `FILETIME` conversion suite: tick math, Unix millis conversions, active session detection (`LastUsedTimeStart > 0 && (LastUsedTimeStop == 0 || LastUsedTimeStop < LastUsedTimeStart)`), and duration calculation.
- Support for `CapabilityKind`: Camera/Webcam, Microphone, Location, and Custom.
- `AppIdentifier` decoder: handles packaged UWP/MSIX package family names (e.g. `Microsoft.WindowsCamera_8wekyb3d8bbwe`) and non-packaged Win32 application keys with `#` path encoding (e.g. `C:#Program Files#Zoom#bin#Zoom.exe` -> `C:\Program Files\Zoom\bin\Zoom.exe`).
- Multi-format parser (`ConsentStoreParser`):
  - Standard Windows Registry `.reg` exports (supporting `hex(b):` little-endian 8-byte QWORDs, `qword:`, `dword:`, and strings).
  - Structured JSON fixtures for mock and CI pipelines.
- Thin live Windows reader (`ConsentStoreReader`) placed strictly behind `#[cfg(windows)]` querying `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore` via `windows::Win32::System::Registry`.

### 2. Live Hardware Indicator State Machine (`indicator.rs`)
- State machine tracking 4 canonical states per capability: `Idle`, `InUse`, `RecentlyUsed`, and `Blocked`.
- Active session tracking: handles multiple concurrent applications holding the same hardware device (e.g. Zoom and OBS both streaming camera); device remains `InUse` until the last app releases it.
- Configurable decay timers: default 5-second amber/yellow warning for `RecentlyUsed` and red warning for `Blocked` before transitioning to `Idle`.
- Seamless re-opening: an app restarting device access during `RecentlyUsed` immediately transitions back to `InUse`.
- `MenuBarPrivacyIndicators` composite model providing visual chips with icon, color token (Emerald Green for Camera, Amber/Orange for Mic, Blue for Location, Red for Blocked, Muted Gray for Idle), app count badges, and descriptive tooltips.

### 3. Per-App Permission Audit and Toggle Model (`permissions.rs`)
- Capability audit matrix for: Camera, Microphone, Location, Clipboard, Notifications across packaged and non-packaged applications.
- Risk assessment engine scoring apps as `Low`, `Medium`, `High`, or `Critical`:
  - Detects dormant over-privileged applications (>30 days since last hardware access while holding sensitive rights).
  - Flags non-packaged untrusted binaries holding unconstrained hardware or clipboard rights.
- Reversible Command Object (`PermissionToggleCommand`):
  - Carries target app, capability, previous state, desired state, and reason.
  - Generates dry-run execution plans (`DryRunPlan`) showing exact target registry keys and values without touching OS state.
  - Reversible: `cmd.reverse()` inverts desired/previous state; double reversal is an identity operation.
- Privacy audit report generating automated revocation recommendations.

### 4. Sensitive Clipboard Alerts & Auto-Clear Store (`clipboard.rs`)
- Pure store (`ClipboardPrivacyStore`) detecting sensitive copies:
  - API Tokens & Secrets: OpenAI (`sk-...`), Anthropic (`sk-ant-...`), GitHub PATs (`ghp_...`, `github_pat_...`), AWS Access Keys (`AKIA...`), and JWT tokens.
  - Credit Card Numbers: Regex pattern with **Luhn algorithm checksum** validation to prevent false positives.
  - US Social Security Numbers (SSN): SSA area/group/serial validation (filtering 000, 666, 9xx, 00, 0000).
  - Cryptographic Private Keys: PEM header detection (`-----BEGIN [A-Z ]+ PRIVATE KEY-----`).
  - High-entropy Passwords and explicit `password=` / `pwd=` prefixes.
- Exclusions: UUIDs, git commit hashes, and an application whitelist (1Password, Bitwarden, KeePass, KeePassXC, Nexus-OS).
- Read alerts (`ClipboardReadAlert`): emits warnings/critical alerts with redacted snippets whenever unauthorized apps read sensitive clipboard contents.
- Timed auto-clear: produces reversible `ClipboardClearCommand` on TTL expiration (30s for payment cards, 60s for tokens).
- `ClipboardRestoreCommand` allows immediate user undo.

### 5. Network-Per-App View Model (`network.rs`)
- Ingestion of discrete network activity samples (`NetworkSample`) with bytes sent, bytes received, remote IP, destination domain, protocol, disposition (Allowed, BlockedTracker, BlockedTelemetry, BlockedUserRule), and category (Telemetry, Analytics, Ads, Content, System).
- Rollup summaries by app over selectable time windows: `Last5Minutes`, `LastHour`, `Last24Hours`, `AllTime`.
- Destination endpoint tracking (top domains/IPs sorted by total bandwidth).
- Telemetry block statistics: calculates percentage of telemetry/tracking traffic blocked and total bytes saved.
- Time-series sparkline bucketing for dashboard activity graphs.

### 6. Kill-Switch Requests (`kill_switch.rs`)
- Pure data command objects for 6 system-wide master kill switches:
  1. `CameraMaster` (hardware/driver disable)
  2. `MicrophoneMaster` (global audio input mute)
  3. `LocationMaster` (OS location services kill)
  4. `TrackerBlocker` (DNS/WFP ad and tracker filters)
  5. `TelemetryShield` (DiagTrack, dmwappushservice, CEIP, Advertising ID)
  6. `CopilotRecallShield` (Windows 11 Recall AI snapshots)
- Emits dry-runnable, reversible `KillSwitchRequest` objects for execution by `nx-privacy` on the shield branch.

### 7. Dashboard View-Model & Pure Layout Snapshots (`layout.rs`, `view_model.rs`)
- Pure layout engine producing deterministic `LayoutNode` trees with floating-point geometry (`Rect`) and widget primitives (`Header`, `TabBar`, `TabButton`, `Section`, `Card`, `IndicatorChip`, `ToggleSwitch`, `Table`, `TableRow`, `Sparkline`, etc.).
- Complete 5-tab dashboard view-model (`DashboardViewModel`):
  - **Overview**: Summary cards, Hardware chips, Master kill-switch toggles, and Posture recommendations.
  - **Permissions**: Per-app capability matrix with individual toggle switches.
  - **Network**: Sparkline activity graph and per-app bandwidth / telemetry block table.
  - **Clipboard**: Memory protection state, auto-clear countdown, and unauthorized read alert log.
  - **Audit Log**: Timeline of privacy and security events.
- Golden snapshot tests verifying exact indented tree formatting and geometric stability across resolutions.

---

## 3. Universal Verification Gates & Command Outputs

### Gate 1: Windows Cross-Check Compilation
```bash
cargo check --workspace --all-targets --target x86_64-pc-windows-gnu
```
**Output:**
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```
Result: **0 errors**.

### Gate 2: Windows Cross-Target Clippy (-D warnings)
```bash
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```
**Output:**
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.13s
```
Result: **Clean (0 warnings)**.

### Gate 3: Formatting Check
```bash
cargo fmt --all -- --check
```
**Output:** Clean (no diff, returncode 0).

### Gate 4: Test Suite on Linux Host
```bash
cargo test -p nx-privacy-dash
```
**Output:**
```
     Running unittests src/lib.rs (target/debug/build/nx-privacy-dash/d488dfab99ba60cc/out/nx_privacy_dash-d488dfab99ba60cc)

running 37 tests
test clipboard::tests::luhn_check_validation ... ok
test consent_store::parser::tests::parse_empty_and_comments_only ... ok
test consent_store::parser::tests::parse_duration_and_active_transitions ... ok
test consent_store::parser::tests::parse_qword_formats ... ok
test consent_store::parser::tests::parse_non_packaged_deep_path ... ok
test consent_store::tests::parse_json_fixture_success ... ok
test consent_store::tests::recently_used_threshold_calculation ... ok
test filetime::tests::filetime_active_status ... ok
test filetime::tests::filetime_conversions ... ok
test indicator::tests::concurrent_apps_partial_stop ... ok
test consent_store::tests::parse_reg_fixture_success ... ok
test indicator::tests::indicator_blocked_transition_and_decay ... ok
test indicator::tests::indicator_in_use_to_recently_used_then_idle ... ok
test indicator::tests::indicator_idle_to_in_use ... ok
test indicator::tests::menu_bar_privacy_indicators_aggregate ... ok
test indicator::tests::recently_used_reopened_to_in_use ... ok
test kill_switch::tests::all_kill_switch_descriptions ... ok
test kill_switch::tests::kill_switch_request_reversal_and_dry_run ... ok
test layout::tests::rect_geometry_and_inset ... ok
test layout::tests::layout_node_tree_formatting_and_search ... ok
test network::tests::empty_view_model ... ok
test network::tests::network_per_app_aggregation ... ok
test network::tests::sparkline_time_buckets ... ok
test permissions::tests::command_double_reversal_is_identity ... ok
test permissions::tests::packaged_app_risk_assessment ... ok
test permissions::tests::permission_store_audit_and_recommendations ... ok
test permissions::tests::permission_toggle_command_reversibility ... ok
test view_model::tests::dashboard_layout_tree_deterministic_approx_eq ... ok
test network::tests::time_window_filtering ... ok
test view_model::tests::dashboard_tab_switching_layouts ... ok
test view_model::tests::dashboard_layout_tree_overview_snapshot ... ok
test clipboard::tests::password_heuristics ... ok
test clipboard::tests::whitelisted_app_exclusion ... ok
test clipboard::tests::aws_and_jwt_and_pem_detection ... ok
test clipboard::tests::invalid_ssn_rejection ... ok
test clipboard::tests::clipboard_store_alert_and_auto_clear_lifecycle ... ok
test clipboard::tests::sensitive_pattern_detects_tokens_and_cards ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/integration_tests.rs (target/debug/build/nx-privacy-dash/b7bd088f4de5c50c/out/integration_tests-b7bd088f4de5c50c)

running 7 tests
test test_responsive_layout_proportions ... ok
test test_golden_snapshot_layout_tree ... ok
test proptest_filetime_unix_roundtrip ... ok
test proptest_rect_invariants ... ok
test proptest_network_sample_aggregation ... ok
test test_end_to_end_dashboard_scenario ... ok
test proptest_permission_toggle_reversibility ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

   Doc-tests nx_privacy_dash

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
Total tests passed: **44 passed; 0 failed** (exceeds the required ≥40 tests).

### Gate 5: Hot Path Safety
- No `panic!`, `unwrap()`, or `expect()` in runtime code or hot paths.
- Hardcoded regexes in constructors are compile-time constants verified by unit tests; all inputs and runtime values use typed `Result` or `Option` with `unwrap_or` / `unwrap_or_default`.

---

## 4. Worktree Cleanliness & Merging Readiness
- Shared file edits:
  - `nexus-os/Cargo.toml`: Added 1 line (`"crates/nx-privacy-dash"` to workspace members).
  - `nexus-os/Cargo.lock`: Added package entry for `nx-privacy-dash`.
- No other workspace files modified.
- Ready for immediate merge into `nexus-int`.
