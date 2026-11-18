//! The search engine: parallel scan of a [`PathIndex`] with ranked top-K.

use super::frecency::{boost_for, FrecencyStore, MAX_BOOST};
use super::matcher::{ancestors_match, band, KindFilter, Query, Term};
use super::{flags, NodeId, PathIndex, NO_PARENT};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// One ranked result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: NodeId,
    pub score: u32,
    pub path: String,
    pub is_dir: bool,
}

/// Search options.
#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub limit: usize,
    pub include_hidden: bool,
    /// Worker threads for the scan (1 = current thread only).
    pub threads: usize,
    /// Indexes smaller than this are scanned on the calling thread.
    pub min_parallel: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            limit: 100,
            include_hidden: false,
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(16),
            min_parallel: 50_000,
        }
    }
}

/// Index + frecency, ready to answer queries.
#[derive(Debug, Default)]
pub struct SearchEngine {
    pub index: PathIndex,
    frecency: FrecencyStore,
    /// Resolved frecency by node id (only for paths the user opened).
    by_id: HashMap<NodeId, f64>,
    /// Clock used to decay frecency when (re)resolving.
    now: u64,
    /// Largest boost any node currently has (tightens the top-K skip bound).
    max_boost: u32,
}

/// Min-heap entry: lowest score (then highest id) on top so it is evicted.
type Ranked = Reverse<(u32, Reverse<NodeId>)>;

impl SearchEngine {
    pub fn new(index: PathIndex, frecency: FrecencyStore, now: u64) -> Self {
        let mut e = SearchEngine {
            index,
            frecency,
            by_id: HashMap::new(),
            now,
            max_boost: 0,
        };
        e.refresh_frecency(now);
        e
    }

    /// Re-resolve frecency paths to ids (after a rebuild or at a new time).
    pub fn refresh_frecency(&mut self, now: u64) {
        self.now = now;
        self.by_id.clear();
        for (path, _) in self.frecency.iter() {
            if let Some(id) = self.index.resolve(path) {
                self.by_id.insert(id, self.frecency.score(path, now));
            }
        }
        self.max_boost = self
            .by_id
            .values()
            .map(|&f| boost_for(f))
            .max()
            .unwrap_or(0);
    }

    /// The user opened `path`.
    pub fn record_open(&mut self, path: &str, now: u64) {
        let s = self.frecency.record(path, now);
        if let Some(id) = self.index.resolve(path) {
            self.by_id.insert(id, s);
            self.max_boost = self.max_boost.max(boost_for(s)).min(MAX_BOOST);
        }
        self.now = now;
    }

    pub fn frecency(&self) -> &FrecencyStore {
        &self.frecency
    }

    /// Privacy: forget all search/open history.
    pub fn clear_history(&mut self) {
        self.frecency.clear();
        self.by_id.clear();
        self.max_boost = 0;
    }

