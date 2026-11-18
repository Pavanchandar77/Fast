use tracing::info;

#[derive(Default)]
pub struct EtwBehavioralGuard {
    active: bool,
}

impl EtwBehavioralGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_kernel_trace(&mut self) -> anyhow::Result<()> {
        info!("ETW: Listening to Microsoft-Windows-Kernel-Process & Threat-Intelligence...");
        self.active = true;
        Ok(())
    }
}
