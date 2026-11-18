//! Pure governor policy engine orchestrating ProBalance, Foreground Boosting,
//! EcoQoS, Game Mode, and leak-free process lifecycle management.

use crate::config::GovernorPolicyConfig;
use crate::mutation::{MutationPlan, PolicyMutation};
use crate::rules::GovernorRules;
use crate::sample::SystemSnapshot;
use crate::state::TrackedProcess;
use crate::types::{EcoQosState, Pid, PriorityClass};
use std::collections::HashMap;

/// The pure Nexus-OS governor policy engine.
///
/// Has zero Windows API dependencies; all state transitions and policy decisions
/// are pure functions that emit reversible `MutationPlan`s.
#[derive(Debug, Clone)]
pub struct GovernorEngine {
    config: GovernorPolicyConfig,
    rules: GovernorRules,
    tracked: HashMap<Pid, TrackedProcess>,
    current_foreground_pid: Option<Pid>,
    previous_foreground_pid: Option<Pid>,
    game_mode_active: bool,
    current_tick: u64,
}

impl GovernorEngine {
    /// Creates a new governor engine with specified configuration and rules.
    pub fn new(config: GovernorPolicyConfig) -> Self {
        let rules = GovernorRules::new(&config.exclude_rules, &config.whitelist_high_priority);
        Self {
            config,
            rules,
            tracked: HashMap::new(),
            current_foreground_pid: None,
            previous_foreground_pid: None,
            game_mode_active: false,
            current_tick: 0,
        }
    }

    /// Access the current engine configuration.
    #[inline]
    pub fn config(&self) -> &GovernorPolicyConfig {
        &self.config
    }

    /// Mutably update the engine configuration.
    pub fn update_config(&mut self, config: GovernorPolicyConfig) {
        self.rules = GovernorRules::new(&config.exclude_rules, &config.whitelist_high_priority);
        self.config = config;
    }

    /// Access the active ruleset.
    #[inline]
    pub fn rules(&self) -> &GovernorRules {
        &self.rules
    }

    /// Returns true if Game Mode is currently active.
    #[inline]
    pub fn is_game_mode_active(&self) -> bool {
        self.game_mode_active
    }

    /// Returns the current foreground PID known to the engine.
    #[inline]
    pub fn current_foreground_pid(&self) -> Option<Pid> {
        self.current_foreground_pid
    }

    /// Number of processes currently tracked in the engine's internal state.
    #[inline]
    pub fn tracked_process_count(&self) -> usize {
        self.tracked.len()
    }

    /// Inspect tracked state for a specific PID.
    pub fn get_tracked_process(&self, pid: Pid) -> Option<&TrackedProcess> {
        self.tracked.get(&pid)
    }

    /// Detects whether Game Mode should be active based on a system snapshot and config.
    pub fn detect_game_mode(&self, snapshot: &SystemSnapshot) -> bool {
        if let Some(forced) = self.config.game_mode_override {
            return forced;
        }

        if !self.config.game_mode_auto_detect {
            return false;
        }

        if snapshot.is_game_active {
            return true;
        }

        // Fullscreen process check
        if let Some(fs_pid) = snapshot.fullscreen_pid {
            if let Some(fs_proc) = snapshot.processes.iter().find(|p| p.pid == fs_pid) {
                if self.rules.is_known_game(&fs_proc.name) {
                    return true;
                }
            }
        }

        // Check if any active process is a known game running in foreground or fullscreen
        snapshot
            .processes
            .iter()
            .any(|p| self.rules.is_known_game(&p.name) && (p.is_foreground || p.is_fullscreen))
    }

