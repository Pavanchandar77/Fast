# STATUS: DONE

## Overview: Leap "privacy-shield" (L8)

Built the real, reversible privacy shield engine for Nexus-OS inside a new dedicated crate `nexus-os/crates/nx-privacy`, coupled with a thin adapter in `crates/os-shield`.

All system-state mutations in `nx-privacy` are planned as reversible diffs with guaranteed backup entries recorded before any modification, enabling one-click undo and identity verification (`apply` followed by `rollback` = identity).

The crate has no unconditional Windows dependency, enabling pure logic, planning, dry-runs, and property testing to execute seamlessly on Linux host environments against in-memory fake systems and fixtures.

---

## Deliverables & Key Subsystems

### 1. Reversible Mutation Planner & Backup Guarantee (`plan.rs`, `backup.rs`, `system.rs`)
- **SystemBackend Abstraction**: A trait decoupling system interactions (registry values, service states, scheduled task states, and hosts file content) from underlying OS calls.
- **In-Memory `FakeSystem`**: Deterministic test fake with a Windows factory-default state fixture, mutation logging, and failure injection for simulating atomic rollback on hardware/OS failures.
- **Guarantee of Zero Unbacked Mutations**: Every single applied mutation records a `BackupEntry` capturing the prior state (or explicitly recording `<did not exist>`) before mutating system state.
- **Atomic Transactional Rollback**: If an operation fails mid-plan, preceding steps in that execution are rolled back automatically to prevent system inconsistency.
- **Dry-Run Engine**: Evaluates a plan against the system, computes diffs, distinguishes effective changes from no-ops, and executes zero mutations.

### 2. Telemetry Elimination (`telemetry.rs`)
- **Services**: Plans disablement and stopping of `DiagTrack` (Connected User Experiences and Telemetry) and `dmwappushservice` (WAP Push Routing Service).
- **Scheduled Tasks**: Plans disablement of customer experience and diagnostics tasks:
  - `\Microsoft\Windows\Customer Experience Improvement Program\Consolidator`
  - `\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip`
  - `\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser`
  - `\Microsoft\Windows\Application Experience\ProgramDataUpdater`
  - `\Microsoft\Windows\Autochk\Proxy`
  - `\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector`
  - `\Microsoft\Windows\Feedback\Siuf\DmClient`
- **Registry Policies**:
  - `HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection` (`AllowTelemetry` = 0, `MaxTelemetryAllowed` = 0, `DoNotShowFeedbackNotifications` = 1)
  - `HKLM\SOFTWARE\Policies\Microsoft\WindowsInkWorkspace` (`AllowSuggestedAppsInWindowsInkWorkspace` = 0)
  - `HKLM\SOFTWARE\Policies\Microsoft\Windows\CloudContent` (`DisableTailoredExperiencesWithDiagnosticData` = 1)
- **Advertising ID**:
  - `HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo` (`Enabled` = 0)
  - `HKLM\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo` (`DisabledByGroupPolicy` = 1)
- **Activity Feed**:
  - `HKLM\SOFTWARE\Policies\Microsoft\Windows\System` (`EnableActivityFeed` = 0, `PublishUserActivities` = 0, `UploadUserActivities` = 0)
- **Windows Copilot & Recall**:
  - Copilot: `TurnOffWindowsCopilot` = 1 in both `HKCU` and `HKLM` under `Software\Policies\Microsoft\Windows\WindowsCopilot`
  - Recall (AI data analysis): `DisableAIDataAnalysis` = 1 in `HKLM` and `HKCU` under `SOFTWARE\Policies\Microsoft\Windows\WindowsAI`
- **Telemetry Hosts**: Blocks known Microsoft telemetry endpoints via managed hosts file entries.

### 3. WFP Per-App Firewall Rule Builder (`wfp.rs`)
- **Dynamic Session by Default**: Default engine creates dynamic filtering sessions using `FWPM_SESSION_FLAG_DYNAMIC` (0x00000001), ensuring filters are auto-flushed by the Windows kernel if Nexus crashes or exits.
- **Persistent Rules Opt-In**: Dedicated opt-in constructor and rule flag (`FWPM_SESSION_FLAG_PERSISTENT`, 0x00000002) for user-requested permanent rules.
- **Fluent Rule Builder**: `WfpRuleBuilder` for configuring per-app rules, inbound/outbound direction, permit/block actions, weights, IP/port/protocol conditions, and application paths.
- **Filter Specification Generator**: Generates typed `WfpFilterSpec` structs matching Win32 WFP ALE connect/accept layer structures for inspection and test verification without requiring a live Windows kernel.

