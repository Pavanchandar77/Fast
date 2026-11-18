//! Command palette (Ctrl+P): fuzzy-ranked commands and places.

use crate::action::{Action, ViewKind};
use crate::input::TextInput;
use crate::keymap::Keymap;
use nx_fs::index::fold;
use nx_fs::index::matcher::{score_term, Term};
use nx_fs::sort::SortKey;
use std::path::PathBuf;

/// One palette row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    /// Shortcut hint ("Ctrl+T"), if bound.
    pub hint: Option<String>,
    pub action: Action,
}

/// Built-in commands (label, action).
pub fn commands() -> Vec<(&'static str, Action)> {
    use Action as A;
    vec![
        ("New tab", A::NewTab),
        ("Close tab", A::CloseTab),
        ("Reopen closed tab", A::ReopenClosedTab),
        ("Toggle split view", A::ToggleSplit),
        ("Go back", A::GoBack),
        ("Go forward", A::GoForward),
        ("Go up", A::GoUp),
        ("Refresh", A::Refresh),
        ("New folder", A::NewFolder),
        ("Rename", A::BeginRename),
        ("Duplicate", A::Duplicate),
        ("Move to Recycle Bin", A::Delete { permanent: false }),
        ("Delete permanently", A::Delete { permanent: true }),
        ("Undo", A::Undo),
        ("Copy", A::Copy),
        ("Cut", A::Cut),
        ("Paste", A::Paste),
        ("Copy path", A::CopyPath),
        ("Copy to other pane", A::CopyToOtherPane),
        ("Move to other pane", A::MoveToOtherPane),
        ("Select all", A::SelectAll),
        ("Quick Look", A::QuickLook),
        ("Properties", A::Properties),
        ("Show/hide hidden files", A::ToggleHidden),
        ("View: icons", A::SetView(ViewKind::Grid)),
        ("View: list", A::SetView(ViewKind::List)),
        ("View: columns", A::SetView(ViewKind::Columns)),
        ("View: details", A::SetView(ViewKind::Details)),
        ("Sort by name", A::SortBy(SortKey::Name)),
        ("Sort by date modified", A::SortBy(SortKey::Modified)),
        ("Sort by size", A::SortBy(SortKey::Size)),
        ("Sort by type", A::SortBy(SortKey::Kind)),
        ("Add to favorites", A::AddFavorite),
        ("Search", A::FocusSearch),
        ("Go to address", A::FocusAddressBar),
    ]
}

/// Palette state.
#[derive(Debug, Clone)]
pub struct Palette {
    pub input: TextInput,
    items: Vec<PaletteItem>,
    results: Vec<usize>,
    pub cursor: usize,
}

impl Palette {
    /// Snapshot commands plus "Go to" entries for the given places.
    pub fn open(keymap: &Keymap, places: &[PathBuf]) -> Self {
        let mut items: Vec<PaletteItem> = commands()
            .into_iter()
            .map(|(label, action)| PaletteItem {
                label: label.to_string(),
                hint: keymap.chord_for(&action).map(|c| c.label()),
                action,
            })
            .collect();
        for p in places {
            items.push(PaletteItem {
                label: format!("Go to {}", p.to_string_lossy()),
                hint: None,
                action: Action::Navigate(p.clone()),
            });
        }
        let results = (0..items.len()).collect();
        Palette {
            input: TextInput::default(),
            items,
            results,
            cursor: 0,
        }
    }

    /// Re-rank after the query changed.
    pub fn refilter(&mut self) {
        let terms: Vec<Term> = self
            .input
            .text()
            .split_whitespace()
            .map(Term::new)
            .collect();
        let mut scored: Vec<(u32, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let name = fold(&it.label);
                let mut total = 0u32;
                for t in &terms {
                    total += score_term(t, name.as_bytes(), true)?;
                }
                Some((total, i))
            })
            .collect();
        // Higher score first; ties keep the curated order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.results = scored.into_iter().map(|(_, i)| i).collect();
        self.cursor = 0;
    }

    pub fn results(&self) -> impl Iterator<Item = &PaletteItem> {
        self.results.iter().filter_map(|&i| self.items.get(i))
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.results.len();
        if n == 0 {
            self.cursor = 0;
            return;
        }
        // Wraps, like VS Code.
        self.cursor = (self.cursor as isize + delta).rem_euclid(n as isize) as usize;
    }

    pub fn selected(&self) -> Option<&PaletteItem> {
        self.results
            .get(self.cursor)
            .and_then(|&i| self.items.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal() -> Palette {
        Palette::open(&Keymap::default(), &["/home/me/Projects".into()])
    }

    #[test]
    fn ranks_prefix_and_fuzzy() {
        let mut p = pal();
        assert_eq!(p.len(), commands().len() + 1);
        p.input.insert("new");
        p.refilter();
        let top: Vec<&str> = p.results().take(2).map(|i| i.label.as_str()).collect();
        assert_eq!(top, vec!["New tab", "New folder"]);
        assert_eq!(
            p.selected().and_then(|i| i.hint.clone()),
            Some("Ctrl+T".into())
        );
        p.input.clear();
        p.input.insert("tglsplt");
        p.refilter();
        assert_eq!(
            p.selected().map(|i| i.action.clone()),
            Some(Action::ToggleSplit)
        );
        p.input.clear();
        p.input.insert("go proj");
        p.refilter();
        assert_eq!(
            p.selected().map(|i| i.action.clone()),
            Some(Action::Navigate("/home/me/Projects".into()))
        );
    }

    #[test]
    fn cursor_wraps_and_empty_results() {
        let mut p = pal();
        p.move_cursor(-1);
        assert_eq!(p.cursor, p.len() - 1);
        p.move_cursor(1);
        assert_eq!(p.cursor, 0);
        p.input.insert("qqqqzzzz");
        p.refilter();
        assert!(p.is_empty());
        p.move_cursor(1);
        assert!(p.selected().is_none());
    }
}
