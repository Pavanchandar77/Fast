//! Single-line text input used by the address bar, search, rename and palette.

/// Editable text with a char-indexed cursor and optional selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    /// Cursor position in chars.
    cursor: usize,
    /// Selection anchor in chars (selection = anchor..cursor).
    anchor: Option<usize>,
}

impl TextInput {
    pub fn new(text: &str) -> Self {
        let n = text.chars().count();
        TextInput {
            text: text.to_string(),
            cursor: n,
            anchor: None,
        }
    }

    /// New input with `start..end` (chars) selected (rename selects the stem).
    pub fn with_selection(text: &str, start: usize, end: usize) -> Self {
        let n = text.chars().count();
        TextInput {
            text: text.to_string(),
            cursor: end.min(n),
            anchor: Some(start.min(n)),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|&a| a != self.cursor)
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }

    fn byte(&self, ch: usize) -> usize {
        self.text
            .char_indices()
            .nth(ch)
            .map_or(self.text.len(), |(b, _)| b)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let (ba, bb) = (self.byte(a), self.byte(b));
        self.text.replace_range(ba..bb, "");
        self.cursor = a;
        self.anchor = None;
        true
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.cursor == 0 {
            return;
        }
        let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
        self.text.replace_range(a..b, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.cursor >= self.text.chars().count() {
            return;
        }
        let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
        self.text.replace_range(a..b, "");
    }

    pub fn left(&mut self) {
        self.anchor = None;
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.anchor = None;
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn home(&mut self) {
        self.anchor = None;
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.anchor = None;
        self.cursor = self.text.chars().count();
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        *self = TextInput::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn editing() {
        let mut t = TextInput::with_selection("report.txt", 0, 6);
        t.insert("résumé");
        assert_eq!(t.text(), "résumé.txt");
        t.backspace();
        t.left();
        t.delete();
        assert_eq!(t.text(), "résu.txt");
        t.home();
        t.insert("my ");
        assert_eq!(t.text(), "my résu.txt");
        t.select_all();
        t.backspace();
        assert_eq!(t.text(), "");
        t.insert("a\nb\t");
        assert_eq!(t.text(), "ab", "control chars dropped");
    }

    proptest! {
        #[test]
        fn never_panics(ops in proptest::collection::vec(0u8..8, 0..60), s in "\\PC{0,5}") {
            let mut t = TextInput::new("héllo");
            for op in ops {
                match op {
                    0 => t.insert(&s),
                    1 => t.backspace(),
                    2 => t.delete(),
                    3 => t.left(),
                    4 => t.right(),
                    5 => t.home(),
                    6 => t.end(),
                    _ => t.select_all(),
                }
                prop_assert!(t.cursor() <= t.text().chars().count());
            }
        }
    }
}
