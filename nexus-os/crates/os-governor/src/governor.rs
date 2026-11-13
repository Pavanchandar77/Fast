use std::sync::Arc;
use parking_lot::RwLock;
use tracing::{info, warn};
use os_core::process::enable_debug_privilege;
use os_core::window::WindowHandle;
use os_config::schema::GovernorConfig;
use crate::app_nap::AppNapManager;
use crate::booster::ForegroundBooster;
use crate::ram_purger::RamPurger;

pub struct Governor {
    config: Arc<RwLock<GovernorConfig>>,
    booster: Arc<ForegroundBooster>,
    ram_purger: Arc<RamPurger>,
    app_nap: Arc<AppNapManager>,
}

impl Governor {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        let booster = Arc::new(ForegroundBooster::new());
        let ram_purger = Arc::new(RamPurger::new(config.clone()));
        let app_nap = Arc::new(AppNapManager::new(config.clone()));

        Self {
            config,
            booster,
            ram_purger,
            app_nap,
        }
    }

    pub fn start(&self) -> anyhow::Result<()> {
        info!("Initializing OS-Governor subsystem...");

        // 1. Escalate SeDebugPrivilege to govern all user & background processes
        if let Err(e) = enable_debug_privilege() {
            warn!("Could not enable SeDebugPrivilege: {:?}", e);
        } else {
            info!("🔒 SeDebugPrivilege acquired successfully");
        }

        // 2. Lock kernel timer resolution to 0.5ms
        let timer_0_5ms = { self.config.read().timer_resolution_0_5ms };
        if timer_0_5ms {
            if let Err(e) = ForegroundBooster::set_timer_resolution_0_5ms() {
                warn!("Could not set 0.5ms timer resolution: {:?}", e);
            }
        }

        // 3. Start background RAM purge and App Nap threads
        self.ram_purger.start_worker();
        self.app_nap.start_worker();

        info!("🚀 OS-Governor running: Foreground Boosting, RAM Purging & App Nap active");
        Ok(())
    }

    pub fn on_foreground_changed(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        let boost_enabled = { self.config.read().dynamic_priority_boost };
        if boost_enabled {
            self.booster.on_foreground_changed(pid);
        }

        self.ram_purger.mark_active(pid);
        self.app_nap.mark_active(pid);
    }

    pub fn on_window_minimized(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        let (trim, nap) = {
            let cfg = self.config.read();
            (cfg.trim_on_minimize, cfg.app_nap_enabled)
        };

        if trim {
            let _ = self.ram_purger.purge_pid(pid);
        }

        if nap {
            let _ = self.app_nap.suspend_pid(pid);
        }
    }

    pub fn on_window_restored(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }
        self.app_nap.mark_active(pid);
        self.ram_purger.mark_active(pid);
    }
}
