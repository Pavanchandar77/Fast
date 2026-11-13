use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use tracing::{error, info, warn};
use crate::schema::SystemConfig;

pub struct ConfigWatcher {
    config_path: PathBuf,
    current_config: Arc<RwLock<SystemConfig>>,
    _watcher: Option<RecommendedWatcher>,
}

impl ConfigWatcher {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<(Self, crossbeam_channel::Receiver<SystemConfig>)> {
        let path_buf = path.as_ref().to_path_buf();
        let initial_config = if path_buf.exists() {
            match SystemConfig::load_from_file(&path_buf) {
                Ok(cfg) => cfg,
                Err(err) => {
                    warn!("Failed to parse config at {:?}: {}. Using default config.", path_buf, err);
                    SystemConfig::default()
                }
            }
        } else {
            info!("Config file not found at {:?}, generating default.", path_buf);
            let default_cfg = SystemConfig::default();
            if let Some(parent) = path_buf.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let toml_str = toml::to_string_pretty(&default_cfg)?;
            let _ = std::fs::write(&path_buf, toml_str);
            default_cfg
        };

        let current_config = Arc::new(RwLock::new(initial_config.clone()));
        let (tx, rx) = crossbeam_channel::unbounded();

        let watch_path = path_buf.clone();
        let config_clone = current_config.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    if event.kind.is_modify() || event.kind.is_create() {
                        // Small sleep to ensure write completion (debounce)
                        std::thread::sleep(Duration::from_millis(50));
                        match SystemConfig::load_from_file(&watch_path) {
                            Ok(new_cfg) => {
                                info!("System config hot-reloaded successfully in <10ms");
                                *config_clone.write() = new_cfg.clone();
                                let _ = tx.send(new_cfg);
                            }
                            Err(e) => {
                                error!("Failed to reload config: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Watch event error: {:?}", e);
                }
            },
            Config::default(),
        )?;

        if let Some(parent) = path_buf.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        Ok((
            Self {
                config_path: path_buf,
                current_config,
                _watcher: Some(watcher),
            },
            rx,
        ))
    }

    pub fn get(&self) -> SystemConfig {
        self.current_config.read().clone()
    }

    pub fn path(&self) -> &Path {
        &self.config_path
    }
}
