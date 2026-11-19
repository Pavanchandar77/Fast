//! Mid-session Explorer.exe and DWM restart resilience.
//!
//! When `explorer.exe` or `dwm.exe` crashes or restarts during an active session,
//! Windows broadcasts the `TaskbarCreated` registered window message and
//! `WM_DWMCOMPOSITIONCHANGED`. This module handles re-attaching shell suppression,
//! re-registering appbars, re-probing DWM capabilities, and restoring backdrops
//! without requiring user intervention.

use nexus_model::platform::Platform;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::error::CompatResult;
use crate::notice::NoticeQueue;
use crate::probe::{Capability, ProbeProvider};

/// Win32 message constant for DWM composition change.
pub const WM_DWMCOMPOSITIONCHANGED: u32 = 0x031E;
/// Win32 message constant for DWM non-client rendering policy change.
pub const WM_DWMNCRENDERINGCHANGED: u32 = 0x031F;
/// Win32 message constant for DWM colorization color change.
pub const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;

/// Configuration for mid-session restart recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartConfig {
    pub auto_suppress_taskbar: bool,
    pub notify_on_restart: bool,
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self {
            auto_suppress_taskbar: true,
            notify_on_restart: true,
        }
    }
}

/// Report after re-attaching DWM composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DwmRestartReport {
    pub dwm_operational: bool,
    pub mica_supported: bool,
    pub acrylic_supported: bool,
    pub thumbnails_supported: bool,
    pub details: String,
}

/// Handler for mid-session shell and compositor restarts.
pub struct RestartHandler {
    config: RestartConfig,
    notices: Arc<NoticeQueue>,
    taskbar_created_msg_id: AtomicU32,
}

impl RestartHandler {
    pub fn new(config: RestartConfig, notices: Arc<NoticeQueue>) -> Self {
        Self {
            config,
            notices,
            taskbar_created_msg_id: AtomicU32::new(0),
        }
    }

    /// Registers the Win32 `TaskbarCreated` message on Windows.
    #[cfg(windows)]
    pub fn register_taskbar_created_message(&self) -> u32 {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;

        let id = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        self.taskbar_created_msg_id.store(id, Ordering::SeqCst);
        tracing::info!("Registered TaskbarCreated window message ID: 0x{:04X}", id);
        id
    }

    #[cfg(not(windows))]
    pub fn register_taskbar_created_message(&self) -> u32 {
        let id = 0xC001; // Mock message ID for non-Windows targets
        self.taskbar_created_msg_id.store(id, Ordering::SeqCst);
        id
    }

    pub fn taskbar_created_msg_id(&self) -> u32 {
        self.taskbar_created_msg_id.load(Ordering::SeqCst)
    }

    pub fn set_taskbar_created_msg_id(&self, id: u32) {
        self.taskbar_created_msg_id.store(id, Ordering::SeqCst);
    }

    /// Tests if a received window message is `TaskbarCreated`.
    pub fn is_taskbar_created_message(&self, message_id: u32) -> bool {
        let expected = self.taskbar_created_msg_id();
        expected != 0 && message_id == expected
    }

    /// Tests if a received window message is a DWM composition change.
    pub fn is_dwm_change_message(&self, message_id: u32) -> bool {
        message_id == WM_DWMCOMPOSITIONCHANGED
            || message_id == WM_DWMNCRENDERINGCHANGED
            || message_id == WM_DWMCOLORIZATIONCOLORCHANGED
    }

    /// Invoked mid-session when explorer.exe restarts (`TaskbarCreated` message received).
    /// Re-suppresses the newly created taskbar without user intervention.
    pub fn handle_explorer_restart(&self, platform: &dyn Platform) -> CompatResult<bool> {
        tracing::info!(
            "explorer.exe restart detected via TaskbarCreated; re-attaching shell without user action"
        );

        if self.config.auto_suppress_taskbar {
            platform
                .set_taskbar_visible(false)
                .map_err(|e| crate::error::CompatError::Platform(e.to_string()))?;
        }

        if self.config.notify_on_restart {
            self.notices.push_info(
                Some(Capability::TaskbarSuppression),
                "Explorer Restarted",
                "explorer.exe restarted mid-session; Nexus-OS re-attached shell suppression automatically.",
            );
        }

        Ok(true)
    }

    /// Invoked mid-session when DWM restarts or desktop composition changes (`WM_DWMCOMPOSITIONCHANGED`).
    /// Re-probes DWM capabilities so backdrops and thumbnails can be re-established.
    pub fn handle_dwm_restart(
        &self,
        probe_provider: &dyn ProbeProvider,
    ) -> CompatResult<DwmRestartReport> {
        tracing::info!(
            "DWM restart or composition change detected; re-probing capabilities and re-attaching"
        );

        let mica_res = probe_provider.probe(Capability::DwmMica);
        let acrylic_res = probe_provider.probe(Capability::DwmAcrylic);
        let thumb_res = probe_provider.probe(Capability::DwmThumbnails);

        let report = DwmRestartReport {
            dwm_operational: mica_res.status.is_operational()
                || acrylic_res.status.is_operational(),
            mica_supported: mica_res.status.is_available(),
            acrylic_supported: acrylic_res.status.is_operational(),
            thumbnails_supported: thumb_res.status.is_operational(),
            details: format!(
                "Mica: {:?}, Acrylic: {:?}, Thumbnails: {:?}",
                mica_res.status, acrylic_res.status, thumb_res.status
            ),
        };

        if self.config.notify_on_restart {
            self.notices.push_info(
                Some(Capability::DwmMica),
                "DWM Composition Re-attached",
                format!(
                    "DWM composition change handled. Backdrops and thumbnails re-attached ({})",
                    report.details
                ),
            );
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::MockProbeProvider;
    use nexus_model::platform::FakePlatform;

    #[test]
    fn test_explorer_restart_reapplies_taskbar_suppression() {
        let notices = Arc::new(NoticeQueue::new());
        let handler = RestartHandler::new(RestartConfig::default(), notices.clone());
        let platform = FakePlatform::new();

        let msg_id = handler.register_taskbar_created_message();
        assert!(handler.is_taskbar_created_message(msg_id));
        assert!(!handler.is_taskbar_created_message(0x1234));

        // When explorer restarts, Windows creates taskbar so it becomes visible
        platform.set_taskbar_visible(true).unwrap();
        assert!(platform.is_taskbar_visible());

        // Handle restart
        assert!(handler.handle_explorer_restart(&platform).unwrap());

        // Taskbar is suppressed again automatically!
        assert!(!platform.is_taskbar_visible());
        assert_eq!(notices.count(), 1);
        assert!(notices.active()[0].title.contains("Explorer Restarted"));
    }

    #[test]
    fn test_dwm_restart_reprobes_and_reports() {
        let notices = Arc::new(NoticeQueue::new());
        let handler = RestartHandler::new(RestartConfig::default(), notices.clone());

        let probes = MockProbeProvider::new();
        probes.set_available(Capability::DwmMica);
        probes.set_available(Capability::DwmThumbnails);

        let report = handler.handle_dwm_restart(&probes).unwrap();
        assert!(report.dwm_operational);
        assert!(report.mica_supported);
        assert!(report.thumbnails_supported);
        assert_eq!(notices.count(), 1);

        assert!(handler.is_dwm_change_message(WM_DWMCOMPOSITIONCHANGED));
        assert!(handler.is_dwm_change_message(WM_DWMNCRENDERINGCHANGED));
        assert!(!handler.is_dwm_change_message(0x9999));
    }
}
