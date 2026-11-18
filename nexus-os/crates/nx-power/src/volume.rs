use crate::error::{PowerError, PowerResult};
use crate::types::PlanStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strongly-typed audio session identifier (e.g. process ID or WASAPI session instance string).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AudioSessionId(pub String);

impl std::fmt::Display for AudioSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An individual application audio session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioSession {
    pub id: AudioSessionId,
    pub process_id: u32,
    pub name: String,
    pub volume: f32, // 0.0..=1.0
    pub is_muted: bool,
    pub peak_meter: f32,
    pub is_system: bool,
}

impl AudioSession {
    pub fn new(
        id: impl Into<String>,
        process_id: u32,
        name: impl Into<String>,
        volume: f32,
        is_muted: bool,
    ) -> Self {
        Self {
            id: AudioSessionId(id.into()),
            process_id,
            name: name.into(),
            volume: volume.clamp(0.0, 1.0),
            is_muted,
            peak_meter: 0.0,
            is_system: false,
        }
    }
}

/// Dry-run report for a volume modification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeDryRun {
    pub session_id: AudioSessionId,
    pub old_volume: f32,
    pub new_volume: f32,
    pub is_no_op: bool,
}

/// Reversible mutation plan for changing volume of an audio session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetVolumePlan {
    pub session_id: AudioSessionId,
    pub old_volume: f32,
    pub new_volume: f32,
    pub status: PlanStatus,
}

impl SetVolumePlan {
    pub fn new(
        store: &VolumeStore,
        session_id: &AudioSessionId,
        new_volume: f32,
    ) -> PowerResult<Self> {
        if !(0.0..=1.0).contains(&new_volume) {
            return Err(PowerError::InvalidVolume(format!("{:.2}", new_volume)));
        }
        let session = store
            .get_session(session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(session_id.to_string()))?;

        Ok(Self {
            session_id: session_id.clone(),
            old_volume: session.volume,
            new_volume,
            status: PlanStatus::DryRun,
        })
    }

    pub fn dry_run(&self) -> VolumeDryRun {
        VolumeDryRun {
            session_id: self.session_id.clone(),
            old_volume: self.old_volume,
            new_volume: self.new_volume,
            is_no_op: (self.old_volume - self.new_volume).abs() < f32::EPSILON,
        }
    }

    pub fn apply(&mut self, store: &mut VolumeStore) -> PowerResult<RevertVolumePlan> {
        let revert = RevertVolumePlan {
            session_id: self.session_id.clone(),
            target_volume: self.old_volume,
        };

        let session = store
            .sessions
            .get_mut(&self.session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(self.session_id.to_string()))?;
        session.volume = self.new_volume;

        self.status = PlanStatus::Applied;
        Ok(revert)
    }
}

/// Reversible undo plan that rolls back volume to its prior level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevertVolumePlan {
    pub session_id: AudioSessionId,
    pub target_volume: f32,
}

impl RevertVolumePlan {
    pub fn revert(&self, store: &mut VolumeStore) -> PowerResult<()> {
        let session = store
            .sessions
            .get_mut(&self.session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(self.session_id.to_string()))?;
        session.volume = self.target_volume;
        Ok(())
    }
}

/// Dry-run report for a mute toggle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MuteDryRun {
    pub session_id: AudioSessionId,
    pub old_mute: bool,
    pub new_mute: bool,
    pub is_no_op: bool,
}

/// Reversible mutation plan for muting or unmuting an audio session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetMutePlan {
    pub session_id: AudioSessionId,
    pub old_mute: bool,
    pub new_mute: bool,
    pub status: PlanStatus,
}

impl SetMutePlan {
    pub fn new(
        store: &VolumeStore,
        session_id: &AudioSessionId,
        new_mute: bool,
    ) -> PowerResult<Self> {
        let session = store
            .get_session(session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(session_id.to_string()))?;

        Ok(Self {
            session_id: session_id.clone(),
            old_mute: session.is_muted,
            new_mute,
            status: PlanStatus::DryRun,
        })
    }

    pub fn dry_run(&self) -> MuteDryRun {
        MuteDryRun {
            session_id: self.session_id.clone(),
            old_mute: self.old_mute,
            new_mute: self.new_mute,
            is_no_op: self.old_mute == self.new_mute,
        }
    }

    pub fn apply(&mut self, store: &mut VolumeStore) -> PowerResult<RevertMutePlan> {
        let revert = RevertMutePlan {
            session_id: self.session_id.clone(),
            target_mute: self.old_mute,
        };

        let session = store
            .sessions
            .get_mut(&self.session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(self.session_id.to_string()))?;
        session.is_muted = self.new_mute;

        self.status = PlanStatus::Applied;
        Ok(revert)
    }
}

/// Reversible undo plan that rolls back mute state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertMutePlan {
    pub session_id: AudioSessionId,
    pub target_mute: bool,
}

impl RevertMutePlan {
    pub fn revert(&self, store: &mut VolumeStore) -> PowerResult<()> {
        let session = store
            .sessions
            .get_mut(&self.session_id)
            .ok_or_else(|| PowerError::AudioSessionNotFound(self.session_id.to_string()))?;
        session.is_muted = self.target_mute;
        Ok(())
    }
}

/// In-memory store of active per-app audio sessions and master audio status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeStore {
    master_volume: f32,
    master_muted: bool,
    sessions: HashMap<AudioSessionId, AudioSession>,
}

impl VolumeStore {
    pub fn new() -> Self {
        Self {
            master_volume: 1.0,
            master_muted: false,
            sessions: HashMap::new(),
        }
    }

    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    pub fn set_master_volume(&mut self, vol: f32) {
        self.master_volume = vol.clamp(0.0, 1.0);
    }

    pub fn is_master_muted(&self) -> bool {
        self.master_muted
    }

    pub fn set_master_muted(&mut self, muted: bool) {
        self.master_muted = muted;
    }

    pub fn upsert_session(&mut self, session: AudioSession) {
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn remove_session(&mut self, id: &AudioSessionId) -> Option<AudioSession> {
        self.sessions.remove(id)
    }

    pub fn get_session(&self, id: &AudioSessionId) -> Option<&AudioSession> {
        self.sessions.get(id)
    }

    pub fn list_sessions(&self) -> Vec<&AudioSession> {
        let mut list: Vec<&AudioSession> = self.sessions.values().collect();
        list.sort_by_key(|s| &s.name);
        list
    }
}

impl Default for VolumeStore {
    fn default() -> Self {
        Self::new()
    }
}
