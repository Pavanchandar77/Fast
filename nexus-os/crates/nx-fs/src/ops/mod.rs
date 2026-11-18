//! File operations engine: copy / move / delete / undo as background jobs.
//!
//! * Jobs run on dedicated worker threads; the UI only sends messages and
//!   receives [`JobEvent`]s, so it never blocks.
//! * Copies are written to a hidden temp file and atomically renamed into
//!   place; replaced files are moved aside first. Every intent is
//!   journalled and fsynced ([`journal`]), so a crash never leaves a
//!   half-written file under the real name and never loses the old version.
//! * Pause / resume / cancel at chunk granularity, conflict prompts with
//!   "apply to all", per-item errors (permission denied) without aborting
//!   the whole job, and undo of the last completed operation.

pub mod exec;
pub mod journal;
pub mod trash;
pub mod undo;

use crate::error::{FsError, FsResult};
use crate::index::IndexChange;
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub use journal::RecoveryReport;
pub use trash::{DirTrash, Trash, TrashedItem};
pub use undo::{UndoRecord, UndoStep};

/// Job identifier.
pub type JobId = u64;

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    Copy {
        dest: PathBuf,
    },
    Move {
        dest: PathBuf,
    },
    Delete {
        to_trash: bool,
    },
    /// Rename one item in place.
    Rename {
        new_name: String,
    },
    /// Create a folder.
    NewFolder {
        parent: PathBuf,
        name: String,
    },
    /// Revert the last completed operation.
    Undo,
}

impl OpKind {
    pub fn label(&self) -> &'static str {
        match self {
            OpKind::Copy { .. } => "copy",
            OpKind::Move { .. } => "move",
            OpKind::Delete { to_trash: true } => "trash",
            OpKind::Delete { to_trash: false } => "delete",
            OpKind::Rename { .. } => "rename",
            OpKind::NewFolder { .. } => "new folder",
            OpKind::Undo => "undo",
        }
    }
}

/// How to handle a destination that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Ask the UI (emits [`JobEvent::Conflict`]).
    #[default]
    Ask,
    Skip,
    Replace,
    /// Rename the incoming item (`name (2).ext`).
    KeepBoth,
    /// Replace only when the source is newer.
    ReplaceIfNewer,
}

/// The UI's answer to a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Skip,
    Replace,
    KeepBoth,
    Cancel,
}

/// A request to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpRequest {
    pub kind: OpKind,
    pub sources: Vec<PathBuf>,
    pub conflict: ConflictPolicy,
}

impl OpRequest {
    pub fn copy(sources: Vec<PathBuf>, dest: impl Into<PathBuf>) -> Self {
        OpRequest {
            kind: OpKind::Copy { dest: dest.into() },
            sources,
            conflict: ConflictPolicy::Ask,
        }
    }
    pub fn move_to(sources: Vec<PathBuf>, dest: impl Into<PathBuf>) -> Self {
        OpRequest {
            kind: OpKind::Move { dest: dest.into() },
            sources,
            conflict: ConflictPolicy::Ask,
        }
    }
    pub fn delete(sources: Vec<PathBuf>, to_trash: bool) -> Self {
        OpRequest {
            kind: OpKind::Delete { to_trash },
            sources,
            conflict: ConflictPolicy::Ask,
        }
    }
    pub fn with_conflict(mut self, policy: ConflictPolicy) -> Self {
        self.conflict = policy;
        self
    }
}

/// Progress snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub job: JobId,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub items_done: u64,
    pub items_total: u64,
    pub current: PathBuf,
    pub bytes_per_sec: f64,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.bytes_total > 0 {
            self.bytes_done as f64 / self.bytes_total as f64
        } else if self.items_total > 0 {
            self.items_done as f64 / self.items_total as f64
        } else {
            1.0
        }
    }
}

/// Final job summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub items_done: u64,
    pub bytes_done: u64,
    pub skipped: u64,
    pub failed: u64,
    pub undoable: bool,
}

