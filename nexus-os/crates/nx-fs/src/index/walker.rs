//! Portable index source: a parallel walk of one or more roots.
//!
//! Used for non-NTFS drives (exFAT USB sticks, network shares) and on Linux.
//! Live updates arrive as [`IndexChange`]s through [`WalkIndexer::changes`]
//! (fed by a directory watcher or by nx-fs's own file operations).

use super::{IndexChange, NodeId, PathIndex, VolumeIndexer};
use crate::entry::EntryKind;
use crate::error::{FsError, FsResult};
use crate::pool::CancelToken;
use crate::walk::{parallel_walk, WalkOptions};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;
use std::path::PathBuf;

/// Walk-based indexer.
pub struct WalkIndexer {
    roots: Vec<PathBuf>,
    opts: WalkOptions,
    tx: Sender<IndexChange>,
    rx: Receiver<IndexChange>,
    /// Errors from the last build (permission denied folders etc.).
    pub last_errors: Vec<FsError>,
}

impl WalkIndexer {
    pub fn new(roots: Vec<PathBuf>, opts: WalkOptions) -> Self {
        let (tx, rx) = unbounded();
        WalkIndexer {
            roots,
            opts,
            tx,
            rx,
            last_errors: Vec::new(),
        }
    }

    /// Sender for live changes.
    pub fn changes(&self) -> Sender<IndexChange> {
        self.tx.clone()
    }
}

impl VolumeIndexer for WalkIndexer {
    fn name(&self) -> &'static str {
        "walker"
    }

    fn build(&mut self, cancel: &CancelToken) -> FsResult<PathIndex> {
        let index = Mutex::new(PathIndex::new());
        let mut jobs: Vec<(PathBuf, NodeId)> = Vec::new();
        {
            let mut ix = index.lock();
            for r in &self.roots {
                let s = r.to_string_lossy();
                jobs.push((r.clone(), ix.add_root(&s)?));
            }
        }
        let insert_err: Mutex<Option<FsError>> = Mutex::new(None);
        let stats = parallel_walk(jobs, &self.opts, cancel, |_, &parent, entries| {
            let mut ix = index.lock();
            let mut descend = Vec::new();
            for (i, e) in entries.iter().enumerate() {
                let name = e.name.to_string_lossy();
                match ix.insert(parent, &name, e.kind == EntryKind::Dir) {
                    Ok(id) => {
                        if e.kind.is_link() {
                            ix.set_flags(id, super::flags::LINK);
                        }
                        if e.kind == EntryKind::Dir || e.link_to_dir {
                            descend.push((i, id));
                        }
                    }
                    Err(err) => {
                        insert_err.lock().get_or_insert(err);
                    }
                }
            }
            descend
        });
        if let Some(e) = insert_err.into_inner() {
            return Err(e);
        }
        if stats.cancelled {
            return Err(FsError::Cancelled);
        }
        self.last_errors = stats.errors;
        // Changes that raced with the build are applied on the next sync.
        let mut index = index.into_inner();
        index.shrink_to_fit();
        Ok(index)
    }

    fn sync(&mut self, index: &mut PathIndex) -> FsResult<usize> {
        let mut n = 0;
        while let Ok(change) = self.rx.try_recv() {
            index.apply(&change)?;
            n += 1;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_tree_and_syncs_changes() {
        let t = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(t.path().join("a/b")).expect("mkdir");
        std::fs::write(t.path().join("a/b/x.txt"), b"x").expect("write");
        std::fs::write(t.path().join("y.md"), b"y").expect("write");
        let mut w = WalkIndexer::new(vec![t.path().to_path_buf()], WalkOptions::default());
        let mut ix = w.build(&CancelToken::new()).expect("build");
        assert_eq!(ix.len(), 5);
        let root = t.path().to_string_lossy().into_owned();
        assert!(ix.resolve(&format!("{root}/a/b/x.txt")).is_some());
        w.changes()
            .send(IndexChange::Created {
                path: format!("{root}/a/new.rs"),
                is_dir: false,
            })
            .expect("send");
        w.changes()
            .send(IndexChange::Removed {
                path: format!("{root}/y.md"),
            })
            .expect("send");
        assert_eq!(w.sync(&mut ix).expect("sync"), 2);
        assert!(ix.resolve(&format!("{root}/a/new.rs")).is_some());
        assert!(ix.resolve(&format!("{root}/y.md")).is_none());
        let c = CancelToken::new();
        c.cancel();
        assert!(matches!(w.build(&c), Err(FsError::Cancelled)));
    }
}