    /// Sets or clears Game Mode, pausing or resuming the governor.
    ///
    /// When entering Game Mode:
    /// - Demotions are paused.
    /// - Any existing demotions are immediately restored to avoid game/streaming stutter.
    pub fn set_game_mode(&mut self, active: bool, timestamp_ms: u64) -> MutationPlan {
        let mut plan = MutationPlan::new(Vec::new(), self.config.dry_run, timestamp_ms);
        if self.game_mode_active == active {
            return plan;
        }

        self.game_mode_active = active;

        if active {
            // When game mode activates, restore all demoted processes immediately
            for proc in self.tracked.values_mut() {
                if proc.is_demoted {
                    plan.push(PolicyMutation::SetPriorityClass {
                        pid: proc.pid,
                        previous: proc.current_priority,
                        new: proc.base_priority,
                    });
                    proc.current_priority = proc.base_priority;
                    proc.is_demoted = false;
                    proc.consecutive_high_cpu_ticks = 0;
                }
            }
        }

        plan
    }

    /// Handles foreground window change events.
    ///
    /// Rules:
    /// - Restores previous foreground process priority to its base class (never demotes to BelowNormal).
    /// - Boosts new foreground process by AT MOST ONE class (e.g. Normal -> AboveNormal).
    /// - NEVER boosts to `High` unless explicitly whitelisted in `whitelist_high_priority`.
    /// - Disables EcoQoS immediately on new foreground process.
    /// - Cancels any ProBalance demotion immediately on new foreground process.
    pub fn on_foreground_changed(
        &mut self,
        new_fg_pid: Option<Pid>,
        timestamp_ms: u64,
    ) -> MutationPlan {
        let mut plan = MutationPlan::new(Vec::new(), self.config.dry_run, timestamp_ms);

        if new_fg_pid == self.current_foreground_pid {
            return plan;
        }

        let old_fg_pid = self.current_foreground_pid;

        // 1. Restore previous foreground process
        if let Some(old_pid) = old_fg_pid {
            if let Some(proc) = self.tracked.get_mut(&old_pid) {
                if proc.is_boosted {
                    plan.push(PolicyMutation::SetPriorityClass {
                        pid: old_pid,
                        previous: proc.current_priority,
                        new: proc.base_priority,
                    });
                    proc.current_priority = proc.base_priority;
                    proc.is_boosted = false;
                }
            }
        }

        // 2. Elevate new foreground process
        if let Some(new_pid) = new_fg_pid {
            if !new_pid.is_system_kernel() {
                let proc = self.tracked.entry(new_pid).or_insert_with(|| {
                    TrackedProcess::new(new_pid, "unknown.exe", self.current_tick)
                });

                // A: Cancel any demotion immediately
                if proc.is_demoted {
                    plan.push(PolicyMutation::SetPriorityClass {
                        pid: new_pid,
                        previous: proc.current_priority,
                        new: proc.base_priority,
                    });
                    proc.current_priority = proc.base_priority;
                    proc.is_demoted = false;
                    proc.consecutive_high_cpu_ticks = 0;
                }

                // B: Disable EcoQoS immediately
                if proc.eco_qos.is_enabled() {
                    plan.push(PolicyMutation::SetEcoQos {
                        pid: new_pid,
                        previous: EcoQosState::Enabled,
                        new: EcoQosState::Disabled,
                    });
                    proc.eco_qos = EcoQosState::Disabled;
                }
                proc.background_idle_ticks = 0;

                // C: Apply foreground priority boost if enabled and not critical/excluded
                if self.config.boost_foreground && !self.rules.is_critical(new_pid, &proc.name) {
                    let is_whitelisted = self.rules.is_high_priority_whitelisted(&proc.name);

                    let target_priority = if is_whitelisted {
                        PriorityClass::High
                    } else {
                        // Safe foreground boost: AT MOST ONE CLASS, never exceed max_boost_class
                        let one_step = proc.base_priority.step_up();
                        if one_step as u8 > self.config.max_boost_class as u8 {
                            self.config.max_boost_class
                        } else {
                            one_step
                        }
                    };

                    if target_priority != proc.current_priority {
                        plan.push(PolicyMutation::SetPriorityClass {
                            pid: new_pid,
                            previous: proc.current_priority,
                            new: target_priority,
                        });
                        proc.current_priority = target_priority;
                        proc.is_boosted = true;
                    }
                }
            }
        }

        self.previous_foreground_pid = old_fg_pid;
        self.current_foreground_pid = new_fg_pid;

        plan
    }

