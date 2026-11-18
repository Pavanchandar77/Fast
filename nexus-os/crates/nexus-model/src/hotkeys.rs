//! Pure hotkey parsing, normalization, and action dispatch model for Nexus-OS.
//! Deterministic, zero platform dependencies, tested against arbitrary fuzz/property inputs.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool, // Super / Windows key
}

impl Modifiers {
    pub const fn empty() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
        }
    }

    pub const fn ctrl() -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: false,
            win: false,
        }
    }

    pub const fn alt() -> Self {
        Self {
            ctrl: false,
            alt: true,
            shift: false,
            win: false,
        }
    }

    pub const fn shift() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: true,
            win: false,
        }
    }

    pub const fn win() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            win: true,
        }
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn with_win(mut self) -> Self {
        self.win = true;
        self
    }

    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.win
    }

    pub fn matches(&self, ctrl: bool, alt: bool, shift: bool, win: bool) -> bool {
        self.ctrl == ctrl && self.alt == alt && self.shift == shift && self.win == win
    }
}

impl fmt::Display for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.win {
            parts.push("Super");
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        write!(f, "{}", parts.join("+"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Return,
    Space,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::A => write!(f, "A"),
            Key::B => write!(f, "B"),
            Key::C => write!(f, "C"),
            Key::D => write!(f, "D"),
            Key::E => write!(f, "E"),
            Key::F => write!(f, "F"),
            Key::G => write!(f, "G"),
            Key::H => write!(f, "H"),
            Key::I => write!(f, "I"),
            Key::J => write!(f, "J"),
            Key::K => write!(f, "K"),
            Key::L => write!(f, "L"),
            Key::M => write!(f, "M"),
            Key::N => write!(f, "N"),
            Key::O => write!(f, "O"),
            Key::P => write!(f, "P"),
            Key::Q => write!(f, "Q"),
            Key::R => write!(f, "R"),
            Key::S => write!(f, "S"),
            Key::T => write!(f, "T"),
            Key::U => write!(f, "U"),
            Key::V => write!(f, "V"),
            Key::W => write!(f, "W"),
            Key::X => write!(f, "X"),
            Key::Y => write!(f, "Y"),
            Key::Z => write!(f, "Z"),
            Key::Num0 => write!(f, "0"),
            Key::Num1 => write!(f, "1"),
            Key::Num2 => write!(f, "2"),
            Key::Num3 => write!(f, "3"),
            Key::Num4 => write!(f, "4"),
            Key::Num5 => write!(f, "5"),
            Key::Num6 => write!(f, "6"),
            Key::Num7 => write!(f, "7"),
            Key::Num8 => write!(f, "8"),
            Key::Num9 => write!(f, "9"),
            Key::F1 => write!(f, "F1"),
            Key::F2 => write!(f, "F2"),
            Key::F3 => write!(f, "F3"),
            Key::F4 => write!(f, "F4"),
            Key::F5 => write!(f, "F5"),
            Key::F6 => write!(f, "F6"),
            Key::F7 => write!(f, "F7"),
            Key::F8 => write!(f, "F8"),
            Key::F9 => write!(f, "F9"),
            Key::F10 => write!(f, "F10"),
            Key::F11 => write!(f, "F11"),
            Key::F12 => write!(f, "F12"),
            Key::Return => write!(f, "Return"),
            Key::Space => write!(f, "Space"),
            Key::Escape => write!(f, "Escape"),
            Key::Tab => write!(f, "Tab"),
            Key::Backspace => write!(f, "Backspace"),
            Key::Delete => write!(f, "Delete"),
            Key::Insert => write!(f, "Insert"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PageUp"),
            Key::PageDown => write!(f, "PageDown"),
            Key::Up => write!(f, "Up"),
            Key::Down => write!(f, "Down"),
            Key::Left => write!(f, "Left"),
            Key::Right => write!(f, "Right"),
        }
    }
}

impl FromStr for Key {
    type Err = HotkeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.len() == 1 {
            if let Some(ch) = s.chars().next() {
                match ch.to_ascii_uppercase() {
                    'A' => return Ok(Key::A),
                    'B' => return Ok(Key::B),
                    'C' => return Ok(Key::C),
                    'D' => return Ok(Key::D),
                    'E' => return Ok(Key::E),
                    'F' => return Ok(Key::F),
                    'G' => return Ok(Key::G),
                    'H' => return Ok(Key::H),
                    'I' => return Ok(Key::I),
                    'J' => return Ok(Key::J),
                    'K' => return Ok(Key::K),
                    'L' => return Ok(Key::L),
                    'M' => return Ok(Key::M),
                    'N' => return Ok(Key::N),
                    'O' => return Ok(Key::O),
                    'P' => return Ok(Key::P),
                    'Q' => return Ok(Key::Q),
                    'R' => return Ok(Key::R),
                    'S' => return Ok(Key::S),
                    'T' => return Ok(Key::T),
                    'U' => return Ok(Key::U),
                    'V' => return Ok(Key::V),
                    'W' => return Ok(Key::W),
                    'X' => return Ok(Key::X),
                    'Y' => return Ok(Key::Y),
                    'Z' => return Ok(Key::Z),
                    '0' => return Ok(Key::Num0),
                    '1' => return Ok(Key::Num1),
                    '2' => return Ok(Key::Num2),
                    '3' => return Ok(Key::Num3),
                    '4' => return Ok(Key::Num4),
                    '5' => return Ok(Key::Num5),
                    '6' => return Ok(Key::Num6),
                    '7' => return Ok(Key::Num7),
                    '8' => return Ok(Key::Num8),
                    '9' => return Ok(Key::Num9),
                    _ => {}
                }
            }
        }

