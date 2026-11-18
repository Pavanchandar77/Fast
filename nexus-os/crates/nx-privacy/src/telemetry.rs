use serde::{Deserialize, Serialize};

use crate::error::ShieldError;
use crate::hosts::HostsPlanner;
use crate::plan::{MutationPlan, MutationStep};
use crate::system::{
    RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus, SystemBackend,
    TaskState,
};
use crate::trackers::TELEMETRY_HOSTS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryKillOptions {
    pub disable_services: bool,
    pub disable_scheduled_tasks: bool,
    pub disable_telemetry_policies: bool,
    pub disable_advertising_id: bool,
    pub disable_activity_history: bool,
    pub disable_copilot: bool,
    pub disable_recall: bool,
    pub block_telemetry_hosts: bool,
}

impl Default for TelemetryKillOptions {
    fn default() -> Self {
        Self {
            disable_services: true,
            disable_scheduled_tasks: true,
            disable_telemetry_policies: true,
            disable_advertising_id: true,
            disable_activity_history: true,
            disable_copilot: true,
            disable_recall: true,
            block_telemetry_hosts: true,
        }
    }
}

pub const TELEMETRY_SERVICES: &[&str] = &["DiagTrack", "dmwappushservice"];

pub const TELEMETRY_SCHEDULED_TASKS: &[&str] = &[
    r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator",
    r"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip",
    r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
    r"\Microsoft\Windows\Application Experience\ProgramDataUpdater",
    r"\Microsoft\Windows\Autochk\Proxy",
    r"\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector",
    r"\Microsoft\Windows\Feedback\Siuf\DmClient",
];

pub struct TelemetryPlanner;

impl TelemetryPlanner {
    /// Builds a completely reversible plan to eliminate Windows telemetry.
    /// Every single change records a backup entry to restore previous state.
    pub fn build_plan(
        options: &TelemetryKillOptions,
        backend: &impl SystemBackend,
    ) -> Result<MutationPlan, ShieldError> {
        let mut plan = MutationPlan::new();

        // 1. Services: DiagTrack and dmwappushservice
        if options.disable_services {
            let disabled_stopped = ServiceState {
                start_type: ServiceStartType::Disabled,
                status: ServiceStatus::Stopped,
            };
            for service in TELEMETRY_SERVICES {
                plan.push(MutationStep::SetService {
                    service_name: service.to_string(),
                    target: disabled_stopped,
                    description: format!("Disable and stop telemetry service: {service}"),
                });
            }
        }

        // 2. Scheduled Tasks
        if options.disable_scheduled_tasks {
            let task_disabled = TaskState { enabled: false };
            for task in TELEMETRY_SCHEDULED_TASKS {
                plan.push(MutationStep::SetTask {
                    task_path: task.to_string(),
                    target: task_disabled,
                    description: format!("Disable telemetry scheduled task: {task}"),
                });
            }
        }

        // 3. Policy Registry Keys: DataCollection, InkWorkspace, CloudContent
        if options.disable_telemetry_policies {
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection".into(),
                value_name: "AllowTelemetry".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Set AllowTelemetry to 0 (Security level)".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection".into(),
                value_name: "MaxTelemetryAllowed".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Set MaxTelemetryAllowed to 0".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection".into(),
                value_name: "DoNotShowFeedbackNotifications".into(),
                target: RegistryValue::Dword(1),
                description: "Policy: Suppress Windows diagnostic feedback notifications".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\WindowsInkWorkspace".into(),
                value_name: "AllowSuggestedAppsInWindowsInkWorkspace".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Disable suggested apps in Windows Ink Workspace".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\CloudContent".into(),
                value_name: "DisableTailoredExperiencesWithDiagnosticData".into(),
                target: RegistryValue::Dword(1),
                description: "Policy: Disable tailored experiences using diagnostic data".into(),
            });
        }

        // 4. Advertising ID
        if options.disable_advertising_id {
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hkcu,
                subkey: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo".into(),
                value_name: "Enabled".into(),
                target: RegistryValue::Dword(0),
                description: "Disable user Advertising ID tracking".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo".into(),
                value_name: "DisabledByGroupPolicy".into(),
                target: RegistryValue::Dword(1),
                description: "Policy: Lock Advertising ID as disabled system-wide".into(),
            });
        }

        // 5. Activity History / Feed
        if options.disable_activity_history {
            let sys_policy = r"SOFTWARE\Policies\Microsoft\Windows\System";
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: sys_policy.into(),
                value_name: "EnableActivityFeed".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Disable Windows timeline and activity feed".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: sys_policy.into(),
                value_name: "PublishUserActivities".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Stop publishing user activity history".into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: sys_policy.into(),
                value_name: "UploadUserActivities".into(),
                target: RegistryValue::Dword(0),
                description: "Policy: Prevent uploading activity feed to cloud".into(),
            });
        }

        // 6. Copilot & Recall Toggles
        if options.disable_copilot {
            let copilot_key = r"Software\Policies\Microsoft\Windows\WindowsCopilot";
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hkcu,
                subkey: copilot_key.into(),
                value_name: "TurnOffWindowsCopilot".into(),
                target: RegistryValue::Dword(1),
                description: "HKCU: Disable Windows Copilot assistant (TurnOffWindowsCopilot = 1)"
                    .into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: copilot_key.into(),
                value_name: "TurnOffWindowsCopilot".into(),
                target: RegistryValue::Dword(1),
                description:
                    "HKLM: Lock Windows Copilot disabled by policy (TurnOffWindowsCopilot = 1)"
                        .into(),
            });
        }

        if options.disable_recall {
            let ai_policy = r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI";
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hklm,
                subkey: ai_policy.into(),
                value_name: "DisableAIDataAnalysis".into(),
                target: RegistryValue::Dword(1),
                description:
                    "HKLM: Disable Windows Recall AI screen snapshots (DisableAIDataAnalysis = 1)"
                        .into(),
            });
            plan.push(MutationStep::SetRegistry {
                hive: RegistryHive::Hkcu,
                subkey: ai_policy.into(),
                value_name: "DisableAIDataAnalysis".into(),
                target: RegistryValue::Dword(1),
                description:
                    "HKCU: Disable Windows Recall for active user (DisableAIDataAnalysis = 1)"
                        .into(),
            });
        }

        // 7. Known Telemetry Hosts
        if options.block_telemetry_hosts {
            let hosts_domains: Vec<String> =
                TELEMETRY_HOSTS.iter().map(|&s| s.to_string()).collect();
            if let Some(step) = HostsPlanner::plan_update(backend, &hosts_domains)? {
                plan.push(step);
            }
        }

        Ok(plan)
    }
}
