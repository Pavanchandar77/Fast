//! Prioritised, cancellable, cached preview generation.

use super::cache::{PreviewCache, Stamp};
use super::{generate, Preview, PreviewOptions, ThumbnailProvider};
use crate::error::{FsError, FsResult};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::{Condvar, Mutex};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Request priority (lower value runs first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Selected item in Quick Look.
    Focused = 0,
    /// Row/tile on screen.
    Visible = 1,
    /// Just off screen (prefetch).
    Near = 2,
    Background = 3,
}

/// Identifies one request.
pub type Ticket = u64;

/// Completion event.
#[derive(Debug)]
pub struct PreviewEvent {
    pub ticket: Ticket,
    pub path: PathBuf,
    pub result: Result<Arc<Preview>, FsError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Pending {
    priority: Priority,
    seq: u64,
    ticket: Ticket,
    path: PathBuf,
}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap: invert so best priority / oldest seq pops first.
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
struct Queue {
    heap: BinaryHeap<Pending>,
    /// Current state per path: (ticket, priority). Heap entries that do not
    /// match are stale and skipped.
    live: HashMap<PathBuf, (Ticket, Priority)>,
    /// In-flight generations: path -> ticket.
    running: HashMap<PathBuf, Ticket>,
    cancelled_running: HashSet<Ticket>,
    next_ticket: Ticket,
    seq: u64,
}

struct Inner {
    queue: Mutex<Queue>,
    cv: Condvar,
    cache: Mutex<PreviewCache>,
    opts: PreviewOptions,
    thumbs: Option<Arc<dyn ThumbnailProvider>>,
    tx: Sender<PreviewEvent>,
    shutdown: AtomicBool,
}

/// The preview service.
pub struct PreviewPipeline {
    inner: Arc<Inner>,
    events: Receiver<PreviewEvent>,
    workers: Vec<JoinHandle<()>>,
}

impl PreviewPipeline {
    pub fn new(
        threads: usize,
        cache_bytes: usize,
        opts: PreviewOptions,
        thumbs: Option<Arc<dyn ThumbnailProvider>>,
    ) -> FsResult<Self> {
        Self::build(threads.max(1), cache_bytes, opts, thumbs)
    }