/// Events from the engine.
#[derive(Debug)]
pub enum JobEvent {
    Queued {
        job: JobId,
        label: &'static str,
    },
    Started {
        job: JobId,
        items_total: u64,
        bytes_total: u64,
    },
    Progress(Progress),
    /// The job is waiting for [`OpEngine::resolve`].
    Conflict {
        job: JobId,
        src: PathBuf,
        dest: PathBuf,
        src_newer: bool,
        dest_is_dir: bool,
    },
    ItemFailed {
        job: JobId,
        path: PathBuf,
        error: FsError,
    },
    Paused {
        job: JobId,
    },
    Resumed {
        job: JobId,
    },
    /// Paths created/removed/renamed (keeps search indexes fresh).
    IndexChanges {
        job: JobId,
        changes: Vec<IndexChange>,
    },
    Finished {
        job: JobId,
        summary: Summary,
    },
    Cancelled {
        job: JobId,
        summary: Summary,
    },
    Failed {
        job: JobId,
        error: FsError,
    },
}

impl JobEvent {
    pub fn job(&self) -> JobId {
        match self {
            JobEvent::Queued { job, .. }
            | JobEvent::Started { job, .. }
            | JobEvent::Conflict { job, .. }
            | JobEvent::ItemFailed { job, .. }
            | JobEvent::Paused { job }
            | JobEvent::Resumed { job }
            | JobEvent::IndexChanges { job, .. }
            | JobEvent::Finished { job, .. }
            | JobEvent::Cancelled { job, .. }
            | JobEvent::Failed { job, .. } => *job,
            JobEvent::Progress(p) => p.job,
        }
    }

    /// Terminal events end a job.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobEvent::Finished { .. } | JobEvent::Cancelled { .. } | JobEvent::Failed { .. }
        )
    }
}

/// A conflict answer: (resolution, apply to all remaining conflicts).
type Answer = (Resolution, bool);

/// Per-job control block (pause / cancel / conflict answers).
pub(crate) struct Control {
    state: Mutex<(bool, bool)>, // (paused, cancelled)
    cv: Condvar,
    answers: (Sender<Answer>, Receiver<Answer>),
}

impl Control {
    fn new() -> Self {
        Control {
            state: Mutex::new((false, false)),
            cv: Condvar::new(),
            answers: unbounded(),
        }
    }

    /// Block while paused; error if cancelled.
    pub(crate) fn checkpoint(&self) -> FsResult<()> {
        let mut s = self.state.lock();
        while s.0 && !s.1 {
            self.cv.wait(&mut s);
        }
        if s.1 {
            Err(FsError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.state.lock().1
    }

    /// Wait for a conflict answer, staying responsive to cancel.
    pub(crate) fn wait_answer(&self) -> FsResult<Answer> {
        loop {
            if self.is_cancelled() {
                return Err(FsError::Cancelled);
            }
            match self.answers.1.recv_timeout(Duration::from_millis(50)) {
                Ok(a) => return Ok(a),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(FsError::Disconnected)
                }
            }
        }
    }

    fn set_paused(&self, p: bool) {
        self.state.lock().0 = p;
        self.cv.notify_all();
    }

    fn cancel(&self) {
        self.state.lock().1 = true;
        self.cv.notify_all();
    }
}

/// Engine configuration.
pub struct EngineConfig {
    pub journal_path: PathBuf,
    pub trash: Arc<dyn Trash>,
    /// Jobs that may run at the same time.
    pub workers: usize,
    /// Copy buffer size.
    pub buffer_size: usize,
    /// Minimum interval between progress events.
    pub progress_interval: Duration,
}

impl EngineConfig {
    pub fn new(journal_path: impl Into<PathBuf>, trash: Arc<dyn Trash>) -> Self {
        EngineConfig {
            journal_path: journal_path.into(),
            trash,
            workers: 2,
            buffer_size: 1 << 20,
            progress_interval: Duration::from_millis(50),
        }
    }
}

pub(crate) struct Shared {
    pub(crate) journal: Mutex<journal::Journal>,
    pub(crate) controls: Mutex<HashMap<JobId, Arc<Control>>>,
    pub(crate) undo: Mutex<Option<UndoRecord>>,
    pub(crate) trash: Arc<dyn Trash>,
    pub(crate) events: Sender<JobEvent>,
    pub(crate) buffer_size: usize,
    pub(crate) progress_interval: Duration,
}

/// The engine handle.
pub struct OpEngine {
    shared: Arc<Shared>,
    events: Receiver<JobEvent>,
    jobs: Option<Sender<(JobId, OpRequest, Arc<Control>)>>,
    workers: Vec<JoinHandle<()>>,
    next_id: AtomicU64,
}

impl OpEngine {
    /// Open the engine, recovering from any crash recorded in the journal.
    pub fn open(cfg: EngineConfig) -> FsResult<(OpEngine, RecoveryReport)> {
        let (journal, report) = journal::Journal::open(&cfg.journal_path)?;
        let (ev_tx, ev_rx) = unbounded();
        let undo = report.last_undo.clone().map(|(job, steps)| UndoRecord {
            job,
            label: "restored".into(),
            steps,
        });
        let shared = Arc::new(Shared {
            journal: Mutex::new(journal),
            controls: Mutex::new(HashMap::new()),
            undo: Mutex::new(undo),
            trash: cfg.trash,
            events: ev_tx,
            buffer_size: cfg.buffer_size.max(4096),
            progress_interval: cfg.progress_interval,
        });
        let (job_tx, job_rx) = unbounded::<(JobId, OpRequest, Arc<Control>)>();
        let mut workers = Vec::new();
        for i in 0..cfg.workers.max(1) {
            let rx = job_rx.clone();
            let shared = Arc::clone(&shared);
            let h = std::thread::Builder::new()
                .name(format!("nx-ops-{i}"))
                .spawn(move || {
                    while let Ok((id, req, ctl)) = rx.recv() {
                        exec::run_job(&shared, id, req, &ctl);
                        shared.controls.lock().remove(&id);
                    }
                })
                .map_err(|e| FsError::from_io("<ops worker>", e))?;
            workers.push(h);
        }
        let engine = OpEngine {
            shared,
            events: ev_rx,
            jobs: Some(job_tx),
            workers,
            next_id: AtomicU64::new(report.max_job + 1),
        };
        Ok((engine, report))
    }

