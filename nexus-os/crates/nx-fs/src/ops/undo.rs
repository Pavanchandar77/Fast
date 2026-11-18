//! Undo steps recorded by file operations.

use super::journal::{field_path, path_field};
use super::trash::TrashedItem;
use std::path::PathBuf;

/// One reversible effect of an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStep {
    /// We created this path (copy target, new folder). Undo removes it.
    Created(PathBuf),
    /// We moved/renamed `from` to `to`. Undo moves it back.
    Moved { from: PathBuf, to: PathBuf },
    /// We replaced `target`; the previous version waits at `backup`.
    Replaced { target: PathBuf, backup: PathBuf },
    /// We sent a path to the trash / Recycle Bin.
    Trashed(TrashedItem),
}

impl UndoStep {
    pub(crate) fn encode(&self) -> String {
        match self {
            UndoStep::Created(p) => format!("Created\t{}", path_field(p)),
            UndoStep::Moved { from, to } => {
                format!("Moved\t{}\t{}", path_field(from), path_field(to))
            }
            UndoStep::Replaced { target, backup } => {
                format!("Replaced\t{}\t{}", path_field(target), path_field(backup))
            }
            UndoStep::Trashed(t) => format!(
                "Trashed\t{}\t{}",
                path_field(&t.original),
                path_field(&t.location)
            ),
        }
    }

    pub(crate) fn decode(f: &[&str]) -> Option<UndoStep> {
        match *f.first()? {
            "Created" => Some(UndoStep::Created(field_path(f.get(1)?)?)),
            "Moved" => Some(UndoStep::Moved {
                from: field_path(f.get(1)?)?,
                to: field_path(f.get(2)?)?,
            }),
            "Replaced" => Some(UndoStep::Replaced {
                target: field_path(f.get(1)?)?,
                backup: field_path(f.get(2)?)?,
            }),
            "Trashed" => Some(UndoStep::Trashed(TrashedItem {
                original: field_path(f.get(1)?)?,
                location: field_path(f.get(2)?)?,
            })),
            _ => None,
        }
    }
}

/// The undo record of the last completed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRecord {
    pub job: u64,
    pub label: String,
    pub steps: Vec<UndoStep>,
}

impl UndoRecord {
    /// Backups that become garbage once this record can no longer be undone.
    pub fn backups(&self) -> impl Iterator<Item = &PathBuf> {
        self.steps.iter().filter_map(|s| match s {
            UndoStep::Replaced { backup, .. } => Some(backup),
            _ => None,
        })
    }
}
