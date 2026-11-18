use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::error::ShieldError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RegistryHive {
    Hklm,
    Hkcu,
}

impl std::fmt::Display for RegistryHive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hklm => write!(f, "HKLM"),
            Self::Hkcu => write!(f, "HKCU"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RegistryValue {
    Dword(u32),
    Qword(u64),
    Sz(String),
    ExpandSz(String),
    MultiSz(Vec<String>),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ServiceStartType {
    Automatic,
    Manual,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ServiceStatus {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ServiceState {
    pub start_type: ServiceStartType,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskState {
    pub enabled: bool,
}

/// Abstract system backend interface enabling pure logic execution on Linux
/// against in-memory fakes and test fixtures, as well as Windows live execution.
pub trait SystemBackend {
    fn get_registry_value(
        &self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValue>, ShieldError>;

    fn set_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
        value: RegistryValue,
    ) -> Result<(), ShieldError>;

    fn delete_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), ShieldError>;

    fn get_service_state(&self, service_name: &str) -> Result<Option<ServiceState>, ShieldError>;

    fn set_service_state(
        &mut self,
        service_name: &str,
        state: ServiceState,
    ) -> Result<(), ShieldError>;

    fn get_task_state(&self, task_path: &str) -> Result<Option<TaskState>, ShieldError>;

    fn set_task_state(&mut self, task_path: &str, state: TaskState) -> Result<(), ShieldError>;

    fn read_hosts_file(&self) -> Result<String, ShieldError>;

    fn write_hosts_file(&mut self, content: &str) -> Result<(), ShieldError>;
}

/// In-memory fake system for deterministic Linux testing and state verification.
#[derive(Debug, Clone, Eq)]
pub struct FakeSystem {
    pub registry: BTreeMap<(RegistryHive, String, String), RegistryValue>,
    pub services: BTreeMap<String, ServiceState>,
    pub tasks: BTreeMap<String, TaskState>,
    pub hosts_file: String,
    pub mutation_log: Vec<String>,
    pub injected_failures: BTreeSet<String>,
}

impl PartialEq for FakeSystem {
    fn eq(&self, other: &Self) -> bool {
        self.registry == other.registry
            && self.services == other.services
            && self.tasks == other.tasks
            && self.hosts_file == other.hosts_file
    }
}

impl Default for FakeSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeSystem {
    pub fn new() -> Self {
        Self {
            registry: BTreeMap::new(),
            services: BTreeMap::new(),
            tasks: BTreeMap::new(),
            hosts_file: String::new(),
            mutation_log: Vec::new(),
            injected_failures: BTreeSet::new(),
        }
    }

    /// Pre-populates a standard Windows factory-default state where telemetry,
    /// tracking, and device permissions are enabled.
    pub fn windows_default() -> Self {
        let mut sys = Self::new();

        // Standard Windows Telemetry services
        sys.services.insert(
            "DiagTrack".into(),
            ServiceState {
                start_type: ServiceStartType::Automatic,
                status: ServiceStatus::Running,
            },
        );
        sys.services.insert(
            "dmwappushservice".into(),
            ServiceState {
                start_type: ServiceStartType::Automatic,
                status: ServiceStatus::Running,
            },
        );
        sys.services.insert(
            "lfsvc".into(),
            ServiceState {
                start_type: ServiceStartType::Manual,
                status: ServiceStatus::Running,
            },
        );

        // Standard Scheduled Tasks
        let tasks = [
            r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator",
            r"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip",
            r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
            r"\Microsoft\Windows\Application Experience\ProgramDataUpdater",
            r"\Microsoft\Windows\Autochk\Proxy",
            r"\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector",
            r"\Microsoft\Windows\Feedback\Siuf\DmClient",
        ];
        for task in tasks {
            sys.tasks.insert(task.into(), TaskState { enabled: true });
        }

        // Standard Policy Registry values (1 = enabled, or not present by default)
        sys.registry.insert(
            (
                RegistryHive::Hkcu,
                r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo".into(),
                "Enabled".into(),
            ),
            RegistryValue::Dword(1),
        );
        sys.registry.insert(
            (
                RegistryHive::Hklm,
                r"SOFTWARE\Policies\Microsoft\Windows\System".into(),
                "EnableActivityFeed".into(),
            ),
            RegistryValue::Dword(1),
        );

        // Standard Camera / Mic / Location permissions
        sys.registry.insert(
            (
                RegistryHive::Hkcu,
                r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam".into(),
                "Value".into(),
            ),
            RegistryValue::Sz("Allow".into()),
        );
        sys.registry.insert(
            (
                RegistryHive::Hkcu,
                r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone".into(),
                "Value".into(),
            ),
            RegistryValue::Sz("Allow".into()),
        );
        sys.registry.insert(
            (
                RegistryHive::Hkcu,
                r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location".into(),
                "Value".into(),
            ),
            RegistryValue::Sz("Allow".into()),
        );

        // Standard hosts file
        sys.hosts_file = "# Copyright (c) 1993-2009 Microsoft Corp.\n#\n127.0.0.1       localhost\n::1             localhost\n".into();

        sys
    }

    pub fn inject_failure(&mut self, key: &str) {
        self.injected_failures.insert(key.into());
    }

    pub fn clear_failures(&mut self) {
        self.injected_failures.clear();
    }
}

impl SystemBackend for FakeSystem {
    fn get_registry_value(
        &self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValue>, ShieldError> {
        let key = format!("{hive}\\{subkey}\\{value_name}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure reading {key}"
            )));
        }
        Ok(self
            .registry
            .get(&(hive, subkey.to_string(), value_name.to_string()))
            .cloned())
    }

    fn set_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
        value: RegistryValue,
    ) -> Result<(), ShieldError> {
        let key = format!("{hive}\\{subkey}\\{value_name}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure writing {key}"
            )));
        }
        self.mutation_log.push(format!("SET_REG {key} = {value:?}"));
        self.registry
            .insert((hive, subkey.to_string(), value_name.to_string()), value);
        Ok(())
    }

    fn delete_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), ShieldError> {
        let key = format!("{hive}\\{subkey}\\{value_name}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure deleting {key}"
            )));
        }
        self.mutation_log.push(format!("DEL_REG {key}"));
        self.registry
            .remove(&(hive, subkey.to_string(), value_name.to_string()));
        Ok(())
    }

    fn get_service_state(&self, service_name: &str) -> Result<Option<ServiceState>, ShieldError> {
        let key = format!("SERVICE:{service_name}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure querying service {service_name}"
            )));
        }
        Ok(self.services.get(service_name).copied())
    }

    fn set_service_state(
        &mut self,
        service_name: &str,
        state: ServiceState,
    ) -> Result<(), ShieldError> {
        let key = format!("SERVICE:{service_name}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure setting service {service_name}"
            )));
        }
        self.mutation_log
            .push(format!("SET_SERVICE {service_name} = {state:?}"));
        self.services.insert(service_name.to_string(), state);
        Ok(())
    }

    fn get_task_state(&self, task_path: &str) -> Result<Option<TaskState>, ShieldError> {
        let key = format!("TASK:{task_path}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure querying task {task_path}"
            )));
        }
        Ok(self.tasks.get(task_path).copied())
    }

    fn set_task_state(&mut self, task_path: &str, state: TaskState) -> Result<(), ShieldError> {
        let key = format!("TASK:{task_path}");
        if self.injected_failures.contains(&key) {
            return Err(ShieldError::Backend(format!(
                "Injected failure setting task {task_path}"
            )));
        }
        self.mutation_log
            .push(format!("SET_TASK {task_path} = {state:?}"));
        self.tasks.insert(task_path.to_string(), state);
        Ok(())
    }

    fn read_hosts_file(&self) -> Result<String, ShieldError> {
        if self.injected_failures.contains("HOSTS_FILE") {
            return Err(ShieldError::Backend(
                "Injected failure reading hosts file".into(),
            ));
        }
        Ok(self.hosts_file.clone())
    }

    fn write_hosts_file(&mut self, content: &str) -> Result<(), ShieldError> {
        if self.injected_failures.contains("HOSTS_FILE") {
            return Err(ShieldError::Backend(
                "Injected failure writing hosts file".into(),
            ));
        }
        self.mutation_log
            .push(format!("WRITE_HOSTS (len={})", content.len()));
        self.hosts_file = content.to_string();
        Ok(())
    }
}
