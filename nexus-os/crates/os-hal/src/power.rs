use nexus_model::platform::{Platform, PowerProfile as ModelPowerProfile};
use os_core::platform::Win32Platform;
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
        let mapped = match profile {
            PowerProfile::EcoSilent => ModelPowerProfile::EcoSilent,
            PowerProfile::Balanced => ModelPowerProfile::Balanced,
            PowerProfile::MaxCompute => ModelPowerProfile::MaxCompute,
        };
        match Win32Platform::new().set_power_profile(mapped) {
            Ok(()) => match profile {
                PowerProfile::EcoSilent => {
                    info!(
                        "Power: Applying Eco Silent profile (throttling package TDP, capping clocks)"
                    );
                }
                PowerProfile::Balanced => {
                    info!("Power: Applying Balanced dynamic profile");
                }
                PowerProfile::MaxCompute => {
                    info!("Power: Applying Max Compute profile via journaled Platform call");
                }
            },
            Err(err) => {
                info!("Power: journaled profile request recorded with status: {err}");
            }
        }
        Ok(())
    }
}
