//! What Spotlight searches on the real machine: installed apps (Start Menu
//! shortcuts on Windows, `.desktop` entries on Linux), recent documents, and
//! Windows Settings pages. Items get short ids (`app:Name`, `doc:Name`) so
//! fuzzy matching runs on names, not on long install paths; the id → path map
//! lives here for the launcher.

use crate::spotlight::Spotlight;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn targets() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The file or URI behind a catalog id (`app:…`, `doc:…`).
pub fn target_of(id: &str) -> Option<String> {
    targets().lock().ok()?.get(id).cloned()
}

fn remember(id: &str, path: &str) {
    if let Ok(mut map) = targets().lock() {
        map.insert(id.to_string(), path.to_string());
    }
}

/// Windows Settings page for a Spotlight setting id.
pub fn settings_uri(id: &str) -> Option<&'static str> {
    Some(match id {
        "dock" => "ms-settings:taskbar",
        "mission-control" => "ms-settings:multitasking",
        "camera" => "ms-settings:privacy-webcam",
        "microphone" => "ms-settings:privacy-microphone",
        "location" => "ms-settings:privacy-location",
        "clipboard" => "ms-settings:clipboard",
        "volume" => "ms-settings:sound",
        "game-mode" => "ms-settings:gaming-gamemode",
        "displays" => "ms-settings:display",
        "keyboard" => "ms-settings:keyboard",
        "wallpaper" => "ms-settings:personalization-background",
        "notifications" => "ms-settings:notifications",
        "focus" => "ms-settings:quiethours",
        "spotlight" => "ms-settings:search",
        "trackpad" => "ms-settings:devices-touchpad",
        "mouse" => "ms-settings:mousetouchpad",
        "network" => "ms-settings:network",
        "bluetooth" => "ms-settings:bluetooth",
        "users" => "ms-settings:otherusers",
        "datetime" => "ms-settings:dateandtime",
        "language" => "ms-settings:regionlanguage",
        "accessibility" => "ms-settings:easeofaccess",
        "storage" => "ms-settings:storagesense",
        "battery" => "ms-settings:batterysaver",
        "settings" => "ms-settings:",
        _ => return None,
    })
}

/// Names that are installer noise rather than apps.
fn is_noise(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "uninstall",
        "readme",
        "read me",
        "release notes",
        "license",
        "help",
        "website",
        "documentation",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

fn walk(dir: &Path, depth: usize, exts: &[&str], out: &mut Vec<PathBuf>) {
    if depth > 4 || out.len() > 4000 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&path, depth + 1, exts, out);
        } else if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_ascii_lowercase();
            if exts.contains(&ext.as_str()) {
                out.push(path);
            }
        }
    }
}

/// Installed apps as (name, launch path), sorted, one entry per name.
pub fn installed_apps() -> Vec<(String, String)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(pd) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(ad) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(ad).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    let mut files = Vec::new();
    for root in &roots {
        walk(root, 0, &["lnk", "url", "appref-ms"], &mut files);
    }
    let mut apps: Vec<(String, String)> = files
        .iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().to_string();
            (!is_noise(&name)).then(|| (name, p.to_string_lossy().to_string()))
        })
        .collect();
    if cfg!(not(windows)) {
        let mut desktop = Vec::new();
        walk(
            Path::new("/usr/share/applications"),
            0,
            &["desktop"],
            &mut desktop,
        );
        apps.extend(desktop.iter().filter_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            if text.lines().any(|l| l.trim() == "NoDisplay=true") {
                return None;
            }
            let name = text.lines().find_map(|l| l.strip_prefix("Name="))?;
            Some((name.to_string(), p.to_string_lossy().to_string()))
        }));
    }
    dedupe(apps)
}

fn dedupe(mut items: Vec<(String, String)>) -> Vec<(String, String)> {
    items.sort_by_key(|a| a.0.to_lowercase());
    items.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    items
}

/// Fill Spotlight from the machine: apps, recent documents, settings and the
/// Nexus apps. Replaces the demo catalog.
pub fn populate(spotlight: &mut Spotlight) {
    let apps = installed_apps();
    let docs = crate::dock::recent_items(40);
    fill(spotlight, &apps, &docs);
}

/// Deterministic part of [`populate`], for tests.
pub fn fill(spotlight: &mut Spotlight, apps: &[(String, String)], docs: &[(String, String)]) {
    spotlight.add_app("explorer", "Files", "Nexus");
    spotlight.add_app("privacy", "Privacy Shield", "Nexus");
    for (name, path) in apps {
        let id = format!("app:{name}");
        remember(&id, path);
        spotlight.add_app(&id, name, "Application");
    }
    for (name, path) in docs {
        let id = format!("doc:{name}");
        remember(&id, path);
        spotlight.add_file(&id, name, "Recent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_and_docs_get_short_ids_and_resolve() {
        let mut s = Spotlight::new();
        fill(
            &mut s,
            &[("Visual Studio Code".into(), "C:\\x\\Code.lnk".into())],
            &[("report.pdf".into(), "C:\\r\\report.pdf.lnk".into())],
        );
        let hits = s.query_str("vsc");
        assert_eq!(hits[0].id, "app:Visual Studio Code");
        assert_eq!(
            target_of("app:Visual Studio Code").as_deref(),
            Some("C:\\x\\Code.lnk")
        );
        assert_eq!(s.query_str("report")[0].id, "doc:report.pdf");
        assert_eq!(settings_uri("bluetooth"), Some("ms-settings:bluetooth"));
        assert!(is_noise("Uninstall Foo"));
    }

    #[test]
    fn dedupe_keeps_one_per_name() {
        let out = dedupe(vec![
            ("Chrome".into(), "a".into()),
            ("chrome".into(), "b".into()),
            ("Arc".into(), "c".into()),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "Arc");
    }
}
