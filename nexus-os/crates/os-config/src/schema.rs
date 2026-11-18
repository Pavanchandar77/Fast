//! Schema re-exports from the pure `nexus-model` domain.
pub use nexus_model::config::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_defaults_cannot_brick() {
        let cfg = GovernorConfig::default();
        assert!(!cfg.app_nap_enabled);
        assert!(!cfg.allow_nt_suspend);
        assert!(!cfg.dynamic_priority_boost);
        assert_eq!(cfg.background_priority, "Normal");
        assert_eq!(cfg.foreground_priority, "AboveNormal");
        assert!(cfg.priority_whitelist.is_empty());
    }

    #[test]
    fn shipped_system_toml_uses_safe_governor_defaults() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/system.toml");
        let cfg = SystemConfig::load_from_file(&path).expect("parse shipped config");
        assert!(!cfg.governor.app_nap_enabled);
        assert!(!cfg.governor.allow_nt_suspend);
        assert!(!cfg.governor.dynamic_priority_boost);
        assert!(!cfg
            .governor
            .background_priority
            .eq_ignore_ascii_case("BelowNormal"));
        assert!(!cfg
            .governor
            .foreground_priority
            .eq_ignore_ascii_case("High"));
        assert!(cfg.governor.priority_whitelist.is_empty());
        assert_eq!(cfg.general.fail_safe_hotkey, "Ctrl+Alt+Shift+Esc");
    }
}