### 4. Tracker & Ad Domain Blocking (`hosts.rs`, `trackers.rs`)
- **Maintained Domain Data**: Curated embedded lists of Microsoft telemetry endpoints (`TELEMETRY_HOSTS`) and ad/tracking domains (`AD_AND_TRACKER_HOSTS`).
- **Flexible Parser**: `parse_tracker_list` parses raw hosts entries (`0.0.0.0 domain`, `127.0.0.1 domain`, `::1 domain`), domain lists, comments (`# ...`), whitespace, normalizes lowercase, and strips invalid hostnames and loopbacks.
- **Idempotent Hosts Planner**:
  - Delimited managed block (`# === BEGIN NEXUS SHIELD BLOCKLIST (DO NOT EDIT) ===` ... `# === END NEXUS SHIELD BLOCKLIST ===`).
  - Idempotent: re-running with unchanged domains generates zero mutations (noop).
  - Preserves 100% of custom user entries and comments located before or after the block.
  - Preserves CRLF and LF line ending styles.
  - Clean removal restores original hosts file content.

### 5. Hardware Kill Switches (`killswitch.rs`)
- **Camera**: Sets `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy\LetAppsAccessCamera` = 2 (Deny) and `ConsentStore\webcam\Value` = "Deny".
- **Microphone**: Sets `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy\LetAppsAccessMicrophone` = 2 (Deny) and `ConsentStore\microphone\Value` = "Deny".
- **Location**: Sets `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy\LetAppsAccessLocation` = 2 (Deny), `ConsentStore\location\Value` = "Deny", and disables/stops the Windows Geolocation service (`lfsvc`).
- Fully dry-run-able and reversible back to prior or allowed states.

### 6. Minimal Shared File Edits & `os-shield` Integration
- Modified only `nexus-os/Cargo.toml` (1-line workspace member addition) and `Cargo.lock`.
- Adapted `os-shield/src/lib.rs` and `os-shield/src/wfp.rs` to re-export and connect to `nx_privacy::wfp::WfpEngine` and `nx_privacy::shield::PrivacyShield`.

---

## Universal Gates Verification Results

### Gate 1: Cross-Target Windows Check
Command: `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu`
Output:
```
    Checking nx-privacy v0.1.0 (/workspace/nx-wt-privacy-shield/nexus-os/crates/nx-privacy)
    Checking os-shield v0.1.0 (/workspace/nx-wt-privacy-shield/nexus-os/crates/os-shield)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s
```
Status: **0 errors, 0 warnings**

### Gate 2: Windows Clippy
Command: `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
Output:
```
    Checking nx-privacy v0.1.0 (/workspace/nx-wt-privacy-shield/nexus-os/crates/nx-privacy)
    Checking os-shield v0.1.0 (/workspace/nx-wt-privacy-shield/nexus-os/crates/os-shield)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.98s
```
Status: **Clean (0 warnings)**

### Gate 3: Formatting Check
Command: `cargo fmt --all -- --check`
Output: Clean (exit code 0)
Status: **Pass**

### Gate 4: Test Suite on Linux (Target: ≥40 tests)
Command: `cargo test -p nx-privacy`
Output:
```
     Running tests/test_hosts.rs (target/debug/build/nx-privacy/a23468e56edc5bf8/out/test_hosts-a23468e56edc5bf8)
