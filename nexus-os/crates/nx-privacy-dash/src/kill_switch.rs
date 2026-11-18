//! Pure data model for kill-switch requests.
//!
//! Kill-switch requests represent intents (e.g. camera master mute, telemetry kill,
//! tracker block) as reversible, dry-run-able command objects. The actual OS mutations
//! (firewall rules, services, registry policies) are executed by `nx-privacy` on the
//! shield leap branch.

use crate::permissions::DryRunPlan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Master kill-switch categories available in Nexus-OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum KillSwitchKind {
    /// Hardware/policy disable for all system webcams.
    CameraMaster,
    /// Hardware/subsystem mute for all microphones.
    MicrophoneMaster,
    /// System-wide location services kill switch.
    LocationMaster,
    /// Network-wide ad & telemetry tracker domain blocker.
    TrackerBlocker,
    /// Windows DiagTrack, dmwappushservice, and CEIP telemetry shield.
    TelemetryShield,
    /// Windows 11 Recall & Copilot background snapshot shield.
    CopilotRecallShield,
}

impl KillSwitchKind {
    pub fn all() -> [Self; 6] {
        [
            Self::CameraMaster,
            Self::MicrophoneMaster,
            Self::LocationMaster,
            Self::TrackerBlocker,
            Self::TelemetryShield,
            Self::CopilotRecallShield,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CameraMaster => "Camera Master Kill-Switch",
            Self::MicrophoneMaster => "Microphone Master Kill-Switch",
            Self::LocationMaster => "Location Privacy Guard",
            Self::TrackerBlocker => "Tracker & Ad Blocker",
            Self::TelemetryShield => "Windows Telemetry Shield",
            Self::CopilotRecallShield => "Recall & AI Snapshot Shield",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::CameraMaster => "Instantly cut off hardware access to all video capture devices.",
            Self::MicrophoneMaster => {
                "Instantly mute and disable audio input capture across all apps."
            }
            Self::LocationMaster => "Block GPS, Wi-Fi triangulation, and IP geolocation queries.",
            Self::TrackerBlocker => "Block known tracking, ad, and analytics domains via DNS/WFP.",
            Self::TelemetryShield => {
                "Reversibly disable DiagTrack, Advertising ID, and CEIP metrics."
            }
            Self::CopilotRecallShield => {
                "Prevent Windows Recall from capturing screen activity and AI indexers."
            }
        }
    }
}

impl fmt::Display for KillSwitchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// A reversible dry-run-able kill-switch command object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillSwitchRequest {
    pub request_id: u64,
    pub kind: KillSwitchKind,
    pub enable: bool,
    pub requested_by: String,
    pub timestamp_ms: u64,
}

impl KillSwitchRequest {
    /// Create the inverse request to revert this kill switch operation.
    pub fn reverse(&self) -> Self {
        Self {
            request_id: self.request_id.wrapping_add(1),
            kind: self.kind,
            enable: !self.enable,
            requested_by: format!("Revert: {}", self.requested_by),
            timestamp_ms: self.timestamp_ms,
        }
    }

    /// Produce a dry-run execution plan without touching system state.
    pub fn dry_run(&self) -> DryRunPlan {
        let action = if self.enable { "Engage" } else { "Disengage" };
        let (target_key, old_v, new_v) = match self.kind {
            KillSwitchKind::CameraMaster => (
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy\\LetAppsAccessCamera",
                if self.enable { "Allow" } else { "Deny" },
                if self.enable { "Deny" } else { "Allow" },
            ),
            KillSwitchKind::MicrophoneMaster => (
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy\\LetAppsAccessMicrophone",
                if self.enable { "Allow" } else { "Deny" },
                if self.enable { "Deny" } else { "Allow" },
            ),
            KillSwitchKind::LocationMaster => (
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\LocationAndSensors\\DisableLocation",
                if self.enable { "0" } else { "1" },
                if self.enable { "1" } else { "0" },
            ),
            KillSwitchKind::TrackerBlocker => (
                "Nexus::Shield\\Wfp\\TrackerFilters",
                if self.enable { "0 rules" } else { "Active" },
                if self.enable { "Active" } else { "0 rules" },
            ),
            KillSwitchKind::TelemetryShield => (
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection\\AllowTelemetry",
                if self.enable {
                    "Full (3)"
                } else {
                    "Security (0)"
                },
                if self.enable {
                    "Security (0)"
                } else {
                    "Full (3)"
                },
            ),
            KillSwitchKind::CopilotRecallShield => (
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsAI\\DisableAIDataAnalysis",
                if self.enable { "0" } else { "1" },
                if self.enable { "1" } else { "0" },
            ),
        };

        DryRunPlan {
            target_description: format!("{} {}", action, self.kind.display_name()),
            target_registry_key: Some(target_key.into()),
            old_value: old_v.into(),
            new_value: new_v.into(),
            is_reversible: true,
            safety_notes: "Pure intent command. Delivered to nx-privacy engine for execution with backup journal entry.".into(),
        }
    }
}

