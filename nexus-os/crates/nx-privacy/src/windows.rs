#![cfg(windows)]

//! Live Windows backend. Every mutation is applied through the Platform
//! trait, which records it in the never-brick journal before touching the OS.

use std::path::PathBuf;

use nexus_model::platform::{Platform, RegistryValueData, ServiceRunState};
use os_core::platform::Win32Platform;

use crate::error::ShieldError;
use crate::system::{
    RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus, SystemBackend,
    TaskState,
};

pub struct WindowsSystem {
    hosts_path: PathBuf,
    platform: Win32Platform,
}

impl Default for WindowsSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsSystem {
    pub fn new() -> Self {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let hosts_path = PathBuf::from(system_root).join(r"System32\drivers\etc\hosts");
        Self {
            hosts_path,
            platform: Win32Platform::new(),
        }
    }

    pub fn with_hosts_path(hosts_path: PathBuf) -> Self {
        Self {
            hosts_path,
            platform: Win32Platform::new(),
        }
    }

    fn map_err(err: nexus_model::platform::PlatformError) -> ShieldError {
        ShieldError::Backend(err.to_string())
    }

    fn hive_str(hive: RegistryHive) -> &'static str {
        match hive {
            RegistryHive::Hklm => "HKLM",
            RegistryHive::Hkcu => "HKCU",
        }
    }

    fn to_platform_value(value: &RegistryValue) -> RegistryValueData {
        match value {
            RegistryValue::Dword(v) => RegistryValueData::Dword(*v),
            RegistryValue::Qword(v) => RegistryValueData::Qword(*v),
            RegistryValue::Sz(s) | RegistryValue::ExpandSz(s) => RegistryValueData::Sz(s.clone()),
            RegistryValue::MultiSz(list) => RegistryValueData::Sz(list.join("\n")),
            RegistryValue::Binary(bytes) => RegistryValueData::Binary(bytes.clone()),
        }
    }

    fn from_platform_value(value: RegistryValueData) -> RegistryValue {
        match value {
            RegistryValueData::Dword(v) => RegistryValue::Dword(v),
            RegistryValueData::Qword(v) => RegistryValue::Qword(v),
            RegistryValueData::Sz(s) => RegistryValue::Sz(s),
            RegistryValueData::Binary(bytes) => RegistryValue::Binary(bytes),
        }
    }

    fn to_platform_service(state: ServiceState) -> ServiceRunState {
        if matches!(state.start_type, ServiceStartType::Disabled) {
            ServiceRunState::Disabled
        } else if matches!(state.status, ServiceStatus::Running) {
            ServiceRunState::Running
        } else {
            ServiceRunState::Stopped
        }
    }

    fn from_platform_service(state: ServiceRunState) -> ServiceState {
        match state {
            ServiceRunState::Running => ServiceState {
                start_type: ServiceStartType::Automatic,
                status: ServiceStatus::Running,
            },
            ServiceRunState::Stopped => ServiceState {
                start_type: ServiceStartType::Manual,
                status: ServiceStatus::Stopped,
            },
            ServiceRunState::Disabled => ServiceState {
                start_type: ServiceStartType::Disabled,
                status: ServiceStatus::Stopped,
            },
        }
    }
}

impl SystemBackend for WindowsSystem {
    fn get_registry_value(
        &self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValue>, ShieldError> {
        let value = self
            .platform
            .get_registry_value(Self::hive_str(hive), subkey, value_name)
            .map_err(Self::map_err)?;
        Ok(value.map(Self::from_platform_value))
    }

    fn set_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
        value: RegistryValue,
    ) -> Result<(), ShieldError> {
        self.platform
            .set_registry_value(
                Self::hive_str(hive),
                subkey,
                value_name,
                Self::to_platform_value(&value),
            )
            .map_err(Self::map_err)
    }

    fn delete_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), ShieldError> {
        self.platform
            .delete_registry_value(Self::hive_str(hive), subkey, value_name)
            .map_err(Self::map_err)
    }

    fn get_service_state(&self, service_name: &str) -> Result<Option<ServiceState>, ShieldError> {
        let state = self
            .platform
            .get_service_state(service_name)
            .map_err(Self::map_err)?;
        Ok(state.map(Self::from_platform_service))
    }

    fn set_service_state(
        &mut self,
        service_name: &str,
        state: ServiceState,
    ) -> Result<(), ShieldError> {
        self.platform
            .set_service_state(service_name, Self::to_platform_service(state))
            .map_err(Self::map_err)
    }

    fn get_task_state(&self, task_path: &str) -> Result<Option<TaskState>, ShieldError> {
        let state = self
            .platform
            .get_service_state(&format!("task:{task_path}"))
            .map_err(Self::map_err)?;
        Ok(state.map(|run| TaskState {
            enabled: !matches!(run, ServiceRunState::Disabled),
        }))
    }

    fn set_task_state(&mut self, task_path: &str, state: TaskState) -> Result<(), ShieldError> {
        let run = if state.enabled {
            ServiceRunState::Running
        } else {
            ServiceRunState::Disabled
        };
        self.platform
            .set_service_state(&format!("task:{task_path}"), run)
            .map_err(Self::map_err)
    }

    fn read_hosts_file(&self) -> Result<String, ShieldError> {
        match self
            .platform
            .read_system_file(&self.hosts_path.to_string_lossy())
        {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(_) => Ok(String::new()),
        }
    }

    fn write_hosts_file(&mut self, content: &str) -> Result<(), ShieldError> {
        self.platform
            .write_system_file(&self.hosts_path.to_string_lossy(), content.as_bytes())
            .map_err(Self::map_err)
    }
}
