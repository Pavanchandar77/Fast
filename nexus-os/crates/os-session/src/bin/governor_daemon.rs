//! nexus-governor: Isolated process and RAM governor micro-daemon.
//! Runs independently of UI/compositor processes using documented Win32 Job Objects.

use os_config::watcher::ConfigWatcher;
use os_governor::Governor;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

fn main() -> anyhow::Result<()> {
    os_core::recovery::install_panic_hook();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting nexus-governor micro-daemon...");

    let dry_run = std::env::args().any(|arg| arg == "--dry-run");
    os_core::recovery::bootstrap(dry_run)?;
    let _restore = os_core::recovery::RestoreOnDrop::new();
    os_shield::attach(true, os_core::recovery::is_dry_run());

    let config_path = find_config_path();
    let (watcher, _config_rx) = ConfigWatcher::new(&config_path)?;
    let initial_config = watcher.get();

    let governor_config = Arc::new(RwLock::new(initial_config.governor));
    let governor = Governor::new(governor_config);
    governor.start()?;

    info!("✅ nexus-governor active: Job Objects CPU Rate Throttler & RAM Purger online.");

    loop {
        std::thread::park();
    }
}

fn find_config_path() -> PathBuf {
    let local = PathBuf::from("config/system.toml");
    if local.exists() {
        return local;
    }
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        let user_cfg = PathBuf::from(user_profile)
            .join(".config")
            .join("nexus-os")
            .join("system.toml");
        if user_cfg.exists() {
            return user_cfg;
        }
    }
    local
}
