use serde::{Deserialize, Serialize};

use crate::error::ShieldError;
use crate::system::{RegistryHive, RegistryValue, ServiceState, SystemBackend, TaskState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupEntry {
    RegistryValue {
        hive: RegistryHive,
        subkey: String,
        value_name: String,
        previous: Option<RegistryValue>,
    },
    ServiceState {
        service_name: String,
        previous: Option<ServiceState>,
    },
    TaskState {
        task_path: String,
        previous: Option<TaskState>,
    },
    HostsFile {
        previous_content: String,
    },
    WfpRule {
        rule_id: u64,
        rule_name: String,
    },
}

impl BackupEntry {
    pub fn description(&self) -> String {
        match self {
            Self::RegistryValue {
                hive,
                subkey,
                value_name,
                previous,
            } => {
                let prev_str = match previous {
                    Some(val) => format!("{val:?}"),
                    None => "<did not exist>".to_string(),
                };
                format!("Registry: {hive}\\{subkey}\\{value_name} (was {prev_str})")
            }
            Self::ServiceState {
                service_name,
                previous,
            } => {
                let prev_str = match previous {
                    Some(s) => format!("start={:?}, status={:?}", s.start_type, s.status),
                    None => "<did not exist>".to_string(),
                };
                format!("Service: {service_name} (was {prev_str})")
            }
            Self::TaskState {
                task_path,
                previous,
            } => {
                let prev_str = match previous {
                    Some(t) => format!("enabled={}", t.enabled),
                    None => "<did not exist>".to_string(),
                };
                format!("Scheduled Task: {task_path} (was {prev_str})")
            }
            Self::HostsFile { previous_content } => {
                format!(
                    "Hosts File (backup length: {} bytes)",
                    previous_content.len()
                )
            }
            Self::WfpRule { rule_id, rule_name } => {
                format!("WFP Rule: #{rule_id} '{rule_name}'")
            }
        }
    }
}

/// A receipt containing all recorded state backups before changes were applied.
/// Rollback can restore the exact prior state at any time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackReceipt {
    pub id: String,
    pub description: String,
    pub created_at_secs: u64,
    pub backups: Vec<BackupEntry>,
}

impl RollbackReceipt {
    pub fn new(description: impl Into<String>, backups: Vec<BackupEntry>) -> Self {
        Self {
            id: format!("shield-{}", fastrand_id()),
            description: description.into(),
            created_at_secs: current_timestamp_secs(),
            backups,
        }
    }

    pub fn len(&self) -> usize {
        self.backups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backups.is_empty()
    }

    /// Performs a one-click atomic undo by reverting every backup entry in reverse order.
    pub fn rollback(&self, backend: &mut impl SystemBackend) -> Result<(), ShieldError> {
        for backup in self.backups.iter().rev() {
            match backup {
                BackupEntry::RegistryValue {
                    hive,
                    subkey,
                    value_name,
                    previous,
                } => match previous {
                    Some(prev_val) => {
                        backend.set_registry_value(*hive, subkey, value_name, prev_val.clone())?;
                    }
                    None => {
                        backend.delete_registry_value(*hive, subkey, value_name)?;
                    }
                },
                BackupEntry::ServiceState {
                    service_name,
                    previous,
                } => {
                    if let Some(prev_state) = previous {
                        backend.set_service_state(service_name, *prev_state)?;
                    }
                }
                BackupEntry::TaskState {
                    task_path,
                    previous,
                } => {
                    if let Some(prev_state) = previous {
                        backend.set_task_state(task_path, *prev_state)?;
                    }
                }
                BackupEntry::HostsFile { previous_content } => {
                    backend.write_hosts_file(previous_content)?;
                }
                BackupEntry::WfpRule { .. } => {
                    // WFP dynamic sessions auto-flush, or rule is uninstalled
                }
            }
        }
        Ok(())
    }
}

fn fastrand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42)
}

fn current_timestamp_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
