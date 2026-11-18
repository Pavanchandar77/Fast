use crate::app_nap::AppNapManager;
use crate::booster::ForegroundBooster;
use crate::job_object::{JobThrottleManager, HANDLE};
use crate::ram_purger::RamPurger;
#[cfg(windows)]
use nx_governor_policy::EcoQosState;
use nx_governor_policy::{
    GovernorEngine, GovernorPolicyConfig, MutationPlan, Pid, PolicyMutation, PriorityClass,
    SystemSnapshot,
};
use os_config::schema::GovernorConfig;
use os_core::process::{enable_debug_privilege, ProcessHandle};
use os_core::window::WindowHandle;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    ProcessPowerThrottling, SetProcessInformation, PROCESS_CREATION_FLAGS,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE,
};

pub struct Governor {
    config: Arc<RwLock<GovernorConfig>>,
    engine: Arc<RwLock<GovernorEngine>>,
    _booster: Arc<ForegroundBooster>,
    ram_purger: Arc<RamPurger>,
    app_nap: Arc<AppNapManager>,
    job_throttler: Arc<JobThrottleManager>,
    running: Arc<AtomicBool>,
}

fn to_policy_config(cfg: &GovernorConfig) -> GovernorPolicyConfig {
    let max_boost =
        PriorityClass::from_name(&cfg.foreground_priority).unwrap_or(PriorityClass::AboveNormal);

    GovernorPolicyConfig {
        boost_foreground: cfg.dynamic_priority_boost,
        max_boost_class: max_boost,
        whitelist_high_priority: cfg.whitelist_high_priority.clone(),
        pro_balance_enabled: cfg.pro_balance_enabled,
        pro_balance_cpu_threshold_pct: 50.0,
        pro_balance_system_cpu_threshold_pct: 60.0,
        pro_balance_sustained_ticks: 2,
        pro_balance_restore_ticks: 2,
        eco_qos_enabled: cfg.eco_qos_enabled,
        eco_qos_background_idle_ticks: (cfg.app_nap_idle_secs / 2).max(1) as u32,
        game_mode_auto_detect: cfg.game_mode_auto_detect,
        allow_suspend: cfg.allow_suspend,
        allow_below_normal_default: false,
        dry_run: cfg.dry_run,
        game_mode_override: None,
        exclude_rules: cfg.exclude_from_nap.clone(),
    }
}

impl Governor {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        let policy_config = {
            let cfg = config.read();
            to_policy_config(&cfg)
        };
        let engine = Arc::new(RwLock::new(GovernorEngine::new(policy_config)));
        let booster = Arc::new(ForegroundBooster::new());
        let ram_purger = Arc::new(RamPurger::new(config.clone()));
        let app_nap = Arc::new(AppNapManager::new(config.clone()));
        let job_throttler = Arc::new(JobThrottleManager::new());
        let running = Arc::new(AtomicBool::new(false));

