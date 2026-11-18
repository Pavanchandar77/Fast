//! Background folder-size calculation with a cache.
//!
//! Sizes are computed with the parallel walker (links never followed, so
//! junction loops and hard-linked trees cannot inflate or hang it), on the
//! background lane, and reported with intermediate progress.

use crate::pool::{CancelToken, Lane, ThreadPool};
use crate::walk::{parallel_walk, WalkOptions};
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Result of a size scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderSize {
    pub bytes: u64,
    pub files: u64,
    pub dirs: u64,
    /// Sub-folders that could not be read (permission denied...).
    pub errors: u64,
    /// The scan is still running (intermediate report) or was cancelled.
    pub partial: bool,
}

/// Size update event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeEvent {
    pub path: PathBuf,
    pub size: FolderSize,
}

/// Compute a folder size synchronously, calling `progress` periodically.
pub fn folder_size(
    path: &Path,
    threads: usize,
    cancel: &CancelToken,
    mut progress: impl FnMut(FolderSize) + Send,
) -> FolderSize {
    let bytes = AtomicU64::new(0);
    let files = AtomicU64::new(0);
    let last = Mutex::new(Instant::now());
    let report = Mutex::new(&mut progress);
    let opts = WalkOptions {
        threads,
        follow_links: false,
        with_metadata: true,
        ..WalkOptions::default()
    };
    let stats = parallel_walk(
        vec![(path.to_path_buf(), ())],
        &opts,
        cancel,
        |_, _, entries| {
            let mut b = 0;
            let mut f = 0;
            let mut sub = Vec::new();
            for (i, e) in entries.iter().enumerate() {
                if e.kind.is_dir() {
                    sub.push((i, ()));
                } else {
                    b += e.size;
                    f += 1;
                }
            }
            let total_b = bytes.fetch_add(b, Ordering::Relaxed) + b;
            let total_f = files.fetch_add(f, Ordering::Relaxed) + f;
            let mut l = last.lock();
            if l.elapsed() >= Duration::from_millis(100) {
                *l = Instant::now();
                (report.lock())(FolderSize {
                    bytes: total_b,
                    files: total_f,
                    dirs: 0,
                    errors: 0,
                    partial: true,
                });
            }
            sub
        },
    );
    FolderSize {
        bytes: bytes.load(Ordering::Relaxed),
        files: files.load(Ordering::Relaxed),
        dirs: stats.dirs.saturating_sub(1),
        errors: stats.error_count,
        partial: stats.cancelled,
    }
}

/// Cached, asynchronous folder sizes.
pub struct SizeService {
    cache: Arc<Mutex<HashMap<PathBuf, FolderSize>>>,
    running: Arc<Mutex<HashMap<PathBuf, CancelToken>>>,
    threads: usize,
}

impl SizeService {
    pub fn new(threads: usize) -> Self {
        SizeService {
            cache: Arc::default(),
            running: Arc::default(),
            threads: threads.max(1),
        }
    }

    pub fn cached(&self, path: &Path) -> Option<FolderSize> {
        self.cache.lock().get(path).copied()
    }

    /// Start (or reuse) a background scan; events go to `tx`.
    pub fn request(&self, pool: &ThreadPool, path: PathBuf, tx: Sender<SizeEvent>) -> bool {
        if let Some(size) = self.cached(&path) {
            let _ = tx.send(SizeEvent { path, size });
            return true;
        }
        let token = {
            let mut running = self.running.lock();
            if running.contains_key(&path) {
                return true;
            }
            let t = CancelToken::new();
            running.insert(path.clone(), t.clone());
            t
        };
        let (cache, running, threads) = (
            Arc::clone(&self.cache),
            Arc::clone(&self.running),
            self.threads,
        );
        pool.spawn(Lane::Background, move || {
            let p2 = path.clone();
            let tx2 = tx.clone();
            let size = folder_size(&path, threads, &token, move |s| {
                let _ = tx2.send(SizeEvent {
                    path: p2.clone(),
                    size: s,
                });
            });
            running.lock().remove(&path);
            if !size.partial {
                cache.lock().insert(path.clone(), size);
            }
            let _ = tx.send(SizeEvent { path, size });
        })
    }

    pub fn cancel(&self, path: &Path) {
        if let Some(t) = self.running.lock().remove(path) {
            t.cancel();
        }
    }

    /// A file changed below `path`: drop it and all cached ancestors.
    pub fn invalidate(&self, path: &Path) {
        self.cache.lock().retain(|k, _| !path.starts_with(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_tree_and_caches() {
        let t = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(t.path().join("a/b")).expect("mkdir");
        std::fs::write(t.path().join("a/x"), vec![0u8; 100]).expect("write");
        std::fs::write(t.path().join("a/b/y"), vec![0u8; 23]).expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(t.path(), t.path().join("a/b/loop")).expect("symlink");
        let s = folder_size(t.path(), 2, &CancelToken::new(), |_| {});
        assert_eq!(s.bytes, 123, "link is a leaf, never followed");
        assert_eq!(s.dirs, 2);
        assert!(!s.partial);

        let pool = ThreadPool::new(2, "sz").expect("pool");
        let svc = SizeService::new(2);
        let (tx, rx) = crossbeam_channel::unbounded();
        svc.request(&pool, t.path().join("a"), tx.clone());
        let ev = rx.iter().find(|e| !e.size.partial).expect("final");
        assert!(ev.size.bytes >= 123);
        assert!(svc.cached(&t.path().join("a")).is_some());
        svc.invalidate(&t.path().join("a/b/y"));
        assert!(svc.cached(&t.path().join("a")).is_none());
    }
}
