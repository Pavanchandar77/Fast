//! Parallel, loop-safe directory traversal (jwalk-style).
//!
//! Workers share a LIFO stack of directories (depth-first keeps memory low).
//! Links are not followed unless asked; when they are, every directory's
//! identity (device + inode, or canonical path on Windows) is recorded so
//! symlink and junction cycles terminate.

use crate::entry::EntryKind;
use crate::error::FsError;
use crate::path::to_native;
use crate::pool::CancelToken;
use parking_lot::{Condvar, Mutex};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Traversal options.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub threads: usize,
    /// Follow symlinks/junctions into directories (loop-protected).
    pub follow_links: bool,
    /// Hard recursion limit (defends against pathological trees).
    pub max_depth: usize,
    /// Call `lstat` for every entry to get sizes/mtimes.
    pub with_metadata: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(16),
            follow_links: false,
            max_depth: 512,
            with_metadata: false,
        }
    }
}

/// One child of a directory as seen by the walker.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    pub name: OsString,
    pub kind: EntryKind,
    /// Size in bytes when `with_metadata` (0 otherwise or for dirs).
    pub size: u64,
    /// The entry is a link that resolves to a directory and may be descended.
    pub link_to_dir: bool,
}

/// Summary of a walk.
#[derive(Debug, Default)]
pub struct WalkStats {
    pub dirs: u64,
    pub entries: u64,
    /// Errors (permission denied etc.), capped to the first 64.
    pub errors: Vec<FsError>,
    pub error_count: u64,
    pub loops_skipped: u64,
    pub cancelled: bool,
}

struct Job<T> {
    path: PathBuf,
    depth: usize,
    tag: T,
}

struct State<T> {
    stack: Vec<Job<T>>,
    active: usize,
    stats: WalkStats,
}

/// Walk `roots` in parallel. For every directory read, `visit(dir, tag,
/// entries)` is called and returns the children to descend as
/// `(index into entries, child tag)`.
pub fn parallel_walk<T, F>(
    roots: Vec<(PathBuf, T)>,
    opts: &WalkOptions,
    cancel: &CancelToken,
    visit: F,
) -> WalkStats
where
    T: Send,
    F: Fn(&Path, &T, &[WalkEntry]) -> Vec<(usize, T)> + Sync,
{
    let visited: Mutex<HashSet<DirIdentity>> = Mutex::new(HashSet::new());
    if opts.follow_links {
        let mut v = visited.lock();
        for (p, _) in &roots {
            if let Some(id) = dir_identity(p) {
                v.insert(id);
            }
        }
    }
    let state = Mutex::new(State {
        stack: roots
            .into_iter()
            .map(|(path, tag)| Job {
                path,
                depth: 0,
                tag,
            })
            .collect(),
        active: 0,
        stats: WalkStats::default(),
    });
    let cv = Condvar::new();
    std::thread::scope(|s| {
        for _ in 0..opts.threads.max(1) {
            s.spawn(|| worker(&state, &cv, opts, cancel, &visit, &visited));
        }
    });
    let mut stats = state.into_inner().stats;
    stats.cancelled = cancel.is_cancelled();
    stats
}

fn worker<T, F>(
    state: &Mutex<State<T>>,
    cv: &Condvar,
    opts: &WalkOptions,
    cancel: &CancelToken,
    visit: &F,
    visited: &Mutex<HashSet<DirIdentity>>,
) where
    F: Fn(&Path, &T, &[WalkEntry]) -> Vec<(usize, T)>,
{
    loop {
        let job = {
            let mut st = state.lock();
            loop {
                if cancel.is_cancelled() {
                    st.stack.clear();
                }
                if let Some(job) = st.stack.pop() {
                    st.active += 1;
                    break job;
                }
                if st.active == 0 {
                    cv.notify_all();
                    return;
                }
                cv.wait(&mut st);
            }
        };
        let mut local = WalkStats::default();
        let children = process(&job, opts, visit, visited, &mut local);
        let mut st = state.lock();
        st.stack.extend(children);
        st.active -= 1;
        merge(&mut st.stats, local);
        cv.notify_all();
    }
}

fn process<T, F>(
    job: &Job<T>,
    opts: &WalkOptions,
    visit: &F,
    visited: &Mutex<HashSet<DirIdentity>>,
    stats: &mut WalkStats,
) -> Vec<Job<T>>
where
    F: Fn(&Path, &T, &[WalkEntry]) -> Vec<(usize, T)>,
{
    let entries = match read_entries(&job.path, opts) {
        Ok(e) => e,
        Err(e) => {
            stats.error_count += 1;
            stats.errors.push(e);
            return Vec::new();
        }
    };
    stats.dirs += 1;
    stats.entries += entries.len() as u64;
    let picks = visit(&job.path, &job.tag, &entries);
    let mut out = Vec::with_capacity(picks.len());
    if job.depth + 1 > opts.max_depth {
        return out;
    }
    for (idx, tag) in picks {
        let Some(e) = entries.get(idx) else { continue };
        let descend = e.kind.is_dir() || (opts.follow_links && e.link_to_dir);
        if !descend {
            continue;
        }
        let path = job.path.join(&e.name);
        if opts.follow_links {
            if let Some(id) = dir_identity(&path) {
                if !visited.lock().insert(id) {
                    stats.loops_skipped += 1;
                    continue;
                }
            }
        }
        out.push(Job {
            path,
            depth: job.depth + 1,
            tag,
        });
    }
    out
}

