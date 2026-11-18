//! Declarative configuration model and strict validation engine for Nexus-OS.
//! Ensures all configuration values (gaps, ratios, spring parameters, governor rates, hotkeys)
//! are fully validated and bounded before system application.

use crate::hotkeys::{Hotkey, Key};
use crate::spring::SpringConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SystemConfig {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub compositor: CompositorConfig,

    #[serde(default)]
    pub governor: GovernorConfig,

    #[serde(default)]
    pub vfs: VfsConfig,

    #[serde(default)]
    pub shield: ShieldConfig,

    #[serde(default)]
    pub hal: HalConfig,

    #[serde(default)]
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralConfig {
    pub name: String,
    pub version: String,
    pub log_level: String,
    pub fail_safe_hotkey: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            name: "Nexus Subsidiary OS".into(),
            version: "0.1.0".into(),
            log_level: "info".into(),
            fail_safe_hotkey: "Ctrl+Alt+Shift+Esc".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompositorConfig {
    pub layout: String,
    pub inner_gap: i32,
    pub outer_gap: i32,
    pub master_ratio: f32,
    pub force_disable_dwm_transitions: bool,
    pub suppress_native_taskbar: bool,
    #[serde(default)]
    pub spring: SpringConfigDto,
    #[serde(default)]
    pub decorations: DecorationsConfig,
    #[serde(default)]
    pub top_bar: TopBarConfig,
    #[serde(default)]
    pub dock: DockConfig,
    #[serde(default)]
    pub command_bar: CommandBarConfig,
}

impl Default for CompositorConfig {
    fn default() -> Self {
        Self {
            layout: "dwindle".into(),
            inner_gap: 8,
            outer_gap: 12,
            master_ratio: 0.55,
            force_disable_dwm_transitions: true,
            suppress_native_taskbar: true,
            spring: SpringConfigDto::default(),
            decorations: DecorationsConfig::default(),
            top_bar: TopBarConfig::default(),
            dock: DockConfig::default(),
            command_bar: CommandBarConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SpringConfigDto {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
    pub rest_velocity_threshold: f32,
    pub rest_displacement_threshold: f32,
}

impl Default for SpringConfigDto {
    fn default() -> Self {
        Self {
            stiffness: 280.0,
            damping: 26.0,
            mass: 1.0,
            rest_velocity_threshold: 0.05,
            rest_displacement_threshold: 0.5,
        }
    }
}

impl From<SpringConfigDto> for SpringConfig {
    fn from(dto: SpringConfigDto) -> Self {
        Self {
            stiffness: dto.stiffness,
            damping: dto.damping,
            mass: dto.mass,
            rest_velocity_threshold: dto.rest_velocity_threshold,
            rest_displacement_threshold: dto.rest_displacement_threshold,
        }
    }
}

impl From<SpringConfig> for SpringConfigDto {
    fn from(cfg: SpringConfig) -> Self {
        Self {
            stiffness: cfg.stiffness,
            damping: cfg.damping,
            mass: cfg.mass,
            rest_velocity_threshold: cfg.rest_velocity_threshold,
            rest_displacement_threshold: cfg.rest_displacement_threshold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecorationsConfig {
    pub corner_radius: f32,
    pub border_width: f32,
    pub active_border_color: String,
    pub inactive_border_color: String,
    pub blur_enabled: bool,
    pub blur_radius: f32,
    pub blur_opacity: f32,
}

impl Default for DecorationsConfig {
    fn default() -> Self {
        Self {
            corner_radius: 12.0,
            border_width: 1.0,
            active_border_color: "#5E81AC".into(),
            inactive_border_color: "#2E3440".into(),
            blur_enabled: true,
            blur_radius: 24.0,
            blur_opacity: 0.78,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopBarConfig {
    pub enabled: bool,
    pub height: i32,
    pub font_family: String,
    pub font_size: f32,
    pub widgets: Vec<String>,
}

impl Default for TopBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 28,
            font_family: "Segoe UI Variable Text".into(),
            font_size: 11.0,
            widgets: vec![
                "workspaces".into(),
                "window_title".into(),
                "cpu_ram".into(),
                "audio_slider".into(),
                "power_profile".into(),
                "clock".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DockConfig {
    pub enabled: bool,
    pub height: i32,
    pub icon_size: i32,
    pub magnification_scale: f32,
    pub magnification_distance: f32,
    pub autohide: bool,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 64,
            icon_size: 48,
            magnification_scale: 1.45,
            magnification_distance: 120.0,
            autohide: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandBarConfig {
    pub hotkey: String,
    pub max_results: usize,
    pub fuzzy_threshold: f32,
    pub placeholder: String,
}

impl Default for CommandBarConfig {
    fn default() -> Self {
        Self {
            hotkey: "Super+Space".into(),
            max_results: 8,
            fuzzy_threshold: 0.6,
            placeholder: "Search apps, files, kill processes, calculate...".into(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernorConfig {
    pub ram_purge_enabled: bool,
    pub ram_purge_delay_secs: u64,
    pub trim_on_minimize: bool,
    pub trim_on_unfocused: bool,
    pub app_nap_enabled: bool,
    pub app_nap_idle_secs: u64,
    pub cpu_throttle_percent: u32,
    pub exclude_from_nap: Vec<String>,
    pub dynamic_priority_boost: bool,
    pub foreground_priority: String,
    pub background_priority: String,
    pub boost_gpu_scheduling: bool,
    pub perceived_fast_close: bool,
    pub force_terminate_unresponsive_ms: u64,
    pub game_mode_auto_detect: bool,
    pub timer_resolution_0_5ms: bool,
    pub unpark_cpu_cores: bool,
    pub suspend_shell_shaders_on_fullscreen: bool,

    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub whitelist_high_priority: Vec<String>,
    #[serde(default = "default_true")]
    pub eco_qos_enabled: bool,
    #[serde(default = "default_true")]
    pub pro_balance_enabled: bool,
    #[serde(default)]
    pub allow_suspend: bool,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            ram_purge_enabled: false,
            ram_purge_delay_secs: 10,
            trim_on_minimize: false,
            trim_on_unfocused: false,
            app_nap_enabled: false,
            app_nap_idle_secs: 30,
            cpu_throttle_percent: 1,
            exclude_from_nap: vec![
                "spotify.exe".into(),
                "discord.exe".into(),
                "slack.exe".into(),
                "obs64.exe".into(),
                "steam.exe".into(),
                "cargo.exe".into(),
                "rustc.exe".into(),
                "cl.exe".into(),
            ],
            dynamic_priority_boost: true,
            foreground_priority: "AboveNormal".into(),
            background_priority: "Normal".into(),
            boost_gpu_scheduling: true,
            perceived_fast_close: true,
            force_terminate_unresponsive_ms: 5000,
            game_mode_auto_detect: true,
            timer_resolution_0_5ms: false,
            unpark_cpu_cores: false,
            suspend_shell_shaders_on_fullscreen: true,
            dry_run: false,
            whitelist_high_priority: Vec::new(),
            eco_qos_enabled: true,
            pro_balance_enabled: true,
            allow_suspend: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VfsConfig {
    pub mft_indexing_enabled: bool,
    pub volumes: Vec<String>,
    pub quicklook_hotkey: String,
    pub syntax_highlighting_theme: String,
    pub max_preview_size_mb: u64,
}

impl Default for VfsConfig {
    fn default() -> Self {
        Self {
            mft_indexing_enabled: true,
            volumes: vec!["C:".into()],
            quicklook_hotkey: "Space".into(),
            syntax_highlighting_theme: "Monokai".into(),
            max_preview_size_mb: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShieldConfig {
    pub block_windows_telemetry: bool,
    pub telemetry_blacklist: Vec<String>,
    pub protected_vault_paths: Vec<String>,
    pub edr_enabled: bool,
    pub block_lsass_access: bool,
    pub block_remote_thread_injection: bool,
    pub whitelist_build_dirs: Vec<String>,
}

impl Default for ShieldConfig {
    fn default() -> Self {
        Self {
            block_windows_telemetry: true,
            telemetry_blacklist: vec![
                "v10.events.data.microsoft.com".into(),
                "telemetry.microsoft.com".into(),
                "watson.telemetry.microsoft.com".into(),
                "settings-win.data.microsoft.com".into(),
            ],
            protected_vault_paths: vec![
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
                "~/.azure".into(),
                "~/.config/nexus-os".into(),
            ],
            edr_enabled: true,
            block_lsass_access: true,
            block_remote_thread_injection: true,
            whitelist_build_dirs: vec![
                "target".into(),
                "node_modules".into(),
                ".git".into(),
                "build".into(),
                "dist".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HalConfig {
    pub modern_standby_killer: bool,
    pub force_hibernate_on_critical_lid: bool,
    pub lock_bluetooth_a2dp: bool,
    pub default_power_profile: String,
}

impl Default for HalConfig {
    fn default() -> Self {
        Self {
            modern_standby_killer: true,
            force_hibernate_on_critical_lid: true,
            lock_bluetooth_a2dp: true,
            default_power_profile: "MaxCompute".into(),
        }
    }
}

fn default_mod_key() -> String {
    "Super".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeybindingsConfig {
    #[serde(default = "default_mod_key", alias = "mod")]
    pub mod_key: String,
    pub bindings: HashMap<String, String>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        bindings.insert("Return".into(), "exec:alacritty.exe".into());
        bindings.insert("q".into(), "window:close".into());
        bindings.insert("f".into(), "window:toggle_fullscreen".into());
        bindings.insert("v".into(), "window:toggle_floating".into());
        bindings.insert("h".into(), "focus:left".into());
        bindings.insert("l".into(), "focus:right".into());
        bindings.insert("k".into(), "focus:up".into());
        bindings.insert("j".into(), "focus:down".into());
        bindings.insert("1".into(), "workspace:1".into());
        bindings.insert("2".into(), "workspace:2".into());
        bindings.insert("3".into(), "workspace:3".into());
        bindings.insert("4".into(), "workspace:4".into());
        bindings.insert("5".into(), "workspace:5".into());
        bindings.insert("Shift+1".into(), "move_to_workspace:1".into());
        bindings.insert("Shift+2".into(), "move_to_workspace:2".into());
        bindings.insert("Shift+3".into(), "move_to_workspace:3".into());
        bindings.insert("Shift+4".into(), "move_to_workspace:4".into());
        bindings.insert("Shift+5".into(), "move_to_workspace:5".into());
        bindings.insert("Space".into(), "shell:command_bar".into());

        Self {
            mod_key: "Super".into(),
            bindings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigValidationError {
    #[error("Field '{field}' value {value} out of range [{min}, {max}]")]
    OutOfRange {
        field: &'static str,
        value: String,
        min: String,
        max: String,
    },
    #[error("Invalid hotkey in field '{field}': {reason}")]
    InvalidHotkey { field: &'static str, reason: String },
    #[error("Invalid layout name: '{0}'")]
    InvalidLayout(String),
    #[error("Empty required field: '{0}'")]
    EmptyField(&'static str),
}

impl SystemConfig {
    pub fn parse(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = Self::parse(&content)?;
        config
            .validate()
            .map_err(|errs| anyhow::anyhow!("Configuration validation failed: {:?}", errs))?;
        Ok(config)
    }

    /// Comprehensive validation of all configuration parameters.
    /// Ensures robust, crash-free execution.
    pub fn validate(&self) -> Result<(), Vec<ConfigValidationError>> {
        let mut errors = Vec::new();

        // 1. Gaps
        if self.compositor.inner_gap < 0 || self.compositor.inner_gap > 200 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "compositor.inner_gap",
                value: self.compositor.inner_gap.to_string(),
                min: "0".into(),
                max: "200".into(),
            });
        }
        if self.compositor.outer_gap < 0 || self.compositor.outer_gap > 200 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "compositor.outer_gap",
                value: self.compositor.outer_gap.to_string(),
                min: "0".into(),
                max: "200".into(),
            });
        }

        // 2. Master ratio
        if self.compositor.master_ratio < 0.1 || self.compositor.master_ratio > 0.9 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "compositor.master_ratio",
                value: self.compositor.master_ratio.to_string(),
                min: "0.1".into(),
                max: "0.9".into(),
            });
        }

        // 3. Layout name
        let valid_layouts = [
            "dwindle",
            "master_stack",
            "scrolling",
            "monocle",
            "floating",
        ];
        if !valid_layouts.contains(&self.compositor.layout.to_lowercase().as_str()) {
            errors.push(ConfigValidationError::InvalidLayout(
                self.compositor.layout.clone(),
            ));
        }

        // 4. Spring physics
        let spring = SpringConfig::from(self.compositor.spring);
        if let Err(e) = spring.validate() {
            errors.push(ConfigValidationError::InvalidHotkey {
                field: "compositor.spring",
                reason: e.to_string(),
            });
        }

        // 5. Governor rate
        if self.governor.cpu_throttle_percent < 1 || self.governor.cpu_throttle_percent > 100 {
            errors.push(ConfigValidationError::OutOfRange {
                field: "governor.cpu_throttle_percent",
                value: self.governor.cpu_throttle_percent.to_string(),
                min: "1".into(),
                max: "100".into(),
            });
        }

        // 6. Fail-safe hotkey: must be a valid hotkey with Escape key
        match Hotkey::parse(&self.general.fail_safe_hotkey) {
            Ok(hk) => {
                if hk.key != Key::Escape || hk.modifiers.is_empty() {
                    errors.push(ConfigValidationError::InvalidHotkey {
                        field: "general.fail_safe_hotkey",
                        reason: "Fail-safe hotkey must specify the Escape key with at least one modifier (e.g. Ctrl+Alt+Shift+Esc)".into(),
                    });
                }
            }
            Err(e) => {
                errors.push(ConfigValidationError::InvalidHotkey {
                    field: "general.fail_safe_hotkey",
                    reason: e.to_string(),
                });
            }
        }

        // 7. Command bar hotkey
        if let Err(e) = Hotkey::parse(&self.compositor.command_bar.hotkey) {
            errors.push(ConfigValidationError::InvalidHotkey {
                field: "compositor.command_bar.hotkey",
                reason: e.to_string(),
            });
        }

        // 8. Keybindings validation
        for (binding, action) in &self.keybindings.bindings {
            let full_combo = format!("{}+{}", self.keybindings.mod_key, binding);
            if let Err(e) = Hotkey::parse(&full_combo) {
                // Also try parsing without mod prefix (in case user explicitly gave Mod+...)
                if Hotkey::parse(binding).is_err() {
                    errors.push(ConfigValidationError::InvalidHotkey {
                        field: "keybindings.bindings",
                        reason: format!("Failed to parse hotkey '{}': {}", full_combo, e),
                    });
                }
            }
            if action.trim().is_empty() {
                errors.push(ConfigValidationError::EmptyField("keybindings action"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = SystemConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_gap_detected() {
        let mut config = SystemConfig::default();
        config.compositor.inner_gap = -5;
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::OutOfRange {
                field: "compositor.inner_gap",
                ..
            }
        )));
    }

    #[test]
    fn test_invalid_failsafe_hotkey() {
        let mut config = SystemConfig::default();
        config.general.fail_safe_hotkey = "Ctrl+Alt+A".into(); // Not Escape!
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::InvalidHotkey {
                field: "general.fail_safe_hotkey",
                ..
            }
        )));
    }

    #[test]
    fn test_invalid_layout_name() {
        let mut config = SystemConfig::default();
        config.compositor.layout = "non_existent_layout".into();
        let errs = config.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ConfigValidationError::InvalidLayout(_))));
    }

    #[test]
    fn test_outer_gap_out_of_range() {
        let mut config = SystemConfig::default();
        config.compositor.outer_gap = 250;
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::OutOfRange {
                field: "compositor.outer_gap",
                ..
            }
        )));
    }

    #[test]
    fn test_master_ratio_out_of_range() {
        let mut config = SystemConfig::default();
        config.compositor.master_ratio = 0.05;
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::OutOfRange {
                field: "compositor.master_ratio",
                ..
            }
        )));
    }

    #[test]
    fn test_cpu_throttle_percent_out_of_range() {
        let mut config = SystemConfig::default();
        config.governor.cpu_throttle_percent = 0;
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::OutOfRange {
                field: "governor.cpu_throttle_percent",
                ..
            }
        )));
    }

    #[test]
    fn test_command_bar_invalid_hotkey() {
        let mut config = SystemConfig::default();
        config.compositor.command_bar.hotkey = "InvalidHotkey123".into();
        let errs = config.validate().unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            ConfigValidationError::InvalidHotkey {
                field: "compositor.command_bar.hotkey",
                ..
            }
        )));
    }

    #[test]
    fn test_spring_dto_conversion() {
        let dto = SpringConfigDto {
            stiffness: 300.0,
            damping: 25.0,
            mass: 1.5,
            rest_velocity_threshold: 0.01,
            rest_displacement_threshold: 0.1,
        };
        let cfg = SpringConfig::from(dto);
        assert_eq!(cfg.stiffness, 300.0);
        assert_eq!(cfg.mass, 1.5);
        let dto_back = SpringConfigDto::from(cfg);
        assert_eq!(dto, dto_back);
    }

    #[test]
    fn test_empty_action_in_keybindings() {
        let mut config = SystemConfig::default();
        config.keybindings.bindings.insert("x".into(), "   ".into());
        let errs = config.validate().unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ConfigValidationError::EmptyField(_))));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn proptest_arbitrary_toml_parse_never_panics(s in "\\PC*") {
            let _ = SystemConfig::parse(&s);
        }

        #[test]
        fn proptest_randomized_config_validation_never_panics(
            inner_gap in -100i32..500i32,
            outer_gap in -100i32..500i32,
            master_ratio in -2.0f32..3.0f32,
            throttle_percent in 0u32..200u32,
            ref failsafe in "\\PC*",
        ) {
            let mut cfg = SystemConfig::default();
            cfg.compositor.inner_gap = inner_gap;
            cfg.compositor.outer_gap = outer_gap;
            cfg.compositor.master_ratio = master_ratio;
            cfg.governor.cpu_throttle_percent = throttle_percent;
            cfg.general.fail_safe_hotkey = failsafe.clone();

            let _ = cfg.validate();
        }
    }
}
