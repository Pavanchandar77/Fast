//! Small non-blocking notice model and queue for update resilience.
//! Ensures degraded capabilities or re-applied settings alert the user without blocking shell execution.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::probe::Capability;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResilienceNotice {
    pub id: u64,
    pub severity: NoticeSeverity,
    pub capability: Option<Capability>,
    pub title: String,
    pub message: String,
    pub timestamp_secs: u64,
    pub dismissed: bool,
}

impl ResilienceNotice {
    pub fn new(
        id: u64,
        severity: NoticeSeverity,
        capability: Option<Capability>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            id,
            severity,
            capability,
            title: title.into(),
            message: message.into(),
            timestamp_secs,
            dismissed: false,
        }
    }
}

/// Thread-safe queue of non-blocking resilience notices.
#[derive(Debug, Default, Clone)]
pub struct NoticeQueue {
    next_id: Arc<AtomicU64>,
    notices: Arc<RwLock<Vec<ResilienceNotice>>>,
}

impl NoticeQueue {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            notices: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn push(
        &self,
        severity: NoticeSeverity,
        capability: Option<Capability>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let notice = ResilienceNotice::new(id, severity, capability, title, message);
        tracing::info!(
            "Resilience notice [#{}] {:?}: {} - {}",
            id,
            notice.severity,
            notice.title,
            notice.message
        );
        self.notices.write().push(notice);
        id
    }

    pub fn push_warning(
        &self,
        capability: Option<Capability>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> u64 {
        self.push(NoticeSeverity::Warning, capability, title, message)
    }

    pub fn push_info(
        &self,
        capability: Option<Capability>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> u64 {
        self.push(NoticeSeverity::Info, capability, title, message)
    }

    pub fn active(&self) -> Vec<ResilienceNotice> {
        self.notices
            .read()
            .iter()
            .filter(|n| !n.dismissed)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<ResilienceNotice> {
        self.notices.read().clone()
    }

    pub fn dismiss(&self, id: u64) -> bool {
        let mut guard = self.notices.write();
        if let Some(n) = guard.iter_mut().find(|n| n.id == id) {
            n.dismissed = true;
            true
        } else {
            false
        }
    }

    pub fn clear(&self) {
        self.notices.write().clear();
    }

    pub fn count(&self) -> usize {
        self.notices.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notice_queue_lifecycle() {
        let queue = NoticeQueue::new();
        let id1 = queue.push_info(
            Some(Capability::DwmMica),
            "Feature Degraded",
            "Mica downgraded to Acrylic blur",
        );
        let id2 = queue.push_warning(
            Some(Capability::TelemetryPolicies),
            "Settings Re-applied",
            "Restored privacy policies after Windows Update",
        );

        assert_eq!(queue.count(), 2);
        assert_eq!(queue.active().len(), 2);

        assert!(queue.dismiss(id1));
        assert_eq!(queue.active().len(), 1);
        assert_eq!(queue.active()[0].id, id2);

        queue.clear();
        assert_eq!(queue.count(), 0);
    }
}