fn merge(into: &mut WalkStats, from: WalkStats) {
    into.dirs += from.dirs;
    into.entries += from.entries;
    into.error_count += from.error_count;
    into.loops_skipped += from.loops_skipped;
    for e in from.errors {
        if into.errors.len() < 64 {
            into.errors.push(e);
        }
    }
}

/// Read one directory without following links.
pub fn read_entries(dir: &Path, opts: &WalkOptions) -> Result<Vec<WalkEntry>, FsError> {
    let rd = std::fs::read_dir(to_native(dir)).map_err(|e| FsError::from_io(dir, e))?;
    let mut out = Vec::new();
    for item in rd {
        let Ok(item) = item else { continue };
        let Ok(ft) = item.file_type() else { continue };
        let kind = if ft.is_symlink() {
            EntryKind::Symlink
        } else if ft.is_dir() {
            EntryKind::Dir
        } else if ft.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let size = if opts.with_metadata && kind == EntryKind::File {
            item.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        let link_to_dir = kind == EntryKind::Symlink
            && opts.follow_links
            && std::fs::metadata(item.path())
                .map(|m| m.is_dir())
                .unwrap_or(false);
        out.push(WalkEntry {
            name: item.file_name(),
            kind,
            size,
            link_to_dir,
        });
    }
    Ok(out)
}

/// Identity of a directory for loop detection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DirIdentity {
    #[cfg(unix)]
    Inode(u64, u64),
    #[cfg_attr(unix, allow(dead_code))]
    Canonical(PathBuf),
}

fn dir_identity(path: &Path) -> Option<DirIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path)
            .ok()
            .map(|m| DirIdentity::Inode(m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        std::fs::canonicalize(to_native(path))
            .ok()
            .map(DirIdentity::Canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().expect("tempdir");
        for d in ["a/b/c", "a/d", "e"] {
            std::fs::create_dir_all(t.path().join(d)).expect("mkdir");
        }
        for f in ["a/1.txt", "a/b/2.txt", "a/b/c/3.txt", "e/4.txt", "5.txt"] {
            std::fs::write(t.path().join(f), b"hello").expect("write");
        }
        t
    }

    #[test]
    fn walks_everything_once() {
        let t = tree();
        let files = AtomicU64::new(0);
        let stats = parallel_walk(
            vec![(t.path().to_path_buf(), ())],
            &WalkOptions::default(),
            &CancelToken::new(),
            |_, _, es| {
                files.fetch_add(
                    es.iter().filter(|e| e.kind == EntryKind::File).count() as u64,
                    Ordering::Relaxed,
                );
                es.iter()
                    .enumerate()
                    .filter(|(_, e)| e.kind.is_dir())
                    .map(|(i, _)| (i, ()))
                    .collect()
            },
        );
        assert_eq!(files.load(Ordering::Relaxed), 5);
        assert_eq!(stats.dirs, 6);
        assert_eq!(stats.error_count, 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loops_terminate() {
        let t = tree();
        std::os::unix::fs::symlink(t.path(), t.path().join("a/b/loop")).expect("symlink");
        std::os::unix::fs::symlink(t.path().join("a"), t.path().join("e/back")).expect("symlink");
        let opts = WalkOptions {
            follow_links: true,
            ..WalkOptions::default()
        };
        let stats = parallel_walk(
            vec![(t.path().to_path_buf(), ())],
            &opts,
            &CancelToken::new(),
            |_, _, es| es.iter().enumerate().map(|(i, _)| (i, ())).collect(),
        );
        assert_eq!(stats.dirs, 6, "each real dir visited exactly once");
        assert!(stats.loops_skipped >= 2);
        // Without following, links are leaves.
        let stats = parallel_walk(
            vec![(t.path().to_path_buf(), ())],
            &WalkOptions::default(),
            &CancelToken::new(),
            |_, _, es| es.iter().enumerate().map(|(i, _)| (i, ())).collect(),
        );
        assert_eq!(stats.dirs, 6);
        assert_eq!(stats.loops_skipped, 0);
    }

    #[test]
    fn missing_root_is_a_typed_error_and_cancel_stops() {
        let stats = parallel_walk(
            vec![(PathBuf::from("/definitely/not/here"), ())],
            &WalkOptions::default(),
            &CancelToken::new(),
            |_, _, _| Vec::new(),
        );
        assert!(matches!(
            stats.errors.first(),
            Some(FsError::NotFound { .. })
        ));
        let t = tree();
        let c = CancelToken::new();
        c.cancel();
        let stats = parallel_walk(
            vec![(t.path().to_path_buf(), ())],
            &WalkOptions::default(),
            &c,
            |_, _, _| Vec::new(),
        );
        assert!(stats.cancelled);
        assert_eq!(stats.dirs, 0);
    }
}
