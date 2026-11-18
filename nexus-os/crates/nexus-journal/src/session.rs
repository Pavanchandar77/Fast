//! Record-then-apply helper shared by tests and (conceptually) the Win32 side.
//!
//! Dry-run logs the mutation and does not record it and does not apply it,
//! so a later restore has nothing to undo.

use crate::journal::{Journal, JournalError, RestoreReport};
use crate::mutation::Mutation;
use crate::mutator::SystemMutator;

pub struct Session<M> {
    journal: Journal,
    system: M,
    dry_run: bool,
    logged: Vec<Mutation>,
}

impl<M: SystemMutator> Session<M> {
    pub fn new(journal: Journal, system: M, dry_run: bool) -> Self {
        Self {
            journal,
            system,
            dry_run,
            logged: Vec::new(),
        }
    }

    pub fn logged(&self) -> &[Mutation] {
        &self.logged
    }

    pub fn system(&self) -> &M {
        &self.system
    }

    pub fn system_mut(&mut self) -> &mut M {
        &mut self.system
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// `true` when the caller must still apply the mutation.
    /// Dry-run returns `false` after logging and writes nothing.
    pub fn record_only(&mut self, mutation: &Mutation) -> Result<bool, JournalError> {
        if self.dry_run {
            self.logged.push(mutation.clone());
            return Ok(false);
        }
        self.journal.record(mutation)?;
        Ok(true)
    }

    /// Fsync the record, then apply. On apply failure the record is removed
    /// so a mutation that never stuck is not replayed as something to undo.
    /// A crash between record and apply leaves the record in place; restore
    /// then writes `previous`, which is still the live value.
    pub fn commit(&mut self, mutation: Mutation) -> Result<(), JournalError> {
        if !self.record_only(&mutation)? {
            return Ok(());
        }
        if let Err(e) = self.system.apply(&mutation) {
            let _ = self.journal.remove_last_equal(&mutation);
            return Err(e);
        }
        Ok(())
    }

    pub fn restore(&mut self) -> Result<RestoreReport, JournalError> {
        self.journal.restore_all(&mut self.system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;
    use crate::mutation::{Mutation, PriorityClass};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch_path() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nxjr-session-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("state.nxjr")
    }

    #[test]
    fn dry_run_logs_without_applying_or_recording() {
        let path = scratch_path();
        let journal = Journal::open(&path).unwrap();
        let mut session = Session::new(journal, FakeSystem::clean(), true);
        let mutation = Mutation::ProcessPriority {
            pid: 1,
            previous: PriorityClass::Normal,
            new: PriorityClass::High,
        };
        session.commit(mutation.clone()).unwrap();
        assert_eq!(session.logged(), &[mutation]);
        assert!(session.journal().is_empty());
        assert_eq!(session.system().priority_of(1), PriorityClass::Normal);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