running 8 tests
test test_hosts_clean_removal ... ok
test test_hosts_append_block_to_standard_file ... ok
test test_hosts_handles_empty_file ... ok
test test_hosts_idempotency_same_domains ... ok
test test_hosts_preserves_crlf_line_endings ... ok
test test_hosts_preserves_user_entries_before_and_after ... ok
test test_hosts_removal_noop_when_no_block_present ... ok
test test_hosts_planner_backend_apply_and_rollback_identity ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_killswitch.rs (target/debug/build/nx-privacy/f052531567c5d3b3/out/test_killswitch-f052531567c5d3b3)
running 4 tests
test test_microphone_kill_switch_blocking ... ok
test test_camera_kill_switch_blocking ... ok
test test_location_kill_switch_blocking_and_service ... ok
test test_kill_switch_all_apply_and_rollback_identity ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_plan.rs (target/debug/build/nx-privacy/a3da9fb01901c086/out/test_plan-a3da9fb01901c086)
running 6 tests
test test_apply_records_some_for_existing_key_and_restores_on_rollback ... ok
test test_apply_records_none_for_missing_key_and_deletes_on_rollback ... ok
test test_dry_run_performs_zero_mutations ... ok
test test_dry_run_identifies_noops ... ok
test test_transactional_abort_and_rollback_on_failure ... ok
test test_backup_entry_descriptions ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_shield.rs (target/debug/build/nx-privacy/49ba71381fadba8a/out/test_shield-49ba71381fadba8a)
running 6 tests
test test_privacy_shield_default_config ... ok
test test_privacy_shield_selective_telemetry_options ... ok
test test_privacy_shield_custom_blocked_domains_included ... ok
test test_privacy_shield_apply_and_rollback_identity ... ok
test test_guarantee_no_change_applied_without_backup_entry ... ok
test test_privacy_shield_dry_run_zero_mutations ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_system.rs (target/debug/build/nx-privacy/57e065f5a98865db/out/test_system-57e065f5a98865db)
running 7 tests
test test_fake_system_hosts_crud ... ok
test test_fake_system_initialization_empty ... ok
test test_fake_system_failure_injection ... ok
test test_fake_system_task_crud ... ok
test test_fake_system_registry_crud ... ok
test test_fake_system_windows_default_fixture ... ok
test test_fake_system_service_crud ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_telemetry.rs (target/debug/build/nx-privacy/0e83549973d00a0b/out/test_telemetry-0e83549973d00a0b)
running 6 tests
test test_telemetry_plan_covers_all_scheduled_tasks ... ok
test test_telemetry_plan_covers_all_required_services ... ok
test test_telemetry_plan_covers_copilot_and_recall ... ok
test test_telemetry_plan_covers_policies_advertising_activity ... ok
test test_telemetry_plan_apply_rollback_identity ... ok
test test_telemetry_apply_disables_services_and_tasks ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_trackers.rs (target/debug/build/nx-privacy/5da5cbfd7b2d5ed2/out/test_trackers-5da5cbfd7b2d5ed2)
running 7 tests
test test_parse_hosts_format_with_ips ... ok
test test_parse_filters_invalid_hostnames_and_localhost ... ok
test test_parse_ignores_comments_and_inline_comments ... ok
test test_parse_plain_domain_list ... ok
test test_shipped_ad_hosts_are_valid ... ok
test test_parse_normalizes_case_and_deduplicates ... ok
test test_shipped_telemetry_hosts_are_valid ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/test_wfp.rs (target/debug/build/nx-privacy/70d2c196eaf6e6d5/out/test_wfp-70d2c196eaf6e6d5)
running 9 tests
test test_wfp_engine_defaults_to_dynamic_session ... ok
test test_wfp_engine_persistent_session_opt_in ... ok
test test_wfp_engine_add_and_remove_rules ... ok
test test_wfp_rule_builder_per_app_outbound_block ... ok
test test_wfp_filter_specs_generation_on_structs ... ok
test test_wfp_rule_builder_persistent_opt_in ... ok
test test_wfp_rule_builder_per_app_inbound_permit ... ok
test test_wfp_rule_builder_validation_empty_name ... ok
test test_wfp_rule_builder_validation_no_conditions ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
Total Tests Passed: **53 passed, 0 failed** (Target: ≥40 tests).

### Gate 5: Hot Paths Code Quality Audit
Command: `grep -rn "unwrap\|expect\|panic" nexus-os/crates/nx-privacy/src/`
Output:
- 0 instances of `.unwrap()`, `.expect()`, or `panic!` across all modules in `nx-privacy/src/`.
- Safe fallbacks only (`.unwrap_or(0)` / `.unwrap_or_else(...)`).

---

## Commit History on `leap/privacy-shield`
1. `20259b9`: `nx-privacy: reversible privacy shield planner, telemetry kill, WFP builder, hosts blocker, kill switches`
2. `b44ca63`: `os-shield: adapt to nx-privacy engine and register in workspace`

## Remaining Issues & Next Steps
- None within scope of L8.
- Integration with L9 `privacy-dashboard` (when ready on its separate branch) will consume `PrivacyShield` and `RollbackReceipt` without any schema conflicts.