    /// Process exit hook: completely purges internal state and emits cleanup mutations.
    ///
    /// Guarantees that any new process reusing this PID starts fresh with no inherited caps.
    pub fn on_process_exit(&mut self, pid: Pid, timestamp_ms: u64) -> MutationPlan {
        let mut plan = MutationPlan::new(Vec::new(), self.config.dry_run, timestamp_ms);

        if let Some(proc) = self.tracked.remove(&pid) {
            // Emit release job to free any OS handle/attachment
            plan.push(PolicyMutation::ReleaseJob { pid });

            // If it was demoted or boosted, emit restore so OS state is clean
            if proc.is_demoted || proc.is_boosted {
                plan.push(PolicyMutation::SetPriorityClass {
                    pid,
                    previous: proc.current_priority,
                    new: proc.base_priority,
                });
            }

            if proc.eco_qos.is_enabled() {
                plan.push(PolicyMutation::SetEcoQos {
                    pid,
                    previous: EcoQosState::Enabled,
                    new: EcoQosState::Disabled,
                });
            }
        }

        if self.current_foreground_pid == Some(pid) {
            self.current_foreground_pid = None;
        }
        if self.previous_foreground_pid == Some(pid) {
            self.previous_foreground_pid = None;
        }

        plan
    }

    /// Evaluates a periodic system telemetry snapshot and emits any required mutations.
    pub fn evaluate_tick(&mut self, snapshot: &SystemSnapshot) -> MutationPlan {
        self.current_tick = self.current_tick.wrapping_add(1);
        let mut plan = MutationPlan::new(Vec::new(), self.config.dry_run, snapshot.timestamp_ms);

        // 1. Update Game Mode state
        let should_game_mode = self.detect_game_mode(snapshot);
        let gm_plan = self.set_game_mode(should_game_mode, snapshot.timestamp_ms);
        for m in gm_plan.mutations {
            plan.push(m);
        }

        // 2. Handle foreground window transition if changed
        if snapshot.foreground_pid != self.current_foreground_pid {
            let fg_plan =
                self.on_foreground_changed(snapshot.foreground_pid, snapshot.timestamp_ms);
            for m in fg_plan.mutations {
                plan.push(m);
            }
        }

        // Update process names and last seen tick from snapshot
        for sample in &snapshot.processes {
            if sample.pid.is_system_kernel() {
                continue;
            }

            let tick = self.current_tick;
            let proc = self
                .tracked
                .entry(sample.pid)
                .or_insert_with(|| TrackedProcess::new(sample.pid, sample.name.clone(), tick));

            if proc.name == "unknown.exe" && !sample.name.is_empty() {
                proc.name = sample.name.clone();
            }
            proc.last_seen_tick = tick;
        }

        // If Game Mode is active, skip all demotions and EcoQoS throttles
        if self.game_mode_active {
            return plan;
        }

        // 3. ProBalance Evaluation
        let system_cpu_high =
            snapshot.total_cpu_pct >= self.config.pro_balance_system_cpu_threshold_pct;

        for sample in &snapshot.processes {
            let pid = sample.pid;
            if pid.is_system_kernel() {
                continue;
            }

            // SAFETY INVARIANTS:
            // 1. No critical process is ever demoted.
            // 2. No foreground process is ever demoted.
            // 3. Excluded processes are never demoted.
            // 4. Known games are never demoted.
            let is_fg = Some(pid) == self.current_foreground_pid || sample.is_foreground;
            let is_crit = self.rules.is_critical(pid, &sample.name);
            let is_excl = self.rules.is_excluded(pid, &sample.name);
            let is_game = self.rules.is_known_game(&sample.name);

            if is_fg || is_crit || is_excl || is_game {
                // If a protected process was somehow demoted, restore it immediately
                if let Some(proc) = self.tracked.get_mut(&pid) {
                    if proc.is_demoted {
                        plan.push(PolicyMutation::SetPriorityClass {
                            pid,
                            previous: proc.current_priority,
                            new: proc.base_priority,
                        });
                        proc.current_priority = proc.base_priority;
                        proc.is_demoted = false;
                        proc.consecutive_high_cpu_ticks = 0;
                    }
                }
                continue;
            }

            if let Some(proc) = self.tracked.get_mut(&pid) {
                if self.config.pro_balance_enabled {
                    let is_hog = system_cpu_high
                        && sample.cpu_usage_pct >= self.config.pro_balance_cpu_threshold_pct;

                    if is_hog {
                        proc.consecutive_high_cpu_ticks =
                            proc.consecutive_high_cpu_ticks.saturating_add(1);
                        proc.consecutive_normal_cpu_ticks = 0;

                        if proc.consecutive_high_cpu_ticks
                            >= self.config.pro_balance_sustained_ticks
                            && !proc.is_demoted
                        {
                            // ProBalance demotes by AT MOST ONE class
                            let demoted = proc.current_priority.step_down();
                            if demoted != proc.current_priority {
                                plan.push(PolicyMutation::SetPriorityClass {
                                    pid,
                                    previous: proc.current_priority,
                                    new: demoted,
                                });
                                proc.current_priority = demoted;
                                proc.is_demoted = true;
                            }
                        }
                    } else {
                        // Process is within normal CPU bounds or system CPU is no longer under pressure
                        proc.consecutive_high_cpu_ticks = 0;
                        if proc.is_demoted {
                            proc.consecutive_normal_cpu_ticks =
                                proc.consecutive_normal_cpu_ticks.saturating_add(1);

                            // Automatic restore once normal CPU is sustained
                            if proc.consecutive_normal_cpu_ticks
                                >= self.config.pro_balance_restore_ticks
                            {
                                plan.push(PolicyMutation::SetPriorityClass {
                                    pid,
                                    previous: proc.current_priority,
                                    new: proc.base_priority,
                                });
                                proc.current_priority = proc.base_priority;
                                proc.is_demoted = false;
                                proc.consecutive_normal_cpu_ticks = 0;
                            }
                        }
                    }
                }

                // 4. EcoQoS for Background Apps
                let is_active_computation = sample.cpu_usage_pct >= 10.0;
                if self.config.eco_qos_enabled && !is_fg && !is_crit && !is_excl {
                    if is_active_computation {
                        proc.background_idle_ticks = 0;
                    } else {
                        proc.background_idle_ticks = proc.background_idle_ticks.saturating_add(1);
                    }

                    if proc.background_idle_ticks >= self.config.eco_qos_background_idle_ticks
                        && !proc.eco_qos.is_enabled()
                    {
                        plan.push(PolicyMutation::SetEcoQos {
                            pid,
                            previous: EcoQosState::Disabled,
                            new: EcoQosState::Enabled,
                        });
                        proc.eco_qos = EcoQosState::Enabled;
                    }
                }
            }
        }

        plan
    }

