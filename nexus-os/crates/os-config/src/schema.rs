use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            ram_purge_enabled: true,
            ram_purge_delay_secs: 10,
            trim_on_minimize: true,
            trim_on_unfocused: true,
            app_nap_enabled: true,
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
            foreground_priority: "High".into(),
            background_priority: "BelowNormal".into(),
            boost_gpu_scheduling: true,
            perceived_fast_close: true,
            force_terminate_unresponsive_ms: 400,
            game_mode_auto_detect: true,
            timer_resolution_0_5ms: true,
            unpark_cpu_cores: true,
            suspend_shell_shaders_on_fullscreen: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            compositor: CompositorConfig::default(),
            governor: GovernorConfig::default(),
            vfs: VfsConfig::default(),
            shield: ShieldConfig::default(),
            hal: HalConfig::default(),
            keybindings: KeybindingsConfig::default(),
        }
    }
}

impl SystemConfig {
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(content)?;
        Ok(config)
    }

    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }
}
