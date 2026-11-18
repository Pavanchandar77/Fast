//! Minimal apply/revert surface. Win32 code implements this; tests use
//! [`crate::FakeSystem`]. A broader `Platform` trait can delegate to it.

use crate::journal::JournalError;
use crate::mutation::Mutation;

/// Applies and reverts a [`Mutation`].
///
/// `revert` must be idempotent: calling it when the mutation was recorded
/// but never applied (crash in between) returns the system to `previous`,
/// which is already the live state.
pub trait SystemMutator {
    fn apply(&mut self, mutation: &Mutation) -> Result<(), JournalError>;
    fn revert(&mut self, mutation: &Mutation) -> Result<(), JournalError>;
}