/// In-memory store for kill-switch status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillSwitchStore {
    states: BTreeMap<KillSwitchKind, bool>,
    history: Vec<KillSwitchRequest>,
    next_id: u64,
}

impl Default for KillSwitchStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KillSwitchStore {
    pub fn new() -> Self {
        let mut states = BTreeMap::new();
        for kind in KillSwitchKind::all() {
            states.insert(kind, false);
        }

        Self {
            states,
            history: Vec::new(),
            next_id: 1,
        }
    }

    /// Whether a specific kill switch is engaged.
    pub fn is_engaged(&self, kind: KillSwitchKind) -> bool {
        self.states.get(&kind).copied().unwrap_or(false)
    }

    /// Create a request to set the kill switch state.
    pub fn create_request(
        &mut self,
        kind: KillSwitchKind,
        enable: bool,
        requested_by: &str,
        now_ms: u64,
    ) -> KillSwitchRequest {
        let req = KillSwitchRequest {
            request_id: self.next_id,
            kind,
            enable,
            requested_by: requested_by.to_string(),
            timestamp_ms: now_ms,
        };
        self.next_id += 1;
        req
    }

    /// Apply a request to the in-memory state model (mutates pure state only).
    pub fn apply_request(&mut self, req: &KillSwitchRequest) {
        self.states.insert(req.kind, req.enable);
        self.history.push(req.clone());
    }

    /// All states.
    pub fn all_states(&self) -> &BTreeMap<KillSwitchKind, bool> {
        &self.states
    }

    /// Request history.
    pub fn history(&self) -> &[KillSwitchRequest] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_request_reversal_and_dry_run() {
        let mut store = KillSwitchStore::new();
        assert!(!store.is_engaged(KillSwitchKind::CameraMaster));

        // Create engage request
        let req = store.create_request(KillSwitchKind::CameraMaster, true, "User Toggle", 12345);
        assert!(req.enable);
        assert_eq!(req.kind, KillSwitchKind::CameraMaster);

        // Dry run verification
        let plan = req.dry_run();
        assert!(plan.is_reversible);
        assert!(plan.target_description.contains("Engage"));
        assert!(plan
            .target_registry_key
            .unwrap()
            .contains("LetAppsAccessCamera"));

        // Apply request to in-memory store
        store.apply_request(&req);
        assert!(store.is_engaged(KillSwitchKind::CameraMaster));

        // Reversal
        let rev = req.reverse();
        assert!(!rev.enable);
        assert_eq!(rev.kind, KillSwitchKind::CameraMaster);
        store.apply_request(&rev);
        assert!(!store.is_engaged(KillSwitchKind::CameraMaster));
    }

    #[test]
    fn all_kill_switch_descriptions() {
        for kind in KillSwitchKind::all() {
            assert!(!kind.display_name().is_empty());
            assert!(!kind.description().is_empty());
            let req = KillSwitchRequest {
                request_id: 1,
                kind,
                enable: true,
                requested_by: "Test".into(),
                timestamp_ms: 0,
            };
            let plan = req.dry_run();
            assert!(plan.is_reversible);
            assert!(plan.target_registry_key.is_some());
        }
    }
}
