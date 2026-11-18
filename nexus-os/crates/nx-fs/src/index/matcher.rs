//! Query parsing and name matching/scoring.
//!
//! Query syntax (Everything-compatible where it matters):
//! * space-separated terms must all match the file name (AND);
//! * `a/b` or `a\b`: the last segment matches the name and earlier segments
//!   must match ancestor folder names, nearest-last (`src/main` finds
//!   `.../src/bin/main.rs`);
//! * `ext:rs,toml` filters by extension; `type:dir` / `type:file` by kind.
//!
//! Each term is matched exact > prefix > word-boundary > substring > fuzzy
//! (subsequence); a 64-bit character mask rejects most names before any byte
//! comparison happens.

use super::{char_mask, fold};
use memchr::memmem;

/// Score bands. Higher is better.
pub mod band {
    pub const EXACT: u32 = 1000;
    pub const PREFIX: u32 = 800;
    pub const BOUNDARY: u32 = 600;
    pub const SUBSTRING: u32 = 400;
    /// Fuzzy scores are in `1..=FUZZY_MAX`.
    pub const FUZZY_MAX: u32 = 300;
}

/// Kind filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    Any,
    Dirs,
    Files,
}

/// One search term with its precompiled finder.
#[derive(Debug, Clone)]
pub struct Term {
    pub folded: Vec<u8>,
    finder: memmem::Finder<'static>,
    pub mask: u64,
}

impl Term {
    pub fn new(text: &str) -> Self {
        let folded = fold(text).into_owned().into_bytes();
        let finder = memmem::Finder::new(&folded).into_owned();
        let mask = char_mask(&folded);
        Term {
            folded,
            finder,
            mask,
        }
    }
}

impl Term {
    /// Substring test against a folded name.
    #[inline]
    pub fn matches(&self, folded: &[u8]) -> bool {
        self.finder.find(folded).is_some()
    }
}

/// A parsed query.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub terms: Vec<Term>,
    /// Ancestor segments, outermost first.
    pub ancestors: Vec<Term>,
    pub kind: KindFilter,
    /// Lower-case extensions without dots.
    pub extensions: Vec<String>,
    /// Union of all name-term masks.
    pub mask: u64,
    /// Allow fuzzy (subsequence) matches.
    pub fuzzy: bool,
}

impl Query {
    /// Parse user input.
    pub fn parse(input: &str) -> Query {
        let mut q = Query {
            fuzzy: true,
            ..Default::default()
        };
        for word in input.split_whitespace() {
            if let Some(exts) = word.strip_prefix("ext:") {
                q.extensions.extend(
                    exts.split([',', ';'])
                        .filter(|e| !e.is_empty())
                        .map(|e| e.trim_start_matches('.').to_ascii_lowercase()),
                );
                continue;
            }
            if let Some(kind) = word.strip_prefix("type:") {
                q.kind = match kind {
                    "dir" | "folder" | "d" => KindFilter::Dirs,
                    "file" | "f" => KindFilter::Files,
                    _ => KindFilter::Any,
                };
                continue;
            }
            if word.contains(['/', '\\']) {
                let segs: Vec<&str> = word.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
                if let Some((last, dirs)) = segs.split_last() {
                    q.ancestors.extend(dirs.iter().map(|d| Term::new(d)));
                    q.terms.push(Term::new(last));
                }
                continue;
            }
            q.terms.push(Term::new(word));
        }
        q.mask = q.terms.iter().fold(0, |m, t| m | t.mask);
        q
    }

    /// True when the query has nothing to match on.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.ancestors.is_empty()
            && self.extensions.is_empty()
            && self.kind == KindFilter::Any
    }

    /// Score a folded name against all terms; `None` if any term fails.
    /// The result is the mean term score plus a length-closeness bonus.
    #[inline]
    pub fn score_name(&self, folded: &[u8]) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(band::SUBSTRING);
        }
        let mut total = 0u32;
        for t in &self.terms {
            total += score_term(t, folded, self.fuzzy)?;
        }
        let n = self.terms.len() as u32;
        let qlen: usize = self.terms.iter().map(|t| t.folded.len()).sum();
        let closeness = (qlen.min(folded.len()) * 100 / folded.len().max(1)) as u32;
        Some(total / n + closeness)
    }

    /// Extension filter on the original name.
    #[inline]
    pub fn ext_ok(&self, folded: &[u8]) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        let Some(dot) = memchr::memrchr(b'.', folded) else {
            return false;
        };
        let ext = &folded[dot + 1..];
        self.extensions.iter().any(|e| e.as_bytes() == ext)
    }
}

/// Score one term against a folded name.
#[inline]
pub fn score_term(t: &Term, name: &[u8], allow_fuzzy: bool) -> Option<u32> {
    let q = &t.folded;
    if q.is_empty() {
        return Some(band::SUBSTRING);
    }
    if q.len() > name.len() {
        return None;
    }
    if let Some(pos) = t.finder.find(name) {
        if pos == 0 {
            return Some(if q.len() == name.len() {
                band::EXACT
            } else {
                band::PREFIX
            });
        }
        // Prefer the first word-boundary occurrence if any.
        if is_boundary(name, pos) || t.finder.find_iter(name).any(|p| is_boundary(name, p)) {
            return Some(band::BOUNDARY);
        }
        return Some(band::SUBSTRING);
    }
    if allow_fuzzy && q.len() >= 2 {
        return fuzzy_score(q, name);
    }
    None
}

