//! Reversible system state mutations, mutation plans, and simulated verification state.

use crate::types::{EcoQosState, Pid, PriorityClass};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An individual, reversible system mutation emitted by the governor policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyMutation {
    /// Mutates process scheduling priority class.
    SetPriorityClass {
        pid: Pid,
        previous: PriorityClass,
        new: PriorityClass,
    },
    /// Enables or disables EcoQoS (Efficiency Mode / ProcessPowerThrottling).
    SetEcoQos {
        pid: Pid,
        previous: EcoQosState,
        new: EcoQosState,
    },
    /// Sets or removes an optional Job Object CPU rate limit (e.g. for soft throttling).
    SetCpuRateLimit {
        pid: Pid,
        previous_pct: Option<u32>,
        new_pct: Option<u32>,
    },
    /// Releases Job Object attachment / state cleanup on process exit.
    ReleaseJob { pid: Pid },
}

impl PolicyMutation {
    /// Target process ID for this mutation.
    #[inline]
    pub const fn pid(&self) -> Pid {
        match self {
            Self::SetPriorityClass { pid, .. }
            | Self::SetEcoQos { pid, .. }
            | Self::SetCpuRateLimit { pid, .. }
            | Self::ReleaseJob { pid } => *pid,
        }
    }

    /// Inverts this mutation, producing the exact reversing action.
    ///
    /// Applying an inverted mutation undoes the mutation's effect on the target system state.
    #[must_use]
    pub fn invert(&self) -> Option<Self> {
        match self {
            Self::SetPriorityClass { pid, previous, new } => {
                if previous == new {
                    None
                } else {
                    Some(Self::SetPriorityClass {
                        pid: *pid,
                        previous: *new,
                        new: *previous,
                    })
                }
            }
            Self::SetEcoQos { pid, previous, new } => {
                if previous == new {
                    None
                } else {
                    Some(Self::SetEcoQos {
                        pid: *pid,
                        previous: *new,
                        new: *previous,
                    })
                }
            }
            Self::SetCpuRateLimit {
                pid,
                previous_pct,
                new_pct,
            } => {
                if previous_pct == new_pct {
                    None
                } else {
                    Some(Self::SetCpuRateLimit {
                        pid: *pid,
                        previous_pct: *new_pct,
                        new_pct: *previous_pct,
                    })
                }
            }
            Self::ReleaseJob { .. } => None,
        }
    }

    /// Checks if this mutation actually produces an observable state change.
    #[inline]
    pub fn is_effective(&self) -> bool {
        match self {
            Self::SetPriorityClass { previous, new, .. } => previous != new,
            Self::SetEcoQos { previous, new, .. } => previous != new,
            Self::SetCpuRateLimit {
                previous_pct,
                new_pct,
                ..
            } => previous_pct != new_pct,
            Self::ReleaseJob { .. } => true,
        }
    }
}

/// An ordered sequence of mutations emitted during a policy tick or event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MutationPlan {
    /// Ordered list of mutations.
    pub mutations: Vec<PolicyMutation>,
    /// Whether this plan was generated in dry-run mode.
    pub is_dry_run: bool,
    /// Timestamp in milliseconds when this plan was created.
    pub timestamp_ms: u64,
}

impl MutationPlan {
    /// Creates a new mutation plan.
    pub fn new(mutations: Vec<PolicyMutation>, is_dry_run: bool, timestamp_ms: u64) -> Self {
        Self {
            mutations,
            is_dry_run,
            timestamp_ms,
        }
    }

