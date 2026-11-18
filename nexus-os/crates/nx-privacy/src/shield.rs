use serde::{Deserialize, Serialize};

use crate::backup::RollbackReceipt;
use crate::error::ShieldError;
use crate::hosts::HostsPlanner;
use crate::killswitch::{KillSwitchConfig, KillSwitchPlanner};
use crate::plan::{DryRunReport, MutationPlan, MutationStep};
use crate::system::SystemBackend;
use crate::telemetry::{TelemetryKillOptions, TelemetryPlanner};
use crate::trackers::{AD_AND_TRACKER_HOSTS, TELEMETRY_HOSTS};
use crate::wfp::{WfpEngine, WfpRule};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyShieldConfig {
    pub telemetry: TelemetryKillOptions,
    pub kill_switches: KillSwitchConfig,
    pub block_trackers: bool,
    pub custom_blocked_domains: Vec<String>,
    pub wfp_rules: Vec<WfpRule>,
    pub dynamic_wfp_session: bool,
}

impl Default for PrivacyShieldConfig {
    fn default() -> Self {
        Self {
            telemetry: TelemetryKillOptions::default(),
            kill_switches: KillSwitchConfig::default(),
            block_trackers: true,
            custom_blocked_domains: Vec::new(),
            wfp_rules: Vec::new(),
            dynamic_wfp_session: true,
        }
    }
}

pub struct PrivacyShield {
    config: PrivacyShieldConfig,
    wfp_engine: WfpEngine,
}

impl Default for PrivacyShield {
    fn default() -> Self {
        Self::new(PrivacyShieldConfig::default())
    }
}

impl PrivacyShield {
    pub fn new(config: PrivacyShieldConfig) -> Self {
        let wfp_engine = if config.dynamic_wfp_session {
            WfpEngine::new()
        } else {
            WfpEngine::new_persistent()
        };
        Self { config, wfp_engine }
    }

    pub fn config(&self) -> &PrivacyShieldConfig {
        &self.config
    }

    pub fn wfp_engine(&self) -> &WfpEngine {
        &self.wfp_engine
    }

    pub fn wfp_engine_mut(&mut self) -> &mut WfpEngine {
        &mut self.wfp_engine
    }

    /// Plans all system mutations across telemetry, device kill-switches,
    /// tracker hosts blocking, and WFP rules.
    pub fn plan(&self, backend: &impl SystemBackend) -> Result<MutationPlan, ShieldError> {
        let mut overall_plan = MutationPlan::new();

        // 1. Telemetry kill plan
        let telemetry_plan = TelemetryPlanner::build_plan(&self.config.telemetry, backend)?;
        overall_plan.extend(telemetry_plan);

        // 2. Kill switch plan (camera, mic, location)
        let killswitch_plan = KillSwitchPlanner::build_plan(&self.config.kill_switches, backend)?;
        overall_plan.extend(killswitch_plan);

        // 3. Trackers & Ad domain blocking
        if self.config.block_trackers {
            let mut all_domains: Vec<String> = TELEMETRY_HOSTS
                .iter()
                .chain(AD_AND_TRACKER_HOSTS.iter())
                .map(|&s| s.to_string())
                .collect();

            for custom in &self.config.custom_blocked_domains {
                all_domains.push(custom.clone());
            }

            if let Some(step) = HostsPlanner::plan_update(backend, &all_domains)? {
                overall_plan.push(step);
            }
        }

        // 4. WFP rules
        for rule in &self.config.wfp_rules {
            overall_plan.push(MutationStep::InstallWfpRule {
                rule: rule.clone(),
                description: format!("WFP: Install rule #{}: '{}'", rule.rule_id, rule.name),
            });
        }

        Ok(overall_plan)
    }

    /// Dry run: logs the plan and diffs, verifies zero mutations are performed.
    pub fn dry_run(&self, backend: &impl SystemBackend) -> Result<DryRunReport, ShieldError> {
        let plan = self.plan(backend)?;
        plan.dry_run(backend)
    }

    /// Applies the planned mutations against the backend.
    /// Invariant: Records a backup entry before modifying any state.
    pub fn apply(&self, backend: &mut impl SystemBackend) -> Result<RollbackReceipt, ShieldError> {
        let plan = self.plan(backend)?;
        plan.apply(backend)
    }

    /// Reverts all changes in a rollback receipt, restoring prior system state.
    pub fn rollback(
        &self,
        receipt: &RollbackReceipt,
        backend: &mut impl SystemBackend,
    ) -> Result<(), ShieldError> {
        receipt.rollback(backend)
    }
}
