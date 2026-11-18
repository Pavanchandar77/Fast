use crate::geometry::Direction;
use crate::layout::LayoutKind;
use crate::snapping::SnapZone;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configurable modifier key used for tiling hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ModKey {
    #[default]
    Super, // Windows key
    Alt,
    Ctrl,
    Shift,
}

impl std::str::FromStr for ModKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "super" | "win" | "windows" | "mod" => Ok(Self::Super),
            "alt" | "opt" | "option" => Ok(Self::Alt),
            "ctrl" | "control" => Ok(Self::Ctrl),
            "shift" => Ok(Self::Shift),
            other => Err(format!("Unknown modifier key: '{other}'")),
        }
    }
}

/// Bitmask-style modifier state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub super_key: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        super_key: false,
        alt: false,
        ctrl: false,
        shift: false,
    };

    pub fn with_super(mut self) -> Self {
        self.super_key = true;
        self
    }

    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn matches_mod_key(&self, mod_key: ModKey) -> bool {
        match mod_key {
            ModKey::Super => self.super_key,
            ModKey::Alt => self.alt,
            ModKey::Ctrl => self.ctrl,
            ModKey::Shift => self.shift,
        }
    }
}

/// Normalized virtual key codes for keyboard bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    Char(char),
    Return,
    Space,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    F(u8),
}

impl Key {
    pub fn from_vk(vk: u32) -> Option<Self> {
        match vk {
            0x08 => Some(Self::Backspace),
            0x09 => Some(Self::Tab),
            0x0D => Some(Self::Return),
            0x1B => Some(Self::Escape),
            0x20 => Some(Self::Space),
            0x25 => Some(Self::Left),
            0x26 => Some(Self::Up),
            0x27 => Some(Self::Right),
            0x28 => Some(Self::Down),
            0x2E => Some(Self::Delete),
            0x30..=0x39 => {
                let digit = (b'0' + (vk - 0x30) as u8) as char;
                Some(Self::Char(digit))
            }
            0x41..=0x5A => {
                let ch = (b'A' + (vk - 0x41) as u8) as char;
                Some(Self::Char(ch))
            }
            0x70..=0x7B => {
                let f_num = (vk - 0x70 + 1) as u8;
                Some(Self::F(f_num))
            }
            _ => None,
        }
    }
}

/// Abstract window manager action triggered by hotkeys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    CloseWindow,
    ToggleFullscreen,
    ToggleFloating,
    FocusDirection(Direction),
    MoveDirection(Direction),
    CycleWindow(bool),
    FocusWorkspace(u8),
    MoveToWorkspace(u8),
    CycleLayout,
    SetLayout(LayoutKind),
    Snap(SnapZone),
    CommandBar,
    Terminal,
    PanicFailSafe,
    Execute(String),
    Custom(String),
}

/// Parsed keybinding trigger.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombination {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl KeyCombination {
    pub fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }
}

/// Maps key combinations to actions with configurable default mod key and
/// strict passthrough protection for Windows reserved shortcuts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingMap {
    pub mod_key: ModKey,
    pub bindings: HashMap<KeyCombination, Action>,
}

impl Default for KeybindingMap {
    fn default() -> Self {
        Self::new(ModKey::Super)
    }
}

