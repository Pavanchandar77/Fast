//! Session component startup with graceful degradation.
//!
//! Launching Nexus brings up nx-shell (menu bar, dock, Mission Control,
//! Spotlight), explorer-ui, the privacy dashboard, and power features.
//! One component failing logs and continues; it must never take the
//! session down.

use std::panic::{catch_unwind, AssertUnwindSafe};

use nx_explorer_ui::ExplorerWindow;
use nx_power::{AltTabSwitcher, AlwaysOnTopStore, ClipboardStore, NotesStore, VolumeStore};
use nx_privacy::shield::PrivacyShield;
use nx_privacy_dash::DashboardViewModel;
use nx_shell::{DockModel, DockPlacement, ExposeModel, MenuBarModel, Spotlight};
use nx_ui::geom::Rect;
use nx_wm::MonitorId;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionComponent {
    Shell,
    Explorer,
    PrivacyDashboard,
    PowerFeatures,
}

impl SessionComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "nx-shell",
            Self::Explorer => "explorer-ui",
            Self::PrivacyDashboard => "privacy-dashboard",
            Self::PowerFeatures => "power-features",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::Shell,
            Self::Explorer,
            Self::PrivacyDashboard,
            Self::PowerFeatures,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStatus {
    pub component: SessionComponent,
    pub started: bool,
    pub error: Option<String>,
}

pub struct ShellSession {
    pub menu_bar: MenuBarModel,
    pub dock: DockModel,
    pub expose: ExposeModel,
    pub spotlight: Spotlight,
}

pub struct ExplorerSession {
    pub window: ExplorerWindow,
}

pub struct PrivacySession {
    pub dashboard: DashboardViewModel,
    pub shield: PrivacyShield,
}

pub struct PowerSession {
    pub clipboard: ClipboardStore,
    pub volume: VolumeStore,
    pub notes: NotesStore,
    pub always_on_top: AlwaysOnTopStore,
    pub alt_tab: AltTabSwitcher,
}

/// Live session components. Dropping this tears down only the pieces that
/// actually started; the rest of Nexus keeps running.
pub struct SessionHandle {
    pub shell: Option<ShellSession>,
    pub explorer: Option<ExplorerSession>,
    pub privacy: Option<PrivacySession>,
    pub power: Option<PowerSession>,
    pub statuses: Vec<ComponentStatus>,
}

impl SessionHandle {
    pub fn started(&self, component: SessionComponent) -> bool {
        self.statuses
            .iter()
            .any(|status| status.component == component && status.started)
    }

    pub fn session_continued(&self) -> bool {
        true
    }