    /// Run a query.
    pub fn search(&self, query: &Query, opts: &SearchOptions) -> Vec<SearchHit> {
        if query.is_empty() || opts.limit == 0 {
            return Vec::new();
        }
        let n = self.index.capacity_ids();
        let threads = if n < opts.min_parallel {
            1
        } else {
            opts.threads.max(1)
        };
        let heap = if threads == 1 {
            self.scan(query, opts, 0, n)
        } else {
            let chunk = n.div_ceil(threads);
            std::thread::scope(|s| {
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        let (lo, hi) = (t * chunk, ((t + 1) * chunk).min(n));
                        s.spawn(move || self.scan(query, opts, lo, hi))
                    })
                    .collect();
                let mut merged: BinaryHeap<Ranked> = BinaryHeap::with_capacity(opts.limit + 1);
                for h in handles {
                    // A panicking scan thread only loses its chunk.
                    if let Ok(part) = h.join() {
                        for item in part {
                            push_bounded(&mut merged, item, opts.limit);
                        }
                    }
                }
                merged
            })
        };
        let mut hits: Vec<SearchHit> = heap
            .into_iter()
            .map(|Reverse((score, Reverse(id)))| SearchHit {
                id,
                score,
                path: self.index.full_path(id),
                is_dir: self.index.is_dir(id),
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.path.len().cmp(&b.path.len()))
                .then_with(|| a.path.cmp(&b.path))
        });
        hits
    }

    fn scan(&self, q: &Query, opts: &SearchOptions, lo: usize, hi: usize) -> BinaryHeap<Ranked> {
        let ix = &self.index;
        let mut heap: BinaryHeap<Ranked> = BinaryHeap::with_capacity(opts.limit + 1);
        let need = q.mask;
        let max_boost = self.max_boost;
        // Ancestor/hidden state memoised per folder for this scan.
        let mut chain = ChainMemo::new(ix, &q.ancestors);
        let single = match q.terms.as_slice() {
            [t] if !t.folded.is_empty() => Some(t),
            _ => None,
        };
        for i in lo..hi {
            let id = i as NodeId;
            // Deleted nodes have mask 0, so the mask test rejects them too
            // whenever the query has any characters.
            if ix.mask(id) & need != need {
                continue;
            }
            let fl = ix.node_flags(id);
            if fl & (flags::DELETED | flags::ROOT) != 0 {
                continue;
            }
            if !opts.include_hidden && fl & flags::HIDDEN != 0 {
                continue;
            }
            match q.kind {
                KindFilter::Dirs if fl & flags::DIR == 0 => continue,
                KindFilter::Files if fl & flags::DIR != 0 => continue,
                _ => {}
            }
            let folded = ix.fold_bytes(id);
            if !q.ext_ok(folded) {
                continue;
            }
            // Single-term upper bound without matching: a name that does not
            // start with the term scores at most BOUNDARY + closeness.
            if let (Some(t), true) = (single, heap.len() >= opts.limit) {
                if folded.first() != t.folded.first() {
                    if let Some(Reverse((worst, _))) = heap.peek() {
                        let closeness =
                            (t.folded.len().min(folded.len()) * 100 / folded.len().max(1)) as u32;
                        if band::BOUNDARY + closeness + max_boost < *worst {
                            continue;
                        }
                    }
                }
            }
            let Some(mut score) = q.score_name(folded) else {
                continue;
            };
            // Cheap bound: even the maximum frecency boost cannot get it in.
            if heap.len() >= opts.limit {
                if let Some(Reverse((worst, _))) = heap.peek() {
                    if score + max_boost < *worst {
                        continue;
                    }
                }
            }
            let parent = ix.parent(id);
            let (anc_ok, hidden) = chain.state(parent);
            if !anc_ok || (!opts.include_hidden && hidden) {
                continue;
            }
            if let Some(&f) = self.by_id.get(&id) {
                score += boost_for(f);
            }
            push_bounded(&mut heap, Reverse((score, Reverse(id))), opts.limit);
        }
        heap
    }
}

fn push_bounded(heap: &mut BinaryHeap<Ranked>, item: Ranked, limit: usize) {
    if heap.len() < limit {
        heap.push(item);
    } else if let Some(top) = heap.peek() {
        // `top` is the worst kept result; replace it if `item` is better.
        if item < *top {
            heap.pop();
            heap.push(item);
        }
    }
}

/// Per-scan memo over folders: how many ancestor terms (outermost first)
/// match greedily from the root down to a folder, and whether the chain has
/// a hidden folder. Each folder is evaluated once per scan, so deep trees
/// cost O(folders) instead of O(hits x depth).
struct ChainMemo<'a> {
    ix: &'a PathIndex,
    terms: &'a [Term],
    /// 0 = unknown, else `(matched + 1) | HIDDEN_BIT`.
    memo: Vec<u8>,
    stack: Vec<NodeId>,
}

impl<'a> ChainMemo<'a> {
    const HIDDEN_BIT: u8 = 0x80;
    /// Max ancestor terms the byte encoding holds; longer queries fall back
    /// to walking the chain.
    const MAX_TERMS: usize = 120;
    const MAX_DEPTH: usize = 4096;