impl KeybindingMap {
    pub fn new(mod_key: ModKey) -> Self {
        Self {
            mod_key,
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, combo: KeyCombination, action: Action) {
        self.bindings.insert(combo, action);
    }

    /// Looks up an action for the given modifiers and key.
    pub fn lookup(&self, modifiers: Modifiers, key: Key) -> Option<&Action> {
        let combo = KeyCombination::new(modifiers, key);
        self.bindings.get(&combo)
    }

    /// Checks if a shortcut is a native Windows shortcut that should NOT be swallowed
    /// unless explicitly bound by the user in `bindings`.
    pub fn is_windows_reserved_shortcut(modifiers: Modifiers, key: Key) -> bool {
        // Alt+Tab
        if modifiers.alt && !modifiers.ctrl && !modifiers.super_key && key == Key::Tab {
            return true;
        }

        // Ctrl+Shift+Esc
        if modifiers.ctrl && modifiers.shift && !modifiers.alt && !modifiers.super_key && key == Key::Escape {
            return true;
        }

        // Win + reserved keys
        if modifiers.super_key && !modifiers.alt && !modifiers.ctrl {
            match key {
                Key::Space if !modifiers.shift => true, // Switch input language
                Key::Char('Q') | Key::Char('q') if !modifiers.shift => true, // Windows Search
                Key::Char(c) if ('1'..='9').contains(&c) && !modifiers.shift => true, // Taskbar app launch
                Key::Char('D') | Key::Char('d') => true, // Show desktop
                Key::Char('E') | Key::Char('e') => true, // Explorer
                Key::Char('L') | Key::Char('l') => true, // Lock PC
                Key::Char('R') | Key::Char('r') => true, // Run dialog
                Key::Char('X') | Key::Char('x') => true, // Quick link menu
                Key::Char('Z') | Key::Char('z') => true, // Snap flyout
                Key::Tab => true, // Task view
                _ => false,
            }
        } else {
            false
        }
    }

    /// Parses keybindings configuration table into this map.
    pub fn load_from_config(
        mod_key_str: &str,
        raw_bindings: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let mod_key = mod_key_str.parse::<ModKey>().unwrap_or(ModKey::Super);
        let mut map = Self::new(mod_key);

        for (key_str, action_str) in raw_bindings {
            let combo = parse_key_combination(key_str, mod_key)?;
            let action = parse_action(action_str)?;
            map.bind(combo, action);
        }

        Ok(map)
    }
}

/// Parses a key combination string like `"Return"`, `"Shift+1"`, `"Super+Space"`, `"Ctrl+Alt+Shift+Esc"`.
pub fn parse_key_combination(s: &str, default_mod: ModKey) -> Result<KeyCombination, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("Empty keybinding string".into());
    }

    let mut modifiers = Modifiers::NONE;
    let mut explicit_mod = false;
    let mut key_part = None;

    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "super" | "win" | "windows" => {
                modifiers.super_key = true;
                explicit_mod = true;
            }
            "alt" | "opt" => {
                modifiers.alt = true;
                explicit_mod = true;
            }
            "ctrl" | "control" => {
                modifiers.ctrl = true;
                explicit_mod = true;
            }
            "shift" => {
                modifiers.shift = true;
            }
            other => {
                key_part = Some(other.to_string());
            }
        }
    }

    // If no explicit modifier like Super/Alt/Ctrl was specified, apply default_mod
    if !explicit_mod {
        match default_mod {
            ModKey::Super => modifiers.super_key = true,
            ModKey::Alt => modifiers.alt = true,
            ModKey::Ctrl => modifiers.ctrl = true,
            ModKey::Shift => modifiers.shift = true,
        }
    }

    let key_str = key_part.ok_or_else(|| format!("Missing key in combination: '{s}'"))?;
    let key = parse_key(&key_str)?;

    Ok(KeyCombination::new(modifiers, key))
}

fn parse_key(s: &str) -> Result<Key, String> {
    match s.to_ascii_lowercase().as_str() {
        "return" | "enter" => Ok(Key::Return),
        "space" => Ok(Key::Space),
        "esc" | "escape" => Ok(Key::Escape),
        "tab" => Ok(Key::Tab),
        "backspace" => Ok(Key::Backspace),
        "delete" | "del" => Ok(Key::Delete),
        "left" => Ok(Key::Left),
        "right" => Ok(Key::Right),
        "up" => Ok(Key::Up),
        "down" => Ok(Key::Down),
        f if f.starts_with('f') && f.len() > 1 => {
            let num: u8 = f[1..].parse().map_err(|_| format!("Invalid function key: '{s}'"))?;
            if (1..=12).contains(&num) {
                Ok(Key::F(num))
            } else {
                Err(format!("Function key out of range: '{s}'"))
            }
        }
        single if single.chars().count() == 1 => {
            let ch = single.chars().next().ok_or_else(|| "Empty char".to_string())?.to_ascii_uppercase();
            Ok(Key::Char(ch))
        }
        other => Err(format!("Unrecognized key: '{other}'")),
    }
}

