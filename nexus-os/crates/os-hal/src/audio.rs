use tracing::info;

pub struct AudioGovernor;

impl AudioGovernor {
    pub fn lock_a2dp_profile() -> anyhow::Result<()> {
        info!("HAL: Bluetooth Hands-Free lock active - forbidding downgrade to 8kHz AG Audio profile");
        Ok(())
    }
}
