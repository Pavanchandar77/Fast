//! Strict, validated tool calls. The model may only *propose* one of these;
//! everything is parsed with serde, range/charset-checked, and classified by
//! risk before anything touches the desktop.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AssistantError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowOp {
    Minimize,
    Maximize,
    Restore,
    SnapLeft,
    SnapRight,
    Center,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shield {
    Camera,
    Microphone,
    Location,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerOp {
    Sleep,
    Restart,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
pub enum Tool {
    OpenApp {
        app: String,
    },
    OpenFolder {
        path: String,
    },
    OpenFile {
        path: String,
    },
    SetVolume {
        level: u8,
    },
    SetBrightness {
        level: u8,
    },
    Mute {
        on: bool,
    },
    SetWifi {
        on: bool,
    },
    SetBluetooth {
        on: bool,
    },
    PrivacyShield {
        shield: Shield,
        on: bool,
    },
    FocusMode {
        on: bool,
    },
    Spotlight {
        query: String,
    },
    SetTimer {
        seconds: u32,
        #[serde(default)]
        label: String,
    },
    Stopwatch {
        #[serde(default = "yes")]
        on: bool,
    },
    Remind {
        minutes: u32,
        text: String,
    },
    TypeText {
        text: String,
    },
    PressKeys {
        keys: String,
    },
    Window {
        op: WindowOp,
    },
    Screenshot {},
    LockScreen {},
    WebSearch {
        query: String,
    },
    Weather {
        #[serde(default)]
        city: String,
    },
    DeleteFile {
        path: String,
    },
    Power {
        op: PowerOp,
    },
    Reply {
        text: String,
    },
}

fn yes() -> bool {
    true
}

/// How careful the island must be before running a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    /// Runs immediately (still audit-logged).
    Safe,
    /// Needs an explicit CONFIRM tap in the island.
    Confirm,
}

pub const TOOL_NAMES: &[&str] = &[
    "open_app",
    "open_folder",
    "open_file",
    "set_volume",
    "set_brightness",
    "mute",
    "set_wifi",
    "set_bluetooth",
    "privacy_shield",
    "focus_mode",
    "spotlight",
    "set_timer",
    "stopwatch",
    "remind",
    "type_text",
    "press_keys",
    "window",
    "screenshot",
    "lock_screen",
    "web_search",
    "weather",
    "delete_file",
    "power",
    "reply",
];

/// Compact Qwen-style tool schema lines for the system prompt.
pub fn schema_lines() -> String {
    let defs: &[(&str, &str, &str)] = &[
        ("open_app", "Launch an app by name (notepad, calculator, edge, chrome, terminal, settings, paint, explorer, spotify)", r#"{"app":"string"}"#),
        ("open_folder", "Open a folder in Explorer. Use downloads, documents, desktop, pictures, music, videos, home, or a full path", r#"{"path":"string"}"#),
        ("open_file", "Open a file with its default app", r#"{"path":"string"}"#),
        ("set_volume", "Set system volume percent", r#"{"level":"integer 0-100"}"#),
        ("set_brightness", "Set screen brightness percent", r#"{"level":"integer 0-100"}"#),
        ("mute", "Mute or unmute audio", r#"{"on":"boolean"}"#),
        ("set_wifi", "Turn Wi-Fi on or off", r#"{"on":"boolean"}"#),
        ("set_bluetooth", "Turn Bluetooth on or off", r#"{"on":"boolean"}"#),
        ("privacy_shield", "Block (on=true) or allow camera, microphone or location", r#"{"shield":"camera|microphone|location","on":"boolean"}"#),
        ("focus_mode", "Do Not Disturb on/off", r#"{"on":"boolean"}"#),
        ("spotlight", "Open Spotlight search for files and apps", r#"{"query":"string"}"#),
        ("set_timer", "Start a countdown timer", r#"{"seconds":"integer","label":"string"}"#),
        ("stopwatch", "Start or stop the stopwatch", r#"{"on":"boolean"}"#),
        ("remind", "Remind me after some minutes", r#"{"minutes":"integer","text":"string"}"#),
        ("type_text", "Type text into the focused window", r#"{"text":"string"}"#),
        ("press_keys", "Press a key chord like ctrl+s or alt+tab", r#"{"keys":"string"}"#),
        ("window", "Act on the focused window", r#"{"op":"minimize|maximize|restore|snap_left|snap_right|center|close"}"#),
        ("screenshot", "Take a screenshot", "{}"),
        ("lock_screen", "Lock the PC", "{}"),
        ("web_search", "Search the web for facts, news or anything not on this PC", r#"{"query":"string"}"#),
        ("weather", "Current weather", r#"{"city":"string"}"#),
        ("delete_file", "Delete a file (moves to Recycle Bin)", r#"{"path":"string"}"#),
        ("power", "Sleep, restart or shut down", r#"{"op":"sleep|restart|shutdown"}"#),
        ("reply", "Answer conversationally when no other tool fits", r#"{"text":"string"}"#),
    ];
    defs.iter()
        .map(|(n, d, p)| format!(r#"{{"name":"{n}","description":"{d}","parameters":{p}}}"#))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the first balanced JSON object from model output.
fn first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in text[start..].char_indices() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Coerce common small-model slips (numbers as strings, "true" strings,
/// percentages) before strict deserialization.
fn normalize(mut v: Value) -> Value {
    if let Some(args) = v.get_mut("arguments").and_then(Value::as_object_mut) {
        for (k, val) in args.iter_mut() {
            if let Value::String(s) = val {
                let t = s.trim().trim_end_matches('%');
                let numeric_key = matches!(k.as_str(), "level" | "seconds" | "minutes");
                if numeric_key {
                    if let Ok(n) = t.parse::<f64>() {
                        *val = Value::from(n.round().max(0.0) as u64);
                    }
                } else if k == "on" {
                    match t.to_ascii_lowercase().as_str() {
                        "true" | "on" | "yes" => *val = Value::Bool(true),
                        "false" | "off" | "no" => *val = Value::Bool(false),
                        _ => {}
                    }
                }
            } else if let Value::Number(n) = val {
                if let Some(f) = n.as_f64() {
                    if f.fract() != 0.0 || f < 0.0 {
                        *val = Value::from(f.round().max(0.0) as u64);
                    }
                }
            }
        }
    } else if v.get("arguments").is_none() {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("arguments".into(), Value::Object(Default::default()));
        }
    }
    v
}

/// Parse `{"name": ..., "arguments": {...}}` (optionally wrapped in `<tool_call>`).
pub fn parse_tool_call(text: &str) -> Result<Tool, AssistantError> {
    let json = first_json_object(text)
        .ok_or_else(|| AssistantError::BadToolCall("no JSON object".into()))?;
    let value: Value =
        serde_json::from_str(json).map_err(|e| AssistantError::BadToolCall(e.to_string()))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !TOOL_NAMES.contains(&name) {
        return Err(AssistantError::BadToolCall(format!(
            "unknown tool '{name}'"
        )));
    }
    let tool: Tool = serde_json::from_value(normalize(value))
        .map_err(|e| AssistantError::BadToolCall(e.to_string()))?;
    tool.validate()?;
    Ok(tool)
}

const MAX_TEXT: usize = 2000;

fn safe_name(s: &str) -> bool {
    !s.trim().is_empty()
        && s.len() <= 80
        && s.chars()
            .all(|c| c.is_alphanumeric() || " ._-+:".contains(c))
}

fn safe_path(s: &str) -> bool {
    !s.trim().is_empty()
        && s.len() <= 400
        && !s.contains(['\0', '|', '<', '>', '"', '\n', '\r', '`', '$', '&', ';'])
}

impl Tool {
    pub fn name(&self) -> &'static str {
        let v = serde_json::to_value(self).ok();
        let n = v
            .as_ref()
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("reply");
        TOOL_NAMES
            .iter()
            .copied()
            .find(|t| *t == n)
            .unwrap_or("reply")
    }

    pub fn validate(&self) -> Result<(), AssistantError> {
        let bad = |m: &str| Err(AssistantError::BadToolCall(m.to_string()));
        match self {
            Tool::OpenApp { app } if !safe_name(app) => bad("app name has unsafe characters"),
            Tool::OpenFolder { path } | Tool::OpenFile { path } | Tool::DeleteFile { path }
                if !safe_path(path) =>
            {
                bad("path has unsafe characters")
            }
            Tool::SetVolume { level } | Tool::SetBrightness { level } if *level > 100 => {
                bad("level must be 0-100")
            }
            Tool::SetTimer { seconds, .. } if *seconds == 0 || *seconds > 86_400 => {
                bad("timer must be 1s..24h")
            }
            Tool::Remind { minutes, text }
                if *minutes == 0 || *minutes > 10_080 || text.len() > 200 =>
            {
                bad("reminder out of range")
            }
            Tool::TypeText { text } if text.is_empty() || text.len() > MAX_TEXT => {
                bad("text length")
            }
            Tool::PressKeys { keys } if crate::exec::parse_chord(keys).is_none() => {
                bad("unknown key chord")
            }
            Tool::WebSearch { query } | Tool::Spotlight { query }
                if query.trim().is_empty() || query.len() > 300 =>
            {
                bad("query length")
            }
            Tool::Weather { city } if city.len() > 80 => bad("city length"),
            _ => Ok(()),
        }
    }

    /// Destructive, irreversible, or "sends something" actions need a confirm.
    pub fn risk(&self) -> Risk {
        match self {
            Tool::DeleteFile { .. } => Risk::Confirm,
            Tool::Power { op } if *op != PowerOp::Sleep => Risk::Confirm,
            Tool::Window {
                op: WindowOp::Close,
            } => Risk::Confirm,
            // Enter/newline can submit a form or send a chat message.
            Tool::TypeText { text } if text.contains('\n') => Risk::Confirm,
            Tool::PressKeys { keys } if keys.to_ascii_lowercase().contains("enter") => {
                Risk::Confirm
            }
            _ => Risk::Safe,
        }
    }

    /// Only web_search/weather leave the device.
    pub fn uses_network(&self) -> bool {
        matches!(self, Tool::WebSearch { .. } | Tool::Weather { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Tool::OpenApp { app } => format!("Open {app}"),
            Tool::OpenFolder { path } => format!("Open folder {path}"),
            Tool::OpenFile { path } => format!("Open {path}"),
            Tool::SetVolume { level } => format!("Volume → {level}%"),
            Tool::SetBrightness { level } => format!("Brightness → {level}%"),
            Tool::Mute { on } => (if *on { "Mute" } else { "Unmute" }).into(),
            Tool::SetWifi { on } => format!("Wi-Fi {}", onoff(*on)),
            Tool::SetBluetooth { on } => format!("Bluetooth {}", onoff(*on)),
            Tool::PrivacyShield { shield, on } => {
                format!("{shield:?} {}", if *on { "blocked" } else { "allowed" })
            }
            Tool::FocusMode { on } => format!("Focus {}", onoff(*on)),
            Tool::Spotlight { query } => format!("Spotlight “{query}”"),
            Tool::SetTimer { seconds, .. } => format!("Timer {}", fmt_secs(*seconds)),
            Tool::Stopwatch { on } => {
                format!("Stopwatch {}", if *on { "started" } else { "stopped" })
            }
            Tool::Remind { minutes, text } => format!("Remind in {minutes} min: {text}"),
            Tool::TypeText { text } => format!("Type “{}”", truncate(text, 40)),
            Tool::PressKeys { keys } => format!("Press {keys}"),
            Tool::Window { op } => format!("Window {op:?}"),
            Tool::Screenshot {} => "Screenshot".into(),
            Tool::LockScreen {} => "Lock screen".into(),
            Tool::WebSearch { query } => format!("Search web “{query}”"),
            Tool::Weather { city } => format!("Weather {city}"),
            Tool::DeleteFile { path } => format!("Delete {path}"),
            Tool::Power { op } => format!("{op:?}"),
            Tool::Reply { .. } => "Reply".into(),
        }
    }
}

fn onoff(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

pub fn fmt_secs(s: u32) -> String {
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapped_tool_call() {
        let t = parse_tool_call(
            "<tool_call>\n{\"name\": \"set_volume\", \"arguments\": {\"level\": 30}}\n</tool_call>",
        )
        .unwrap();
        assert_eq!(t, Tool::SetVolume { level: 30 });
        assert_eq!(t.risk(), Risk::Safe);
        assert_eq!(t.name(), "set_volume");
    }

    #[test]
    fn coerces_string_numbers_and_bools() {
        let t =
            parse_tool_call(r#"{"name":"set_brightness","arguments":{"level":"45%"}}"#).unwrap();
        assert_eq!(t, Tool::SetBrightness { level: 45 });
        let t = parse_tool_call(r#"{"name":"set_wifi","arguments":{"on":"off"}}"#).unwrap();
        assert_eq!(t, Tool::SetWifi { on: false });
        let t = parse_tool_call(r#"{"name":"screenshot"}"#).unwrap();
        assert_eq!(t, Tool::Screenshot {});
    }

    #[test]
    fn rejects_unknown_tools_and_bad_args() {
        assert!(parse_tool_call(r#"{"name":"format_disk","arguments":{}}"#).is_err());
        assert!(parse_tool_call(r#"{"name":"set_volume","arguments":{"level":300}}"#).is_err());
        assert!(
            parse_tool_call(r#"{"name":"open_app","arguments":{"app":"calc & del *"}}"#).is_err()
        );
        assert!(parse_tool_call(r#"{"name":"open_folder","arguments":{"path":"a|b"}}"#).is_err());
        assert!(
            parse_tool_call(r#"{"name":"press_keys","arguments":{"keys":"ctrl+banana"}}"#).is_err()
        );
        assert!(parse_tool_call("no json here").is_err());
    }

    #[test]
    fn destructive_tools_need_confirm() {
        assert_eq!(
            Tool::DeleteFile {
                path: "a.txt".into()
            }
            .risk(),
            Risk::Confirm
        );
        assert_eq!(
            Tool::Power {
                op: PowerOp::Shutdown
            }
            .risk(),
            Risk::Confirm
        );
        assert_eq!(Tool::Power { op: PowerOp::Sleep }.risk(), Risk::Safe);
        assert_eq!(
            Tool::Window {
                op: WindowOp::Close
            }
            .risk(),
            Risk::Confirm
        );
        assert_eq!(
            Tool::PressKeys {
                keys: "enter".into()
            }
            .risk(),
            Risk::Confirm
        );
        assert_eq!(
            Tool::TypeText {
                text: "hi\n".into()
            }
            .risk(),
            Risk::Confirm
        );
        assert_eq!(Tool::TypeText { text: "hi".into() }.risk(), Risk::Safe);
    }

    #[test]
    fn only_search_and_weather_use_network() {
        assert!(Tool::WebSearch { query: "x".into() }.uses_network());
        assert!(!Tool::OpenApp {
            app: "notepad".into()
        }
        .uses_network());
    }
}