#[inline]
fn is_boundary(name: &[u8], pos: usize) -> bool {
    pos == 0 || matches!(name[pos - 1], b' ' | b'_' | b'-' | b'.' | b'(' | b'[')
}

/// Subsequence match with bonuses for word starts and consecutive runs.
/// Returns a score in `1..=FUZZY_MAX` or `None`.
pub fn fuzzy_score(q: &[u8], name: &[u8]) -> Option<u32> {
    let mut pos = 0usize;
    let mut score: i32 = 0;
    let mut prev: Option<usize> = None;
    for &c in q {
        let rel = memchr::memchr(c, &name[pos..])?;
        let at = pos + rel;
        score += 10;
        if is_boundary(name, at) {
            score += 20;
        }
        match prev {
            Some(p) if p + 1 == at => score += 15,
            Some(p) => score -= ((at - p - 1) as i32).min(10),
            None => score -= (at as i32).min(15),
        }
        prev = Some(at);
        pos = at + 1;
    }
    let max = (q.len() as i32) * 45;
    let scaled = (score.max(1) as i64 * i64::from(band::FUZZY_MAX) / i64::from(max.max(1))) as u32;
    Some(scaled.clamp(1, band::FUZZY_MAX))
}

/// Check that ancestor terms match folder names on the way up, in order.
pub fn ancestors_match<'a>(terms: &[Term], mut ancestors: impl Iterator<Item = &'a [u8]>) -> bool {
    // terms are outermost-first; walk from the innermost term upwards.
    for t in terms.iter().rev() {
        loop {
            let Some(name) = ancestors.next() else {
                return false;
            };
            if t.finder.find(name).is_some() {
                break;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn s(q: &str, name: &str) -> Option<u32> {
        Query::parse(q).score_name(fold(name).as_bytes())
    }

    #[test]
    fn bands_are_ordered() {
        let exact = s("readme.md", "README.md").expect("exact");
        let prefix = s("read", "readme.md").expect("prefix");
        let boundary = s("me", "read_me.md").expect("boundary");
        let sub = s("adm", "readme.md").expect("substring");
        let fuzzy = s("rdme", "readme.md").expect("fuzzy");
        assert!(
            exact > prefix && prefix > boundary && boundary > sub && sub > fuzzy,
            "{exact} {prefix} {boundary} {sub} {fuzzy}"
        );
        assert!(s("xyz", "readme.md").is_none());
        assert!(s("read zzz", "readme.md").is_none(), "AND semantics");
        assert!(s("read md", "readme.md").is_some());
    }

    #[test]
    fn parse_filters_and_paths() {
        let q = Query::parse("ext:rs,.TOML type:file src/main");
        assert_eq!(q.extensions, vec!["rs", "toml"]);
        assert_eq!(q.kind, KindFilter::Files);
        assert_eq!(q.terms.len(), 1);
        assert_eq!(q.ancestors.len(), 1);
        assert!(q.ext_ok(b"main.rs"));
        assert!(!q.ext_ok(b"main.c"));
        assert!(!q.ext_ok(b"makefile"));
        assert!(Query::parse("  ").is_empty());
    }

    #[test]
    fn ancestors_in_order() {
        let q = Query::parse("nx/src/lib");
        let up: Vec<&[u8]> = vec![b"src", b"nx-fs", b"crates", b"home"];
        assert!(ancestors_match(&q.ancestors, up.into_iter()));
        let up: Vec<&[u8]> = vec![b"nx-fs", b"src"];
        assert!(
            !ancestors_match(&q.ancestors, up.into_iter()),
            "order matters"
        );
    }

    #[test]
    fn fuzzy_prefers_word_starts() {
        let a = fuzzy_score(b"fb", b"foo_bar").expect("match");
        let b = fuzzy_score(b"fb", b"xfxxxxb").expect("match");
        assert!(a > b);
        assert!(fuzzy_score(b"ba", b"ab").is_none());
    }

    proptest! {
        #[test]
        fn substring_always_matches(name in "[a-zA-Z0-9_. -]{1,30}", start in 0usize..30, len in 1usize..10) {
            let folded = fold(&name).into_owned();
            let start = start.min(folded.len() - 1);
            let end = (start + len).min(folded.len());
            let sub = &folded[start..end];
            prop_assume!(!sub.trim().is_empty() && !sub.contains(' '));
            let t = Term::new(sub);
            let sc = score_term(&t, folded.as_bytes(), false);
            prop_assert!(sc.is_some_and(|v| v >= band::SUBSTRING));
            // Mask prefilter is sound: a matching term's mask is a subset.
            prop_assert_eq!(char_mask(folded.as_bytes()) & t.mask, t.mask);
        }

        #[test]
        fn fuzzy_matches_iff_subsequence(q in "[a-c]{2,4}", name in "[a-d]{0,8}") {
            let is_subseq = {
                let mut it = name.bytes();
                q.bytes().all(|c| it.any(|n| n == c))
            };
            let r = fuzzy_score(q.as_bytes(), name.as_bytes());
            prop_assert_eq!(r.is_some(), is_subseq);
            if let Some(v) = r { prop_assert!((1..=band::FUZZY_MAX).contains(&v)); }
        }
    }
}
