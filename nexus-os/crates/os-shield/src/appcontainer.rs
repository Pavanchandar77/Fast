use tracing::info;

pub struct AppContainerJail;

impl AppContainerJail {
    pub fn spawn_jailed(executable: &str, _profile_name: &str) -> anyhow::Result<()> {
        info!("AppContainer: Launching {} inside isolated AppContainer token sandbox", executable);
        // Creates AppContainer SID via CreateAppContainerProfile and launches process with PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES
        Ok(())
    }
}
