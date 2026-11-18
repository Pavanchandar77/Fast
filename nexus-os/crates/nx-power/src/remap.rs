use crate::error::{PowerError, PowerResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Virtual key code enumeration for keyboard remapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,

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
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,

    Escape,
    Tab,
    CapsLock,
    Space,
    Enter,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,

    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,

    PrintScreen,
    ScrollLock,
    Pause,

    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftShift,
    RightShift,
    LeftWin,
    RightWin,

    Raw(u16),
}

/// Modifier keys state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
        win: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
        win: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        win: false,
    };
    pub const WIN: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        win: true,
    };

    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.win
    }
}

/// A combination of modifier keys and a base key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    pub modifiers: KeyModifiers,
    pub key: KeyCode,
}

impl KeyChord {
    pub const fn new(modifiers: KeyModifiers, key: KeyCode) -> Self {
        Self { modifiers, key }
    }

    pub const fn plain(key: KeyCode) -> Self {
        Self {
            modifiers: KeyModifiers::NONE,
            key,
        }
    }
}

/// Destination output for a remapping rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemapTarget {
    Key(KeyCode),
    Chord(KeyChord),
    Disabled,
}

/// A single remapping entry with optional per-application scoping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemapRule {
    pub id: u64,
    pub source: KeyChord,
    pub target: RemapTarget,
    pub app_scope: Option<String>,
    pub enabled: bool,
}

/// Keyboard remapping table with protected Windows shortcut safeguards and cycle detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemapTable {
    rules: Vec<RemapRule>,
    allow_system_shortcut_override: bool,
    next_rule_id: u64,
}

impl RemapTable {
    pub fn new(allow_system_shortcut_override: bool) -> Self {
        Self {
            rules: Vec::new(),
            allow_system_shortcut_override,
            next_rule_id: 1,
        }
    }

    /// List of stock Windows shortcuts protected by default.
    pub fn is_protected_system_shortcut(chord: &KeyChord) -> bool {
        // Win + L
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyL
        {
            return true;
        }
        // Win + X
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyX
        {
            return true;
        }
        // Win + R
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyR
        {
            return true;
        }
        // Win + E
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyE
        {
            return true;
        }
        // Win + D
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyD
        {
            return true;
        }
        // Win + I
        if chord.modifiers.win
            && !chord.modifiers.ctrl
            && !chord.modifiers.alt
            && chord.key == KeyCode::KeyI
        {
            return true;
        }
        // Alt + Tab
        if chord.modifiers.alt && chord.key == KeyCode::Tab {
            return true;
        }
        // Ctrl + Alt + Delete
        if chord.modifiers.ctrl && chord.modifiers.alt && chord.key == KeyCode::Delete {
            return true;
        }

        false
    }

    /// Adds a remap rule after validating protected system shortcuts and cycle prevention.
    pub fn add_rule(
        &mut self,
        source: KeyChord,
        target: RemapTarget,
        app_scope: Option<String>,
    ) -> PowerResult<u64> {
        // 1. Guard stock Windows system shortcuts
        if !self.allow_system_shortcut_override && Self::is_protected_system_shortcut(&source) {
            return Err(PowerError::ProtectedShortcut(format!(
                "Cannot steal stock Windows shortcut {:?}",
                source
            )));
        }

        // 2. Cycle detection: e.g. A -> B and B -> A
        if let RemapTarget::Chord(target_chord) = &target {
            if &source == target_chord {
                return Err(PowerError::CircularRemap(
                    "Self-referential remap".to_string(),
                ));
            }

            // Trace mapping chain
            let mut visited = HashSet::new();
            visited.insert(source);
            visited.insert(*target_chord);

            let mut current = *target_chord;
            for _ in 0..16 {
                if let Some(RemapTarget::Chord(next_chord)) =
                    self.resolve_chord(current, app_scope.as_deref())
                {
                    if next_chord == source {
                        return Err(PowerError::CircularRemap(format!(
                            "Circular remap chain detected involving {:?}",
                            source
                        )));
                    }
                    if !visited.insert(next_chord) {
                        break;
                    }
                    current = next_chord;
                } else {
                    break;
                }
            }
        }

        let id = self.next_rule_id;
        self.next_rule_id = self.next_rule_id.saturating_add(1);

        self.rules.push(RemapRule {
            id,
            source,
            target,
            app_scope,
            enabled: true,
        });

        Ok(id)
    }

    pub fn remove_rule(&mut self, id: u64) -> bool {
        let prev_len = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < prev_len
    }

    pub fn rules(&self) -> &[RemapRule] {
        &self.rules
    }

    /// Resolves an input key chord against active rules. App-specific rules take precedence.
    pub fn resolve_chord(&self, chord: KeyChord, current_app: Option<&str>) -> Option<RemapTarget> {
        // 1. Check app-specific enabled rules first
        if let Some(app) = current_app {
            let app_lower = app.to_ascii_lowercase();
            if let Some(rule) = self.rules.iter().find(|r| {
                r.enabled
                    && r.source == chord
                    && r.app_scope
                        .as_ref()
                        .map(|s| s.to_ascii_lowercase() == app_lower)
                        .unwrap_or(false)
            }) {
                return Some(rule.target.clone());
            }
        }

        // 2. Check global enabled rules
        self.rules
            .iter()
            .find(|r| r.enabled && r.source == chord && r.app_scope.is_none())
            .map(|r| r.target.clone())
    }
}

impl Default for RemapTable {
    fn default() -> Self {
        Self::new(false)
    }
}