    pub fn events(&self) -> &Receiver<JobEvent> {
        &self.events
    }

    /// Queue an operation.
    pub fn submit(&self, req: OpRequest) -> FsResult<JobId> {
        if req.sources.is_empty() && !matches!(req.kind, OpKind::Undo | OpKind::NewFolder { .. }) {
            return Err(FsError::InvalidPath {
                path: PathBuf::new(),
                reason: "no source items",
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let ctl = Arc::new(Control::new());
        self.shared.controls.lock().insert(id, Arc::clone(&ctl));
        let _ = self.shared.events.send(JobEvent::Queued {
            job: id,
            label: req.kind.label(),
        });
        let tx = self.jobs.as_ref().ok_or(FsError::Disconnected)?;
        tx.send((id, req, ctl)).map_err(|_| FsError::Disconnected)?;
        Ok(id)
    }

    /// Undo the last completed operation (runs as its own job).
    pub fn undo(&self) -> FsResult<Option<JobId>> {
        if self.shared.undo.lock().is_none() {
            return Ok(None);
        }
        self.submit(OpRequest {
            kind: OpKind::Undo,
            sources: Vec::new(),
            conflict: ConflictPolicy::Skip,
        })
        .map(Some)
    }

    /// Label of what undo would revert.
    pub fn undo_label(&self) -> Option<String> {
        self.shared.undo.lock().as_ref().map(|u| u.label.clone())
    }

    fn control(&self, job: JobId) -> Option<Arc<Control>> {
        self.shared.controls.lock().get(&job).cloned()
    }

    pub fn pause(&self, job: JobId) -> bool {
        self.control(job).map(|c| c.set_paused(true)).is_some()
            && self.shared.events.send(JobEvent::Paused { job }).is_ok()
    }

    pub fn resume(&self, job: JobId) -> bool {
        self.control(job).map(|c| c.set_paused(false)).is_some()
            && self.shared.events.send(JobEvent::Resumed { job }).is_ok()
    }

    pub fn cancel(&self, job: JobId) -> bool {
        self.control(job).map(|c| c.cancel()).is_some()
    }

    /// Answer a pending conflict. `apply_to_all` reuses it for the rest of the job.
    pub fn resolve(&self, job: JobId, resolution: Resolution, apply_to_all: bool) -> bool {
        self.control(job)
            .is_some_and(|c| c.answers.0.send((resolution, apply_to_all)).is_ok())
    }
}

impl Drop for OpEngine {
    fn drop(&mut self) {
        for c in self.shared.controls.lock().values() {
            c.cancel();
        }
        self.jobs.take();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests;
