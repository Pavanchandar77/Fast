use crate::error::{PowerError, PowerResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strongly-typed identifier for a quick note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NoteId(pub u64);

impl std::fmt::Display for NoteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NoteId({})", self.0)
    }
}

/// Color theme or tint for a quick note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum NoteColor {
    #[default]
    Yellow,
    Blue,
    Green,
    Pink,
    Purple,
    Gray,
    Custom(u32),
}

/// A quick note item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub color: NoteColor,
    pub pinned: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Reversible action plan for undoing a note mutation or resurrection from deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteRevertPlan {
    RestoreContent {
        id: NoteId,
        previous_title: String,
        previous_content: String,
        previous_updated_at: u64,
    },
    Undelete {
        note: Note,
    },
}

impl NoteRevertPlan {
    pub fn revert(&self, store: &mut NotesStore) -> PowerResult<()> {
        match self {
            Self::RestoreContent {
                id,
                previous_title,
                previous_content,
                previous_updated_at,
            } => {
                let note = store
                    .notes
                    .get_mut(id)
                    .ok_or(PowerError::NoteNotFound(id.0))?;
                note.title = previous_title.clone();
                note.content = previous_content.clone();
                note.updated_at_ms = *previous_updated_at;
                Ok(())
            }
            Self::Undelete { note } => {
                store.notes.insert(note.id, note.clone());
                Ok(())
            }
        }
    }
}

/// Persistent store for quick notes with full-text search, tagging, pinning, and reversible edits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotesStore {
    notes: HashMap<NoteId, Note>,
    next_id: u64,
}

impl NotesStore {
    pub fn new() -> Self {
        Self {
            notes: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn get(&self, id: NoteId) -> Option<&Note> {
        self.notes.get(&id)
    }

    /// Creates and stores a new note, returning its `NoteId`.
    pub fn create_note(
        &mut self,
        title: impl Into<String>,
        content: impl Into<String>,
        tags: Vec<String>,
        color: NoteColor,
        timestamp_ms: u64,
    ) -> NoteId {
        let id = NoteId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        let note = Note {
            id,
            title: title.into(),
            content: content.into(),
            tags,
            color,
            pinned: false,
            created_at_ms: timestamp_ms,
            updated_at_ms: timestamp_ms,
        };

        self.notes.insert(id, note);
        id
    }

    /// Updates the title and content of an existing note, returning a revert plan for rollback.
    pub fn update_note(
        &mut self,
        id: NoteId,
        new_title: impl Into<String>,
        new_content: impl Into<String>,
        timestamp_ms: u64,
    ) -> PowerResult<NoteRevertPlan> {
        let note = self
            .notes
            .get_mut(&id)
            .ok_or(PowerError::NoteNotFound(id.0))?;

        let revert = NoteRevertPlan::RestoreContent {
            id,
            previous_title: note.title.clone(),
            previous_content: note.content.clone(),
            previous_updated_at: note.updated_at_ms,
        };

        note.title = new_title.into();
        note.content = new_content.into();
        note.updated_at_ms = timestamp_ms;

        Ok(revert)
    }

    pub fn set_pinned(&mut self, id: NoteId, pinned: bool) -> PowerResult<()> {
        let note = self
            .notes
            .get_mut(&id)
            .ok_or(PowerError::NoteNotFound(id.0))?;
        note.pinned = pinned;
        Ok(())
    }

    /// Deletes a note and returns an `Undelete` revert plan.
    pub fn delete_note(&mut self, id: NoteId) -> PowerResult<NoteRevertPlan> {
        let note = self
            .notes
            .remove(&id)
            .ok_or(PowerError::NoteNotFound(id.0))?;
        Ok(NoteRevertPlan::Undelete { note })
    }

    /// Full-text search across titles, content, and tags. Pinned notes rank first.
    pub fn search(&self, query: &str) -> Vec<&Note> {
        let q = query.trim().to_ascii_lowercase();
        let mut results: Vec<&Note> = self
            .notes
            .values()
            .filter(|n| {
                if q.is_empty() {
                    return true;
                }
                n.title.to_ascii_lowercase().contains(&q)
                    || n.content.to_ascii_lowercase().contains(&q)
                    || n.tags.iter().any(|t| t.to_ascii_lowercase().contains(&q))
            })
            .collect();

        // Sort: pinned first, then newest updated_at
        results.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
        });

        results
    }

    pub fn filter_by_tag(&self, tag: &str) -> Vec<&Note> {
        let tag_lower = tag.trim().to_ascii_lowercase();
        let mut results: Vec<&Note> = self
            .notes
            .values()
            .filter(|n| n.tags.iter().any(|t| t.to_ascii_lowercase() == tag_lower))
            .collect();
        results.sort_by_key(|a| std::cmp::Reverse(a.pinned));
        results
    }

    pub fn list_pinned(&self) -> Vec<&Note> {
        let mut pinned: Vec<&Note> = self.notes.values().filter(|n| n.pinned).collect();
        pinned.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
        pinned
    }

    pub fn list_all(&self) -> Vec<&Note> {
        let mut all: Vec<&Note> = self.notes.values().collect();
        all.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
        });
        all
    }
}
