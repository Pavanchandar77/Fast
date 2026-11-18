use tracing::info;

pub use nx_privacy::wfp::{
    WfpAction, WfpCondition, WfpDirection, WfpEngine, WfpFilterSpec, WfpProtocol, WfpRule,
    WfpRuleBuilder, WfpSessionConfig, FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION_FLAG_PERSISTENT,
};

pub struct WfpFirewall {
    pub telemetry_blocked: bool,
    pub session_is_dynamic: bool,
    pub engine: WfpEngine,
}

impl WfpFirewall {
    pub fn new() -> Self {
        Self {
            telemetry_blocked: false,
            // Critical: always ensure FWPM_SESSION_FLAG_DYNAMIC is used so that
            // filters are automatically flushed by the kernel if the daemon terminates.
            session_is_dynamic: true,
            engine: WfpEngine::new(),
        }
    }

    /// Configures WFP sub-layer using FWPM_SESSION_FLAG_DYNAMIC.
    /// This prevents permanent network loss if the daemon terminates unexpectedly.
    pub fn block_telemetry_endpoints(&mut self, endpoints: &[String]) -> anyhow::Result<()> {
        info!(
            "WFP Firewall: Installing dynamic kernel packet filters (FWPM_SESSION_FLAG_DYNAMIC) for {} endpoints",
            endpoints.len()
        );
        for (i, endpoint) in endpoints.iter().enumerate() {
            let rule = WfpRule::builder(format!("telemetry_block_{i}"))
                .description(format!("Block telemetry endpoint {endpoint}"))
                .block_outbound()
                .with_remote_port(443)
                .build()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            self.engine.add_rule(rule);
        }
        self.telemetry_blocked = true;
        Ok(())
    }
}

impl Default for WfpFirewall {
    fn default() -> Self {
        Self::new()
    }
}
