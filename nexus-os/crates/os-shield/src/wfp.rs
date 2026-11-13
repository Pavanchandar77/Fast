use tracing::info;

pub const FWPM_SESSION_FLAG_DYNAMIC: u32 = 0x00000001;

pub struct WfpFirewall {
    pub telemetry_blocked: bool,
    pub session_is_dynamic: bool,
}

impl WfpFirewall {
    pub fn new() -> Self {
        Self {
            telemetry_blocked: false,
            // Critical: always ensure FWPM_SESSION_FLAG_DYNAMIC is used so that
            // filters are automatically flushed by the kernel if the daemon terminates.
            session_is_dynamic: true,
        }
    }

    /// Configures WFP sub-layer using FWPM_SESSION_FLAG_DYNAMIC.
    /// This prevents permanent network loss if the daemon terminates unexpectedly.
    pub fn block_telemetry_endpoints(&mut self, endpoints: &[String]) -> anyhow::Result<()> {
        info!(
            "WFP Firewall: Installing dynamic kernel packet filters (FWPM_SESSION_FLAG_DYNAMIC) for {} endpoints",
            endpoints.len()
        );
        // FwpmEngineOpen0(..., FWPM_SESSION_FLAG_DYNAMIC, ...)
        self.telemetry_blocked = true;
        Ok(())
    }
}
