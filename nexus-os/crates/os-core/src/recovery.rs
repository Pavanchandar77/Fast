//! Process-wide journal. Mutations are recorded before they are applied.
//!
//! Restore runs on clean exit ([`RestoreOnDrop`]), on panic, from the
//! watchdog after the shell process has exited, and on the next startup when
//! a stale file is still on disk. The panic path never re-locks the journal
//! file: the same process already holds that lock.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

use nexus_journal::{Journal, JournalError, Mutation, Session, SystemMutator};
use tracing::{info, warn};

use crate::effects::Win32Mutator;
use crate::error::{CoreError, CoreResult};
use crate::hollow_shell::HollowShellManager;
use crate::window::{hwnd_from_bits, WindowHandle};
use crate::Rect;

struct State {
    session: Session<Win32Mutator>,
    seen_positions: HashSet<u64>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn map_err(err: JournalError) -> CoreError {
    CoreError::Generic(err.to_string())
}

fn lock() -> CoreResult<MutexGuard<'static, State>> {
    let state = STATE.get().ok_or_else(|| {
        CoreError::Generic("journal is not installed; refusing to mutate the system".into())
    })?;
    match state.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => Ok(poisoned.into_inner()),
    }
}

/// `LOCALAPPDATA/nexus-os/state.nxjr`, else the user state dir, else the cwd.
pub fn default_journal_path() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("nexus-os").join("state.nxjr");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("nexus-os")
            .join("state.nxjr");
    }
    PathBuf::from("nexus-state.nxjr")
}

/// Open the journal, revert anything a previous process left behind, and
/// install it for this process. Dry-run still replays a stale file: those
/// entries were applied for real.
pub fn bootstrap(dry_run: bool) -> CoreResult<()> {
    let path = default_journal_path();
    let mut journal = Journal::open(&path).map_err(map_err)?;
    if !journal.is_empty() {
        warn!(
            path = %path.display(),
            entries = journal.entries().len(),
            "stale journal found; restoring before startup continues"
        );
        let mut system = Win32Mutator;
        let report = journal.restore_all(&mut system).map_err(map_err)?;
        info!(
            reverted = report.reverted,
            failures = report.failures.len(),
            "startup restore finished"
        );
    }
    let state = State {
        session: Session::new(journal, Win32Mutator, dry_run),
        seen_positions: HashSet::new(),
    };
    STATE
        .set(Mutex::new(state))
        .map_err(|_| CoreError::Generic("journal already installed".into()))?;
    Ok(())
}

pub fn is_dry_run() -> bool {
    lock()
        .map(|guard| guard.session.is_dry_run())
        .unwrap_or(false)
}

/// Record `mutation` and then apply it. Dry-run logs and applies nothing.
pub fn commit(mutation: &Mutation) -> CoreResult<()> {
    let should_apply = {
        let mut guard = lock()?;
        guard.session.record_only(mutation).map_err(map_err)?
    };
    if !should_apply {
        info!("dry-run: {mutation:?}");
        return Ok(());
    }
    let mut system = Win32Mutator;
    if let Err(err) = system.apply(mutation) {
        if let Ok(mut guard) = lock() {
            let _ = guard.session.journal_mut().remove_last_equal(mutation);
        }
        return Err(map_err(err));
    }
    Ok(())
}

/// Revert the newest entry matching `pred` and drop it from the journal.
pub fn undo_last(pred: impl Fn(&Mutation) -> bool) -> CoreResult<bool> {
    let mutation = {
        let guard = lock()?;
        if guard.session.is_dry_run() {
            return Ok(false);
        }
        guard
            .session
            .journal()
            .entries()
            .iter()
            .rev()
            .find(|entry| pred(entry))
            .cloned()
    };
    let Some(mutation) = mutation else {
        return Ok(false);
    };
    let mut system = Win32Mutator;
    let revert_result = system.revert(&mutation);
    let mut guard = lock()?;
    let _ = guard.session.journal_mut().remove_last_equal(&mutation);
    if let Mutation::WindowPosition { hwnd, .. } = &mutation {
        guard.seen_positions.remove(hwnd);
    }
    revert_result.map_err(map_err)?;
    Ok(true)
}

/// Journal the original rectangle once, then move the window every frame.
/// `SWP_ASYNCWINDOWPOS` is applied inside [`WindowHandle::place_async`].
pub fn place_foreign_window(hwnd: u64, previous: Rect, new_rect: Rect) -> CoreResult<()> {
    let recorded = {
        let mut guard = lock()?;
        if guard.session.is_dry_run() {
            if guard.seen_positions.insert(hwnd) {
                info!("dry-run: move hwnd {hwnd:#x} to {new_rect:?}");
            }
            return Ok(());
        }
        if !guard.seen_positions.insert(hwnd) {
            false
        } else {
            let mutation = Mutation::WindowPosition {
                hwnd,
                previous,
                new: new_rect,
            };
            if let Err(err) = guard.session.record_only(&mutation) {
                guard.seen_positions.remove(&hwnd);
                return Err(map_err(err));
            }
            true
        }
    };
    let handle = WindowHandle::new(hwnd_from_bits(hwnd));
    let win_rect = windows::Win32::Foundation::RECT {
        left: new_rect.x,
        top: new_rect.y,
        right: new_rect.x.saturating_add(new_rect.width),
        bottom: new_rect.y.saturating_add(new_rect.height),
    };
    if let Err(err) = handle.place_async(win_rect) {
        if recorded {
            if let Ok(mut guard) = lock() {
                let mutation = Mutation::WindowPosition {
                    hwnd,
                    previous,
                    new: new_rect,
                };
                let _ = guard.session.journal_mut().remove_last_equal(&mutation);
                guard.seen_positions.remove(&hwnd);
            }
        }
        return Err(err);
    }
    Ok(())
}

/// Revert every recorded mutation. Safe to call when this process still holds
/// the journal lock, including from a panic hook.
pub fn restore_best_effort() {
    if let Some(state) = STATE.get() {
        match state.try_lock() {
            Ok(mut guard) => {
                let _ = guard.session.restore();
            }
            Err(TryLockError::WouldBlock) => revert_durable_only(),
            Err(TryLockError::Poisoned(poisoned)) => {
                let mut guard = poisoned.into_inner();
                let _ = guard.session.restore();
            }
        }
        return;
    }
    revert_durable_only();
}

fn revert_durable_only() {
    let path = default_journal_path();
    let Ok(entries) = Journal::read_durable(&path) else {
        return;
    };
    let mut system = Win32Mutator;
    for mutation in entries.into_iter().rev() {
        let _ = system.revert(&mutation);
    }
}

/// Open the journal in this process and revert it. The shell must already
/// have exited, otherwise the file lock is still held.
pub fn restore_journal_file() -> CoreResult<()> {
    let path = default_journal_path();
    if !path.exists() {
        return Ok(());
    }
    let mut journal = Journal::open(&path).map_err(map_err)?;
    let mut system = Win32Mutator;
    let report = journal.restore_all(&mut system).map_err(map_err)?;
    info!(
        path = %path.display(),
        reverted = report.reverted,
        failures = report.failures.len(),
        "watchdog restore finished"
    );
    for failure in &report.failures {
        warn!("restore step failed: {failure}");
    }
    Ok(())
}

/// Restores the journal and the taskbar when the shell returns from `main`.
pub struct RestoreOnDrop;

impl RestoreOnDrop {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RestoreOnDrop {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        restore_best_effort();
        HollowShellManager::restore_taskbar();
    }
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("NEXUS PANIC: {info}");
        restore_best_effort();
        HollowShellManager::restore_taskbar();
    }));
}