    fn build(
        threads: usize,
        cache_bytes: usize,
        opts: PreviewOptions,
        thumbs: Option<Arc<dyn ThumbnailProvider>>,
    ) -> FsResult<Self> {
        let (tx, rx) = unbounded();
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue::default()),
            cv: Condvar::new(),
            cache: Mutex::new(PreviewCache::new(cache_bytes)),
            opts,
            thumbs,
            tx,
            shutdown: AtomicBool::new(false),
        });
        let mut workers = Vec::new();
        for i in 0..threads {
            let inner = Arc::clone(&inner);
            let h = std::thread::Builder::new()
                .name(format!("nx-preview-{i}"))
                .spawn(move || worker(&inner))
                .map_err(|e| FsError::from_io("<preview worker>", e))?;
            workers.push(h);
        }
        Ok(PreviewPipeline {
            inner,
            events: rx,
            workers,
        })
    }

    /// Completion events.
    pub fn events(&self) -> &Receiver<PreviewEvent> {
        &self.events
    }

    /// Cached preview without touching the disk (may be slightly stale; the
    /// worker re-validates on the next request).
    pub fn cached(&self, path: &Path) -> Option<Arc<Preview>> {
        self.inner.cache.lock().get(path)
    }

    /// Request a preview. Re-requesting a pending path upgrades its priority
    /// and returns the same ticket.
    pub fn request(&self, path: PathBuf, priority: Priority) -> Ticket {
        let mut q = self.inner.queue.lock();
        q.seq += 1;
        let seq = q.seq;
        if let Some(&(ticket, old)) = q.live.get(&path) {
            if priority < old {
                q.live.insert(path.clone(), (ticket, priority));
                q.heap.push(Pending {
                    priority,
                    seq,
                    ticket,
                    path,
                });
                self.inner.cv.notify_one();
            }
            return ticket;
        }
        if let Some(&ticket) = q.running.get(&path) {
            // Already being generated: the result is on its way.
            if !q.cancelled_running.contains(&ticket) {
                return ticket;
            }
        }
        q.next_ticket += 1;
        let ticket = q.next_ticket;
        q.live.insert(path.clone(), (ticket, priority));
        q.heap.push(Pending {
            priority,
            seq,
            ticket,
            path,
        });
        self.inner.cv.notify_one();
        ticket
    }

    /// The viewport changed: `visible` paths become Visible, everything else
    /// pending is demoted to Background (still useful, never in the way).
    pub fn set_visible(&self, visible: &[PathBuf]) {
        let vis: HashSet<&PathBuf> = visible.iter().collect();
        let mut q = self.inner.queue.lock();
        let mut entries: Vec<(PathBuf, Ticket, Priority)> = q
            .live
            .iter()
            .map(|(p, &(t, pr))| (p.clone(), t, pr))
            .collect();
        entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.1.cmp(&b.1)));
        q.heap.clear();
        for (path, ticket, old) in entries {
            let pr = if old == Priority::Focused {
                old
            } else if vis.contains(&path) {
                Priority::Visible
            } else {
                Priority::Background
            };
            q.seq += 1;
            let seq = q.seq;
            q.live.insert(path.clone(), (ticket, pr));
            q.heap.push(Pending {
                priority: pr,
                seq,
                ticket,
                path,
            });
        }
    }

    /// Cancel one request (pending or running). Returns true if it was known.
    pub fn cancel(&self, ticket: Ticket) -> bool {
        let mut q = self.inner.queue.lock();
        let before = q.live.len();
        q.live.retain(|_, (t, _)| *t != ticket);
        let was_running = q.running.values().any(|&t| t == ticket);
        if was_running {
            q.cancelled_running.insert(ticket);
        }
        before != q.live.len() || was_running
    }

    /// Cancel everything (folder changed).
    pub fn cancel_all(&self) {
        let mut q = self.inner.queue.lock();
        q.live.clear();
        q.heap.clear();
        let running: Vec<Ticket> = q.running.values().copied().collect();
        q.cancelled_running.extend(running);
    }

    pub fn pending(&self) -> usize {
        self.inner.queue.lock().live.len()
    }

    /// Drop a path from the cache (file changed or deleted).
    pub fn invalidate(&self, path: &Path) {
        self.inner.cache.lock().remove(path);
    }

    /// Privacy: forget every cached preview.
    pub fn clear_cache(&self) {
        self.inner.cache.lock().clear();
    }
}

