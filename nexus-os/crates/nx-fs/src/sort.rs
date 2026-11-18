//! Finder/Explorer-style sorting: natural number order, case-insensitive,
//! folders first.

use crate::entry::DirEntry;
use std::cmp::Ordering;

/// Column a listing can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
    Kind,
}

/// Sort specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SortSpec {
    pub key: SortKey,
    pub ascending: bool,
    /// Keep directories grouped before files (Explorer default).
    pub dirs_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            key: SortKey::Name,
            ascending: true,
            dirs_first: true,
        }
    }
}

/// Natural, case-insensitive comparison: `file2` < `file10`, `a` == `A`
/// (ties broken by the raw string so the order is total and stable).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_cmp_ci(a, b).then_with(|| a.cmp(b))
}

fn natural_cmp_ci(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return natural_cmp_ascii(a.as_bytes(), b.as_bytes());
    }
    natural_cmp_tokens(a, b)
}

/// Byte-level fast path for ASCII names; identical order to the token path.
fn natural_cmp_ascii(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    loop {
        let (x, y) = match (a.get(i), b.get(j)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&x), Some(&y)) => (x, y),
        };
        match (x.is_ascii_digit(), y.is_ascii_digit()) {
            (true, true) => {
                let ei = i + a[i..].iter().take_while(|c| c.is_ascii_digit()).count();
                let ej = j + b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
                let (na, nb) = (&a[i..ei], &b[j..ej]);
                let za = na.iter().take_while(|&&c| c == b'0').count();
                let zb = nb.iter().take_while(|&&c| c == b'0').count();
                let (ta, tb) = (&na[za..], &nb[zb..]);
                let ord = ta
                    .len()
                    .cmp(&tb.len())
                    .then_with(|| ta.cmp(tb))
                    .then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
                i = ei;
                j = ej;
            }
            (true, false) => return b'0'.cmp(&y.to_ascii_lowercase()).then(Ordering::Less),
            (false, true) => return x.to_ascii_lowercase().cmp(&b'0').then(Ordering::Greater),
            (false, false) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                i += 1;
                j += 1;
            }
        }
    }
}

fn natural_cmp_tokens(a: &str, b: &str) -> Ordering {
    let mut ta = Tokens { s: a };
    let mut tb = Tokens { s: b };
    loop {
        match (ta.next(), tb.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = x.cmp_token(&y);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// A natural-sort token: a run of ASCII digits, or one lower-cased char.
enum Token<'a> {
    Num(&'a str),
    Char(char),
}

impl Token<'_> {
    /// Total order over tokens. Numbers occupy the position of `'0'` in the
    /// character order, so the overall order stays a lexicographic order over
    /// a totally ordered alphabet (and therefore transitive).
    fn cmp_token(&self, other: &Token<'_>) -> Ordering {
        match (self, other) {
            (Token::Num(x), Token::Num(y)) => {
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                tx.len()
                    .cmp(&ty.len())
                    .then_with(|| tx.cmp(ty))
                    .then_with(|| x.len().cmp(&y.len()))
            }
            (Token::Num(_), Token::Char(c)) => '0'.cmp(c).then(Ordering::Less),
            (Token::Char(c), Token::Num(_)) => c.cmp(&'0').then(Ordering::Greater),
            (Token::Char(x), Token::Char(y)) => x.cmp(y),
        }
    }
}

struct Tokens<'a> {
    s: &'a str,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;
    fn next(&mut self) -> Option<Token<'a>> {
        let first = self.s.chars().next()?;
        if first.is_ascii_digit() {
            let n = self.s.bytes().take_while(u8::is_ascii_digit).count();
            let (num, rest) = self.s.split_at(n);
            self.s = rest;
            return Some(Token::Num(num));
        }
        self.s = &self.s[first.len_utf8()..];
        // Single-char lowercase keeps tokens 1:1 with input chars; multi-char
        // expansions (rare, e.g. 'İ') use their first char.
        let lower = first.to_lowercase().next().unwrap_or(first);
        Some(Token::Char(lower))
    }
}