    fn new(ix: &'a PathIndex, terms: &'a [Term]) -> Self {
        ChainMemo {
            ix,
            terms,
            memo: vec![0; ix.capacity_ids()],
            stack: Vec::new(),
        }
    }

    /// `(all ancestor terms matched, chain contains a hidden folder)` for
    /// the chain root..=`node`.
    fn state(&mut self, node: NodeId) -> (bool, bool) {
        if self.terms.len() > Self::MAX_TERMS {
            let anc = ancestors_match(
                self.terms,
                AncestorIter {
                    ix: self.ix,
                    cur: node,
                },
            );
            return (anc, hidden_chain(self.ix, node));
        }
        let mut cur = node;
        self.stack.clear();
        while cur != NO_PARENT
            && self.memo.get(cur as usize) == Some(&0)
            && self.stack.len() < Self::MAX_DEPTH
        {
            self.stack.push(cur);
            cur = self.ix.parent(cur);
        }
        let (mut h, mut hid) = match self.memo.get(cur as usize) {
            Some(&v) if v != 0 && self.stack.len() < Self::MAX_DEPTH => (
                usize::from(v & !Self::HIDDEN_BIT) - 1,
                v & Self::HIDDEN_BIT != 0,
            ),
            _ => (0, false),
        };
        while let Some(n) = self.stack.pop() {
            if h < self.terms.len() && self.terms[h].matches(self.ix.fold_bytes(n)) {
                h += 1;
            }
            hid |= self.ix.node_flags(n) & flags::HIDDEN != 0;
            if let Some(slot) = self.memo.get_mut(n as usize) {
                *slot = (h as u8 + 1) | if hid { Self::HIDDEN_BIT } else { 0 };
            }
        }
        (h == self.terms.len(), hid)
    }
}

/// True if `start` or any of its ancestors is hidden.
fn hidden_chain(ix: &PathIndex, start: NodeId) -> bool {
    let mut cur = start;
    let mut guard = 0;
    while cur != NO_PARENT && guard < 4096 {
        if ix.node_flags(cur) & flags::HIDDEN != 0 {
            return true;
        }
        cur = ix.parent(cur);
        guard += 1;
    }
    false
}

struct AncestorIter<'a> {
    ix: &'a PathIndex,
    cur: NodeId,
}

