//! Streaming directory listing.
//!
//! A listing is produced in batches: a small first batch so the first rows
//! paint within a frame (< 16 ms) even for 100k-entry folders, then large
//! batches for throughput. Listings run on the interactive lane of the pool
//! and are cancellable (navigating away cancels the old listing).

pub(crate) mod portable;
#[cfg(windows)]
pub(crate) use crate::platform::windows::listing as native;
#[cfg(not(windows))]
pub(crate) use portable as native;

use crate::entry::DirEntry;
use crate::error::{FsError, FsResult};
use crate::pool::{CancelToken, Lane, ThreadPool};
use crossbeam_channel::{unbounded, Receiver};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Listing knobs.
#[derive(Debug, Clone, Copy)]
pub struct ListOptions {
    /// Size of the first batch (paint the first screen ASAP).
    pub first_batch: usize,
    /// Size of every following batch.
    pub batch: usize,
    /// Fetch size/mtime (free on Windows; one `lstat` per entry elsewhere).
    pub with_metadata: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions {
            first_batch: 128,
            batch: 4096,
            with_metadata: true,
        }
    }
}

/// Events emitted by an asynchronous listing.
#[derive(Debug)]
pub enum ListEvent {
    Batch {
        request: u64,
        entries: Vec<DirEntry>,
    },
    Done {
        request: u64,
        total: usize,
        elapsed: Duration,
    },
    Failed {
        request: u64,
        error: FsError,
    },
}

impl ListEvent {
    pub fn request(&self) -> u64 {
        match self {
            ListEvent::Batch { request, .. }
            | ListEvent::Done { request, .. }
            | ListEvent::Failed { request, .. } => *request,
        }
    }
}

/// Batches entries and hands them to a sink; shared by all backends.
pub(crate) struct Batcher<'a> {
    buf: Vec<DirEntry>,
    next_size: usize,
    batch: usize,
    total: usize,
    sink: &'a mut dyn FnMut(Vec<DirEntry>) -> ControlFlow<()>,
}

impl<'a> Batcher<'a> {
    pub(crate) fn new(
        opts: &ListOptions,
        sink: &'a mut dyn FnMut(Vec<DirEntry>) -> ControlFlow<()>,
    ) -> Self {
        let first = opts.first_batch.max(1);
        Batcher {
            buf: Vec::with_capacity(first),
            next_size: first,
            batch: opts.batch.max(1),
            total: 0,
            sink,
        }
    }

    /// Add an entry; returns `Break` if the consumer stopped listening.
    pub(crate) fn push(&mut self, e: DirEntry) -> ControlFlow<()> {
        self.buf.push(e);
        if self.buf.len() >= self.next_size {
            return self.flush();
        }
        ControlFlow::Continue(())
    }

    pub(crate) fn flush(&mut self) -> ControlFlow<()> {
        if self.buf.is_empty() {
            return ControlFlow::Continue(());
        }
        self.next_size = self.batch;
        let full = std::mem::replace(&mut self.buf, Vec::with_capacity(self.batch));
        self.total += full.len();
        (self.sink)(full)
    }

    pub(crate) fn total(&self) -> usize {
        self.total + self.buf.len()
    }
}

/// List `dir` synchronously, delivering batches to `sink`.
/// Returns the number of entries delivered.
pub fn list_dir_with(
    dir: &Path,
    opts: &ListOptions,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Vec<DirEntry>) -> ControlFlow<()>,
) -> FsResult<usize> {
    let mut batcher = Batcher::new(opts, sink);
    native::list(dir, opts, cancel, &mut batcher)?;
    let _ = batcher.flush();
    if cancel.is_cancelled() {
        return Err(FsError::Cancelled);
    }
    Ok(batcher.total())
}

/// Convenience: list a whole directory into a `Vec`.
pub fn list_dir(dir: &Path) -> FsResult<Vec<DirEntry>> {
    let mut out = Vec::new();
    list_dir_with(
        dir,
        &ListOptions::default(),
        &CancelToken::new(),
        &mut |b| {
            out.extend(b);
            ControlFlow::Continue(())
        },
    )?;
    Ok(out)
}

/// Handle to an in-flight asynchronous listing.
pub struct ListingHandle {
    pub request: u64,
    pub events: Receiver<ListEvent>,
    cancel: CancelToken,
}

