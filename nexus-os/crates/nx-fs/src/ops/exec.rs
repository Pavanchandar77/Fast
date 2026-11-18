//! Job execution. One [`Exec`] per running job.

use super::journal::{remove_any, Record};
use super::undo::{UndoRecord, UndoStep};
use super::{
    ConflictPolicy, Control, JobEvent, JobId, OpKind, OpRequest, Progress, Resolution, Shared,
    Summary,
};
use crate::error::{FsError, FsResult, IoContext};
use crate::index::IndexChange;
use crate::path::{is_within, to_native, unique_name, validate_file_name};
use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Files up to this size are copied with the OS fast path
/// (`copy_file_range` / `CopyFileExW`); larger ones are chunked so pause,
/// cancel and progress stay responsive.
const FAST_COPY_MAX: u64 = 4 << 20;

pub(crate) fn run_job(sh: &Shared, id: JobId, req: OpRequest, ctl: &Control) {
    let mut ex = Exec::new(sh, id, ctl, req.conflict);
    if let Err(e) = ex.journal(&Record::Begin {
        job: id,
        kind: req.kind.label().into(),
    }) {
        let _ = sh.events.send(JobEvent::Failed { job: id, error: e });
        return;
    }
    let is_undo = matches!(req.kind, OpKind::Undo);
    let result = match &req.kind {
        OpKind::Copy { dest } => ex.run_copy(&req.sources, dest),
        OpKind::Move { dest } => ex.run_move(&req.sources, dest),
        OpKind::Delete { to_trash } => ex.run_delete(&req.sources, *to_trash),
        OpKind::Rename { new_name } => ex.run_rename(&req.sources, new_name),
        OpKind::NewFolder { parent, name } => ex.run_new_folder(parent, name),
        OpKind::Undo => ex.run_undo(),
    };
    ex.emit_progress(true);
    if !ex.changes.is_empty() {
        let changes = std::mem::take(&mut ex.changes);
        let _ = sh.events.send(JobEvent::IndexChanges { job: id, changes });
    }
    let outcome = match &result {
        Ok(()) => "ok",
        Err(FsError::Cancelled) => "cancelled",
        Err(_) => "failed",
    };
    let _ = ex.journal(&Record::End {
        job: id,
        outcome: outcome.into(),
    });
    // Partial work is undoable too (a cancelled copy can be rolled back).
    if !is_undo && !ex.steps.is_empty() {
        ex.summary.undoable = true;
        let rec = UndoRecord {
            job: id,
            label: req.kind.label().into(),
            steps: std::mem::take(&mut ex.steps),
        };
        let old = sh.undo.lock().replace(rec);
        if let Some(old) = old {
            for b in old.backups() {
                let _ = remove_any(b);
            }
        }
    }
    let summary = ex.summary.clone();
    let ev = match result {
        Ok(()) => JobEvent::Finished { job: id, summary },
        Err(FsError::Cancelled) => JobEvent::Cancelled { job: id, summary },
        Err(error) => JobEvent::Failed { job: id, error },
    };
    let _ = sh.events.send(ev);
}

