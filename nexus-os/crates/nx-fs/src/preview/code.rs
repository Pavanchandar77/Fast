//! Lightweight code previews: language detection and a tiny lexer that
//! classifies comments, strings, numbers and keywords for syntax colouring.

/// Token classes for highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
}

/// A highlighted span within one line (byte offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Language description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub name: &'static str,
    pub line_comment: &'static str,
    pub keywords: &'static [&'static str],
}

const C_LIKE: &[&str] = &[
    "if", "else", "for", "while", "return", "break", "continue", "switch", "case", "struct",
    "enum", "const", "static", "void", "class", "public", "private", "new", "true", "false",
    "null", "import", "export", "function", "let", "var", "async", "await",
];
const RUST: &[&str] = &[
    "fn", "let", "mut", "pub", "impl", "struct", "enum", "trait", "use", "mod", "match", "if",
    "else", "for", "while", "loop", "return", "self", "Self", "crate", "super", "where", "as",
    "in", "const", "static", "unsafe", "async", "await", "move", "dyn", "true", "false",
];
const PY: &[&str] = &[
    "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while", "in", "is",
    "not", "and", "or", "None", "True", "False", "with", "as", "lambda", "yield", "try", "except",
    "finally", "raise", "pass",
];
const SHELL: &[&str] = &[
    "if", "then", "else", "fi", "for", "do", "done", "case", "esac", "function", "echo", "export",
    "local",
];
const NONE: &[&str] = &[];

/// Detect the language from a file extension.
pub fn language_for(ext: &str) -> Option<Language> {
    let (name, lc, kw): (&str, &str, &[&str]) = match ext {
        "rs" => ("Rust", "//", RUST),
        "c" | "h" | "cpp" | "hpp" | "cc" | "cs" | "java" | "kt" | "go" | "swift" => {
            ("C-like", "//", C_LIKE)
        }
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => ("JavaScript", "//", C_LIKE),
        "py" | "pyw" => ("Python", "#", PY),
        "sh" | "bash" | "zsh" | "ps1" => ("Shell", "#", SHELL),
        "toml" | "ini" | "cfg" | "conf" | "yaml" | "yml" => ("Config", "#", NONE),
        "json" => ("JSON", "", NONE),
        "wgsl" | "hlsl" | "glsl" => ("Shader", "//", C_LIKE),
        "sql" => ("SQL", "--", NONE),
        "lua" => ("Lua", "--", NONE),
        _ => return None,
    };
    Some(Language {
        name,
        line_comment: lc,
        keywords: kw,
    })
}

/// Highlight one line. Block comments are not tracked across lines (cheap
/// and good enough for a preview).
pub fn highlight_line(line: &str, lang: &Language) -> Vec<Span> {
    let b = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !lang.line_comment.is_empty() && line[i..].starts_with(lang.line_comment) {
            spans.push(Span {
                start: i,
                end: b.len(),
                kind: TokenKind::Comment,
            });
            break;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            spans.push(Span {
                start,
                end: i,
                kind: TokenKind::String,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: TokenKind::Number,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if lang.keywords.contains(&&line[start..i]) {
                spans.push(Span {
                    start,
                    end: i,
                    kind: TokenKind::Keyword,
                });
            }
            continue;
        }
        // Skip the whole UTF-8 sequence.
        i += line[i..].chars().next().map_or(1, char::len_utf8);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust() {
        let lang = language_for("rs").expect("rust");
        let line = r#"let x = "a\"b"; // note 42"#;
        let spans = highlight_line(line, &lang);
        let kinds: Vec<_> = spans
            .iter()
            .map(|s| (s.kind, &line[s.start..s.end]))
            .collect();
        assert_eq!(kinds[0], (TokenKind::Keyword, "let"));
        assert_eq!(kinds[1], (TokenKind::String, r#""a\"b""#));
        assert_eq!(kinds[2], (TokenKind::Comment, "// note 42"));
        assert!(language_for("exe").is_none());
        // Unicode and unterminated strings never panic.
        let _ = highlight_line("é \"unterminated \\", &lang);
        assert_eq!(
            highlight_line("x = 3.14", &language_for("py").expect("py"))[0].kind,
            TokenKind::Number
        );
    }
}
