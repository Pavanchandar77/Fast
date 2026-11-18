use serde::{Deserialize, Serialize};

use crate::error::ShieldError;
use crate::plan::{MutationPlan, MutationStep};
use crate::system::{
    RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus, SystemBackend,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KillSwitchConfig {
    pub camera_blocked: bool,
    pub microphone_blocked: bool,
    pub location_blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrivacyDevice {
    Camera,
    Microphone,
    Location,
}

pub struct KillSwitchPlanner;

impl KillSwitchPlanner {
    pub fn build_plan(
        config: &KillSwitchConfig,
        _backend: &impl SystemBackend,
    ) -> Result<MutationPlan, ShieldError> {
        let mut plan = MutationPlan::new();

        let app_privacy_policy = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
        let consent_store =
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

        // 1. Camera Kill Switch
        let (cam_policy_val, cam_consent_val) = if config.camera_blocked {
            (RegistryValue::Dword(2), RegistryValue::Sz("Deny".into()))
        } else {
            (RegistryValue::Dword(0), RegistryValue::Sz("Allow".into()))
        };
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hklm,
            subkey: app_privacy_policy.into(),
            value_name: "LetAppsAccessCamera".into(),
            target: cam_policy_val,
            description: format!(
                "Camera Policy: set LetAppsAccessCamera to {}",
                if config.camera_blocked {
                    "2 (Deny)"
                } else {
                    "0 (Allow)"
                }
            ),
        });
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hkcu,
            subkey: format!(r"{consent_store}\webcam"),
            value_name: "Value".into(),
            target: cam_consent_val,
            description: format!(
                "Camera ConsentStore: set Value to {}",
                if config.camera_blocked {
                    "Deny"
                } else {
                    "Allow"
                }
            ),
        });

        // 2. Microphone Kill Switch
        let (mic_policy_val, mic_consent_val) = if config.microphone_blocked {
            (RegistryValue::Dword(2), RegistryValue::Sz("Deny".into()))
        } else {
            (RegistryValue::Dword(0), RegistryValue::Sz("Allow".into()))
        };
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hklm,
            subkey: app_privacy_policy.into(),
            value_name: "LetAppsAccessMicrophone".into(),
            target: mic_policy_val,
            description: format!(
                "Microphone Policy: set LetAppsAccessMicrophone to {}",
                if config.microphone_blocked {
                    "2 (Deny)"
                } else {
                    "0 (Allow)"
                }
            ),
        });
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hkcu,
            subkey: format!(r"{consent_store}\microphone"),
            value_name: "Value".into(),
            target: mic_consent_val,
            description: format!(
                "Microphone ConsentStore: set Value to {}",
                if config.microphone_blocked {
                    "Deny"
                } else {
                    "Allow"
                }
            ),
        });

        // 3. Location Kill Switch
        let (loc_policy_val, loc_consent_val) = if config.location_blocked {
            (RegistryValue::Dword(2), RegistryValue::Sz("Deny".into()))
        } else {
            (RegistryValue::Dword(0), RegistryValue::Sz("Allow".into()))
        };
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hklm,
            subkey: app_privacy_policy.into(),
            value_name: "LetAppsAccessLocation".into(),
            target: loc_policy_val,
            description: format!(
                "Location Policy: set LetAppsAccessLocation to {}",
                if config.location_blocked {
                    "2 (Deny)"
                } else {
                    "0 (Allow)"
                }
            ),
        });
        plan.push(MutationStep::SetRegistry {
            hive: RegistryHive::Hkcu,
            subkey: format!(r"{consent_store}\location"),
            value_name: "Value".into(),
            target: loc_consent_val,
            description: format!(
                "Location ConsentStore: set Value to {}",
                if config.location_blocked {
                    "Deny"
                } else {
                    "Allow"
                }
            ),
        });

        // Geolocation Service (lfsvc)
        let loc_service_target = if config.location_blocked {
            ServiceState {
                start_type: ServiceStartType::Disabled,
                status: ServiceStatus::Stopped,
            }
        } else {
            ServiceState {
                start_type: ServiceStartType::Manual,
                status: ServiceStatus::Running,
            }
        };
        plan.push(MutationStep::SetService {
            service_name: "lfsvc".into(),
            target: loc_service_target,
            description: format!(
                "Geolocation Service (lfsvc): set to {}",
                if config.location_blocked {
                    "Disabled/Stopped"
                } else {
                    "Manual/Running"
                }
            ),
        });

        Ok(plan)
    }
}
