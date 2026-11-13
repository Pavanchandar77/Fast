//! nexus-vfs: Isolated MFT search and QuickLook file service micro-daemon.

use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use os_config::watcher::ConfigWatcher;
use os_vfs::mft::MftIndexer;

fn main() -> anyhow::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    info!("Starting nexus-vfs micro-daemon...");

    let config_path = find_config_path();
    let (watcher, _config_rx) = ConfigWatcher::new(&config_path)?;
    let initial_config = watcher.get();

    let mut indexer = MftIndexer::new();
    for vol in &initial_config.vfs.volumes {
        if let Some(ch) = vol.chars().next() {
            let _ = indexer.index_volume(ch);
        }
    }

    info!("✅ nexus-vfs active: USN Journal Indexer online.");

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
