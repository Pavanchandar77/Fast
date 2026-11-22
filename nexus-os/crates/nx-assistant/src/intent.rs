//! Deterministic fallback router. Used when the 0.6B model emits an invalid or
//! unknown tool call, so common commands never fail just because a tiny model
//! slipped. Every result still goes through `Tool::validate` and risk checks.

use crate::tools::{PowerOp, Shield, Tool, WindowOp};

fn number_in(text: &str) -> Option<u32> {
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

fn after<'a>(text: &'a str, keys: &[&str]) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    keys.iter().find_map(|k| {
        lower
            .find(k)
            .map(|i| text[i + k.len()..].trim().trim_end_matches(['.', '?', '!']))
    })
}

fn on_off(lower: &str) -> Option<bool> {
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words
        .iter()
        .any(|w| matches!(*w, "off" | "disable" | "stop" | "block" | "kill"))
    {
        Some(false)
    } else if words
        .iter()
        .any(|w| matches!(*w, "on" | "enable" | "start" | "allow"))
    {
        Some(true)
    } else {
        None
    }
}

fn duration_secs(lower: &str) -> Option<u32> {
    let n = number_in(lower)?;
    if lower.contains("hour") {
        Some(n * 3600)
    } else if lower.contains("sec") {
        Some(n)
    } else {
        Some(n * 60)
    }
}

pub fn route(text: &str) -> Option<Tool> {
    let lower = text.to_ascii_lowercase();
    let l = lower.as_str();
    let has = |k: &str| l.contains(k);

    if let Some(about) = about_self(l) {
        return Some(about);
    }

    if has("search the web")
        || has("google ")
        || has("look up")
        || l.starts_with("search for")
        || l.starts_with("who ")
        || l.starts_with("what is")
        || l.starts_with("what's")
        || l.starts_with("when ")
    {
        if has("weather") {
            let city = after(text, &[" in "]).unwrap_or("").to_string();
            return Some(Tool::Weather { city });
        }
        let q = after(
            text,
            &["search the web for", "search for", "google", "look up"],
        )
        .unwrap_or(text)
        .to_string();
        return Some(Tool::WebSearch { query: q });
    }
    if has("weather") {
        let city = after(text, &[" in "]).unwrap_or("").to_string();
        return Some(Tool::Weather { city });
    }
    if has("volume") {
        if let Some(n) = number_in(l) {
            return Some(Tool::SetVolume {
                level: n.min(100) as u8,
            });
        }
    }
    if has("unmute") {
        return Some(Tool::Mute { on: false });
    }
    if has("mute") {
        return Some(Tool::Mute { on: true });
    }
    if has("brightness") || has("brighter") || has("dimmer") {
        if let Some(n) = number_in(l) {
            return Some(Tool::SetBrightness {
                level: n.min(100) as u8,
            });
        }
    }
    if has("wifi") || has("wi-fi") {
        return on_off(l).map(|on| Tool::SetWifi { on });
    }
    if has("bluetooth") {
        return on_off(l).map(|on| Tool::SetBluetooth { on });
    }
    for (word, shield) in [
        ("camera", Shield::Camera),
        ("webcam", Shield::Camera),
        ("mic", Shield::Microphone),
        ("location", Shield::Location),
    ] {
        if has(word) {
            if let Some(allow) = on_off(l) {
                return Some(Tool::PrivacyShield { shield, on: !allow });
            }
        }
    }
    if has("do not disturb") || has("focus") || has("dnd") {
        return Some(Tool::FocusMode {
            on: on_off(l).unwrap_or(true),
        });
    }
    if has("stopwatch") {
        return Some(Tool::Stopwatch {
            on: on_off(l).unwrap_or(true),
        });
    }
    if has("timer") {
        return duration_secs(l).map(|seconds| Tool::SetTimer {
            seconds,
            label: String::new(),
        });
    }
    if has("remind me") {
        let minutes = duration_secs(l).map(|s| (s / 60).max(1)).unwrap_or(10);
        let what = after(text, &[" to "]).unwrap_or("Reminder").to_string();
        return Some(Tool::Remind {
            minutes,
            text: what,
        });
    }
    if has("screenshot") || has("screen shot") {
        return Some(Tool::Screenshot {});
    }
    if has("lock") && (has("screen") || has("pc") || has("computer")) {
        return Some(Tool::LockScreen {});
    }
    if has("shut down") || has("shutdown") {
        return Some(Tool::Power {
            op: PowerOp::Shutdown,
        });
    }
    if has("restart") || has("reboot") {
        return Some(Tool::Power {
            op: PowerOp::Restart,
        });
    }
    if has("sleep") {
        return Some(Tool::Power { op: PowerOp::Sleep });
    }
    for (word, op) in [
        ("minimize", WindowOp::Minimize),
        ("maximize", WindowOp::Maximize),
        ("snap left", WindowOp::SnapLeft),
        ("snap right", WindowOp::SnapRight),
        ("center", WindowOp::Center),
        ("close", WindowOp::Close),
        ("restore", WindowOp::Restore),
    ] {
        if has(word) && (has("window") || has("this") || has("it")) {
            return Some(Tool::Window { op });
        }
    }
    if let Some(t) = after(text, &["type "]) {
        return Some(Tool::TypeText {
            text: t.trim_matches(['"', '\'']).to_string(),
        });
    }
    if let Some(k) = after(text, &["press "]) {
        return Some(Tool::PressKeys {
            keys: k.to_string(),
        });
    }
    if let Some(p) = after(text, &["delete "]) {
        return Some(Tool::DeleteFile {
            path: p.to_string(),
        });
    }
    for folder in [
        "downloads",
        "documents",
        "desktop",
        "pictures",
        "music",
        "videos",
    ] {
        if has(folder) && (has("open") || has("show") || has("go to")) {
            return Some(Tool::OpenFolder {
                path: folder.to_string(),
            });
        }
    }
    if has("spotlight") || l.starts_with("find ") {
        let q = after(text, &["for ", "find "]).unwrap_or("").to_string();
        if !q.is_empty() {
            return Some(Tool::Spotlight { query: q });
        }
    }
    if let Some(app) = after(text, &["open ", "launch ", "start "]) {
        let app = app.trim_start_matches("the ").trim_end_matches(" app");
        return Some(Tool::OpenApp {
            app: app.to_string(),
        });
    }
    None
}