/// Parses an action string like `"window:close"`, `"focus:left"`, `"workspace:1"`, `"exec:alacritty.exe"`.
pub fn parse_action(s: &str) -> Result<Action, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("window:close") {
        return Ok(Action::CloseWindow);
    }
    if s.eq_ignore_ascii_case("window:toggle_fullscreen") || s.eq_ignore_ascii_case("fullscreen") {
        return Ok(Action::ToggleFullscreen);
    }
    if s.eq_ignore_ascii_case("window:toggle_floating") || s.eq_ignore_ascii_case("floating") {
        return Ok(Action::ToggleFloating);
    }
    if s.eq_ignore_ascii_case("focus:left") {
        return Ok(Action::FocusDirection(Direction::Left));
    }
    if s.eq_ignore_ascii_case("focus:right") {
        return Ok(Action::FocusDirection(Direction::Right));
    }
    if s.eq_ignore_ascii_case("focus:up") {
        return Ok(Action::FocusDirection(Direction::Up));
    }
    if s.eq_ignore_ascii_case("focus:down") {
        return Ok(Action::FocusDirection(Direction::Down));
    }
    if s.eq_ignore_ascii_case("move:left") {
        return Ok(Action::MoveDirection(Direction::Left));
    }
    if s.eq_ignore_ascii_case("move:right") {
        return Ok(Action::MoveDirection(Direction::Right));
    }
    if s.eq_ignore_ascii_case("move:up") {
        return Ok(Action::MoveDirection(Direction::Up));
    }
    if s.eq_ignore_ascii_case("move:down") {
        return Ok(Action::MoveDirection(Direction::Down));
    }
    if s.eq_ignore_ascii_case("cycle:next") {
        return Ok(Action::CycleWindow(true));
    }
    if s.eq_ignore_ascii_case("cycle:prev") {
        return Ok(Action::CycleWindow(false));
    }
    if s.eq_ignore_ascii_case("layout:cycle") || s.eq_ignore_ascii_case("layout:next") {
        return Ok(Action::CycleLayout);
    }
    if s.eq_ignore_ascii_case("shell:command_bar") || s.eq_ignore_ascii_case("command_bar") {
        return Ok(Action::CommandBar);
    }
    if s.eq_ignore_ascii_case("shell:terminal") || s.eq_ignore_ascii_case("terminal") {
        return Ok(Action::Terminal);
    }
    if s.eq_ignore_ascii_case("shell:panic") || s.eq_ignore_ascii_case("panic") {
        return Ok(Action::PanicFailSafe);
    }

    if let Some(ws_str) = s.strip_prefix("workspace:") {
        let ws: u8 = ws_str.trim().parse().map_err(|_| format!("Invalid workspace index in '{s}'"))?;
        return Ok(Action::FocusWorkspace(ws));
    }

    if let Some(ws_str) = s.strip_prefix("move_to_workspace:") {
        let ws: u8 = ws_str.trim().parse().map_err(|_| format!("Invalid workspace index in '{s}'"))?;
        return Ok(Action::MoveToWorkspace(ws));
    }

    if let Some(cmd) = s.strip_prefix("exec:") {
        return Ok(Action::Execute(cmd.trim().to_string()));
    }

    if let Some(l_str) = s.strip_prefix("layout:") {
        let kind = l_str.trim().parse::<LayoutKind>()?;
        return Ok(Action::SetLayout(kind));
    }

    if let Some(snap_str) = s.strip_prefix("snap:") {
        let zone = match snap_str.trim().to_ascii_lowercase().as_str() {
            "left" => SnapZone::LeftHalf,
            "right" => SnapZone::RightHalf,
            "top" => SnapZone::TopHalf,
            "bottom" => SnapZone::BottomHalf,
            "top_left" => SnapZone::TopLeft,
            "top_right" => SnapZone::TopRight,
            "bottom_left" => SnapZone::BottomLeft,
            "bottom_right" => SnapZone::BottomRight,
            "maximize" => SnapZone::Maximize,
            other => return Err(format!("Unknown snap zone: '{other}'")),
        };
        return Ok(Action::Snap(zone));
    }

    Ok(Action::Custom(s.to_string()))
}