        match s.to_lowercase().as_str() {
            "return" | "enter" => Ok(Key::Return),
            "space" | "spacebar" => Ok(Key::Space),
            "escape" | "esc" => Ok(Key::Escape),
            "tab" => Ok(Key::Tab),
            "backspace" => Ok(Key::Backspace),
            "delete" | "del" => Ok(Key::Delete),
            "insert" | "ins" => Ok(Key::Insert),
            "home" => Ok(Key::Home),
            "end" => Ok(Key::End),
            "pageup" | "pgup" => Ok(Key::PageUp),
            "pagedown" | "pgdn" => Ok(Key::PageDown),
            "up" => Ok(Key::Up),
            "down" => Ok(Key::Down),
            "left" => Ok(Key::Left),
            "right" => Ok(Key::Right),
            "f1" => Ok(Key::F1),
            "f2" => Ok(Key::F2),
            "f3" => Ok(Key::F3),
            "f4" => Ok(Key::F4),
            "f5" => Ok(Key::F5),
            "f6" => Ok(Key::F6),
            "f7" => Ok(Key::F7),
            "f8" => Ok(Key::F8),
            "f9" => Ok(Key::F9),
            "f10" => Ok(Key::F10),
            "f11" => Ok(Key::F11),
            "f12" => Ok(Key::F12),
            _ => Err(HotkeyParseError::UnknownKey(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Hotkey {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// Parses a hotkey string like "Ctrl+Alt+Shift+Esc", "Super+Space", "Super+Shift+1".
    pub fn parse(s: &str) -> Result<Self, HotkeyParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(HotkeyParseError::EmptyString);
        }

        let parts: Vec<&str> = s
            .split('+')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(HotkeyParseError::EmptyString);
        }

        let mut modifiers = Modifiers::empty();
        let mut key_candidate: Option<Key> = None;

        for part in parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "opt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "super" | "win" | "windows" | "cmd" | "command" | "mod" => modifiers.win = true,
                _ => {
                    if key_candidate.is_some() {
                        return Err(HotkeyParseError::MultipleKeys(s.to_string()));
                    }
                    let k = Key::from_str(part)?;
                    key_candidate = Some(k);
                }
            }
        }

