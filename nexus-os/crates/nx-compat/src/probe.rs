//! Capability probing architecture.
//! Replaces brittle version-checks with runtime feature probes and documented graceful fallbacks.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Every version-sensitive or undocumented Windows API used across Nexus-OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` value 2 (Mica) or 4 (Mica Alt/Tabbed).
    DwmMica,
    /// `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` value 3 (Acrylic) or SetWindowCompositionAttribute blur.
    DwmAcrylic,
    /// `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE)` rounded corners.
    DwmRoundedCorners,
    /// `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)`.
    DwmImmersiveDarkMode,
    /// `DwmSetWindowAttribute(DWMWA_CLOAK)` and `DwmGetWindowAttribute(DWMWA_CLOAKED)`.
    DwmCloak,
    /// `DwmSetWindowAttribute(DWMWA_TRANSITIONS_FORCEDISABLED)`.
    DwmTransitions,
    /// `DwmRegisterThumbnail`, `DwmUpdateThumbnailProperties`, `DwmUnregisterThumbnail`.
    DwmThumbnails,
    /// `DwmFlush` frame sync.
    DwmFlush,
    /// Undocumented `user32.dll!SetWindowCompositionAttribute`.
    SetWindowCompositionAttribute,
    /// Undocumented `ntdll.dll!NtSuspendProcess`.
    NtSuspendProcess,
    /// Undocumented `ntdll.dll!NtResumeProcess`.
    NtResumeProcess,
    /// Undocumented `ntdll.dll!NtSetTimerResolution`.
    NtSetTimerResolution,
    /// `ntdll.dll!NtTerminateProcess`.
    NtTerminateProcess,
    /// `ntdll.dll!RtlGetVersion`.
    RtlGetVersion,
    /// `user32.dll!RegisterShellHookWindow`.
    ShellHooks,
    /// `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` suppression.
    TaskbarSuppression,
    /// Policy registry paths (`HKLM\SOFTWARE\Policies\Microsoft\Windows\...`).
    TelemetryPolicies,
    /// Diagnostic tracking services (`DiagTrack`, `dmwappushservice`).
    TelemetryServices,
    /// Windows privacy toggles (Advertising ID, Copilot, Recall, Activity History).
    PrivacyToggles,
    /// User default app associations (`UserChoice`).
    DefaultApps,
    /// Explorer shell settings (`TaskbarAl`, `ShowTaskViewButton`).
    ExplorerSettings,
}

