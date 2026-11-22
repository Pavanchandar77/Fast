//! A user's own `system.toml` (from an older version) wins, but the shell
//! keys that shipped later are still added when that key is free, so new
//! features like Alt+Space Spotlight work without editing the file.

use crate::schema::SystemConfig;

/// The shipped config, compiled in.
const SHIPPED: &str = include_str!("../../../config/system.toml");

fn has_explicit_mod(key: &str) -> bool {
    key.split('+').any(|p| {
        matches!(
            p.trim().to_ascii_lowercase().as_str(),
            "alt" | "opt" | "ctrl" | "control" | "super" | "win" | "windows"
        )
    })
}

fn norm(key: &str) -> String {
    let mut parts: Vec<String> = key
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let last = parts.pop().unwrap_or_default();
    parts.sort();
    parts.push(last);
    parts.join("+")
}

/// Add shipped bindings whose key the user has not bound. Keys without an
/// explicit modifier depend on `mod_key`, so they are only added when the
/// user's mod key matches the shipped one. Returns the keys added.
pub fn merge_missing_bindings(cfg: &mut SystemConfig, shipped: &SystemConfig) -> Vec<String> {
    let same_mod = cfg
        .keybindings
        .mod_key
        .eq_ignore_ascii_case(&shipped.keybindings.mod_key);
    let taken: std::collections::HashSet<String> =
        cfg.keybindings.bindings.keys().map(|k| norm(k)).collect();
    let mut added = Vec::new();
    let mut keys: Vec<&String> = shipped.keybindings.bindings.keys().collect();
    keys.sort();
    for key in keys {
        if taken.contains(&norm(key)) || !(same_mod || has_explicit_mod(key)) {
            continue;
        }
        let action = shipped.keybindings.bindings[key].clone();
        cfg.keybindings.bindings.insert(key.clone(), action);
        added.push(key.clone());
    }
    added
}

/// The shipped config, parsed (None if it ever fails to parse).
pub fn shipped() -> Option<SystemConfig> {
    SystemConfig::parse(SHIPPED).ok()
}

/// Load `path` and fill in missing shipped key bindings.
pub fn load_with_defaults(path: &std::path::Path) -> anyhow::Result<SystemConfig> {
    let mut cfg = SystemConfig::load_from_file(path)?;
    if let Some(shipped) = shipped() {
        let added = merge_missing_bindings(&mut cfg, &shipped);
        if !added.is_empty() {
            tracing::info!("config: added default key bindings {:?}", added);
        }
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_user_config_gets_new_shell_keys_but_keeps_its_own() {
        let shipped = shipped().expect("shipped config parses");
        let mut user = shipped.clone();
        user.keybindings.bindings.clear();
        user.keybindings
            .bindings
            .insert("alt + space".into(), "exec:my-launcher.exe".into());
        user.keybindings.mod_key = "Alt".into();
        let added = merge_missing_bindings(&mut user, &shipped);
        // The user's own Alt+Space survives.
        assert_eq!(
            user.keybindings.bindings["alt + space"],
            "exec:my-launcher.exe"
        );
        assert!(!added.iter().any(|k| norm(k) == "alt+space"));
        // Explicit-modifier keys are added; bare keys are not (mod differs).
        assert!(added.iter().any(|k| norm(k) == "alt+ctrl+v"));
        assert!(!added.iter().any(|k| k == "q"));
    }

    #[test]
    fn same_mod_gets_everything_missing() {
        let shipped = shipped().expect("shipped config parses");
        let mut user = shipped.clone();
        user.keybindings.bindings.retain(|k, _| k == "q");
        let added = merge_missing_bindings(&mut user, &shipped);
        assert_eq!(
            user.keybindings.bindings.len(),
            shipped.keybindings.bindings.len()
        );
        assert!(!added.contains(&"q".to_string()));
    }
}