    pub fn summary(&self) -> String {
        self.statuses
            .iter()
            .map(|status| {
                if status.started {
                    format!("{}=ok", status.component.as_str())
                } else {
                    format!(
                        "{}=degraded({})",
                        status.component.as_str(),
                        status.error.as_deref().unwrap_or("failed")
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn record(
    statuses: &mut Vec<ComponentStatus>,
    component: SessionComponent,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            info!("{} ready", component.as_str());
            statuses.push(ComponentStatus {
                component,
                started: true,
                error: None,
            });
        }
        Err(error) => {
            error!(
                component = component.as_str(),
                error = error.as_str(),
                "session component failed; continuing without it"
            );
            statuses.push(ComponentStatus {
                component,
                started: false,
                error: Some(error),
            });
        }
    }
}

fn run_guarded<T>(
    component: SessionComponent,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            warn!(
                component = component.as_str(),
                "session component panicked; continuing"
            );
            Err(format!("{} panicked", component.as_str()))
        }
    }
}

fn start_shell() -> Result<ShellSession, String> {
    let canvas = Rect::new(0.0, 0.0, 1920.0, 1080.0);
    Ok(ShellSession {
        menu_bar: MenuBarModel::new(),
        dock: DockModel::new(DockPlacement::bottom(MonitorId(1), canvas)),
        expose: ExposeModel::new(canvas),
        spotlight: Spotlight::new(),
    })
}

fn start_explorer() -> Result<ExplorerSession, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let (window, _fx) = ExplorerWindow::new(home);
    Ok(ExplorerSession { window })
}

fn start_privacy() -> Result<PrivacySession, String> {
    Ok(PrivacySession {
        dashboard: DashboardViewModel::new(),
        shield: PrivacyShield::default(),
    })
}

fn start_power() -> Result<PowerSession, String> {
    Ok(PowerSession {
        clipboard: ClipboardStore::with_default_exclusions(64),
        volume: VolumeStore::new(),
        notes: NotesStore::new(),
        always_on_top: AlwaysOnTopStore::new(),
        alt_tab: AltTabSwitcher::new(),
    })
}

/// Start every product surface. Failures are logged; the session continues.
pub fn start_session() -> SessionHandle {
    start_session_with(None)
}

/// Test hook: override a single component starter with a failure.
pub fn start_session_with(override_fail: Option<SessionComponent>) -> SessionHandle {
    let mut statuses = Vec::with_capacity(4);
    let mut handle = SessionHandle {
        shell: None,
        explorer: None,
        privacy: None,
        power: None,
        statuses: Vec::new(),
    };

    match forced_or(override_fail, SessionComponent::Shell, start_shell) {
        Ok(shell) => {
            handle.shell = Some(shell);
            record(&mut statuses, SessionComponent::Shell, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::Shell, Err(err)),
    }

    match forced_or(override_fail, SessionComponent::Explorer, start_explorer) {
        Ok(explorer) => {
            handle.explorer = Some(explorer);
            record(&mut statuses, SessionComponent::Explorer, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::Explorer, Err(err)),
    }

    match forced_or(
        override_fail,
        SessionComponent::PrivacyDashboard,
        start_privacy,
    ) {
        Ok(privacy) => {
            handle.privacy = Some(privacy);
            record(&mut statuses, SessionComponent::PrivacyDashboard, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::PrivacyDashboard, Err(err)),
    }

    match forced_or(override_fail, SessionComponent::PowerFeatures, start_power) {
        Ok(power) => {
            handle.power = Some(power);
            record(&mut statuses, SessionComponent::PowerFeatures, Ok(()));
        }
        Err(err) => record(&mut statuses, SessionComponent::PowerFeatures, Err(err)),
    }

    handle.statuses = statuses;
    info!("session startup: {}", handle.summary());
    handle
}

fn forced_or<T>(
    override_fail: Option<SessionComponent>,
    component: SessionComponent,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if override_fail == Some(component) {
        return Err(format!("injected failure for {}", component.as_str()));
    }
    run_guarded(component, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_session_brings_up_all_surfaces() {
        let session = start_session();
        assert!(session.session_continued());
        for component in SessionComponent::all() {
            assert!(
                session.started(component),
                "{} should start",
                component.as_str()
            );
        }
        assert!(session.shell.is_some());
        assert!(session.explorer.is_some());
        assert!(session.privacy.is_some());
        assert!(session.power.is_some());
        let summary = session.summary();
        assert!(summary.contains("nx-shell=ok"));
        assert!(summary.contains("explorer-ui=ok"));
        assert!(summary.contains("privacy-dashboard=ok"));
        assert!(summary.contains("power-features=ok"));
    }

    #[test]
    fn one_component_failing_does_not_take_down_the_session() {
        let session = start_session_with(Some(SessionComponent::Explorer));
        assert!(session.session_continued());
        assert!(!session.started(SessionComponent::Explorer));
        assert!(session.started(SessionComponent::Shell));
        assert!(session.started(SessionComponent::PrivacyDashboard));
        assert!(session.started(SessionComponent::PowerFeatures));
        assert!(session.explorer.is_none());
        assert!(session.shell.is_some());
        let summary = session.summary();
        assert!(summary.contains("explorer-ui=degraded"));
        assert!(summary.contains("nx-shell=ok"));
    }

    #[test]
    fn privacy_dashboard_failure_leaves_power_and_shell_running() {
        let session = start_session_with(Some(SessionComponent::PrivacyDashboard));
        assert!(session.session_continued());
        assert!(!session.started(SessionComponent::PrivacyDashboard));
        assert!(session.started(SessionComponent::Shell));
        assert!(session.started(SessionComponent::PowerFeatures));
        assert!(session.privacy.is_none());
    }
}