/// Compare two entries under `spec`. Total order (falls back to name).
pub fn compare_entries(a: &DirEntry, b: &DirEntry, spec: SortSpec) -> Ordering {
    if spec.dirs_first {
        let ord = b.kind.is_dir().cmp(&a.kind.is_dir());
        if ord != Ordering::Equal {
            return ord;
        }
    }
    let primary = match spec.key {
        SortKey::Name => Ordering::Equal,
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Kind => a.extension().cmp(&b.extension()),
    };
    let ord = primary.then_with(|| natural_cmp(&a.display_name, &b.display_name));
    if spec.ascending {
        ord
    } else {
        ord.reverse()
    }
}

/// Sort a listing in place (keys are computed once per entry, then the
/// sort compares plain bytes: 100k names sort in a few ms).
pub fn sort_entries(entries: &mut [DirEntry], spec: SortSpec) {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    sort_indices(entries, &mut order, spec);
    apply_permutation(entries, order);
}

/// Sort `idx` (indices into `entries`) by `spec`, same order as
/// [`compare_entries`]. Used by UIs that keep a view over unsorted storage.
pub fn sort_indices(entries: &[DirEntry], idx: &mut [usize], spec: SortSpec) {
    struct Keyed {
        dir: bool,
        primary: Primary,
        name: Vec<u8>,
        at: usize,
    }
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Primary {
        None,
        Size(u64),
        Modified(Option<std::time::SystemTime>),
        Kind(String),
    }
    let mut keyed: Vec<Keyed> = idx
        .iter()
        .filter_map(|&at| {
            let e = entries.get(at)?;
            Some(Keyed {
                dir: e.kind.is_dir(),
                primary: match spec.key {
                    SortKey::Name => Primary::None,
                    SortKey::Size => Primary::Size(e.size),
                    SortKey::Modified => Primary::Modified(e.modified),
                    SortKey::Kind => Primary::Kind(e.extension()),
                },
                name: natural_key(&e.display_name),
                at,
            })
        })
        .collect();
    keyed.sort_unstable_by(|a, b| {
        if spec.dirs_first {
            let ord = b.dir.cmp(&a.dir);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        let ord = a
            .primary
            .cmp(&b.primary)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| entries[a.at].display_name.cmp(&entries[b.at].display_name));
        if spec.ascending {
            ord
        } else {
            ord.reverse()
        }
    });
    for (slot, k) in idx.iter_mut().zip(keyed) {
        *slot = k.at;
    }
}

/// Byte key whose lexicographic order equals the case-insensitive natural
/// order: a digit run becomes `'0'`, its significant length (u16 BE), the
/// significant digits and its raw length; other chars become their
/// lower-cased UTF-8.
pub fn natural_key(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 8);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            let end = i + b[i..].iter().take_while(|d| d.is_ascii_digit()).count();
            let run = &b[i..end];
            let zeros = run.iter().take_while(|&&d| d == b'0').count();
            let sig = &run[zeros..];
            out.push(b'0');
            out.extend_from_slice(&(sig.len().min(u16::MAX as usize) as u16).to_be_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(run.len().min(u16::MAX as usize) as u16).to_be_bytes());
            i = end;
        } else if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
            i += 1;
        } else {
            let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let mut buf = [0u8; 4];
            out.extend_from_slice(lower.encode_utf8(&mut buf).as_bytes());
            i += ch.len_utf8().max(1);
        }
    }
    out
}

