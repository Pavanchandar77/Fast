//! A small two-lane thread pool and cancellation tokens.
//!
//! Interactive work (the listing of the folder the user just opened, visible
//! thumbnails) always runs before background work (indexing, folder sizes).
//! A panicking job never takes a worker down.

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Cooperative cancellation flag shared between a job and its owner.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Scheduling lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// User is waiting for this (first rows, visible previews).
    Interactive,
    /// Nice-to-have (index builds, folder sizes, prefetch).
    Background,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Fixed-size pool with an interactive and a background queue.
pub struct ThreadPool {
    hi: Option<Sender<Job>>,
    lo: Option<Sender<Job>>,
    workers: Vec<JoinHandle<()>>,
    panics: Arc<AtomicUsize>,
}

impl ThreadPool {
    /// Spawn `threads` workers (at least one).
    pub fn new(threads: usize, name: &str) -> std::io::Result<Self> {
        let (hi_tx, hi_rx) = unbounded::<Job>();
        let (lo_tx, lo_rx) = unbounded::<Job>();
        let panics = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for i in 0..threads.max(1) {
            let (hi, lo, panics) = (hi_rx.clone(), lo_rx.clone(), Arc::clone(&panics));
            let handle = std::thread::Builder::new()
                .name(format!("{name}-{i}"))
                .spawn(move || worker_loop(&hi, &lo, &panics))?;
            workers.push(handle);
        }
        Ok(ThreadPool {
            hi: Some(hi_tx),
            lo: Some(lo_tx),
            workers,
            panics,
        })
    }

    /// Pool sized to the machine (`available_parallelism`, capped at 16).
    pub fn with_default_size(name: &str) -> std::io::Result<Self> {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(16);
        Self::new(n, name)
    }

    /// Queue a job. Returns `false` if the pool is shutting down.
    pub fn spawn(&self, lane: Lane, job: impl FnOnce() + Send + 'static) -> bool {
        let tx = match lane {
            Lane::Interactive => &self.hi,
            Lane::Background => &self.lo,
        };
        match tx {
            Some(tx) => tx.send(Box::new(job)).is_ok(),
            None => false,
        }
    }

    /// Number of jobs that panicked (they are isolated, but we count them).
    pub fn panic_count(&self) -> usize {
        self.panics.load(Ordering::Relaxed)
    }

    pub fn threads(&self) -> usize {
        self.workers.len()
    }
}

fn worker_loop(hi: &Receiver<Job>, lo: &Receiver<Job>, panics: &AtomicUsize) {
    loop {
        let job = match hi.try_recv() {
            Ok(job) => job,
            Err(_) => {
                crossbeam_channel::select! {
                    recv(hi) -> j => match j { Ok(j) => j, Err(_) => match lo.recv() { Ok(j) => j, Err(_) => return } },
                    recv(lo) -> j => match j { Ok(j) => j, Err(_) => match hi.recv() { Ok(j) => j, Err(_) => return } },
                }
            }
        };
        if catch_unwind(AssertUnwindSafe(job)).is_err() {
            panics.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.hi.take();
        self.lo.take();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn runs_jobs_and_survives_panics() {
        let pool = ThreadPool::new(2, "t").expect("pool");
        let (tx, rx) = mpsc::channel();
        assert!(pool.spawn(Lane::Background, || panic!("boom")));
        for i in 0..10 {
            let tx = tx.clone();
            pool.spawn(
                if i % 2 == 0 {
                    Lane::Interactive
                } else {
                    Lane::Background
                },
                move || {
                    let _ = tx.send(i);
                },
            );
        }
        let mut got: Vec<i32> = (0..10).map(|_| rx.recv().expect("job result")).collect();
        got.sort();
        assert_eq!(got, (0..10).collect::<Vec<_>>());
        drop(pool);
    }

    #[test]
    fn cancel_token_is_shared() {
        let t = CancelToken::new();
        let c = t.clone();
        assert!(!c.is_cancelled());
        t.cancel();
        assert!(c.is_cancelled());
    }
}