        let key = key_candidate.ok_or_else(|| HotkeyParseError::MissingKey(s.to_string()))?;
        Ok(Self { modifiers, key })
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.is_empty() {
            write!(f, "{}", self.key)
        } else {
            write!(f, "{}+{}", self.modifiers, self.key)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    PanicFailSafe,
    CommandBar,
    Terminal,
    CloseWindow,
    ToggleFullscreen,
    ToggleFloating,
    FocusDirection(Direction),
    FocusWorkspace(u8),
    MoveToWorkspace(u8),
    Custom(String),
}

impl HotkeyAction {
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        match s.to_lowercase().as_str() {
            "panic:failsafe" | "panic" | "failsafe" => Self::PanicFailSafe,
            "shell:command_bar" | "commandbar" | "command_bar" => Self::CommandBar,
            "terminal" | "exec:alacritty.exe" | "exec:terminal" => Self::Terminal,
            "window:close" | "close" => Self::CloseWindow,
            "window:toggle_fullscreen" | "fullscreen" => Self::ToggleFullscreen,
            "window:toggle_floating" | "floating" => Self::ToggleFloating,
            "focus:left" => Self::FocusDirection(Direction::Left),
            "focus:right" => Self::FocusDirection(Direction::Right),
            "focus:up" => Self::FocusDirection(Direction::Up),
            "focus:down" => Self::FocusDirection(Direction::Down),
            _ => {
                if let Some(ws_str) = s.strip_prefix("workspace:") {
                    if let Ok(ws) = ws_str.parse::<u8>() {
                        return Self::FocusWorkspace(ws);
                    }
                }
                if let Some(ws_str) = s.strip_prefix("move_to_workspace:") {
                    if let Ok(ws) = ws_str.parse::<u8>() {
                        return Self::MoveToWorkspace(ws);
                    }
                }
                Self::Custom(s.to_string())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyParseError {
    #[error("Hotkey string is empty")]
    EmptyString,
    #[error("Unknown key identifier: '{0}'")]
    UnknownKey(String),
    #[error("No key specified in hotkey string: '{0}'")]
    MissingKey(String),
    #[error("Multiple primary keys specified in hotkey string: '{0}'")]
    MultipleKeys(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_parse_fail_safe_hotkey() {
        let hk = Hotkey::parse("Ctrl+Alt+Shift+Esc").unwrap();
        assert!(hk.modifiers.ctrl);
        assert!(hk.modifiers.alt);
        assert!(hk.modifiers.shift);
        assert!(!hk.modifiers.win);
        assert_eq!(hk.key, Key::Escape);
    }

    #[test]
    fn test_parse_super_space() {
        let hk = Hotkey::parse("Super+Space").unwrap();
        assert!(hk.modifiers.win);
        assert!(!hk.modifiers.ctrl);
        assert_eq!(hk.key, Key::Space);
    }

    #[test]
    fn test_parse_case_insensitivity() {
        let hk1 = Hotkey::parse("ctrl+alt+del").unwrap();
        let hk2 = Hotkey::parse("CTRL + ALT + DELETE").unwrap();
        assert_eq!(hk1, hk2);
        assert_eq!(hk1.key, Key::Delete);
    }

    #[test]
    fn test_parse_action_dispatch() {
        assert_eq!(
            HotkeyAction::parse("shell:command_bar"),
            HotkeyAction::CommandBar
        );
        assert_eq!(
            HotkeyAction::parse("window:close"),
            HotkeyAction::CloseWindow
        );
        assert_eq!(
            HotkeyAction::parse("workspace:3"),
            HotkeyAction::FocusWorkspace(3)
        );
        assert_eq!(
            HotkeyAction::parse("move_to_workspace:2"),
            HotkeyAction::MoveToWorkspace(2)
        );
        assert_eq!(
            HotkeyAction::parse("focus:left"),
            HotkeyAction::FocusDirection(Direction::Left)
        );
    }

    #[test]
    fn test_invalid_hotkey_errors() {
        assert!(matches!(
            Hotkey::parse(""),
            Err(HotkeyParseError::EmptyString)
        ));
        assert!(matches!(
            Hotkey::parse("   "),
            Err(HotkeyParseError::EmptyString)
        ));
        assert!(matches!(
            Hotkey::parse("Ctrl+Alt+Shift"),
            Err(HotkeyParseError::MissingKey(_))
        ));
        assert!(matches!(
            Hotkey::parse("A+B"),
            Err(HotkeyParseError::MultipleKeys(_))
        ));
        assert!(matches!(
            Hotkey::parse("Ctrl+UnknownButton"),
            Err(HotkeyParseError::UnknownKey(_))
        ));
    }

    #[test]
    fn test_display_roundtrip() {
        let original = Hotkey::new(Modifiers::ctrl().with_alt().with_shift(), Key::Escape);
        let s = original.to_string();
        let parsed = Hotkey::parse(&s).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_modifiers_helpers() {
        let m = Modifiers::empty()
            .with_ctrl()
            .with_alt()
            .with_shift()
            .with_win();
        assert!(m.ctrl && m.alt && m.shift && m.win);
        assert!(!m.is_empty());
        assert!(m.matches(true, true, true, true));
        assert!(!m.matches(true, false, true, true));

        let single = Modifiers::win();
        assert!(single.win && !single.ctrl);
    }

    #[test]
    fn test_key_parsing_function_keys() {
        assert_eq!(Key::from_str("F1").unwrap(), Key::F1);
        assert_eq!(Key::from_str("f4").unwrap(), Key::F4);
        assert_eq!(Key::from_str("F12").unwrap(), Key::F12);
    }

    #[test]
    fn test_key_parsing_special_keys() {
        assert_eq!(Key::from_str("Tab").unwrap(), Key::Tab);
        assert_eq!(Key::from_str("Backspace").unwrap(), Key::Backspace);
        assert_eq!(Key::from_str("Delete").unwrap(), Key::Delete);
        assert_eq!(Key::from_str("Insert").unwrap(), Key::Insert);
        assert_eq!(Key::from_str("Home").unwrap(), Key::Home);
        assert_eq!(Key::from_str("End").unwrap(), Key::End);
        assert_eq!(Key::from_str("PageUp").unwrap(), Key::PageUp);
        assert_eq!(Key::from_str("PageDown").unwrap(), Key::PageDown);
        assert_eq!(Key::from_str("Up").unwrap(), Key::Up);
        assert_eq!(Key::from_str("Down").unwrap(), Key::Down);
        assert_eq!(Key::from_str("Left").unwrap(), Key::Left);
        assert_eq!(Key::from_str("Right").unwrap(), Key::Right);
    }

    #[test]
    fn test_hotkey_mod_alias() {
        let hk1 = Hotkey::parse("mod+Return").unwrap();
        assert!(hk1.modifiers.win);
        assert_eq!(hk1.key, Key::Return);

        let hk2 = Hotkey::parse("windows+space").unwrap();
        assert!(hk2.modifiers.win);
        assert_eq!(hk2.key, Key::Space);
    }

    #[test]
    fn test_action_parsing_all_directions() {
        assert_eq!(
            HotkeyAction::parse("focus:right"),
            HotkeyAction::FocusDirection(Direction::Right)
        );
        assert_eq!(
            HotkeyAction::parse("focus:up"),
            HotkeyAction::FocusDirection(Direction::Up)
        );
        assert_eq!(
            HotkeyAction::parse("focus:down"),
            HotkeyAction::FocusDirection(Direction::Down)
        );
        assert_eq!(
            HotkeyAction::parse("window:toggle_floating"),
            HotkeyAction::ToggleFloating
        );
        assert_eq!(
            HotkeyAction::parse("window:toggle_fullscreen"),
            HotkeyAction::ToggleFullscreen
        );
        assert_eq!(
            HotkeyAction::parse("exec:custom_script.bat"),
            HotkeyAction::Custom("exec:custom_script.bat".into())
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn proptest_hotkey_parser_never_panics(arbitrary_str in "\\PC*") {
            // Hotkey::parse must NEVER panic on any random Unicode input
            let _ = Hotkey::parse(&arbitrary_str);
        }

        #[test]
        fn proptest_action_parser_never_panics(arbitrary_str in "\\PC*") {
            let _ = HotkeyAction::parse(&arbitrary_str);
        }
    }
}