impl ListingHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ListingHandle {
    fn drop(&mut self) {
        // Nobody listens any more: stop the worker early.
        self.cancel.cancel();
    }
}

/// Start listing `dir` on the pool's interactive lane.
pub fn spawn_listing(
    pool: &ThreadPool,
    dir: PathBuf,
    opts: ListOptions,
    request: u64,
) -> FsResult<ListingHandle> {
    let (tx, rx) = unbounded();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let ok = pool.spawn(Lane::Interactive, move || {
        let start = Instant::now();
        let mut send = |entries: Vec<DirEntry>| {
            if token.is_cancelled() || tx.send(ListEvent::Batch { request, entries }).is_err() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let res = list_dir_with(&dir, &opts, &token, &mut send);
        let ev = match res {
            Ok(total) => ListEvent::Done {
                request,
                total,
                elapsed: start.elapsed(),
            },
            Err(error) => ListEvent::Failed { request, error },
        };
        let _ = tx.send(ev);
    });
    if !ok {
        return Err(FsError::Disconnected);
    }
    Ok(ListingHandle {
        request,
        events: rx,
        cancel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_in_growing_batches() {
        let t = tempfile::tempdir().expect("tempdir");
        for i in 0..300 {
            std::fs::write(t.path().join(format!("f{i}.txt")), b"x").expect("write");
        }
        std::fs::create_dir(t.path().join("sub")).expect("mkdir");
        let opts = ListOptions {
            first_batch: 10,
            batch: 100,
            with_metadata: true,
        };
        let mut sizes = Vec::new();
        let total = list_dir_with(t.path(), &opts, &CancelToken::new(), &mut |b| {
            sizes.push(b.len());
            ControlFlow::Continue(())
        })
        .expect("list");
        assert_eq!(total, 301);
        assert_eq!(sizes[0], 10);
        assert_eq!(sizes[1], 100);
        assert_eq!(sizes.iter().sum::<usize>(), 301);
        let all = list_dir(t.path()).expect("list");
        let sub = all.iter().find(|e| e.display_name == "sub").expect("sub");
        assert!(sub.kind.is_dir());
        let f = all
            .iter()
            .find(|e| e.display_name == "f1.txt")
            .expect("file");
        assert_eq!(f.size, 1);
        assert!(f.modified.is_some());
    }

    #[test]
    fn missing_dir_is_not_found_and_file_is_typed() {
        assert!(matches!(
            list_dir(Path::new("/nope/nope")),
            Err(FsError::NotFound { .. })
        ));
        let t = tempfile::tempdir().expect("tempdir");
        let f = t.path().join("file");
        std::fs::write(&f, b"x").expect("write");
        assert!(list_dir(&f).is_err());
    }

    #[test]
    fn async_listing_streams_and_cancels() {
        let t = tempfile::tempdir().expect("tempdir");
        for i in 0..50 {
            std::fs::write(t.path().join(format!("{i}")), b"").expect("write");
        }
        let pool = ThreadPool::new(2, "ls").expect("pool");
        let h = spawn_listing(
            &pool,
            t.path().to_path_buf(),
            ListOptions {
                first_batch: 5,
                batch: 10,
                with_metadata: false,
            },
            7,
        )
        .expect("spawn");
        let mut n = 0;
        loop {
            match h.events.recv().expect("event") {
                ListEvent::Batch { request, entries } => {
                    assert_eq!(request, 7);
                    n += entries.len();
                }
                ListEvent::Done { total, .. } => {
                    assert_eq!(total, 50);
                    break;
                }
                ListEvent::Failed { error, .. } => panic!("{error}"),
            }
        }
        assert_eq!(n, 50);
        let h2 =
            spawn_listing(&pool, t.path().to_path_buf(), ListOptions::default(), 8).expect("spawn");
        h2.cancel();
        let last = h2.events.iter().last().expect("terminal event");
        assert!(matches!(
            last,
            ListEvent::Done { .. }
                | ListEvent::Failed {
                    error: FsError::Cancelled,
                    ..
                }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_reported_not_followed() {
        let t = tempfile::tempdir().expect("tempdir");
        std::os::unix::fs::symlink(t.path(), t.path().join("self")).expect("symlink");
        let all = list_dir(t.path()).expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, crate::entry::EntryKind::Symlink);
    }
}