        Self {
            config,
            engine,
            _booster: booster,
            ram_purger,
            app_nap,
            job_throttler,
            running,
        }
    }

    pub fn start(&self) -> anyhow::Result<()> {
        info!("Initializing OS-Governor subsystem with Smart Governor policy...");

        // 1. Escalate SeDebugPrivilege to govern user & background processes
        if let Err(e) = enable_debug_privilege() {
            warn!("Could not enable SeDebugPrivilege: {:?}", e);
        } else {
            info!("🔒 SeDebugPrivilege acquired successfully");
        }

        // 2. Lock kernel timer resolution to 0.5ms ONLY if explicitly configured (default: off)
        let timer_0_5ms = { self.config.read().timer_resolution_0_5ms };
        if timer_0_5ms {
            if let Err(e) = ForegroundBooster::set_timer_resolution_0_5ms() {
                warn!("Could not set 0.5ms timer resolution: {:?}", e);
            }
        }

        // 3. Start background RAM purge only if specifically enabled (default: off)
        let ram_purge_enabled = { self.config.read().ram_purge_enabled };
        if ram_purge_enabled {
            self.ram_purger.start_worker();
        }

        // 4. Start pure governor telemetry sampling worker
        self.running.store(true, Ordering::SeqCst);
        self.start_policy_worker();

        info!("🚀 Smart OS-Governor active: EcoQoS, ProBalance, Safe Boost & Game Mode online");
        Ok(())
    }

    pub fn on_foreground_changed(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let plan = self
            .engine
            .write()
            .on_foreground_changed(Some(Pid(pid)), now_ms);

        self.execute_plan(&plan);
        self.ram_purger.mark_active(pid);
        self.app_nap.mark_active(pid);
    }

    pub fn on_window_minimized(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        let trim = { self.config.read().trim_on_minimize };
        if trim {
            let _ = self.ram_purger.purge_pid(pid);
        }
        // Safe policy: never hard-cap to 1% or NtSuspendProcess on window hide/minimize!
    }

    pub fn on_window_restored(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }
        self.app_nap.mark_active(pid);
        self.ram_purger.mark_active(pid);
    }

    pub fn on_process_exit(&self, pid: u32) {
        if pid == 0 {
            return;
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let plan = self.engine.write().on_process_exit(Pid(pid), now_ms);
        self.execute_plan(&plan);
        self.job_throttler.on_process_exit(pid);
    }

    fn execute_plan(&self, plan: &MutationPlan) {
        let is_dry_run = { self.config.read().dry_run };

        for mutation in &plan.mutations {
            if is_dry_run {
                info!("[DRY-RUN] Plan mutation: {:?}", mutation);
                continue;
            }

            match mutation {
                PolicyMutation::SetPriorityClass { pid, new, .. } => {
                    #[cfg(windows)]
                    {
                        let win32_flags = PROCESS_CREATION_FLAGS(new.to_win32());
                        if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32()) {
                            let _ = handle.set_priority(win32_flags);
                        }
                    }
                    #[cfg(not(windows))]
                    let _ = (pid, new);
                }
                PolicyMutation::SetEcoQos { pid, new, .. } => {
                    #[cfg(windows)]
                    {
                        if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32()) {
                            let state = PROCESS_POWER_THROTTLING_STATE {
                                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                                StateMask: match new {
                                    EcoQosState::Enabled => {
                                        PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                                    }
                                    EcoQosState::Disabled => 0,
                                },
                            };
                            unsafe {
                                let _ = SetProcessInformation(
                                    handle.handle(),
                                    ProcessPowerThrottling,
                                    &state as *const _ as *const c_void,
                                    std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                                );
                            }
                        }
                    }
                    #[cfg(not(windows))]
                    let _ = (pid, new);
                }
                PolicyMutation::SetCpuRateLimit { pid, new_pct, .. } => {
                    if let Some(pct) = new_pct {
                        if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32()) {
                            let _ = job_throttler_throttle(
                                &self.job_throttler,
                                pid.as_u32(),
                                handle.handle(),
                                *pct,
                            );
                        }
                    } else {
                        self.job_throttler.unthrottle_process(pid.as_u32());
                    }
                }
                PolicyMutation::ReleaseJob { pid } => {
                    self.job_throttler.on_process_exit(pid.as_u32());
                }
            }
        }
    }

    fn start_policy_worker(&self) {
        let engine = self.engine.clone();
        let running = self.running.clone();
        let config = self.config.clone();
        let job_throttler = self.job_throttler.clone();

        std::thread::spawn(move || {
            let mut last_tick = Instant::now();

            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1000));
                let _dt = last_tick.elapsed();
                last_tick = Instant::now();

                // Build telemetry snapshot
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let snapshot = SystemSnapshot::new(15.0, now_ms);

                let plan = engine.write().evaluate_tick(&snapshot);

                let is_dry_run = { config.read().dry_run };
                for mutation in &plan.mutations {
                    if is_dry_run {
                        info!("[DRY-RUN] Worker mutation: {:?}", mutation);
                        continue;
                    }

                    match mutation {
                        PolicyMutation::SetPriorityClass { pid, new, .. } => {
                            #[cfg(windows)]
                            {
                                let win32_flags = PROCESS_CREATION_FLAGS(new.to_win32());
                                if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32())
                                {
                                    let _ = handle.set_priority(win32_flags);
                                }
                            }
                            #[cfg(not(windows))]
                            let _ = (pid, new);
                        }
                        PolicyMutation::SetEcoQos { pid, new, .. } => {
                            #[cfg(windows)]
                            {
                                if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32())
                                {
                                    let state = PROCESS_POWER_THROTTLING_STATE {
                                        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                                        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                                        StateMask: match new {
                                            EcoQosState::Enabled => {
                                                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                                            }
                                            EcoQosState::Disabled => 0,
                                        },
                                    };
                                    unsafe {
                                        let _ = SetProcessInformation(
                                            handle.handle(),
                                            ProcessPowerThrottling,
                                            &state as *const _ as *const c_void,
                                            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>()
                                                as u32,
                                        );
                                    }
                                }
                            }
                            #[cfg(not(windows))]
                            let _ = (pid, new);
                        }
                        PolicyMutation::SetCpuRateLimit { pid, new_pct, .. } => {
                            if let Some(pct) = new_pct {
                                if let Ok(handle) = ProcessHandle::open_for_governance(pid.as_u32())
                                {
                                    let _ = job_throttler.throttle_process(
                                        pid.as_u32(),
                                        handle.handle(),
                                        *pct,
                                    );
                                }
                            } else {
                                job_throttler.unthrottle_process(pid.as_u32());
                            }
                        }
                        PolicyMutation::ReleaseJob { pid } => {
                            job_throttler.on_process_exit(pid.as_u32());
                        }
                    }
                }
            }
        });
    }
}

#[inline]
fn job_throttler_throttle(
    throttler: &JobThrottleManager,
    pid: u32,
    handle: HANDLE,
    rate_percent: u32,
) -> anyhow::Result<()> {
    throttler.throttle_process(pid, handle, rate_percent)
}

impl Drop for Governor {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let plan = self.engine.write().reset_all(now_ms);
        self.execute_plan(&plan);
    }
}
