//! Keyboard-first navigation: key chords and the default bindings.
//!
//! Defaults keep every Windows Explorer shortcut people already know and add
//! Finder / power-user ones (Space = Quick Look, Ctrl+P palette, F6 split).

use crate::action::{Action, CursorMove, ViewKind};
use nx_fs::sort::SortKey;
use std::collections::HashMap;

/// Logical keys (layout-independent; letters are upper-case ASCII).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    F(u8),
}

/// Modifier set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: true,
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
}

/// A key plus modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub mods: Mods,
}

impl Chord {
    pub const fn new(key: Key, mods: Mods) -> Self {
        Chord { key, mods }
    }
    pub const fn plain(key: Key) -> Self {
        Chord {
            key,
            mods: Mods::NONE,
        }
    }
    pub const fn ctrl(c: char) -> Self {
        Chord {
            key: Key::Char(c),
            mods: Mods::CTRL,
        }
    }

    /// Human-readable label ("Ctrl+Shift+N").
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.mods.ctrl {
            s.push_str("Ctrl+");
        }
        if self.mods.alt {
            s.push_str("Alt+");
        }
        if self.mods.shift {
            s.push_str("Shift+");
        }
        match self.key {
            Key::Char(c) => s.push(c),
            Key::F(n) => s.push_str(&format!("F{n}")),
            k => s.push_str(&format!("{k:?}")),
        }
        s
    }
}

/// Chord -> action bindings (user-remappable).
#[derive(Debug, Clone)]
pub struct Keymap {
    map: HashMap<Chord, Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        use Action as A;
        use Key as K;
        let mv = |m: CursorMove, extend: bool| A::MoveCursor { by: m, extend };
        let b: Vec<(Chord, Action)> = vec![
            // Navigation (Explorer)
            (Chord::new(K::Left, Mods::ALT), A::GoBack),
            (Chord::new(K::Right, Mods::ALT), A::GoForward),
            (Chord::new(K::Up, Mods::ALT), A::GoUp),
            (Chord::plain(K::Backspace), A::GoBack),
            (Chord::plain(K::F(5)), A::Refresh),
            (Chord::ctrl('R'), A::Refresh),
            (Chord::ctrl('L'), A::FocusAddressBar),
            (Chord::new(K::Char('D'), Mods::ALT), A::FocusAddressBar),
            (Chord::plain(K::F(4)), A::FocusAddressBar),
            (Chord::ctrl('F'), A::FocusSearch),
            (Chord::ctrl('E'), A::FocusSearch),
            (Chord::plain(K::F(3)), A::FocusSearch),
            // Tabs and panes
            (Chord::ctrl('T'), A::NewTab),
            (Chord::ctrl('W'), A::CloseTab),
            (
                Chord::new(K::Char('T'), Mods::CTRL_SHIFT),
                A::ReopenClosedTab,
            ),
            (Chord::new(K::Tab, Mods::CTRL), A::NextTab),
            (Chord::new(K::Tab, Mods::CTRL_SHIFT), A::PrevTab),
            (Chord::plain(K::F(6)), A::ToggleSplit),
            (Chord::plain(K::Tab), A::FocusOtherPane),
            // Cursor & selection
            (Chord::plain(K::Up), mv(CursorMove::Up, false)),
            (Chord::plain(K::Down), mv(CursorMove::Down, false)),
            (Chord::plain(K::Left), mv(CursorMove::Left, false)),
            (Chord::plain(K::Right), mv(CursorMove::Right, false)),
            (Chord::plain(K::PageUp), mv(CursorMove::PageUp, false)),
            (Chord::plain(K::PageDown), mv(CursorMove::PageDown, false)),
            (Chord::plain(K::Home), mv(CursorMove::Home, false)),
            (Chord::plain(K::End), mv(CursorMove::End, false)),
            (Chord::new(K::Up, Mods::SHIFT), mv(CursorMove::Up, true)),
            (Chord::new(K::Down, Mods::SHIFT), mv(CursorMove::Down, true)),
            (Chord::new(K::Left, Mods::SHIFT), mv(CursorMove::Left, true)),
            (
                Chord::new(K::Right, Mods::SHIFT),
                mv(CursorMove::Right, true),
            ),
            (Chord::new(K::Home, Mods::SHIFT), mv(CursorMove::Home, true)),
            (Chord::new(K::End, Mods::SHIFT), mv(CursorMove::End, true)),
            (
                Chord::new(K::PageUp, Mods::SHIFT),
                mv(CursorMove::PageUp, true),
            ),
            (
                Chord::new(K::PageDown, Mods::SHIFT),
                mv(CursorMove::PageDown, true),
            ),
            (Chord::ctrl('A'), A::SelectAll),
            (Chord::new(K::Space, Mods::CTRL), A::ToggleSelectCursor),
            (Chord::plain(K::Enter), A::OpenSelection),
            (Chord::plain(K::Space), A::QuickLook),
            // File operations
            (Chord::ctrl('C'), A::Copy),
            (Chord::ctrl('X'), A::Cut),
            (Chord::ctrl('V'), A::Paste),
            (Chord::ctrl('D'), A::Duplicate),
            (Chord::ctrl('Z'), A::Undo),
            (Chord::plain(K::Delete), A::Delete { permanent: false }),
            (
                Chord::new(K::Delete, Mods::SHIFT),
                A::Delete { permanent: true },
            ),
            (Chord::plain(K::F(2)), A::BeginRename),
            (Chord::new(K::Char('N'), Mods::CTRL_SHIFT), A::NewFolder),
            (Chord::new(K::Char('C'), Mods::CTRL_SHIFT), A::CopyPath),
            (Chord::new(K::F(5), Mods::SHIFT), A::CopyToOtherPane),
            (Chord::new(K::F(6), Mods::SHIFT), A::MoveToOtherPane),
            (Chord::new(K::Enter, Mods::ALT), A::Properties),
            // Views
            (
                Chord::new(K::Char('1'), Mods::CTRL),
                A::SetView(ViewKind::Grid),
            ),
            (
                Chord::new(K::Char('2'), Mods::CTRL),
                A::SetView(ViewKind::List),
            ),
            (
                Chord::new(K::Char('3'), Mods::CTRL),
                A::SetView(ViewKind::Columns),
            ),
            (
                Chord::new(K::Char('4'), Mods::CTRL),
                A::SetView(ViewKind::Details),
            ),
            (Chord::ctrl('H'), A::ToggleHidden),
            (
                Chord::new(K::Char('1'), Mods::new_ctrl_alt()),
                A::SortBy(SortKey::Name),
            ),
            (
                Chord::new(K::Char('2'), Mods::new_ctrl_alt()),
                A::SortBy(SortKey::Modified),
            ),
            (
                Chord::new(K::Char('3'), Mods::new_ctrl_alt()),
                A::SortBy(SortKey::Size),
            ),
            (
                Chord::new(K::Char('4'), Mods::new_ctrl_alt()),
                A::SortBy(SortKey::Kind),
            ),
            // Palette
            (Chord::ctrl('P'), A::OpenPalette),
            (Chord::new(K::Char('P'), Mods::CTRL_SHIFT), A::OpenPalette),
            (Chord::new(K::Char('B'), Mods::CTRL), A::AddFavorite),
        ];
        Keymap {
            map: b.into_iter().collect(),
        }
    }
}

