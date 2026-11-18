//! Data structures representing Windows CapabilityAccessManager ConsentStore.

use crate::filetime::FileTime;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported capability types monitored in ConsentStore.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CapabilityKind {
    Webcam,
    Microphone,
    Location,
    Custom(String),
}

impl CapabilityKind {
    /// Parse capability string from registry key name (case-insensitive).
    pub fn from_key_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "webcam" | "camera" => Self::Webcam,
            "microphone" | "mic" => Self::Microphone,
            "location" => Self::Location,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Registry key name used in Windows ConsentStore.
    pub fn registry_key_name(&self) -> &'static str {
        match self {
            Self::Webcam => "webcam",
            Self::Microphone => "microphone",
            Self::Location => "location",
            Self::Custom(_) => "custom",
        }
    }

    /// User-friendly display title.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Webcam => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::Custom(_) => "Other Capability",
        }
    }
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// App identification for packaged UWP apps and non-packaged desktop Win32 applications.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AppIdentifier {
    /// Packaged Store / UWP / MSIX application.
    Packaged {
        package_family_name: String,
        display_name: String,
    },
    /// Non-packaged Win32 desktop application.
    NonPackaged {
        raw_key: String,
        executable_path: String,
        binary_name: String,
        display_name: String,
    },
}

impl AppIdentifier {
    /// Construct from packaged family name, e.g. `Microsoft.WindowsCamera_8wekyb3d8bbwe`.
    pub fn from_package_family_name(pfn: &str) -> Self {
        let name_part = pfn.split('_').next().unwrap_or(pfn);
        let display_name = name_part
            .rsplit('.')
            .next()
            .unwrap_or(name_part)
            .to_string();

        Self::Packaged {
            package_family_name: pfn.to_string(),
            display_name,
        }
    }

    /// Construct from non-packaged registry key (where path separators are `#`).
    /// E.g. `C:#Program Files#Zoom#bin#Zoom.exe` -> `C:\Program Files\Zoom\bin\Zoom.exe`.
    pub fn from_non_packaged_key(raw_key: &str) -> Self {
        let executable_path = raw_key.replace('#', "\\");
        let binary_name = executable_path
            .rsplit('\\')
            .next()
            .unwrap_or(&executable_path)
            .to_string();

        let display_name = if let Some(stem) = binary_name.strip_suffix(".exe") {
            stem.to_string()
        } else {
            binary_name.clone()
        };

        Self::NonPackaged {
            raw_key: raw_key.to_string(),
            executable_path,
            binary_name,
            display_name,
        }
    }

    /// Construct desktop app with explicit path.
    pub fn from_desktop_path(path: &str) -> Self {
        let raw_key = path.replace('\\', "#");
        Self::from_non_packaged_key(&raw_key)
    }

    /// User-visible friendly display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Packaged { display_name, .. } => display_name.as_str(),
            Self::NonPackaged { display_name, .. } => display_name.as_str(),
        }
    }

    /// Executable path or package identifier.
    pub fn identifier(&self) -> &str {
        match self {
            Self::Packaged {
                package_family_name,
                ..
            } => package_family_name.as_str(),
            Self::NonPackaged {
                executable_path, ..
            } => executable_path.as_str(),
        }
    }

    /// Whether this represents a modern packaged app.
    pub fn is_packaged(&self) -> bool {
        matches!(self, Self::Packaged { .. })
    }
}

impl fmt::Display for AppIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Permission state recorded in ConsentStore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConsentStatus {
    Allow,
    Deny,
    Prompt,
    Unspecified,
}

impl ConsentStatus {
    pub fn from_value_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            "prompt" => Self::Prompt,
            _ => Self::Unspecified,
        }
    }

    pub fn to_value_str(&self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Prompt => "Prompt",
            Self::Unspecified => "",
        }
    }
}

/// A parsed record of app capability access from CapabilityAccessManager ConsentStore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub app_id: AppIdentifier,
    pub capability: CapabilityKind,
    pub status: ConsentStatus,
    pub last_used_start: Option<FileTime>,
    pub last_used_stop: Option<FileTime>,
    pub attribution: Option<String>,
}

impl ConsentRecord {
    /// Check if this app is currently actively using the capability.
    pub fn is_currently_active(&self) -> bool {
        match self.last_used_start {
            Some(start) if !start.is_zero() => start.is_currently_active(self.last_used_stop),
            _ => false,
        }
    }

    /// Active or last session duration in milliseconds.
    pub fn session_duration_ms(&self, now_ms: u64) -> u64 {
        match self.last_used_start {
            Some(start) => start.duration_ms(self.last_used_stop, now_ms),
            None => 0,
        }
    }

    /// Check if access was finished recently within `threshold_ms` of `now_ms`.
    pub fn was_recently_used(&self, now_ms: u64, threshold_ms: u64) -> bool {
        if self.is_currently_active() {
            return false;
        }

        let Some(stop) = self.last_used_stop else {
            return false;
        };

        if stop.is_zero() {
            return false;
        }

        let stop_ms = stop.to_unix_millis();
        if stop_ms == 0 || stop_ms > now_ms {
            return false;
        }

        (now_ms.saturating_sub(stop_ms)) <= threshold_ms
    }

    /// When the access finished (in Unix millis), or None if still active or never run.
    pub fn stop_time_ms(&self) -> Option<u64> {
        self.last_used_stop
            .filter(|s| !s.is_zero())
            .map(|s| s.to_unix_millis())
    }

    /// When the access started (in Unix millis), or None if never run.
    pub fn start_time_ms(&self) -> Option<u64> {
        self.last_used_start
            .filter(|s| !s.is_zero())
            .map(|s| s.to_unix_millis())
    }
}