pub(crate) struct Exec<'a> {
    sh: &'a Shared,
    id: JobId,
    ctl: &'a Control,
    policy: ConflictPolicy,
    steps: Vec<UndoStep>,
    changes: Vec<IndexChange>,
    summary: Summary,
    items_total: u64,
    bytes_total: u64,
    started: Instant,
    last_emit: Instant,
    current: PathBuf,
    buf: Vec<u8>,
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl<'a> Exec<'a> {
    fn new(sh: &'a Shared, id: JobId, ctl: &'a Control, policy: ConflictPolicy) -> Self {
        let now = Instant::now();
        Exec {
            sh,
            id,
            ctl,
            policy,
            steps: Vec::new(),
            changes: Vec::new(),
            summary: Summary::default(),
            items_total: 0,
            bytes_total: 0,
            started: now,
            last_emit: now,
            current: PathBuf::new(),
            buf: Vec::new(),
        }
    }

    fn journal(&self, r: &Record) -> FsResult<()> {
        self.sh.journal.lock().append(r)
    }

    fn step(&mut self, s: UndoStep) -> FsResult<()> {
        self.journal(&Record::Step {
            job: self.id,
            step: s.clone(),
        })?;
        self.steps.push(s);
        Ok(())
    }

    fn emit_progress(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_emit) < self.sh.progress_interval {
            return;
        }
        self.last_emit = now;
        let secs = now.duration_since(self.started).as_secs_f64().max(1e-6);
        let _ = self.sh.events.send(JobEvent::Progress(Progress {
            job: self.id,
            bytes_done: self.summary.bytes_done,
            bytes_total: self.bytes_total,
            items_done: self.summary.items_done,
            items_total: self.items_total,
            current: self.current.clone(),
            bytes_per_sec: self.summary.bytes_done as f64 / secs,
        }));
    }

    /// Record a per-item failure and keep going (cancel still propagates).
    fn item_result(&mut self, path: &Path, r: FsResult<()>) -> FsResult<()> {
        match r {
            Ok(()) => Ok(()),
            Err(FsError::Cancelled) => Err(FsError::Cancelled),
            Err(error) => {
                self.summary.failed += 1;
                let _ = self.sh.events.send(JobEvent::ItemFailed {
                    job: self.id,
                    path: path.to_path_buf(),
                    error,
                });
                Ok(())
            }
        }
    }

    fn start(&mut self, items: u64, bytes: u64) {
        self.items_total = items;
        self.bytes_total = bytes;
        let _ = self.sh.events.send(JobEvent::Started {
            job: self.id,
            items_total: items,
            bytes_total: bytes,
        });
    }

    /// Count items and bytes below `p` without following links.
    fn measure(&self, p: &Path) -> (u64, u64) {
        let mut stack = vec![p.to_path_buf()];
        let (mut items, mut bytes) = (0u64, 0u64);
        while let Some(cur) = stack.pop() {
            if self.ctl.is_cancelled() {
                break;
            }
            let Ok(md) = std::fs::symlink_metadata(to_native(&cur)) else {
                continue;
            };
            items += 1;
            if md.is_dir() {
                if let Ok(rd) = std::fs::read_dir(to_native(&cur)) {
                    stack.extend(rd.flatten().map(|e| cur.join(e.file_name())));
                }
            } else {
                bytes += md.len();
            }
        }
        (items, bytes)
    }

    fn resolve_conflict(
        &mut self,
        src: &Path,
        dest: &Path,
        smd: &Metadata,
        dmd: &Metadata,
    ) -> FsResult<Resolution> {
        let src_newer = match (smd.modified(), dmd.modified()) {
            (Ok(a), Ok(b)) => a > b,
            _ => false,
        };
        match self.policy {
            ConflictPolicy::Skip => Ok(Resolution::Skip),
            ConflictPolicy::Replace => Ok(Resolution::Replace),
            ConflictPolicy::KeepBoth => Ok(Resolution::KeepBoth),
            ConflictPolicy::ReplaceIfNewer => Ok(if src_newer {
                Resolution::Replace
            } else {
                Resolution::Skip
            }),
            ConflictPolicy::Ask => {
                let _ = self.sh.events.send(JobEvent::Conflict {
                    job: self.id,
                    src: src.to_path_buf(),
                    dest: dest.to_path_buf(),
                    src_newer,
                    dest_is_dir: dmd.is_dir(),
                });
                let (res, all) = self.ctl.wait_answer()?;
                if all {
                    self.policy = match res {
                        Resolution::Skip => ConflictPolicy::Skip,
                        Resolution::Replace => ConflictPolicy::Replace,
                        Resolution::KeepBoth => ConflictPolicy::KeepBoth,
                        Resolution::Cancel => ConflictPolicy::Ask,
                    };
                }
                if res == Resolution::Cancel {
                    return Err(FsError::Cancelled);
                }
                Ok(res)
            }
        }
    }

    /// Hidden sibling path for temps/backups: `.name.tag-<job>`.
    fn sibling(&self, target: &Path, tag: &str) -> PathBuf {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut short: String = name.chars().take(180).collect();
        if short.is_empty() {
            short.push('x');
        }
        let file = format!(".{short}.{tag}-{}", self.id);
        match target.parent() {
            Some(p) => p.join(file),
            None => PathBuf::from(file),
        }
    }

    /// Move an existing target aside so it can be restored on undo/crash.
    fn stash(&mut self, target: &Path) -> FsResult<PathBuf> {
        let backup = self.sibling(target, "nxold");
        self.journal(&Record::Backup {
            job: self.id,
            target: target.to_path_buf(),
            backup: backup.clone(),
        })?;
        std::fs::rename(to_native(target), to_native(&backup)).at(target)?;
        Ok(backup)
    }

    fn unique_in(dir: &Path, name: &OsStr) -> PathBuf {
        let n = unique_name(&name.to_string_lossy(), |c| {
            std::fs::symlink_metadata(to_native(&dir.join(c))).is_ok()
        });
        dir.join(n)
    }

    // ---------------------------------------------------------------- copy

    fn run_copy(&mut self, sources: &[PathBuf], dest: &Path) -> FsResult<()> {
        let md = std::fs::metadata(to_native(dest)).at(dest)?;
        if !md.is_dir() {
            return Err(FsError::InvalidPath {
                path: dest.to_path_buf(),
                reason: "destination is not a folder",
            });
        }
        let (mut items, mut bytes) = (0, 0);
        for s in sources {
            let (i, b) = self.measure(s);
            items += i;
            bytes += b;
        }
        self.start(items, bytes);
        for src in sources {
            let r = match src.file_name() {
                Some(name) => self.copy_into(src, dest, name, false),
                None => Err(FsError::InvalidPath {
                    path: src.clone(),
                    reason: "cannot copy a root",
                }),
            };
            self.item_result(src, r)?;
        }
        Ok(())
    }

    /// Copy `src` to `dest_dir/name`. `fresh` = we created `dest_dir` in
    /// this job (no conflicts possible, children need no undo steps).
    fn copy_into(
        &mut self,
        src: &Path,
        dest_dir: &Path,
        name: &OsStr,
        fresh: bool,
    ) -> FsResult<()> {
        self.ctl.checkpoint()?;
        self.current = src.to_path_buf();
        let md = std::fs::symlink_metadata(to_native(src)).at(src)?;
        if md.is_dir() && is_within(dest_dir, src) {
            return Err(FsError::IntoItself {
                src: src.to_path_buf(),
                dest: dest_dir.to_path_buf(),
            });
        }
        let mut target = dest_dir.join(name);
        if target == src {
            // Duplicate in place ("Copy" / Ctrl+D): Finder-style "name (2)".
            target = Self::unique_in(dest_dir, name);
        }
        let mut replace = false;
        if !fresh {
            if let Ok(tmd) = std::fs::symlink_metadata(to_native(&target)) {
                if md.is_dir() && tmd.is_dir() {
                    return self.copy_children(src, &target, false);
                }
                match self.resolve_conflict(src, &target, &md, &tmd)? {
                    Resolution::Skip => {
                        let (i, b) = self.measure(src);
                        self.summary.skipped += 1;
                        self.summary.items_done += i;
                        self.summary.bytes_done += b;
                        return Ok(());
                    }
                    Resolution::KeepBoth => target = Self::unique_in(dest_dir, name),
                    Resolution::Replace => replace = true,
                    Resolution::Cancel => return Err(FsError::Cancelled),
                }
            }
        }
        if md.is_dir() {
            let backup = if replace {
                Some(self.stash(&target)?)
            } else {
                None
            };
            if let Err(e) = std::fs::create_dir(to_native(&target)) {
                if let Some(b) = backup {
                    let _ = std::fs::rename(to_native(&b), to_native(&target));
                }
                return Err(FsError::from_io(&target, e));
            }
            match backup {
                Some(b) => self.step(UndoStep::Replaced {
                    target: target.clone(),
                    backup: b,
                })?,
                None if !fresh => self.step(UndoStep::Created(target.clone()))?,
                None => {}
            }
            self.changes.push(IndexChange::Created {
                path: path_str(&target),
                is_dir: true,
            });
            let _ = std::fs::set_permissions(to_native(&target), md.permissions());
            return self.copy_children(src, &target, true);
        }
        if md.file_type().is_symlink() {
            self.copy_link(src, &target, replace, fresh)?;
        } else {
            self.copy_file(src, &target, replace, fresh, &md)?;
        }
        self.summary.items_done += 1;
        self.emit_progress(false);
        Ok(())
    }

    fn copy_children(&mut self, src: &Path, target: &Path, fresh: bool) -> FsResult<()> {
        self.summary.items_done += 1;
        let rd = std::fs::read_dir(to_native(src)).at(src)?;
        for child in rd {
            let child = match child {
                Ok(c) => c,
                Err(e) => {
                    self.item_result(src, Err(FsError::from_io(src, e)))?;
                    continue;
                }
            };
            let cp = src.join(child.file_name());
            let r = self.copy_into(&cp, target, &child.file_name(), fresh);
            self.item_result(&cp, r)?;
        }
        Ok(())
    }

    fn copy_file(
        &mut self,
        src: &Path,
        target: &Path,
        replace: bool,
        fresh: bool,
        md: &Metadata,
    ) -> FsResult<()> {
        let tmp = self.sibling(target, "nxpart");
        self.journal(&Record::Temp {
            job: self.id,
            temp: tmp.clone(),
            dest: target.to_path_buf(),
        })?;
        let res = if md.len() <= FAST_COPY_MAX {
            std::fs::copy(to_native(src), to_native(&tmp))
                .map(|_| ())
                .at(src)
        } else {
            self.copy_chunked(src, &tmp)
        };
        if let Err(e) = res {
            let _ = std::fs::remove_file(to_native(&tmp));
            return Err(e);
        }
        self.summary.bytes_done += if md.len() <= FAST_COPY_MAX {
            md.len()
        } else {
            0
        };
        if let Ok(m) = md.modified() {
            if let Ok(f) = File::options().write(true).open(to_native(&tmp)) {
                let _ = f.set_modified(m);
            }
        }
        let backup = if replace {
            Some(self.stash(target)?)
        } else {
            None
        };
        if backup.is_none() && std::fs::symlink_metadata(to_native(target)).is_ok() {
            // Appeared while we were copying: never clobber silently.
            let _ = std::fs::remove_file(to_native(&tmp));
            return Err(FsError::AlreadyExists {
                path: target.to_path_buf(),
            });
        }
        if let Err(e) = std::fs::rename(to_native(&tmp), to_native(target)) {
            let _ = std::fs::remove_file(to_native(&tmp));
            if let Some(b) = &backup {
                let _ = std::fs::rename(to_native(b), to_native(target));
            }
            return Err(FsError::from_io(target, e));
        }
        match backup {
            Some(b) => self.step(UndoStep::Replaced {
                target: target.to_path_buf(),
                backup: b,
            })?,
            None if !fresh => self.step(UndoStep::Created(target.to_path_buf()))?,
            None => {}
        }
        self.changes.push(IndexChange::Created {
            path: path_str(target),
            is_dir: false,
        });
        Ok(())
    }

    fn copy_chunked(&mut self, src: &Path, tmp: &Path) -> FsResult<()> {
        if self.buf.len() != self.sh.buffer_size {
            self.buf = vec![0u8; self.sh.buffer_size];
        }
        let mut input = File::open(to_native(src)).at(src)?;
        let mut out = File::options()
            .write(true)
            .create_new(true)
            .open(to_native(tmp))
            .at(tmp)?;
        loop {
            self.ctl.checkpoint()?;
            let n = input.read(&mut self.buf).at(src)?;
            if n == 0 {
                break;
            }
            out.write_all(&self.buf[..n]).at(tmp)?;
            self.summary.bytes_done += n as u64;
            self.emit_progress(false);
        }
        out.flush().at(tmp)?;
        if let Ok(md) = input.metadata() {
            let _ = out.set_permissions(md.permissions());
        }
        Ok(())
    }

    fn copy_link(&mut self, src: &Path, target: &Path, replace: bool, fresh: bool) -> FsResult<()> {
        let link = std::fs::read_link(to_native(src)).at(src)?;
        let backup = if replace {
            Some(self.stash(target)?)
        } else {
            None
        };
        if let Err(e) = make_symlink(src, &link, target) {
            if let Some(b) = backup {
                let _ = std::fs::rename(to_native(&b), to_native(target));
            }
            return Err(e);
        }
        match backup {
            Some(b) => self.step(UndoStep::Replaced {
                target: target.to_path_buf(),
                backup: b,
            })?,
            None if !fresh => self.step(UndoStep::Created(target.to_path_buf()))?,
            None => {}
        }
        self.changes.push(IndexChange::Created {
            path: path_str(target),
            is_dir: false,
        });
        Ok(())
    }

    // ---------------------------------------------------------------- move

    fn run_move(&mut self, sources: &[PathBuf], dest: &Path) -> FsResult<()> {
        let md = std::fs::metadata(to_native(dest)).at(dest)?;
        if !md.is_dir() {
            return Err(FsError::InvalidPath {
                path: dest.to_path_buf(),
                reason: "destination is not a folder",
            });
        }
        self.start(sources.len() as u64, 0);
        for src in sources {
            let r = match src.file_name() {
                Some(name) => self.move_into(src, dest, name),
                None => Err(FsError::InvalidPath {
                    path: src.clone(),
                    reason: "cannot move a root",
                }),
            };
            self.item_result(src, r)?;
            self.summary.items_done += 1;
            self.emit_progress(false);
        }
        Ok(())
    }

    fn move_into(&mut self, src: &Path, dest_dir: &Path, name: &OsStr) -> FsResult<()> {
        self.ctl.checkpoint()?;
        self.current = src.to_path_buf();
        let md = std::fs::symlink_metadata(to_native(src)).at(src)?;
        let mut target = dest_dir.join(name);
        if target == src {
            self.summary.skipped += 1;
            return Ok(());
        }
        if md.is_dir() && is_within(dest_dir, src) {
            return Err(FsError::IntoItself {
                src: src.to_path_buf(),
                dest: dest_dir.to_path_buf(),
            });
        }
        let mut replace = false;
        if let Ok(tmd) = std::fs::symlink_metadata(to_native(&target)) {
            if !same_file(src, &target) {
                if md.is_dir() && tmd.is_dir() {
                    // Merge folders: move children, then drop the empty source.
                    let rd = std::fs::read_dir(to_native(src)).at(src)?;
                    for child in rd.flatten() {
                        let cp = src.join(child.file_name());
                        let r = self.move_into(&cp, &target, &child.file_name());
                        self.item_result(&cp, r)?;
                    }
                    if std::fs::remove_dir(to_native(src)).is_ok() {
                        self.changes.push(IndexChange::Removed {
                            path: path_str(src),
                        });
                    }
                    return Ok(());
                }
                match self.resolve_conflict(src, &target, &md, &tmd)? {
                    Resolution::Skip => {
                        self.summary.skipped += 1;
                        return Ok(());
                    }
                    Resolution::KeepBoth => target = Self::unique_in(dest_dir, name),
                    Resolution::Replace => replace = true,
                    Resolution::Cancel => return Err(FsError::Cancelled),
                }
            }
        }
        let backup = if replace {
            Some(self.stash(&target)?)
        } else {
            None
        };
        if let Err(e) = move_path(src, &target, Some(self)) {
            if let Some(b) = backup {
                let _ = std::fs::rename(to_native(&b), to_native(&target));
            }
            return Err(e);
        }
        if let Some(b) = backup {
            self.step(UndoStep::Replaced {
                target: target.clone(),
                backup: b,
            })?;
        }
        self.step(UndoStep::Moved {
            from: src.to_path_buf(),
            to: target.clone(),
        })?;
        self.changes.push(IndexChange::Renamed {
            from: path_str(src),
            to: path_str(&target),
        });
        Ok(())
    }

    // -------------------------------------------------------------- delete

    fn run_delete(&mut self, sources: &[PathBuf], to_trash: bool) -> FsResult<()> {
        self.start(sources.len() as u64, 0);
        for src in sources {
            self.ctl.checkpoint()?;
            self.current = src.clone();
            let r = if to_trash {
                match self.sh.trash.trash(src) {
                    Ok(item) => self.step(UndoStep::Trashed(item)),
                    Err(e) => Err(e),
                }
            } else {
                remove_any(src).at(src)
            };
            let ok = r.is_ok();
            self.item_result(src, r)?;
            if ok {
                self.changes.push(IndexChange::Removed {
                    path: path_str(src),
                });
            }
            self.summary.items_done += 1;
            self.emit_progress(false);
        }
        Ok(())
    }

    // ------------------------------------------------------- rename / mkdir

    fn run_rename(&mut self, sources: &[PathBuf], new_name: &str) -> FsResult<()> {
        let [src] = sources else {
            return Err(FsError::InvalidPath {
                path: PathBuf::new(),
                reason: "rename takes exactly one item",
            });
        };
        validate_file_name(new_name).map_err(|reason| FsError::InvalidPath {
            path: PathBuf::from(new_name),
            reason,
        })?;
        self.start(1, 0);
        let parent = src.parent().ok_or_else(|| FsError::InvalidPath {
            path: src.clone(),
            reason: "cannot rename a root",
        })?;
        let target = parent.join(new_name);
        if &target == src {
            return Ok(());
        }
        if std::fs::symlink_metadata(to_native(&target)).is_ok() && !same_file(src, &target) {
            return Err(FsError::AlreadyExists { path: target });
        }
        std::fs::rename(to_native(src), to_native(&target)).at(src)?;
        self.step(UndoStep::Moved {
            from: src.clone(),
            to: target.clone(),
        })?;
        self.changes.push(IndexChange::Renamed {
            from: path_str(src),
            to: path_str(&target),
        });
        self.summary.items_done = 1;
        Ok(())
    }

    fn run_new_folder(&mut self, parent: &Path, name: &str) -> FsResult<()> {
        validate_file_name(name).map_err(|reason| FsError::InvalidPath {
            path: PathBuf::from(name),
            reason,
        })?;
        self.start(1, 0);
        let target = Self::unique_in(parent, OsStr::new(name));
        std::fs::create_dir(to_native(&target)).at(&target)?;
        self.step(UndoStep::Created(target.clone()))?;
        self.changes.push(IndexChange::Created {
            path: path_str(&target),
            is_dir: true,
        });
        self.summary.items_done = 1;
        Ok(())
    }

    // ---------------------------------------------------------------- undo

    fn run_undo(&mut self) -> FsResult<()> {
        let Some(rec) = self.sh.undo.lock().take() else {
            return Ok(());
        };
        self.start(rec.steps.len() as u64, 0);
        for step in rec.steps.iter().rev() {
            self.ctl.checkpoint()?;
            let (path, r) = match step {
                UndoStep::Created(p) => {
                    // Undoing a copy sends the copy to the trash (never a hard delete
                    // of something the user may have edited since).
                    let r = if std::fs::symlink_metadata(to_native(p)).is_ok() {
                        self.sh.trash.trash(p).map(|_| ())
                    } else {
                        Ok(())
                    };
                    if r.is_ok() {
                        self.changes
                            .push(IndexChange::Removed { path: path_str(p) });
                    }
                    (p, r)
                }
                UndoStep::Moved { from, to } => {
                    let r = if std::fs::symlink_metadata(to_native(from)).is_ok()
                        && !same_file(from, to)
                    {
                        Err(FsError::AlreadyExists { path: from.clone() })
                    } else {
                        move_path(to, from, Some(self))
                    };
                    if r.is_ok() {
                        self.changes.push(IndexChange::Renamed {
                            from: path_str(to),
                            to: path_str(from),
                        });
                    }
                    (to, r)
                }
                UndoStep::Replaced { target, backup } => {
                    let r = (|| {
                        if std::fs::symlink_metadata(to_native(target)).is_ok() {
                            remove_any(target).at(target)?;
                        }
                        std::fs::rename(to_native(backup), to_native(target)).at(backup)
                    })();
                    (target, r)
                }
                UndoStep::Trashed(item) => {
                    let r = self.sh.trash.restore(item);
                    if r.is_ok() {
                        let is_dir = std::fs::metadata(to_native(&item.original))
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        self.changes.push(IndexChange::Created {
                            path: path_str(&item.original),
                            is_dir,
                        });
                    }
                    (&item.original, r)
                }
            };
            let path = path.clone();
            self.item_result(&path, r)?;
            self.summary.items_done += 1;
            self.emit_progress(false);
        }
        Ok(())
    }
}

