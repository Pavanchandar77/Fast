//! os-shield: Kernel WFP Telemetry Firewall, ETW behavioral detection, and AppContainer sandbox.

pub mod appcontainer;
pub mod etw;
pub mod wfp;

pub use nx_privacy as privacy;
pub use nx_privacy::shield::{PrivacyShield, PrivacyShieldConfig};
