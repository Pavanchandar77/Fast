//! Thin adapter: the pure policy decides, the journal records, Win32 applies.
//!
//! Every priority, EcoQoS, and job-cap change goes through `os_core::recovery`
//! before it touches the system. `--dry-run` and `governor.dry_run` log the
//! plan and record nothing. Below-normal and idle are clamped away. High and
//! realtime require `priority_whitelist` or `whitelist_high_priority`.

use crate::app_nap::AppNapManager;
use crate::booster::ForegroundBooster;
use crate::job_object::JobThrottleManager;
use crate::ram_purger::RamPurger;
use nexus_model::governor::{
    GovernorPolicy, GovernorPolicyConfig, MemoryDecision, ProcessSnapshot,
};
use nx_governor_policy::{
    EcoQosState, GovernorEngine, GovernorPolicyConfig as SmartConfig, MutationPlan, Pid,
    PolicyMutation, PriorityClass as SmartPriority,
};
use os_config::schema::GovernorConfig;
use os_core::process::{enable_debug_privilege, ProcessHandle};
use os_core::window::WindowHandle;
use os_core::{clamp_priority, Mutation, PriorityClass, PriorityRole};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

pub struct Governor {
    config: Arc<RwLock<GovernorConfig>>,
    engine: Arc<RwLock<GovernorEngine>>,
    ram_purger: Arc<RamPurger>,
    app_nap: Arc<AppNapManager>,
    job_throttler: Arc<JobThrottleManager>,
    running: Arc<AtomicBool>,
}

fn to_policy_config(cfg: &GovernorConfig) -> SmartConfig {
    let mut whitelist = cfg.priority_whitelist.clone();
    for name in &cfg.whitelist_high_priority {
        if !whitelist
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(name))
        {
            whitelist.push(name.clone());
        }
    }

    SmartConfig {
        // One-class boost is the engine's behavior. The shell ships it off
        // until the operator sets `dynamic_priority_boost`.
        boost_foreground: cfg.dynamic_priority_boost,
        max_boost_class: SmartPriority::AboveNormal,
        whitelist_high_priority: whitelist,
        pro_balance_enabled: cfg.pro_balance_enabled,
        pro_balance_cpu_threshold_pct: 50.0,
        pro_balance_system_cpu_threshold_pct: 60.0,
        pro_balance_sustained_ticks: 2,
        pro_balance_restore_ticks: 2,
        eco_qos_enabled: cfg.eco_qos_enabled,
        eco_qos_background_idle_ticks: (cfg.app_nap_idle_secs / 2).max(1) as u32,
        game_mode_auto_detect: cfg.game_mode_auto_detect,
        // The engine does not emit suspends. The Win32 call stays behind
        // `allow_nt_suspend` in the app-nap path.
        allow_suspend: cfg.allow_suspend && cfg.allow_nt_suspend,
        allow_below_normal_default: false,
        dry_run: cfg.dry_run,
        game_mode_override: None,
        exclude_rules: cfg.exclude_from_nap.clone(),
    }
}