/// True if both paths name the same file (case-only renames on Windows).
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (
            std::fs::canonicalize(to_native(a)),
            std::fs::canonicalize(to_native(b)),
        ) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }
}

fn make_symlink(src: &Path, link: &Path, target: &Path) -> FsResult<()> {
    #[cfg(unix)]
    {
        let _ = src;
        std::os::unix::fs::symlink(link, target).at(target)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let is_dir = std::fs::symlink_metadata(to_native(src))
            .map(|m| m.file_type().is_symlink_dir())
            .unwrap_or(false);
        if is_dir {
            std::os::windows::fs::symlink_dir(link, to_native(target)).at(target)
        } else {
            std::os::windows::fs::symlink_file(link, to_native(target)).at(target)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (src, link, target);
        Err(FsError::Unsupported("symlinks"))
    }
}

fn is_cross_device(e: &std::io::Error) -> bool {
    // EXDEV (18) on Unix, ERROR_NOT_SAME_DEVICE (17) on Windows.
    e.kind() == std::io::ErrorKind::CrossesDevices
        || (cfg!(unix) && e.raw_os_error() == Some(18))
        || (cfg!(windows) && e.raw_os_error() == Some(17))
}

/// Rename, falling back to copy + delete across volumes. Creates missing
/// parent folders of `dst` (needed when undo restores into a removed tree).
pub(crate) fn move_path(src: &Path, dst: &Path, ex: Option<&mut Exec<'_>>) -> FsResult<()> {
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() && std::fs::symlink_metadata(to_native(parent)).is_err() {
            std::fs::create_dir_all(to_native(parent)).at(parent)?;
        }
    }
    match std::fs::rename(to_native(src), to_native(dst)) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            if let Err(err) = copy_tree(src, dst, ex) {
                let _ = remove_any(dst);
                return Err(err);
            }
            remove_any(src).at(src)
        }
        Err(e) => Err(FsError::from_io(src, e)),
    }
}

/// Raw recursive copy to a destination that does not exist yet.
fn copy_tree(src: &Path, dst: &Path, mut ex: Option<&mut Exec<'_>>) -> FsResult<()> {
    if let Some(ex) = ex.as_deref_mut() {
        ex.ctl.checkpoint()?;
    }
    let md = std::fs::symlink_metadata(to_native(src)).at(src)?;
    if md.file_type().is_symlink() {
        let link = std::fs::read_link(to_native(src)).at(src)?;
        return make_symlink(src, &link, dst);
    }
    if md.is_dir() {
        std::fs::create_dir(to_native(dst)).at(dst)?;
        for child in std::fs::read_dir(to_native(src)).at(src)? {
            let child = child.at(src)?;
            copy_tree(
                &src.join(child.file_name()),
                &dst.join(child.file_name()),
                ex.as_deref_mut(),
            )?;
        }
        return Ok(());
    }
    std::fs::copy(to_native(src), to_native(dst)).at(src)?;
    if let Some(ex) = ex {
        ex.summary.bytes_done += md.len();
        ex.emit_progress(false);
    }
    Ok(())
}