/// Reorder in place so slot `k` holds the old `entries[order[k]]`
/// (cycle-following swaps, no clones).
fn apply_permutation(entries: &mut [DirEntry], order: Vec<usize>) {
    let n = entries.len().min(order.len());
    let mut done = vec![false; n];
    for i in 0..n {
        if done[i] {
            continue;
        }
        let mut j = i;
        loop {
            done[j] = true;
            let src = order[j];
            if src == i || src >= n || done[src] {
                break;
            }
            entries.swap(j, src);
            j = src;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;
    use proptest::prelude::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "File2", "file1", "file02", "a", "B"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a", "B", "file1", "File2", "file02", "file10"]);
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
        assert_eq!(natural_cmp("Ärger", "zebra"), Ordering::Greater);
        assert_eq!(
            natural_cmp("img99999999999999999999", "img100000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn dirs_first_and_keys() {
        let mut d = DirEntry::new("zdir".into(), EntryKind::Dir, 0);
        d.size = 0;
        let a = DirEntry::new("a.txt".into(), EntryKind::File, 50);
        let b = DirEntry::new("b.rs".into(), EntryKind::File, 5);
        let mut v = vec![a.clone(), b.clone(), d.clone()];
        sort_entries(&mut v, SortSpec::default());
        assert_eq!(v[0].display_name, "zdir");
        sort_entries(
            &mut v,
            SortSpec {
                key: SortKey::Size,
                ascending: false,
                dirs_first: false,
            },
        );
        assert_eq!(v[0].display_name, "a.txt");
        sort_entries(
            &mut v,
            SortSpec {
                key: SortKey::Kind,
                ascending: true,
                dirs_first: true,
            },
        );
        let names: Vec<_> = v.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["zdir", "b.rs", "a.txt"]);
    }

    proptest! {
        #[test]
        fn natural_cmp_is_a_total_order(a in "[a-cA-C0-9é]{0,6}", b in "[a-cA-C0-9é]{0,6}", c in "[a-cA-C0-9é]{0,6}") {
            prop_assert_eq!(natural_cmp(&a, &b), natural_cmp(&b, &a).reverse());
            prop_assert_eq!(natural_cmp(&a, &b) == Ordering::Equal, a == b);
            if natural_cmp(&a, &b) != Ordering::Greater && natural_cmp(&b, &c) != Ordering::Greater {
                prop_assert_ne!(natural_cmp(&a, &c), Ordering::Greater);
            }
        }

        #[test]
        fn ascii_fast_path_matches_token_path(a in "[a-dA-D0-9_. ]{0,10}", b in "[a-dA-D0-9_. ]{0,10}") {
            prop_assert_eq!(natural_cmp_ascii(a.as_bytes(), b.as_bytes()), natural_cmp_tokens(&a, &b));
        }

        #[test]
        fn natural_key_order_matches_natural_cmp(a in "[a-cA-C0-9é_ ]{0,8}", b in "[a-cA-C0-9é_ ]{0,8}") {
            prop_assert_eq!(natural_key(&a).cmp(&natural_key(&b)), natural_cmp_ci(&a, &b));
        }

        #[test]
        fn sort_entries_matches_compare_entries(names in proptest::collection::vec("[a-cA-C0-9.]{1,6}", 0..40), key in 0usize..4, asc: bool) {
            let key = [SortKey::Name, SortKey::Size, SortKey::Kind, SortKey::Modified][key];
            let spec = SortSpec { key, ascending: asc, dirs_first: true };
            let mut names = names;
            names.sort();
            names.dedup();
            let mut v: Vec<DirEntry> = names.iter().map(|n| {
                let kind = if n.contains('.') { EntryKind::File } else { EntryKind::Dir };
                DirEntry::new(n.into(), kind, (n.len() % 3) as u64)
            }).collect();
            sort_entries(&mut v, spec);
            for w in v.windows(2) {
                prop_assert_ne!(compare_entries(&w[0], &w[1], spec), Ordering::Greater);
            }
        }

        #[test]
        fn sorting_is_a_permutation(names in proptest::collection::vec("[a-z0-9]{1,8}", 0..50)) {
            let mut v: Vec<DirEntry> = names.iter().map(|n| DirEntry::new(n.into(), EntryKind::File, n.len() as u64)).collect();
            for key in [SortKey::Name, SortKey::Size, SortKey::Kind] {
                sort_entries(&mut v, SortSpec { key, ascending: true, dirs_first: true });
                let mut got: Vec<_> = v.iter().map(|e| e.display_name.clone()).collect();
                got.sort();
                let mut want = names.clone();
                want.sort();
                prop_assert_eq!(got, want);
            }
        }
    }
}