impl<'a> Iterator for AncestorIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        if self.cur == NO_PARENT {
            return None;
        }
        let id = self.cur;
        self.cur = self.ix.parent(id);
        Some(self.ix.fold_bytes(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexChange;

    fn engine(paths: &[&str]) -> SearchEngine {
        let mut ix = PathIndex::new();
        ix.add_root("/r").expect("root");
        for p in paths {
            ix.apply(&IndexChange::Created {
                path: format!("/r/{p}"),
                is_dir: !p.contains('.'),
            })
            .expect("create");
        }
        SearchEngine::new(ix, FrecencyStore::default(), 0)
    }

    fn top(e: &SearchEngine, q: &str) -> Vec<String> {
        e.search(
            &Query::parse(q),
            &SearchOptions {
                limit: 10,
                threads: 1,
                ..Default::default()
            },
        )
        .into_iter()
        .map(|h| h.path)
        .collect()
    }

    #[test]
    fn ranks_exact_then_prefix_then_substring() {
        let e = engine(&[
            "docs/main.rs",
            "src/main_window.rs",
            "src/domain.rs",
            "x/m_a_i_n.txt",
        ]);
        let r = top(&e, "main.rs");
        assert_eq!(r[0], "/r/docs/main.rs");
        let r = top(&e, "main");
        assert_eq!(r[0], "/r/docs/main.rs");
        assert_eq!(r[1], "/r/src/main_window.rs");
        assert!(r.contains(&"/r/src/domain.rs".to_string()));
        assert!(
            r.contains(&"/r/x/m_a_i_n.txt".to_string()),
            "fuzzy fallback"
        );
        assert_eq!(r.last().map(String::as_str), Some("/r/x/m_a_i_n.txt"));
    }

    #[test]
    fn frecency_lifts_results() {
        let mut e = engine(&["a/report.txt", "b/report.txt", "c/report.txt"]);
        e.record_open("/r/c/report.txt", 10);
        e.record_open("/r/c/report.txt", 11);
        assert_eq!(top(&e, "report")[0], "/r/c/report.txt");
        e.clear_history();
        assert_eq!(top(&e, "report")[0], "/r/a/report.txt");
    }

    #[test]
    fn filters_hidden_kinds_and_paths() {
        let e = engine(&[
            ".git/config.txt",
            "src/config.rs",
            "config",
            "src/bin/config.toml",
        ]);
        let r = top(&e, "config");
        assert!(
            !r.iter().any(|p| p.contains(".git")),
            "hidden subtree excluded"
        );
        assert_eq!(top(&e, "config type:dir"), vec!["/r/config"]);
        assert_eq!(top(&e, "config ext:toml"), vec!["/r/src/bin/config.toml"]);
        assert_eq!(
            top(&e, "src/config"),
            vec!["/r/src/config.rs", "/r/src/bin/config.toml"]
        );
        let all = e.search(
            &Query::parse("config"),
            &SearchOptions {
                include_hidden: true,
                threads: 1,
                limit: 10,
                ..Default::default()
            },
        );
        assert!(all.iter().any(|h| h.path.contains(".git")));
    }

    #[test]
    fn limit_and_parallel_agree() {
        let names: Vec<String> = (0..200)
            .map(|i| format!("d{}/file{i}.txt", i % 7))
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let e = engine(&refs);
        let q = Query::parse("file1");
        let one = e.search(
            &q,
            &SearchOptions {
                limit: 15,
                threads: 1,
                ..Default::default()
            },
        );
        let many = e.search(
            &q,
            &SearchOptions {
                limit: 15,
                threads: 4,
                min_parallel: 0,
                ..Default::default()
            },
        );
        assert_eq!(one.len(), 15);
        assert_eq!(one, many);
        assert_eq!(one[0].path, "/r/d1/file1.txt");
    }

    proptest::proptest! {
        /// The optimised scan (bounds, memoised ancestors) returns exactly
        /// what a brute-force filter + sort would.
        #[test]
        fn scan_matches_brute_force(
            paths in proptest::collection::vec("(\\.?[ab]{1,2}/){0,3}[abc]{1,3}(\\.t)?", 1..60),
            q in "([ab]{1,2}/)?[abc]{1,2}",
            hidden: bool,
            limit in 1usize..8,
        ) {
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            let e = engine(&refs);
            let query = Query::parse(&q);
            let opts = SearchOptions { limit, threads: 1, include_hidden: hidden, ..Default::default() };
            let got: Vec<(u32, String)> = e.search(&query, &opts).into_iter().map(|h| (h.score, h.path)).collect();
            let ix = &e.index;
            let mut want: Vec<(u32, String)> = ix
                .ids()
                .filter(|&id| ix.node_flags(id) & (flags::DELETED | flags::ROOT) == 0)
                .filter(|&id| hidden || (ix.node_flags(id) & flags::HIDDEN == 0 && !hidden_chain(ix, ix.parent(id))))
                .filter(|&id| ancestors_match(&query.ancestors, AncestorIter { ix, cur: ix.parent(id) }))
                .filter_map(|id| query.score_name(ix.fold_bytes(id)).map(|s| (s, ix.full_path(id))))
                .collect();
            want.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(&b.1)));
            let want_scores: Vec<u32> = want.iter().take(limit).map(|w| w.0).collect();
            let got_scores: Vec<u32> = got.iter().map(|g| g.0).collect();
            proptest::prop_assert_eq!(got_scores, want_scores);
            for g in &got {
                proptest::prop_assert!(want.contains(g));
            }
        }
    }
}
