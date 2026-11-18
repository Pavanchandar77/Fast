//! Frecency: frequency x recency with exponential decay (half-life based).
//!
//! Each opened path keeps a decayed score `s` and the time `t` it was last
//! updated: on access `s = s * 2^(-(now-t)/H) + 1`. Reading applies the same
//! decay, so old favourites fade without background work. Stored on disk as
//! a simple tab-separated file written atomically.

use crate::error::{FsError, FsResult, IoContext};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// Default half-life: one week.
pub const DEFAULT_HALF_LIFE_SECS: u64 = 7 * 24 * 3600;

/// Maximum ranking boost frecency can add to a match.
pub const MAX_BOOST: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrecencyEntry {
    pub score: f64,
    pub last: u64,
}

impl FrecencyEntry {
    fn decayed(&self, now: u64, half_life: u64) -> f64 {
        let dt = now.saturating_sub(self.last) as f64;
        self.score * (-dt / half_life.max(1) as f64).exp2()
    }
}

/// Path -> frecency store (paths are compared case-insensitively).
#[derive(Debug, Clone)]
pub struct FrecencyStore {
    entries: HashMap<String, FrecencyEntry>,
    half_life: u64,
    /// Entries below this decayed score are pruned on save.
    prune_below: f64,
}

impl Default for FrecencyStore {
    fn default() -> Self {
        FrecencyStore {
            entries: HashMap::new(),
            half_life: DEFAULT_HALF_LIFE_SECS,
            prune_below: 0.01,
        }
    }
}

fn key(path: &str) -> String {
    super::fold(&path.replace('\\', "/")).into_owned()
}

impl FrecencyStore {
    pub fn new(half_life_secs: u64) -> Self {
        FrecencyStore {
            half_life: half_life_secs.max(1),
            ..Default::default()
        }
    }

    /// Record that `path` was opened at `now` (unix seconds).
    pub fn record(&mut self, path: &str, now: u64) -> f64 {
        let hl = self.half_life;
        let e = self.entries.entry(key(path)).or_insert(FrecencyEntry {
            score: 0.0,
            last: now,
        });
        e.score = e.decayed(now, hl) + 1.0;
        e.last = now.max(e.last);
        e.score
    }

    /// Current decayed score.
    pub fn score(&self, path: &str, now: u64) -> f64 {
        self.entries
            .get(&key(path))
            .map_or(0.0, |e| e.decayed(now, self.half_life))
    }

    /// Ranking boost in `0..=MAX_BOOST` (logarithmic in score).
    pub fn boost(&self, path: &str, now: u64) -> u32 {
        boost_for(self.score(path, now))
    }

    pub fn forget(&mut self, path: &str) -> bool {
        self.entries.remove(&key(path)).is_some()
    }

    /// Wipe all history (privacy: "clear recents").
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FrecencyEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn half_life(&self) -> u64 {
        self.half_life
    }

    /// Top `n` paths by current score (for "Recents").
    pub fn top(&self, n: usize, now: u64) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|(k, e)| (k.clone(), e.decayed(now, self.half_life)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    /// Serialise (pruning faded entries) to `score\tlast\tpath` lines.
    pub fn to_text(&self, now: u64) -> String {
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort();
        let mut out = String::from("# nx-fs frecency v1\n");
        for k in keys {
            let e = &self.entries[k];
            if e.decayed(now, self.half_life) >= self.prune_below {
                out.push_str(&format!("{}\t{}\t{}\n", e.score, e.last, k));
            }
        }
        out
    }

    pub fn from_text(text: &str, origin: &Path) -> FsResult<Self> {
        let mut store = FrecencyStore::default();
        for (i, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let bad = || FsError::StoreCorrupt {
                path: origin.to_path_buf(),
                line: i + 1,
            };
            let score: f64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
            let last: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
            let path = parts.next().ok_or_else(bad)?;
            if !score.is_finite() || score < 0.0 {
                return Err(bad());
            }
            store
                .entries
                .insert(path.to_string(), FrecencyEntry { score, last });
        }
        Ok(store)
    }

    pub fn save(&self, file: &Path, now: u64) -> FsResult<()> {
        write_atomic(file, self.to_text(now).as_bytes())
    }

    pub fn load(file: &Path) -> FsResult<Self> {
        match std::fs::read_to_string(file) {
            Ok(t) => Self::from_text(&t, file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(FsError::from_io(file, e)),
        }
    }
}

/// Map a raw score to a ranking boost.
pub fn boost_for(score: f64) -> u32 {
    if score <= 0.0 {
        return 0;
    }
    ((score.ln_1p() * 60.0) as u32).min(MAX_BOOST)
}

/// Write a file atomically: temp file + fsync + rename.
pub(crate) fn write_atomic(file: &Path, bytes: &[u8]) -> FsResult<()> {
    let tmp = file.with_extension("nxtmp");
    {
        let mut f = std::fs::File::create(&tmp).at(&tmp)?;
        f.write_all(bytes).at(&tmp)?;
        f.sync_all().at(&tmp)?;
    }
    std::fs::rename(&tmp, file).at(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decays_by_half_life() {
        let mut s = FrecencyStore::new(100);
        s.record("C:\\A\\b.txt", 1000);
        s.record("c:/a/B.txt", 1000);
        assert!(
            (s.score("C:/a/b.txt", 1000) - 2.0).abs() < 1e-9,
            "case and separator insensitive"
        );
        assert!((s.score("C:/a/b.txt", 1100) - 1.0).abs() < 1e-9);
        assert!(s.boost("c:/a/b.txt", 1000) > s.boost("c:/a/b.txt", 5000));
        assert_eq!(s.boost("never", 0), 0);
    }

    #[test]
    fn frequent_beats_once_but_recent_beats_stale() {
        let mut s = FrecencyStore::new(100);
        for t in 0..10 {
            s.record("/often", t);
        }
        s.record("/once", 9);
        assert!(s.score("/often", 10) > s.score("/once", 10));
        s.record("/fresh", 2000);
        assert!(s.score("/fresh", 2000) > s.score("/often", 2000));
        assert_eq!(s.top(1, 2000)[0].0, "/fresh");
    }

    #[test]
    fn roundtrip_and_corruption() {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = dir.path().join("frecency.tsv");
        let mut s = FrecencyStore::default();
        s.record("/a/tab\tname", 50);
        s.record("/b", 60);
        s.save(&f, 60).expect("save");
        let back = FrecencyStore::load(&f).expect("load");
        assert_eq!(back.len(), 2);
        assert!((back.score("/b", 60) - 1.0).abs() < 1e-9);
        assert!(FrecencyStore::load(&dir.path().join("missing"))
            .expect("missing ok")
            .is_empty());
        assert!(matches!(
            FrecencyStore::from_text("x\ty\n", &f),
            Err(FsError::StoreCorrupt { line: 1, .. })
        ));
        assert!(boost_for(1e300) <= MAX_BOOST);
    }
}