fn worker(inner: &Inner) {
    loop {
        let (ticket, path) = {
            let mut q = inner.queue.lock();
            loop {
                if inner.shutdown.load(AtomicOrdering::Acquire) {
                    return;
                }
                match q.heap.pop() {
                    Some(p) => {
                        if q.live.get(&p.path) == Some(&(p.ticket, p.priority)) {
                            q.live.remove(&p.path);
                            q.running.insert(p.path.clone(), p.ticket);
                            break (p.ticket, p.path);
                        }
                    }
                    None => inner.cv.wait(&mut q),
                }
            }
        };
        let result = produce(inner, &path);
        let cancelled = {
            let mut q = inner.queue.lock();
            q.running.remove(&path);
            q.cancelled_running.remove(&ticket)
        };
        let result = if cancelled {
            Err(super::cancelled())
        } else {
            result
        };
        if inner
            .tx
            .send(PreviewEvent {
                ticket,
                path,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn produce(inner: &Inner, path: &Path) -> Result<Arc<Preview>, FsError> {
    let stamp = Stamp::of(path).ok_or_else(|| FsError::NotFound {
        path: path.to_path_buf(),
    })?;
    if let Some(hit) = inner.cache.lock().get_valid(path, stamp) {
        return Ok(hit);
    }
    let thumbs = inner.thumbs.as_deref();
    let preview = Arc::new(generate(path, &inner.opts, thumbs)?);
    inner
        .cache
        .lock()
        .insert(path.to_path_buf(), stamp, Arc::clone(&preview));
    Ok(preview)
}

impl Drop for PreviewPipeline {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, AtomicOrdering::Release);
        {
            let _q = self.inner.queue.lock();
            self.inner.cv.notify_all();
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn files(n: usize) -> (tempfile::TempDir, Vec<PathBuf>) {
        let t = tempfile::tempdir().expect("tempdir");
        let v = (0..n)
            .map(|i| {
                let p = t.path().join(format!("f{i}.txt"));
                std::fs::write(&p, format!("file {i}")).expect("write");
                p
            })
            .collect();
        (t, v)
    }

    #[test]
    fn priorities_order_and_cache() {
        let (_t, paths) = files(6);
        let pipe = PreviewPipeline::new(1, 1 << 20, PreviewOptions::default(), None).expect("pipe");
        let mut q = Queue::default();
        for (i, p) in paths.iter().enumerate() {
            let pr = if i == 4 {
                Priority::Focused
            } else if i % 2 == 0 {
                Priority::Visible
            } else {
                Priority::Background
            };
            q.seq += 1;
            q.heap.push(Pending {
                priority: pr,
                seq: q.seq,
                ticket: i as u64,
                path: p.clone(),
            });
        }
        let popped: Vec<u64> = std::iter::from_fn(|| q.heap.pop())
            .map(|p| p.ticket)
            .collect();
        assert_eq!(popped, vec![4, 0, 2, 1, 3, 5]);

        let t = pipe.request(paths[0].clone(), Priority::Visible);
        let ev = pipe
            .events()
            .recv_timeout(Duration::from_secs(5))
            .expect("event");
        assert_eq!(ev.ticket, t);
        assert!(matches!(ev.result.as_deref(), Ok(Preview::Text { .. })));
        assert!(pipe.cached(&paths[0]).is_some());
        pipe.clear_cache();
        assert!(pipe.cached(&paths[0]).is_none());
    }

    #[test]
    fn dedup_cancel_and_errors() {
        let (_t, paths) = files(3);
        {
            let idle =
                PreviewPipeline::build(0, 1 << 20, PreviewOptions::default(), None).expect("pipe");
            let a = idle.request(paths[1].clone(), Priority::Background);
            let b = idle.request(paths[1].clone(), Priority::Focused);
            assert_eq!(a, b, "same path, same ticket");
            assert_eq!(
                idle.inner.queue.lock().live.get(&paths[1]).map(|x| x.1),
                Some(Priority::Focused)
            );
        }
        let pipe = PreviewPipeline::new(2, 1 << 20, PreviewOptions::default(), None).expect("pipe");
        let a = pipe.request(paths[1].clone(), Priority::Focused);
        let missing = pipe.request(PathBuf::from("/definitely/missing"), Priority::Visible);
        let mut seen = HashMap::new();
        for _ in 0..2 {
            let ev = pipe
                .events()
                .recv_timeout(Duration::from_secs(5))
                .expect("event");
            seen.insert(ev.ticket, ev.result.is_ok());
        }
        assert_eq!(seen.get(&a), Some(&true));
        assert_eq!(seen.get(&missing), Some(&false));
        let c = pipe.request(paths[2].clone(), Priority::Background);
        pipe.cancel(c);
        pipe.cancel_all();
        pipe.set_visible(&[paths[0].clone()]);
        assert_eq!(pipe.pending(), 0);
        // A cancelled request either never reports or reports Cancelled.
        if let Ok(ev) = pipe.events().recv_timeout(Duration::from_millis(200)) {
            assert_eq!(ev.ticket, c);
        }
    }

    #[test]
    fn set_visible_reprioritises() {
        let (_t, paths) = files(3);
        // No workers: nothing can run while we reshape the queue.
        let pipe =
            PreviewPipeline::build(0, 1 << 20, PreviewOptions::default(), None).expect("pipe");
        {
            let mut q = pipe.inner.queue.lock();
            for (i, p) in paths.iter().enumerate() {
                q.live
                    .insert(p.clone(), (100 + i as u64, Priority::Background));
            }
        }
        pipe.set_visible(&[paths[2].clone()]);
        let q = pipe.inner.queue.lock();
        assert_eq!(q.live.get(&paths[2]).map(|x| x.1), Some(Priority::Visible));
        assert_eq!(
            q.live.get(&paths[0]).map(|x| x.1),
            Some(Priority::Background)
        );
        assert_eq!(q.heap.peek().map(|p| p.ticket), Some(102));
    }
}
