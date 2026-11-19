//! In-memory system used by unit tests and the crash/restart property test.

use std::collections::{BTreeMap, BTreeSet};

use crate::journal::JournalError;
use crate::mutation::{Mutation, PriorityClass, Rect};
use crate::mutator::SystemMutator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeSystem {
    pub priorities: BTreeMap<u32, PriorityClass>,
    pub job_caps: BTreeMap<u32, u32>,
    pub suspended: BTreeSet<u32>,
    pub taskbar_hidden: bool,
    pub appbars: BTreeSet<(u64, u32)>,
    pub timer_100ns: u32,
    pub cloaked: BTreeSet<u64>,
    pub hidden: BTreeSet<u64>,
    pub positions: BTreeMap<u64, Rect>,
    /// `true` means EcoQoS is enabled for that pid.
    pub eco_qos: BTreeMap<u32, bool>,
    pub settings: BTreeMap<String, String>,
}

impl FakeSystem {
    /// A known-good desktop: normal priorities, taskbar visible, 15.6 ms timer,
    /// three windows sitting at a stable rect. Mutations in tests are built
    /// relative to this snapshot so restore must come back to it.
    pub fn clean() -> Self {
        let mut priorities = BTreeMap::new();
        let mut positions = BTreeMap::new();
        let home = Rect {
            x: 10,
            y: 10,
            width: 200,
            height: 100,
        };
        for id in 1..=3 {
            priorities.insert(id, PriorityClass::Normal);
            positions.insert(u64::from(id), home);
        }
        Self {
            priorities,
            job_caps: BTreeMap::new(),
            suspended: BTreeSet::new(),
            taskbar_hidden: false,
            appbars: BTreeSet::new(),
            timer_100ns: 156_250,
            cloaked: BTreeSet::new(),
            hidden: BTreeSet::new(),
            positions,
            eco_qos: BTreeMap::new(),
            settings: BTreeMap::new(),
        }
    }

    pub fn priority_of(&self, pid: u32) -> PriorityClass {
        self.priorities
            .get(&pid)
            .copied()
            .unwrap_or(PriorityClass::Normal)
    }

    pub fn position_of(&self, hwnd: u64) -> Rect {
        self.positions.get(&hwnd).copied().unwrap_or(Rect {
            x: 10,
            y: 10,
            width: 200,
            height: 100,
        })
    }
}

impl SystemMutator for FakeSystem {
    fn apply(&mut self, mutation: &Mutation) -> Result<(), JournalError> {
        match mutation {
            Mutation::ProcessPriority { pid, new, .. } => {
                self.priorities.insert(*pid, *new);
            }
            Mutation::JobCap {
                pid, new_percent, ..
            } => {
                if *new_percent == 0 {
                    self.job_caps.remove(pid);
                } else {
                    self.job_caps.insert(*pid, *new_percent);
                }
            }
            Mutation::Suspend { pid } => {
                self.suspended.insert(*pid);
            }
            Mutation::TaskbarHide => {
                self.taskbar_hidden = true;
            }
            Mutation::AppBar { hwnd, edge, .. } => {
                self.appbars.insert((*hwnd, *edge));
            }
            Mutation::TimerResolution { new_100ns, .. } => {
                self.timer_100ns = *new_100ns;
            }
            Mutation::WindowCloak { hwnd } => {
                self.cloaked.insert(*hwnd);
            }
            Mutation::WindowHide { hwnd } => {
                self.hidden.insert(*hwnd);
            }
            Mutation::WindowPosition { hwnd, new, .. } => {
                self.positions.insert(*hwnd, *new);
            }
            Mutation::EcoQos { pid, enabled, .. } => {
                self.eco_qos.insert(*pid, *enabled);
            }
            Mutation::Setting { key, new, .. } => {
                self.settings.insert(key.clone(), new.clone());
            }
        }
        Ok(())
    }

    fn revert(&mut self, mutation: &Mutation) -> Result<(), JournalError> {
        match mutation {
            Mutation::ProcessPriority { pid, previous, .. } => {
                self.priorities.insert(*pid, *previous);
            }
            Mutation::JobCap {
                pid,
                previous_percent,
                ..
            } => match previous_percent {
                Some(p) if *p > 0 => {
                    self.job_caps.insert(*pid, *p);
                }
                _ => {
                    self.job_caps.remove(pid);
                }
            },
            Mutation::Suspend { pid } => {
                self.suspended.remove(pid);
            }
            Mutation::TaskbarHide => {
                self.taskbar_hidden = false;
            }
            Mutation::AppBar { hwnd, edge, .. } => {
                self.appbars.remove(&(*hwnd, *edge));
            }
            Mutation::TimerResolution { previous_100ns, .. } => {
                self.timer_100ns = *previous_100ns;
            }
            Mutation::WindowCloak { hwnd } => {
                self.cloaked.remove(hwnd);
            }
            Mutation::WindowHide { hwnd } => {
                self.hidden.remove(hwnd);
            }
            Mutation::WindowPosition { hwnd, previous, .. } => {
                self.positions.insert(*hwnd, *previous);
            }
            Mutation::EcoQos {
                pid,
                previous_enabled,
                ..
            } => {
                self.eco_qos.insert(*pid, *previous_enabled);
            }
            Mutation::Setting { key, previous, .. } => {
                self.settings.insert(key.clone(), previous.clone());
            }
        }
        Ok(())
    }
}