/// Identity questions must not be sent to the web. The model was answering
/// from search snippets and could not say who it is.
fn about_self(l: &str) -> Option<Tool> {
    let asks = l.contains("who are you")
        || l.contains("what are you")
        || l.contains("your name")
        || l.contains("who made you")
        || l.contains("who created")
        || l.contains("your creator")
        || l.contains("who built you")
        || l.contains("what is nexus")
        || l.contains("what's nexus")
        || l.contains("who is nexus")
        || l == "nexus"
        || l.contains("are you an ai")
        || l.contains("are you ai");
    if !asks {
        return None;
    }
    Some(Tool::Reply {
        text: "I'm Nexus. I live in the Dynamic Island on this Windows machine. Pavan created me. I know this computer: the apps that are open, what's playing, your files, volume, brightness, and the things you ask me to do. I am not a web search wearing a name.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_common_commands() {
        assert_eq!(
            route("open my downloads folder"),
            Some(Tool::OpenFolder {
                path: "downloads".into()
            })
        );
        assert_eq!(
            route("set volume to 30"),
            Some(Tool::SetVolume { level: 30 })
        );
        assert_eq!(route("turn off wifi"), Some(Tool::SetWifi { on: false }));
        assert_eq!(
            route("block the camera"),
            Some(Tool::PrivacyShield {
                shield: Shield::Camera,
                on: true
            })
        );
        assert_eq!(
            route("set a timer for 5 minutes"),
            Some(Tool::SetTimer {
                seconds: 300,
                label: String::new()
            })
        );
        assert_eq!(
            route("who are you"),
            Some(Tool::Reply {
                text: "I'm Nexus. I live in the Dynamic Island on this Windows machine. Pavan created me. I know this computer: the apps that are open, what's playing, your files, volume, brightness, and the things you ask me to do. I am not a web search wearing a name.".into(),
            })
        );
        assert!(matches!(
            route("who is the president"),
            Some(Tool::WebSearch { .. })
        ));
        assert_eq!(
            route("open notepad"),
            Some(Tool::OpenApp {
                app: "notepad".into()
            })
        );
        assert!(matches!(
            route("search the web for rust 2026 edition"),
            Some(Tool::WebSearch { .. })
        ));
        assert!(matches!(
            route("shut down"),
            Some(Tool::Power {
                op: PowerOp::Shutdown
            })
        ));
    }
}
