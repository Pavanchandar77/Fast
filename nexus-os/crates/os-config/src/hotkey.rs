//! Fail-safe chord parser. Pure: no Win32 types.
//!
//! `Ctrl+Alt+Shift+Esc` is the default. Tokens are case-insensitive and
//! separated by `+`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyChord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Win32 virtual-key code.
    pub vk: u16,
}

impl HotkeyChord {
    pub fn default_fail_safe() -> Self {
        Self {
            ctrl: true,
            alt: true,
            shift: true,
            win: false,
            vk: 0x1B, // VK_ESCAPE
        }
    }
}

pub fn parse_or_default(spec: &str) -> HotkeyChord {
    parse_hotkey(spec).unwrap_or_else(HotkeyChord::default_fail_safe)
}

pub fn parse_hotkey(spec: &str) -> Option<HotkeyChord> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;
    let mut vk = None;
    for raw in spec.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return None;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => ctrl = true,
            "alt" | "menu" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "win" | "super" | "meta" | "cmd" => win = true,
            other => {
                if vk.is_some() {
                    return None;
                }
                vk = Some(virtual_key(other)?);
            }
        }
    }
    let vk = vk?;
    if !(ctrl || alt || shift || win) {
        return None;
    }
    Some(HotkeyChord {
        ctrl,
        alt,
        shift,
        win,
        vk,
    })
}

fn virtual_key(token: &str) -> Option<u16> {
    match token {
        "esc" | "escape" => Some(0x1B),
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "return" | "enter" => Some(0x0D),
        "backspace" | "bksp" => Some(0x08),
        "delete" | "del" => Some(0x2E),
        "insert" | "ins" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "f1" => Some(0x70),
        "f2" => Some(0x71),
        "f3" => Some(0x72),
        "f4" => Some(0x73),
        "f5" => Some(0x74),
        "f6" => Some(0x75),
        "f7" => Some(0x76),
        "f8" => Some(0x77),
        "f9" => Some(0x78),
        "f10" => Some(0x79),
        "f11" => Some(0x7A),
        "f12" => Some(0x7B),
        other if other.len() == 1 => {
            let c = other.chars().next()?;
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_uppercase() as u16)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chord_parses() {
        let chord = parse_hotkey("Ctrl+Alt+Shift+Esc").unwrap();
        assert_eq!(chord, HotkeyChord::default_fail_safe());
        assert!(chord.ctrl && chord.alt && chord.shift && !chord.win);
        assert_eq!(chord.vk, 0x1B);
    }

    #[test]
    fn modifiers_and_key_are_case_insensitive() {
        let chord = parse_hotkey("ctrl+super+q").unwrap();
        assert!(chord.ctrl && chord.win && !chord.alt);
        assert_eq!(chord.vk, b'Q' as u16);
    }

    #[test]
    fn rejects_bare_key_and_unknown_token() {
        assert!(parse_hotkey("Esc").is_none());
        assert!(parse_hotkey("Ctrl+NotAKey").is_none());
        assert_eq!(parse_or_default("nope"), HotkeyChord::default_fail_safe());
    }
}