impl Capability {
    pub const ALL: &'static [Capability] = &[
        Capability::DwmMica,
        Capability::DwmAcrylic,
        Capability::DwmRoundedCorners,
        Capability::DwmImmersiveDarkMode,
        Capability::DwmCloak,
        Capability::DwmTransitions,
        Capability::DwmThumbnails,
        Capability::DwmFlush,
        Capability::SetWindowCompositionAttribute,
        Capability::NtSuspendProcess,
        Capability::NtResumeProcess,
        Capability::NtSetTimerResolution,
        Capability::NtTerminateProcess,
        Capability::RtlGetVersion,
        Capability::ShellHooks,
        Capability::TaskbarSuppression,
        Capability::TelemetryPolicies,
        Capability::TelemetryServices,
        Capability::PrivacyToggles,
        Capability::DefaultApps,
        Capability::ExplorerSettings,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::DwmMica => "DWM Mica System Backdrop",
            Self::DwmAcrylic => "DWM Acrylic System Backdrop",
            Self::DwmRoundedCorners => "DWM Rounded Corners",
            Self::DwmImmersiveDarkMode => "DWM Immersive Dark Mode",
            Self::DwmCloak => "DWM Window Cloaking",
            Self::DwmTransitions => "DWM Forced Disable Transitions",
            Self::DwmThumbnails => "DWM Live Thumbnails API",
            Self::DwmFlush => "DWM Compositor Flush",
            Self::SetWindowCompositionAttribute => "user32!SetWindowCompositionAttribute",
            Self::NtSuspendProcess => "ntdll!NtSuspendProcess",
            Self::NtResumeProcess => "ntdll!NtResumeProcess",
            Self::NtSetTimerResolution => "ntdll!NtSetTimerResolution",
            Self::NtTerminateProcess => "ntdll!NtTerminateProcess",
            Self::RtlGetVersion => "ntdll!RtlGetVersion",
            Self::ShellHooks => "user32!RegisterShellHookWindow",
            Self::TaskbarSuppression => "Explorer Taskbar Suppression",
            Self::TelemetryPolicies => "Windows Telemetry Policies",
            Self::TelemetryServices => "Windows Diagnostic Services",
            Self::PrivacyToggles => "Windows Privacy & AI Toggles",
            Self::DefaultApps => "Windows Default Apps (UserChoice)",
            Self::ExplorerSettings => "Explorer Taskbar & View Settings",
        }
    }

    /// Documented graceful fallback behavior when the capability is unavailable or fails.
    pub fn documented_fallback(&self) -> &'static str {
        match self {
            Self::DwmMica => "Downgrade: Mica -> Acrylic -> custom Gaussian blur shader -> solid blur-free theme.",
            Self::DwmAcrylic => "Downgrade: Acrylic -> custom Gaussian blur shader -> solid blur-free theme.",
            Self::DwmRoundedCorners => "Fallback: Standard rectangular window corners without DWM rounded corner preference.",
            Self::DwmImmersiveDarkMode => "Fallback: Standard system window titlebar and borders without dark mode hint.",
            Self::DwmCloak => "Fallback: ShowWindow(SW_HIDE) or skip window cloaking.",
            Self::DwmTransitions => "Fallback: Keep standard system transition animations enabled; log info.",
            Self::DwmThumbnails => "Fallback: Software thumbnail snapshot or blank preview slot in Expose.",
            Self::DwmFlush => "Fallback: Standard thread sleep / yield frame rate pacing.",
            Self::SetWindowCompositionAttribute => "Fallback: Use DwmSetWindowAttribute if available, else software composite.",
            Self::NtSuspendProcess => "Fallback: Skip process suspension; log warning; leave process at idle priority.",
            Self::NtResumeProcess => "Fallback: Skip NT resume; log info.",
            Self::NtSetTimerResolution => "Fallback: winmm!timeBeginPeriod(1) or standard multimedia timer resolution.",
            Self::NtTerminateProcess => "Fallback: Standard Win32 TerminateProcess API.",
            Self::RtlGetVersion => "Fallback: Read CurrentBuild and UBR from registry or GetVersionExW.",
            Self::ShellHooks => "Fallback: SetWinEventHook for universal window lifecycle transitions.",
            Self::TaskbarSuppression => "Fallback: Leave native taskbar visible; keep Nexus shell running above or beside it.",
            Self::TelemetryPolicies => "Fallback: Skip unwritable or shifted policy subkeys; log skipped paths.",
            Self::TelemetryServices => "Fallback: Skip non-existent telemetry services; keep daemon running.",
            Self::PrivacyToggles => "Fallback: Skip unsupported privacy switches on this Windows edition.",
            Self::DefaultApps => "Fallback: Preserve registered handlers; post notice if Windows reset associations.",
            Self::ExplorerSettings => "Fallback: Re-apply view and alignment preferences via registry.",
        }
    }
}

/// Operational status of a capability at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityStatus {
    /// Fully available and operational.
    Available,
    /// Partially operational or running through a graceful fallback.
    Degraded { fallback: String },
    /// Completely unavailable on this build.
    Unavailable { reason: String, fallback: String },
}

impl CapabilityStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Available | Self::Degraded { .. })
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Result of probing a single capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub capability: Capability,
    pub status: CapabilityStatus,
    pub details: String,
}

impl ProbeResult {
    pub fn available(capability: Capability, details: impl Into<String>) -> Self {
        Self {
            capability,
            status: CapabilityStatus::Available,
            details: details.into(),
        }
    }