    /// Returns true if this plan has no mutations.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    /// Number of mutations in this plan.
    #[inline]
    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    /// Adds an effective mutation to the plan and logs it (with [DRY-RUN] prefix if dry-run).
    pub fn push(&mut self, mutation: PolicyMutation) {
        if !mutation.is_effective() {
            return;
        }

        if self.is_dry_run {
            match &mutation {
                PolicyMutation::SetPriorityClass { pid, previous, new } => {
                    tracing::info!(
                        "[DRY-RUN] Would set priority for {} from {} to {}",
                        pid,
                        previous,
                        new
                    );
                }
                PolicyMutation::SetEcoQos { pid, previous, new } => {
                    tracing::info!(
                        "[DRY-RUN] Would set EcoQoS for {} from {} to {}",
                        pid,
                        previous,
                        new
                    );
                }
                PolicyMutation::SetCpuRateLimit {
                    pid,
                    previous_pct,
                    new_pct,
                } => {
                    tracing::info!(
                        "[DRY-RUN] Would set CPU rate limit for {} from {:?}% to {:?}%",
                        pid,
                        previous_pct,
                        new_pct
                    );
                }
                PolicyMutation::ReleaseJob { pid } => {
                    tracing::info!("[DRY-RUN] Would release job for {}", pid);
                }
            }
        } else {
            tracing::debug!("Emitting governor mutation: {:?}", mutation);
        }

        self.mutations.push(mutation);
    }

    /// Inverts the plan: inverts every mutation and reverses the sequence order.
    ///
    /// Applying an inverted plan brings a mutated system state back to its exact initial state.
    #[must_use]
    pub fn invert(&self) -> Self {
        let mut inverted_mutations = Vec::with_capacity(self.mutations.len());
        for mutation in self.mutations.iter().rev() {
            if let Some(inv) = mutation.invert() {
                inverted_mutations.push(inv);
            }
        }

        Self {
            mutations: inverted_mutations,
            is_dry_run: self.is_dry_run,
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Simulated per-process state for offline policy verification and dry-run execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedProcessState {
    pub priority: PriorityClass,
    pub eco_qos: EcoQosState,
    pub cpu_rate_pct: Option<u32>,
    pub in_job: bool,
}

impl Default for SimulatedProcessState {
    fn default() -> Self {
        Self {
            priority: PriorityClass::Normal,
            eco_qos: EcoQosState::Disabled,
            cpu_rate_pct: None,
            in_job: false,
        }
    }
}

/// Simulated system state for unit testing and verifying mutation reversibility / identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedSystemState {
    pub processes: HashMap<Pid, SimulatedProcessState>,
}

impl SimulatedSystemState {
    /// Creates a new simulated state with an initial set of processes at Normal priority.
    pub fn new(pids: &[Pid]) -> Self {
        let mut processes = HashMap::with_capacity(pids.len());
        for &pid in pids {
            processes.insert(pid, SimulatedProcessState::default());
        }
        Self { processes }
    }

    /// Applies an individual mutation to the simulated state.
    pub fn apply_mutation(&mut self, mutation: &PolicyMutation) {
        match mutation {
            PolicyMutation::SetPriorityClass { pid, new, .. } => {
                let entry = self.processes.entry(*pid).or_default();
                entry.priority = *new;
            }
            PolicyMutation::SetEcoQos { pid, new, .. } => {
                let entry = self.processes.entry(*pid).or_default();
                entry.eco_qos = *new;
            }
            PolicyMutation::SetCpuRateLimit { pid, new_pct, .. } => {
                let entry = self.processes.entry(*pid).or_default();
                entry.cpu_rate_pct = *new_pct;
                entry.in_job = new_pct.is_some();
            }
            PolicyMutation::ReleaseJob { pid } => {
                if let Some(entry) = self.processes.get_mut(pid) {
                    entry.in_job = false;
                    entry.cpu_rate_pct = None;
                }
            }
        }
    }

    /// Applies an entire plan to the simulated state.
    pub fn apply_plan(&mut self, plan: &MutationPlan) {
        for mutation in &plan.mutations {
            self.apply_mutation(mutation);
        }
    }

    /// Reverts an entire plan by applying its inverted plan.
    pub fn revert_plan(&mut self, plan: &MutationPlan) {
        let inverted = plan.invert();
        self.apply_plan(&inverted);
    }
}
