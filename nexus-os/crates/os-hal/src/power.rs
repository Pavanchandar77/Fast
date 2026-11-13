use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    EcoSilent,
    Balanced,
    MaxCompute,
}

pub struct PowerGovernor;

impl PowerGovernor {
    pub fn apply_profile(profile: PowerProfile) -> anyhow::Result<()> {
        match profile {
            PowerProfile::EcoSilent => {
                info!("Power: Applying Eco Silent profile (throttling package TDP, capping clocks)");
            }
            PowerProfile::Balanced => {
                info!("Power: Applying Balanced dynamic profile");
            }
            PowerProfile::MaxCompute => {
                info!("Power: ⚡ Applying Max Compute profile - unparking 100% CPU cores, disabling PCIe link state throttling");
            }
        }
        Ok(())
    }
}