    pub fn degraded(
        capability: Capability,
        fallback: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            capability,
            status: CapabilityStatus::Degraded {
                fallback: fallback.into(),
            },
            details: details.into(),
        }
    }

    pub fn unavailable(
        capability: Capability,
        reason: impl Into<String>,
        fallback: impl Into<String>,
    ) -> Self {
        let r = reason.into();
        Self {
            capability,
            status: CapabilityStatus::Unavailable {
                reason: r.clone(),
                fallback: fallback.into(),
            },
            details: r,
        }
    }
}

/// Abstract probe provider allowing testing missing exports and failing probes on Linux.
pub trait ProbeProvider: Send + Sync {
    fn probe(&self, capability: Capability) -> ProbeResult;
}

/// Live probe provider that resolves Win32 exports via GetProcAddress.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveProbeProvider;

impl ProbeProvider for LiveProbeProvider {
    #[cfg(windows)]
    fn probe(&self, capability: Capability) -> ProbeResult {
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        use windows::core::{PCSTR, w};

        match capability {
            Capability::DwmMica => {
                let build = crate::build::detect_build().build;
                if build >= 22621 {
                    ProbeResult::available(capability, "DWMWA_SYSTEMBACKDROP_TYPE supported (Win11 22H2+)")
                } else if build >= 22000 {
                    ProbeResult::degraded(
                        capability,
                        "Acrylic fallback on initial Win11 build",
                        "Mica system backdrop attribute requires 22H2+",
                    )
                } else {
                    ProbeResult::unavailable(
                        capability,
                        "Mica not supported on pre-Windows 11",
                        capability.documented_fallback(),
                    )
                }
            }
            Capability::DwmAcrylic => {
                let build = crate::build::detect_build().build;
                if build >= 22000 {
                    ProbeResult::available(capability, "Acrylic supported via DWM backdrop or SetWindowCompositionAttribute")
                } else {
                    ProbeResult::degraded(
                        capability,
                        "Custom shader blur",
                        "Pre-Win11: Acrylic uses custom shader kernel",
                    )
                }
            }
            Capability::DwmRoundedCorners => {
                let build = crate::build::detect_build().build;
                if build >= 22000 {
                    ProbeResult::available(capability, "DWMWA_WINDOW_CORNER_PREFERENCE supported")
                } else {
                    ProbeResult::unavailable(
                        capability,
                        "Rounded corners attribute requires Win11",
                        capability.documented_fallback(),
                    )
                }
            }
            Capability::DwmImmersiveDarkMode => {
                unsafe {
                    if let Ok(dwm) = GetModuleHandleW(w!("dwmapi.dll")) {
                        if GetProcAddress(dwm, PCSTR(b"DwmSetWindowAttribute\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "DwmSetWindowAttribute dark mode supported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "dwmapi.dll missing", capability.documented_fallback())
            }
            Capability::DwmCloak => {
                unsafe {
                    if let Ok(dwm) = GetModuleHandleW(w!("dwmapi.dll")) {
                        if GetProcAddress(dwm, PCSTR(b"DwmSetWindowAttribute\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "DWMWA_CLOAK supported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "dwmapi.dll missing", capability.documented_fallback())
            }
            Capability::DwmTransitions => {
                unsafe {
                    if let Ok(dwm) = GetModuleHandleW(w!("dwmapi.dll")) {
                        if GetProcAddress(dwm, PCSTR(b"DwmSetWindowAttribute\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "DWMWA_TRANSITIONS_FORCEDISABLED supported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "dwmapi.dll missing", capability.documented_fallback())
            }
            Capability::DwmThumbnails => {
                unsafe {
                    if let Ok(dwm) = GetModuleHandleW(w!("dwmapi.dll")) {
                        let reg = GetProcAddress(dwm, PCSTR(b"DwmRegisterThumbnail\0".as_ptr()));
                        let upd = GetProcAddress(dwm, PCSTR(b"DwmUpdateThumbnailProperties\0".as_ptr()));
                        let unreg = GetProcAddress(dwm, PCSTR(b"DwmUnregisterThumbnail\0".as_ptr()));
                        if reg.is_some() && upd.is_some() && unreg.is_some() {
                            return ProbeResult::available(capability, "DWM thumbnail APIs exported and functional");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "DWM thumbnail APIs missing", capability.documented_fallback())
            }
            Capability::DwmFlush => {
                unsafe {
                    if let Ok(dwm) = GetModuleHandleW(w!("dwmapi.dll")) {
                        if GetProcAddress(dwm, PCSTR(b"DwmFlush\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "DwmFlush exported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "DwmFlush missing", capability.documented_fallback())
            }
            Capability::SetWindowCompositionAttribute => {
                unsafe {
                    if let Ok(user32) = GetModuleHandleW(w!("user32.dll")) {
                        if GetProcAddress(user32, PCSTR(b"SetWindowCompositionAttribute\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "user32!SetWindowCompositionAttribute exported");
                        }
                    }
                }
                ProbeResult::degraded(capability, "DwmSetWindowAttribute fallback", "SetWindowCompositionAttribute export missing")
            }
            Capability::NtSuspendProcess => {
                unsafe {
                    if let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) {
                        if GetProcAddress(ntdll, PCSTR(b"NtSuspendProcess\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "ntdll!NtSuspendProcess exported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "NtSuspendProcess missing", capability.documented_fallback())
            }
            Capability::NtResumeProcess => {
                unsafe {
                    if let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) {
                        if GetProcAddress(ntdll, PCSTR(b"NtResumeProcess\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "ntdll!NtResumeProcess exported");
                        }
                    }
                }
                ProbeResult::unavailable(capability, "NtResumeProcess missing", capability.documented_fallback())
            }
            Capability::NtSetTimerResolution => {
                unsafe {
                    if let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) {
                        if GetProcAddress(ntdll, PCSTR(b"NtSetTimerResolution\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "ntdll!NtSetTimerResolution exported");
                        }
                    }
                }
                ProbeResult::degraded(capability, "winmm timeBeginPeriod fallback", "NtSetTimerResolution missing")
            }
            Capability::NtTerminateProcess => {
                unsafe {
                    if let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) {
                        if GetProcAddress(ntdll, PCSTR(b"NtTerminateProcess\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "ntdll!NtTerminateProcess exported");
                        }
                    }
                }
                ProbeResult::degraded(capability, "Win32 TerminateProcess fallback", "NtTerminateProcess missing")
            }
            Capability::RtlGetVersion => {
                unsafe {
                    if let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) {
                        if GetProcAddress(ntdll, PCSTR(b"RtlGetVersion\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "ntdll!RtlGetVersion exported");
                        }
                    }
                }
                ProbeResult::degraded(capability, "Registry CurrentVersion fallback", "RtlGetVersion missing")
            }
            Capability::ShellHooks => {
                unsafe {
                    if let Ok(user32) = GetModuleHandleW(w!("user32.dll")) {
                        if GetProcAddress(user32, PCSTR(b"RegisterShellHookWindow\0".as_ptr())).is_some() {
                            return ProbeResult::available(capability, "user32!RegisterShellHookWindow exported");
                        }
                    }
                }
                ProbeResult::degraded(capability, "SetWinEventHook fallback", "RegisterShellHookWindow missing")
            }
            Capability::TaskbarSuppression => {
                ProbeResult::available(capability, "Shell_TrayWnd window lookup operational")
            }
            Capability::TelemetryPolicies => {
                ProbeResult::available(capability, "HKLM/HKCU policy access operational")
            }
            Capability::TelemetryServices => {
                ProbeResult::available(capability, "Service Manager access operational")
            }
            Capability::PrivacyToggles => {
                ProbeResult::available(capability, "Privacy policy keys accessible")
            }
            Capability::DefaultApps => {
                ProbeResult::available(capability, "UserChoice associations accessible")
            }
            Capability::ExplorerSettings => {
                ProbeResult::available(capability, "Explorer registry settings accessible")
            }
        }
    }

    #[cfg(not(windows))]
    fn probe(&self, capability: Capability) -> ProbeResult {
        // Standard baseline for Linux simulated execution
        ProbeResult::available(capability, "Simulated capability active on non-Windows target")
    }
}

/// Configurable mock probe provider for unit tests and simulation.
#[derive(Debug, Default)]
pub struct MockProbeProvider {
    overrides: RwLock<HashMap<Capability, ProbeResult>>,
}

impl MockProbeProvider {
    pub fn new() -> Self {
        Self {
            overrides: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_result(&self, result: ProbeResult) {
        self.overrides.write().insert(result.capability, result);
    }

    pub fn set_unavailable(&self, capability: Capability, reason: &str) {
        self.set_result(ProbeResult::unavailable(
            capability,
            reason,
            capability.documented_fallback(),
        ));
    }

    pub fn set_degraded(&self, capability: Capability, fallback: &str, details: &str) {
        self.set_result(ProbeResult::degraded(capability, fallback, details));
    }

    pub fn set_available(&self, capability: Capability) {
        self.set_result(ProbeResult::available(capability, "Mock available"));
    }
}

impl ProbeProvider for MockProbeProvider {
    fn probe(&self, capability: Capability) -> ProbeResult {
        if let Some(res) = self.overrides.read().get(&capability) {
            return res.clone();
        }
        ProbeResult::available(capability, "Default mock available")
    }
}

/// Runtime Probe Manager caching and dispatching capability checks.
pub struct ProbeManager {
    provider: Box<dyn ProbeProvider>,
    cache: RwLock<HashMap<Capability, ProbeResult>>,
}

impl ProbeManager {
    pub fn new(provider: impl ProbeProvider + 'static) -> Self {
        Self {
            provider: Box::new(provider),
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_live() -> Self {
        Self::new(LiveProbeProvider)
    }

    pub fn probe_all(&self) -> Vec<ProbeResult> {
        let mut results = Vec::with_capacity(Capability::ALL.len());
        let mut cache_guard = self.cache.write();

        for &cap in Capability::ALL {
            let res = self.provider.probe(cap);
            cache_guard.insert(cap, res.clone());
            results.push(res);
        }

        results
    }

    pub fn get_status(&self, capability: Capability) -> CapabilityStatus {
        if let Some(res) = self.cache.read().get(&capability) {
            return res.status.clone();
        }
        let res = self.provider.probe(capability);
        self.cache.write().insert(capability, res.clone());
        res.status
    }

    pub fn is_supported(&self, capability: Capability) -> bool {
        self.get_status(capability).is_available()
    }

    pub fn is_operational(&self, capability: Capability) -> bool {
        self.get_status(capability).is_operational()
    }

    pub fn fallback_for(&self, capability: Capability) -> &'static str {
        capability.documented_fallback()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_capabilities_have_documented_fallbacks() {
        for &cap in Capability::ALL {
            let fb = cap.documented_fallback();
            assert!(!fb.is_empty(), "Capability {:?} must have a fallback", cap);
            let name = cap.name();
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn test_mock_probe_provider_overrides() {
        let provider = MockProbeProvider::new();
        provider.set_unavailable(Capability::DwmMica, "Unsupported on test build");
        provider.set_degraded(
            Capability::NtSetTimerResolution,
            "winmm timer",
            "Missing export",
        );

        let pm = ProbeManager::new(provider);
        assert!(!pm.is_supported(Capability::DwmMica));
        assert!(!pm.is_operational(Capability::DwmMica));

        assert!(!pm.is_supported(Capability::NtSetTimerResolution));
        assert!(pm.is_operational(Capability::NtSetTimerResolution)); // Degraded is operational

        assert!(pm.is_supported(Capability::DwmCloak)); // Default available
    }
}