    /// Resets all tracked processes to baseline: restores priorities, disables EcoQoS, releases jobs.
    ///
    /// Used for clean shutdown, watchdog recovery, and fail-safe unhooking.
    pub fn reset_all(&mut self, timestamp_ms: u64) -> MutationPlan {
        let mut plan = MutationPlan::new(Vec::new(), self.config.dry_run, timestamp_ms);

        for proc in self.tracked.values_mut() {
            if proc.current_priority != proc.base_priority {
                plan.push(PolicyMutation::SetPriorityClass {
                    pid: proc.pid,
                    previous: proc.current_priority,
                    new: proc.base_priority,
                });
                proc.current_priority = proc.base_priority;
            }

            if proc.eco_qos.is_enabled() {
                plan.push(PolicyMutation::SetEcoQos {
                    pid: proc.pid,
                    previous: EcoQosState::Enabled,
                    new: EcoQosState::Disabled,
                });
                proc.eco_qos = EcoQosState::Disabled;
            }

            plan.push(PolicyMutation::ReleaseJob { pid: proc.pid });

            proc.is_boosted = false;
            proc.is_demoted = false;
            proc.consecutive_high_cpu_ticks = 0;
            proc.consecutive_normal_cpu_ticks = 0;
        }

        self.current_foreground_pid = None;
        self.previous_foreground_pid = None;
        self.game_mode_active = false;

        plan
    }
}
