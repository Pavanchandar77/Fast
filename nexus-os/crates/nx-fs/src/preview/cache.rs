//! Byte-budgeted LRU cache for previews, validated by (mtime, size).

use super::Preview;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// File identity used to invalidate stale previews.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub modified: Option<SystemTime>,
    pub size: u64,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Stamp> {
        let md = std::fs::symlink_metadata(crate::path::to_native(path)).ok()?;
        Some(Stamp {
            modified: md.modified().ok(),
            size: md.len(),
        })
    }
}

struct Slot {
    preview: Arc<Preview>,
    stamp: Stamp,
    bytes: usize,
    tick: u64,
}

/// LRU keyed by path.
pub struct PreviewCache {
    map: HashMap<PathBuf, Slot>,
    order: BTreeMap<u64, PathBuf>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl PreviewCache {
    pub fn new(budget_bytes: usize) -> Self {
        PreviewCache {
            map: HashMap::new(),
            order: BTreeMap::new(),
            tick: 0,
            bytes: 0,
            budget: budget_bytes,
        }
    }

    /// Get without validation (UI fast path); refreshes recency.
    pub fn get(&mut self, path: &Path) -> Option<Arc<Preview>> {
        self.tick += 1;
        let tick = self.tick;
        let slot = self.map.get_mut(path)?;
        self.order.remove(&slot.tick);
        slot.tick = tick;
        self.order.insert(tick, path.to_path_buf());
        Some(Arc::clone(&slot.preview))
    }

    /// Get only if the stamp still matches.
    pub fn get_valid(&mut self, path: &Path, stamp: Stamp) -> Option<Arc<Preview>> {
        if self.map.get(path).is_some_and(|s| s.stamp == stamp) {
            return self.get(path);
        }
        self.remove(path);
        None
    }

    pub fn insert(&mut self, path: PathBuf, stamp: Stamp, preview: Arc<Preview>) {
        self.remove(&path);
        let bytes = preview.approx_bytes() + path.as_os_str().len() + 64;
        if bytes > self.budget {
            return;
        }
        self.tick += 1;
        self.order.insert(self.tick, path.clone());
        self.map.insert(
            path,
            Slot {
                preview,
                stamp,
                bytes,
                tick: self.tick,
            },
        );
        self.bytes += bytes;
        while self.bytes > self.budget {
            let Some((_, victim)) = self.order.pop_first() else {
                break;
            };
            if let Some(s) = self.map.remove(&victim) {
                self.bytes -= s.bytes;
            }
        }
    }

    pub fn remove(&mut self, path: &Path) {
        if let Some(s) = self.map.remove(path) {
            self.order.remove(&s.tick);
            self.bytes -= s.bytes;
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: usize) -> Arc<Preview> {
        Arc::new(Preview::Text {
            text: "x".repeat(n),
            lines: 1,
            truncated: false,
            encoding: "UTF-8",
        })
    }

    #[test]
    fn evicts_lru_and_validates() {
        let st = Stamp {
            modified: None,
            size: 1,
        };
        let mut c = PreviewCache::new(2500);
        c.insert("/a".into(), st, p(800));
        c.insert("/b".into(), st, p(800));
        assert!(c.get(Path::new("/a")).is_some()); // a is now most recent
        c.insert("/c".into(), st, p(800));
        assert!(c.get(Path::new("/b")).is_none(), "b evicted");
        assert!(c.get(Path::new("/a")).is_some());
        assert!(c.bytes() <= 2500);
        assert!(
            c.get_valid(
                Path::new("/a"),
                Stamp {
                    modified: None,
                    size: 2
                }
            )
            .is_none(),
            "stale"
        );
        assert!(c.get(Path::new("/a")).is_none());
        c.insert("/huge".into(), st, p(10_000));
        assert!(
            c.get(Path::new("/huge")).is_none(),
            "over budget never cached"
        );
        c.clear();
        assert!(c.is_empty());
    }
}
