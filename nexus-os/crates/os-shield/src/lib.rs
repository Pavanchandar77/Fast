//! os-shield: Kernel WFP Telemetry Firewall, ETW behavioral detection, and AppContainer sandbox.

pub mod appcontainer;
pub mod etw;
pub mod wfp;

/// Attach the shield to a running session.
///
/// This build does not install WFP filters. Calling the stub installer would
/// report success without a real filter, which is worse than doing nothing.
pub fn attach(journal_ready: bool, dry_run: bool) {
    tracing::info!(
        journal_ready,
        dry_run,
        "os-shield attached; telemetry filters are not installed by this build"
    );
}