fn to_journal_priority(class: SmartPriority) -> PriorityClass {
    match class {
        SmartPriority::Idle => PriorityClass::Idle,
        SmartPriority::BelowNormal => PriorityClass::BelowNormal,
        SmartPriority::Normal => PriorityClass::Normal,
        SmartPriority::AboveNormal => PriorityClass::AboveNormal,
        SmartPriority::High => PriorityClass::High,
        SmartPriority::Realtime => PriorityClass::Realtime,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn whitelist_of(cfg: &GovernorConfig) -> Vec<String> {
    let mut whitelist = cfg.priority_whitelist.clone();
    for name in &cfg.whitelist_high_priority {
        if !whitelist
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(name))
        {
            whitelist.push(name.clone());
        }
    }
    whitelist
}

/// Record a priority change, then apply it. Below-normal and idle are never
/// written. A request that would lower a process only because the policy
/// asked for below-normal is dropped, so a Normal process stays Normal.
fn journal_priority(pid: u32, requested: SmartPriority, cfg: &GovernorConfig) {
    let Ok(handle) = ProcessHandle::open_for_governance(pid) else {
        return;
    };
    let Ok(live) = handle.priority() else {
        return;
    };
    let requested = to_journal_priority(requested);
    let name = crate::app_nap::get_process_name(pid).unwrap_or_default();
    let whitelist = whitelist_of(cfg);
    let role = if requested.rank() > live.rank() {
        PriorityRole::Foreground
    } else {
        PriorityRole::Background
    };
    let clamped = clamp_priority(requested, role, &name, &whitelist);
    if clamped == live {
        return;
    }
    if clamped.rank() < live.rank() && requested.rank() < PriorityClass::Normal.rank() {
        return;
    }
    let mutation = Mutation::ProcessPriority {
        pid,
        previous: live,
        new: clamped,
    };
    if let Err(err) = os_core::recovery::commit(&mutation) {
        warn!("priority journal failed for pid {pid}: {err}");
    }
}

fn journal_eco(pid: u32, enabled: bool) {
    let mutation = Mutation::EcoQos {
        pid,
        previous_enabled: !enabled,
        enabled,
    };
    if let Err(err) = os_core::recovery::commit(&mutation) {
        warn!("EcoQoS journal failed for pid {pid}: {err}");
    }
}

fn apply_plan(plan: &MutationPlan, cfg: &GovernorConfig, jobs: &JobThrottleManager) {
    if plan.is_dry_run || cfg.dry_run || os_core::recovery::is_dry_run() {
        for mutation in &plan.mutations {
            info!("[DRY-RUN] {mutation:?}");
        }
        return;
    }

    for mutation in &plan.mutations {
        match mutation {
            PolicyMutation::SetPriorityClass { pid, new, .. } => {
                journal_priority(pid.as_u32(), *new, cfg);
            }
            PolicyMutation::SetEcoQos { pid, new, .. } => {
                journal_eco(pid.as_u32(), matches!(new, EcoQosState::Enabled));
            }
            PolicyMutation::SetCpuRateLimit { pid, new_pct, .. } => match new_pct {
                Some(pct) => {
                    if let Err(err) = jobs.throttle_process(pid.as_u32(), *pct) {
                        warn!("job cap failed for pid {}: {err}", pid.as_u32());
                    }
                }
                None => jobs.unthrottle_process(pid.as_u32()),
            },
            PolicyMutation::ReleaseJob { pid } => jobs.on_process_exit(pid.as_u32()),
        }
    }
}

impl Governor {
    pub fn new(config: Arc<RwLock<GovernorConfig>>) -> Self {
        let policy_config = {
            let cfg = config.read();
            to_policy_config(&cfg)
        };
        let engine = Arc::new(RwLock::new(GovernorEngine::new(policy_config)));
        let ram_purger = Arc::new(RamPurger::new(config.clone()));
        let app_nap = Arc::new(AppNapManager::new(config.clone()));
        let job_throttler = Arc::new(JobThrottleManager::new());
        let running = Arc::new(AtomicBool::new(false));

        Self {
            config,
            engine,
            ram_purger,
            app_nap,
            job_throttler,
            running,
        }
    }

    pub fn start(&self) -> anyhow::Result<()> {
        info!("Initializing OS-Governor subsystem with Smart Governor policy...");

        if let Err(e) = enable_debug_privilege() {
            warn!("Could not enable SeDebugPrivilege: {:?}", e);
        } else {
            info!("SeDebugPrivilege acquired successfully");
        }

        let timer_0_5ms = { self.config.read().timer_resolution_0_5ms };
        if timer_0_5ms {
            if let Err(e) = ForegroundBooster::set_timer_resolution_0_5ms() {
                warn!("Could not set 0.5ms timer resolution: {:?}", e);
            }
        }

        let ram_purge_enabled = { self.config.read().ram_purge_enabled };
        if ram_purge_enabled {
            self.ram_purger.start_worker();
        }

        // App nap stays off unless `app_nap_enabled`. The worker is the only
        // path that can record a job cap or an opted-in suspend for idle apps.
        self.app_nap.start_worker();

        self.running.store(true, Ordering::SeqCst);
        self.start_policy_worker();

        info!("Smart OS-Governor active: EcoQoS, ProBalance, safe boost, and game mode");
        Ok(())
    }

    fn refresh_engine(&self) {
        let policy = {
            let cfg = self.config.read();
            to_policy_config(&cfg)
        };
        self.engine.write().update_config(policy);
    }

    fn execute_plan(&self, plan: &MutationPlan) {
        let cfg = self.config.read().clone();
        apply_plan(plan, &cfg, &self.job_throttler);
    }

    pub fn on_foreground_changed(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        self.refresh_engine();
        let plan = self
            .engine
            .write()
            .on_foreground_changed(Some(Pid(pid)), now_ms());
        self.execute_plan(&plan);
        self.ram_purger.mark_active(pid);
        self.app_nap.mark_active(pid);
    }

    pub fn on_window_minimized(&self, hwnd: WindowHandle) {
        let pid = hwnd.pid();
        if pid == 0 {
            return;
        }

        // Minimize is not a nap. Tooltips and menus hide constantly. The
        // idle worker is the only nap path, and it stays off unless
        // `app_nap_enabled` is set. A trim only flushes the working set.
        let (trim_on_minimize, exclusions) = {
            let cfg = self.config.read();
            (cfg.trim_on_minimize, cfg.exclude_from_nap.clone())
        };
        let policy_cfg = GovernorPolicyConfig {
            trim_on_minimize,
            app_nap_enabled: false,
            user_exclusions: exclusions,
            dynamic_priority_boost: false,
            ..GovernorPolicyConfig::default()
        };
        let Ok(name) = crate::app_nap::get_process_name(pid) else {
            return;
        };
        let snap = ProcessSnapshot {
            pid,
            name: &name,
            is_foreground: false,
            is_minimized: true,
            is_cloaked: hwnd.is_cloaked(),
            idle_seconds: 0,
        };
        let decision = GovernorPolicy::evaluate_minimize(&snap, &policy_cfg);
        if decision.memory == MemoryDecision::PurgeWorkingSet {
            let _ = self.ram_purger.purge_pid(pid);
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

    pub fn on_process_exit(&self, pid: u32) {
        if pid == 0 {
            return;
        }

        self.refresh_engine();
        let plan = self.engine.write().on_process_exit(Pid(pid), now_ms());
        self.execute_plan(&plan);
        // App-nap caps live in this map even when the policy engine never
        // tracked the pid. Release them so a recycled pid cannot inherit one.
        self.job_throttler.on_process_exit(pid);
    }

    fn start_policy_worker(&self) {
        let engine = self.engine.clone();
        let running = self.running.clone();
        let config = self.config.clone();
        let job_throttler = self.job_throttler.clone();

        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1000));

                let policy = {
                    let cfg = config.read();
                    to_policy_config(&cfg)
                };
                let plan = {
                    let mut engine = engine.write();
                    engine.update_config(policy);
                    // Keep the foreground the event path already recorded.
                    // An empty snapshot would otherwise look like focus loss
                    // and undo the boost on every tick.
                    let fg = engine.current_foreground_pid();
                    let snapshot =
                        nx_governor_policy::SystemSnapshot::new(15.0, now_ms()).with_foreground(fg);
                    engine.evaluate_tick(&snapshot)
                };

                let cfg = config.read().clone();
                apply_plan(&plan, &cfg, &job_throttler);
            }
        });
    }
}

impl Drop for Governor {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.refresh_engine();
        let plan = self.engine.write().reset_all(now_ms());
        self.execute_plan(&plan);
    }
}
