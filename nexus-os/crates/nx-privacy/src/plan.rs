use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::backup::{BackupEntry, RollbackReceipt};
use crate::error::ShieldError;
use crate::system::{RegistryHive, RegistryValue, ServiceState, SystemBackend, TaskState};
use crate::wfp::WfpRule;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationStep {
    SetRegistry {
        hive: RegistryHive,
        subkey: String,
        value_name: String,
        target: RegistryValue,
        description: String,
    },
    DeleteRegistry {
        hive: RegistryHive,
        subkey: String,
        value_name: String,
        description: String,
    },
    SetService {
        service_name: String,
        target: ServiceState,
        description: String,
    },
    SetTask {
        task_path: String,
        target: TaskState,
        description: String,
    },
    UpdateHosts {
        new_content: String,
        domains_count: usize,
        description: String,
    },
    InstallWfpRule {
        rule: WfpRule,
        description: String,
    },
}

impl MutationStep {
    pub fn description(&self) -> &str {
        match self {
            Self::SetRegistry { description, .. } => description,
            Self::DeleteRegistry { description, .. } => description,
            Self::SetService { description, .. } => description,
            Self::SetTask { description, .. } => description,
            Self::UpdateHosts { description, .. } => description,
            Self::InstallWfpRule { description, .. } => description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunDiff {
    pub step_index: usize,
    pub description: String,
    pub would_change: bool,
    pub current_state: String,
    pub target_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRunReport {
    pub total_steps: usize,
    pub effective_changes: usize,
    pub noops: usize,
    pub diffs: Vec<DryRunDiff>,
}

impl DryRunReport {
    pub fn is_clean(&self) -> bool {
        self.effective_changes == 0
    }
}

/// A fully reversible plan of mutations against the system state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationPlan {
    pub steps: Vec<MutationStep>,
}

impl MutationPlan {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn push(&mut self, step: MutationStep) {
        self.steps.push(step);
    }

    pub fn extend(&mut self, other: MutationPlan) {
        self.steps.extend(other.steps);
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn describe(&self) -> Vec<String> {
        self.steps
            .iter()
            .map(|s| s.description().to_string())
            .collect()
    }

    /// Computes a dry-run analysis against the backend.
    /// Absolutely NO mutations are performed on the backend.
    pub fn dry_run(&self, backend: &impl SystemBackend) -> Result<DryRunReport, ShieldError> {
        let mut diffs = Vec::with_capacity(self.steps.len());
        let mut effective_changes = 0;
        let mut noops = 0;

        for (idx, step) in self.steps.iter().enumerate() {
            debug!("Dry run step #{idx}: {}", step.description());
            match step {
                MutationStep::SetRegistry {
                    hive,
                    subkey,
                    value_name,
                    target,
                    description,
                } => {
                    let current = backend.get_registry_value(*hive, subkey, value_name)?;
                    let would_change = current.as_ref() != Some(target);
                    if would_change {
                        effective_changes += 1;
                    } else {
                        noops += 1;
                    }
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change,
                        current_state: format!("{current:?}"),
                        target_state: format!("{target:?}"),
                    });
                }
                MutationStep::DeleteRegistry {
                    hive,
                    subkey,
                    value_name,
                    description,
                } => {
                    let current = backend.get_registry_value(*hive, subkey, value_name)?;
                    let would_change = current.is_some();
                    if would_change {
                        effective_changes += 1;
                    } else {
                        noops += 1;
                    }
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change,
                        current_state: format!("{current:?}"),
                        target_state: "<deleted>".into(),
                    });
                }
                MutationStep::SetService {
                    service_name,
                    target,
                    description,
                } => {
                    let current = backend.get_service_state(service_name)?;
                    let would_change = current.as_ref() != Some(target);
                    if would_change {
                        effective_changes += 1;
                    } else {
                        noops += 1;
                    }
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change,
                        current_state: format!("{current:?}"),
                        target_state: format!("{target:?}"),
                    });
                }
                MutationStep::SetTask {
                    task_path,
                    target,
                    description,
                } => {
                    let current = backend.get_task_state(task_path)?;
                    let would_change = current.as_ref() != Some(target);
                    if would_change {
                        effective_changes += 1;
                    } else {
                        noops += 1;
                    }
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change,
                        current_state: format!("{current:?}"),
                        target_state: format!("{target:?}"),
                    });
                }
                MutationStep::UpdateHosts {
                    new_content,
                    domains_count,
                    description,
                } => {
                    let current = backend.read_hosts_file()?;
                    let would_change = current != *new_content;
                    if would_change {
                        effective_changes += 1;
                    } else {
                        noops += 1;
                    }
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change,
                        current_state: format!("length: {} bytes", current.len()),
                        target_state: format!(
                            "length: {} bytes ({} domains)",
                            new_content.len(),
                            domains_count
                        ),
                    });
                }
                MutationStep::InstallWfpRule { rule, description } => {
                    effective_changes += 1;
                    diffs.push(DryRunDiff {
                        step_index: idx,
                        description: description.clone(),
                        would_change: true,
                        current_state: "<uninstalled>".into(),
                        target_state: format!("Rule #{}: {}", rule.rule_id, rule.name),
                    });
                }
            }
        }

        Ok(DryRunReport {
            total_steps: self.steps.len(),
            effective_changes,
            noops,
            diffs,
        })
    }

    /// Applies the mutation plan against the system backend.
    ///
    /// GUARANTEE:
    /// 1. A backup entry is ALWAYS recorded for each step prior to any mutation.
    /// 2. If any step fails (either reading previous state or applying), all
    ///    previously applied changes in this plan are immediately and automatically
    ///    rolled back to prevent inconsistent system state.
    pub fn apply(&self, backend: &mut impl SystemBackend) -> Result<RollbackReceipt, ShieldError> {
        let mut backups = Vec::with_capacity(self.steps.len());

        for step in &self.steps {
            debug!("Applying mutation: {}", step.description());
            let backup = match step {
                MutationStep::SetRegistry {
                    hive,
                    subkey,
                    value_name,
                    target,
                    ..
                } => {
                    let prev = match backend.get_registry_value(*hive, subkey, value_name) {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(Self::rollback_partial(backend, backups, step, e));
                        }
                    };
                    let backup = BackupEntry::RegistryValue {
                        hive: *hive,
                        subkey: subkey.clone(),
                        value_name: value_name.clone(),
                        previous: prev,
                    };
                    if let Err(e) =
                        backend.set_registry_value(*hive, subkey, value_name, target.clone())
                    {
                        return Err(Self::rollback_partial(backend, backups, step, e));
                    }
                    backup
                }
                MutationStep::DeleteRegistry {
                    hive,
                    subkey,
                    value_name,
                    ..
                } => {
                    let prev = match backend.get_registry_value(*hive, subkey, value_name) {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(Self::rollback_partial(backend, backups, step, e));
                        }
                    };
                    let backup = BackupEntry::RegistryValue {
                        hive: *hive,
                        subkey: subkey.clone(),
                        value_name: value_name.clone(),
                        previous: prev,
                    };
                    if let Err(e) = backend.delete_registry_value(*hive, subkey, value_name) {
                        return Err(Self::rollback_partial(backend, backups, step, e));
                    }
                    backup
                }
                MutationStep::SetService {
                    service_name,
                    target,
                    ..
                } => {
                    let prev = match backend.get_service_state(service_name) {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(Self::rollback_partial(backend, backups, step, e));
                        }
                    };
                    let backup = BackupEntry::ServiceState {
                        service_name: service_name.clone(),
                        previous: prev,
                    };
                    if let Err(e) = backend.set_service_state(service_name, *target) {
                        return Err(Self::rollback_partial(backend, backups, step, e));
                    }
                    backup
                }
                MutationStep::SetTask {
                    task_path, target, ..
                } => {
                    let prev = match backend.get_task_state(task_path) {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(Self::rollback_partial(backend, backups, step, e));
                        }
                    };
                    let backup = BackupEntry::TaskState {
                        task_path: task_path.clone(),
                        previous: prev,
                    };
                    if let Err(e) = backend.set_task_state(task_path, *target) {
                        return Err(Self::rollback_partial(backend, backups, step, e));
                    }
                    backup
                }
                MutationStep::UpdateHosts { new_content, .. } => {
                    let prev = match backend.read_hosts_file() {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(Self::rollback_partial(backend, backups, step, e));
                        }
                    };
                    let backup = BackupEntry::HostsFile {
                        previous_content: prev,
                    };
                    if let Err(e) = backend.write_hosts_file(new_content) {
                        return Err(Self::rollback_partial(backend, backups, step, e));
                    }
                    backup
                }
                MutationStep::InstallWfpRule { rule, .. } => BackupEntry::WfpRule {
                    rule_id: rule.rule_id,
                    rule_name: rule.name.clone(),
                },
            };

            info!("Recorded backup for step: {}", backup.description());
            backups.push(backup);
        }

        Ok(RollbackReceipt::new("applied-privacy-plan", backups))
    }

    fn rollback_partial(
        backend: &mut impl SystemBackend,
        backups: Vec<BackupEntry>,
        step: &MutationStep,
        err: ShieldError,
    ) -> ShieldError {
        let partial_receipt = RollbackReceipt::new("emergency-abort-rollback", backups);
        let _ = partial_receipt.rollback(backend);
        ShieldError::Backend(format!(
            "Mutation failed at step '{}': {err}. Rolled back preceding changes.",
            step.description()
        ))
    }
}
