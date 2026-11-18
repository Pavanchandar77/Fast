//! Configuration schema and safe defaults for the governor policy.

use crate::types::PriorityClass;
use serde::{Deserialize, Serialize};

/// Safe configuration defaults and thresholds for the governor policy engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GovernorPolicyConfig {
    /// Boost foreground window priority by at most one class.
    pub boost_foreground: bool,

    /// Maximum priority class to boost to for standard foreground apps.
    /// Safe default: `AboveNormal`. NEVER `High` unless whitelisted.
    pub max_boost_class: PriorityClass,

    /// Explicit whitelist of executables permitted to receive `High` priority boost.
    pub whitelist_high_priority: Vec<String>,

    /// Enable ProBalance-style temporary demotion of sustained CPU hogs.
    pub pro_balance_enabled: bool,

    /// Per-process CPU usage percentage threshold triggering ProBalance evaluation (e.g. 50%).
    pub pro_balance_cpu_threshold_pct: f32,

    /// Total system CPU percentage threshold before ProBalance initiates demotions (e.g. 60%).
    pub pro_balance_system_cpu_threshold_pct: f32,

    /// Number of consecutive high-CPU ticks required before demoting a CPU hog.
    pub pro_balance_sustained_ticks: u32,

    /// Number of consecutive normal-CPU ticks required before restoring demoted process.
    pub pro_balance_restore_ticks: u32,

    /// Enable EcoQoS (efficiency mode / ProcessPowerThrottling) for background apps.
    pub eco_qos_enabled: bool,

    /// Minimum idle ticks in background before applying EcoQoS.
    pub eco_qos_background_idle_ticks: u32,

    /// Automatically detect fullscreen exclusive apps / games and enter Game Mode.
    pub game_mode_auto_detect: bool,

    /// Never call NtSuspendProcess by default.
    pub allow_suspend: bool,

    /// Never assign BelowNormal to background processes by default (safe default).
    pub allow_below_normal_default: bool,

    /// Dry run mode: emit planned mutations without applying them to the operating system.
    pub dry_run: bool,

    /// Explicit override for Game Mode (None = auto-detect, Some(true) = force active, Some(false) = force inactive).
    pub game_mode_override: Option<bool>,

    /// Excluded processes protected from all demotions, EcoQoS, and throttling.
    pub exclude_rules: Vec<String>,
}

impl Default for GovernorPolicyConfig {
    fn default() -> Self {
        Self {
            boost_foreground: true,
            max_boost_class: PriorityClass::AboveNormal, // Safe default: never HIGH unless whitelisted
            whitelist_high_priority: Vec::new(),
            pro_balance_enabled: true,
            pro_balance_cpu_threshold_pct: 50.0,
            pro_balance_system_cpu_threshold_pct: 60.0,
            pro_balance_sustained_ticks: 2,
            pro_balance_restore_ticks: 2,
            eco_qos_enabled: true,
            eco_qos_background_idle_ticks: 2,
            game_mode_auto_detect: true,
            allow_suspend: false,              // Safe default: NEVER suspend
            allow_below_normal_default: false, // Safe default: no BELOW_NORMAL
            dry_run: false,
            game_mode_override: None,
            exclude_rules: vec![
                "spotify.exe".to_string(),
                "discord.exe".to_string(),
                "slack.exe".to_string(),
                "obs64.exe".to_string(),
                "steam.exe".to_string(),
                "cargo.exe".to_string(),
                "rustc.exe".to_string(),
                "cl.exe".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serde_roundtrip() {
        let config = GovernorPolicyConfig::default();
        let serialized = serde_json::to_string(&config).expect("Serialize");
        let deserialized: GovernorPolicyConfig =
            serde_json::from_str(&serialized).expect("Deserialize");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_config_safe_defaults() {
        let config = GovernorPolicyConfig::default();
        assert!(!config.allow_suspend);
        assert!(!config.allow_below_normal_default);
        assert_eq!(config.max_boost_class, PriorityClass::AboveNormal);
        assert!(!config.dry_run);
        assert!(config.boost_foreground);
        assert!(config.pro_balance_enabled);
        assert!(config.eco_qos_enabled);
    }
}
