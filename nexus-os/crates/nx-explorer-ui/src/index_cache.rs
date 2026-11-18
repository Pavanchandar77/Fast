//! Persistent on-disk index manager with corruption recovery and incremental watcher updates.

use nx_explorer::action::DirDiff;
use nx_fs::error::FsResult;
use nx_fs::index::engine::{SearchEngine, SearchHit, SearchOptions};
use nx_fs::index::frecency::FrecencyStore;
use nx_fs::index::matcher::Query;
use nx_fs::index::{IndexChange, PathIndex};
use std::path::{Path, PathBuf};

pub struct PersistentIndexManager {
    path: PathBuf,
    engine: SearchEngine,
    dirty: bool,
}

impl PersistentIndexManager {
    /// Load index from disk cache, or cleanly rebuild from `rebuild_fn` if missing or corrupt.
    pub fn load_or_rebuild<F>(path: PathBuf, rebuild_fn: F) -> Self
    where
        F: FnOnce() -> PathIndex,
    {
        let engine = if path.exists() {
            match PathIndex::load_file(&path) {
                Ok(idx) => {
                    tracing::info!(
                        path = %path.display(),
                        nodes = idx.len(),
                        "Loaded persistent index from disk"
                    );
                    SearchEngine::new(idx, FrecencyStore::default(), 0)
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "Index cache corrupted or unreadable; rebuilding clean index"
                    );
                    let fresh_idx = rebuild_fn();
                    let _ = fresh_idx.save_file(&path);
                    SearchEngine::new(fresh_idx, FrecencyStore::default(), 0)
                }
            }
        } else {
            let fresh_idx = rebuild_fn();
            let _ = fresh_idx.save_file(&path);
            SearchEngine::new(fresh_idx, FrecencyStore::default(), 0)
        };

        Self {
            path,
            engine,
            dirty: false,
        }
    }

    pub fn engine(&self) -> &SearchEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut SearchEngine {
        &mut self.engine
    }

    /// Search the persistent index.
    pub fn search(&self, query_str: &str, limit: usize) -> Vec<SearchHit> {
        let q = Query::parse(query_str);
        let opts = SearchOptions {
            limit,
            ..Default::default()
        };
        self.engine.search(&q, &opts)
    }

    /// Incrementally apply folder watcher diffs to the search index without rebuilding.
    pub fn apply_diffs(&mut self, dir: &Path, diffs: &[DirDiff]) {
        for diff in diffs {
            match diff {
                DirDiff::Created(entry) => {
                    let full = dir.join(&entry.name);
                    let change = IndexChange::Created {
                        path: full.to_string_lossy().to_string(),
                        is_dir: entry.kind.is_dir(),
                    };
                    let _ = self.engine.index.apply(&change);
                    self.dirty = true;
                }
                DirDiff::Removed(name) => {
                    let full = dir.join(name);
                    let change = IndexChange::Removed {
                        path: full.to_string_lossy().to_string(),
                    };
                    let _ = self.engine.index.apply(&change);
                    self.dirty = true;
                }
                DirDiff::Renamed { from, to } => {
                    let from_full = dir.join(from);
                    let to_full = dir.join(&to.name);
                    let change = IndexChange::Renamed {
                        from: from_full.to_string_lossy().to_string(),
                        to: to_full.to_string_lossy().to_string(),
                    };
                    let _ = self.engine.index.apply(&change);
                    self.dirty = true;
                }
                DirDiff::Updated(_) => {}
            }
        }
    }

    /// Save the index to disk crash-safely if dirty.
    pub fn save_if_dirty(&mut self) -> FsResult<()> {
        if self.dirty {
            self.engine.index.save_file(&self.path)?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Force immediate crash-safe save.
    pub fn save(&mut self) -> FsResult<()> {
        self.engine.index.save_file(&self.path)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> PathIndex {
        let mut idx = PathIndex::new();
        let r = idx.add_root("C:").expect("root");
        let users = idx.insert(r, "Users", true).expect("users");
        let me = idx.insert(users, "Pavan", true).expect("me");
        idx.insert(me, "Document.pdf", false).expect("doc");
        idx.insert(me, "Notes.txt", false).expect("notes");
        idx
    }

    #[test]
    fn load_or_rebuild_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("index.nxix");
        assert!(!path.exists());

        let manager = PersistentIndexManager::load_or_rebuild(path.clone(), sample_index);
        assert!(path.exists(), "must have written rebuilt index");
        assert_eq!(manager.engine().index.len(), 5);

        let hits = manager.search("Document", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, r"C:\Users\Pavan\Document.pdf");
    }

    #[test]
    fn load_or_rebuild_corrupted_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrupt.nxix");

        // Write junk data into index file
        std::fs::write(&path, b"CORRUPTED_GARBAGE_DATA_12345").expect("write junk");

        let mut rebuilt_count = 0;
        let manager = PersistentIndexManager::load_or_rebuild(path.clone(), || {
            rebuilt_count += 1;
            sample_index()
        });

        assert_eq!(rebuilt_count, 1, "must trigger rebuild on corrupt file");
        assert_eq!(manager.engine().index.len(), 5);
    }

    #[test]
    fn incremental_watcher_diff_application() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("index.nxix");

        let mut manager = PersistentIndexManager::load_or_rebuild(path, sample_index);
        let watch_dir = Path::new(r"C:\Users\Pavan");

        // Apply Created diff
        let new_file = nx_fs::DirEntry::new("Project.rs".into(), nx_fs::EntryKind::File, 2048);
        manager.apply_diffs(watch_dir, &[DirDiff::Created(new_file)]);

        let hits = manager.search("Project", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, r"C:\Users\Pavan\Project.rs");

        // Apply Removed diff
        manager.apply_diffs(watch_dir, &[DirDiff::Removed("Document.pdf".into())]);
        let hits_after = manager.search("Document", 10);
        assert!(
            hits_after.is_empty(),
            "removed file must not show in search"
        );
    }
}