impl Mods {
    const fn new_ctrl_alt() -> Mods {
        Mods {
            ctrl: true,
            shift: false,
            alt: true,
        }
    }
}

impl Keymap {
    pub fn lookup(&self, chord: &Chord) -> Option<&Action> {
        self.map.get(&normalize(chord))
    }

    /// Rebind (returns the previous action for that chord).
    pub fn bind(&mut self, chord: Chord, action: Action) -> Option<Action> {
        self.map.insert(normalize(&chord), action)
    }

    pub fn unbind(&mut self, chord: &Chord) -> Option<Action> {
        self.map.remove(&normalize(chord))
    }

    /// First chord bound to `action` (for palette hints).
    pub fn chord_for(&self, action: &Action) -> Option<Chord> {
        let mut v: Vec<&Chord> = self
            .map
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(c, _)| c)
            .collect();
        v.sort_by_key(|c| c.label());
        v.first().map(|c| **c)
    }
}

fn normalize(c: &Chord) -> Chord {
    match c.key {
        Key::Char(ch) => Chord {
            key: Key::Char(ch.to_ascii_uppercase()),
            mods: c.mods,
        },
        _ => *c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_rebinding() {
        let mut k = Keymap::default();
        assert_eq!(
            k.lookup(&Chord::ctrl('t')),
            Some(&Action::NewTab),
            "case-insensitive"
        );
        assert_eq!(
            k.lookup(&Chord::new(Key::Delete, Mods::SHIFT)),
            Some(&Action::Delete { permanent: true })
        );
        assert_eq!(
            k.lookup(&Chord::plain(Key::Space)),
            Some(&Action::QuickLook)
        );
        assert!(
            k.lookup(&Chord::plain(Key::Char('Q'))).is_none(),
            "plain letters are type-to-select"
        );
        k.bind(Chord::ctrl('Q'), Action::CloseTab);
        assert_eq!(
            k.chord_for(&Action::CloseTab).map(|c| c.label()),
            Some("Ctrl+Q".into())
        );
        assert_eq!(
            Chord::new(Key::F(2), Mods::CTRL_SHIFT).label(),
            "Ctrl+Shift+F2"
        );
    }
}
