//! Windows build and UBR (Update Build Revision) detection.
//! Combines `RtlGetVersion` from ntdll with registry metadata from
//! `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` (CurrentBuild, UBR, DisplayVersion).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowsBuildInfo {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub ubr: u32,
    pub display_version: String,
    pub edition: String,
}

impl WindowsBuildInfo {
    pub fn new(
        major: u32,
        minor: u32,
        build: u32,
        ubr: u32,
        display_version: impl Into<String>,
        edition: impl Into<String>,
    ) -> Self {
        Self {
            major,
            minor,
            build,
            ubr,
            display_version: display_version.into(),
            edition: edition.into(),
        }
    }

    /// Full formatted version string, e.g. "10.0.22631.4387 (23H2)"
    pub fn full_version_string(&self) -> String {
        if self.display_version.is_empty() {
            format!("{}.{}.{}.{}", self.major, self.minor, self.build, self.ubr)
        } else {
            format!(
                "{}.{}.{}.{} ({})",
                self.major, self.minor, self.build, self.ubr, self.display_version
            )
        }
    }

    /// True if running on Windows 11 (Build 22000+)
    pub fn is_windows_11(&self) -> bool {
        self.build >= 22000
    }

    /// True if running on Windows 11 22H2 or newer (Build 22621+)
    pub fn is_22h2_or_newer(&self) -> bool {
        self.build >= 22621
    }

    /// True if running on Windows 11 24H2 or newer (Build 26100+)
    pub fn is_24h2_or_newer(&self) -> bool {
        self.build >= 26100
    }

    /// Default baseline Windows 11 build (22631.4387, 23H2) for fallback / mock
    pub fn default_windows_11() -> Self {
        Self {
            major: 10,
            minor: 0,
            build: 22631,
            ubr: 4387,
            display_version: "23H2".to_string(),
            edition: "Windows 11 Pro".to_string(),
        }
    }
}

impl Default for WindowsBuildInfo {
    fn default() -> Self {
        Self::default_windows_11()
    }
}

/// Registry fields extracted from `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryBuildFields {
    pub current_build: Option<u32>,
    pub ubr: Option<u32>,
    pub display_version: Option<String>,
    pub product_name: Option<String>,
}

/// Trait abstracting build detection sources for pure Linux unit testing.
pub trait BuildSource: Send + Sync {
    fn detect_rtl_version(&self) -> Option<(u32, u32, u32)>;
    fn detect_registry_fields(&self) -> RegistryBuildFields;
}

/// Live detection source running against raw Win32 / NT APIs and registry.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveBuildSource;

impl BuildSource for LiveBuildSource {
    #[cfg(windows)]
    fn detect_rtl_version(&self) -> Option<(u32, u32, u32)> {
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        use windows::core::PCSTR;

        #[repr(C)]
        #[allow(non_snake_case)]
        struct OsVersionInfoW {
            dwOSVersionInfoSize: u32,
            dwMajorVersion: u32,
            dwMinorVersion: u32,
            dwBuildNumber: u32,
            dwPlatformId: u32,
            szCSDVersion: [u16; 128],
        }

        type RtlGetVersionFn = unsafe extern "system" fn(*mut OsVersionInfoW) -> i32;

        unsafe {
            let ntdll = GetModuleHandleW(windows::core::w!("ntdll.dll")).ok()?;
            let proc = GetProcAddress(ntdll, PCSTR(b"RtlGetVersion\0".as_ptr()))?;
            let rtl_get_version: RtlGetVersionFn = std::mem::transmute(proc);

            let mut info = OsVersionInfoW {
                dwOSVersionInfoSize: std::mem::size_of::<OsVersionInfoW>() as u32,
                dwMajorVersion: 0,
                dwMinorVersion: 0,
                dwBuildNumber: 0,
                dwPlatformId: 0,
                szCSDVersion: [0; 128],
            };

            if rtl_get_version(&mut info) >= 0 {
                Some((info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber))
            } else {
                None
            }
        }
    }

    #[cfg(not(windows))]
    fn detect_rtl_version(&self) -> Option<(u32, u32, u32)> {
        None
    }

    #[cfg(windows)]
    fn detect_registry_fields(&self) -> RegistryBuildFields {
        use windows::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
            REG_DWORD, REG_SZ,
        };
        use windows::core::w;

        let mut fields = RegistryBuildFields::default();
        let mut hkey = HKEY::default();

        unsafe {
            let status = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
                0,
                KEY_READ,
                &mut hkey,
            );
            if status.is_err() {
                return fields;
            }

            // 1. UBR (DWORD)
            let mut ubr_val: u32 = 0;
            let mut ubr_len = std::mem::size_of::<u32>() as u32;
            let mut val_type = REG_DWORD;
            if RegQueryValueExW(
                hkey,
                w!("UBR"),
                None,
                Some(&mut val_type),
                Some(&mut ubr_val as *mut _ as *mut u8),
                Some(&mut ubr_len),
            )
            .is_ok()
            {
                fields.ubr = Some(ubr_val);
            }

            // 2. DisplayVersion (SZ, e.g. "23H2", "24H2")
            let mut disp_buf = [0u16; 64];
            let mut disp_len = (disp_buf.len() * 2) as u32;
            val_type = REG_SZ;
            if RegQueryValueExW(
                hkey,
                w!("DisplayVersion"),
                None,
                Some(&mut val_type),
                Some(disp_buf.as_mut_ptr() as *mut u8),
                Some(&mut disp_len),
            )
            .is_ok()
            {
                let u16_len = (disp_len / 2) as usize;
                let trimmed_len = disp_buf[..u16_len]
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(u16_len);
                fields.display_version =
                    Some(String::from_utf16_lossy(&disp_buf[..trimmed_len]));
            }

            // 3. CurrentBuild (SZ)
            let mut build_buf = [0u16; 64];
            let mut build_len = (build_buf.len() * 2) as u32;
            val_type = REG_SZ;
            if RegQueryValueExW(
                hkey,
                w!("CurrentBuild"),
                None,
                Some(&mut val_type),
                Some(build_buf.as_mut_ptr() as *mut u8),
                Some(&mut build_len),
            )
            .is_ok()
            {
                let u16_len = (build_len / 2) as usize;
                let trimmed_len = build_buf[..u16_len]
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(u16_len);
                let build_str = String::from_utf16_lossy(&build_buf[..trimmed_len]);
                if let Ok(b) = build_str.parse::<u32>() {
                    fields.current_build = Some(b);
                }
            }

            // 4. ProductName (SZ)
            let mut prod_buf = [0u16; 128];
            let mut prod_len = (prod_buf.len() * 2) as u32;
            val_type = REG_SZ;
            if RegQueryValueExW(
                hkey,
                w!("ProductName"),
                None,
                Some(&mut val_type),
                Some(prod_buf.as_mut_ptr() as *mut u8),
                Some(&mut prod_len),
            )
            .is_ok()
            {
                let u16_len = (prod_len / 2) as usize;
                let trimmed_len = prod_buf[..u16_len]
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(u16_len);
                fields.product_name =
                    Some(String::from_utf16_lossy(&prod_buf[..trimmed_len]));
            }

            let _ = RegCloseKey(hkey);
        }

        fields
    }

    #[cfg(not(windows))]
    fn detect_registry_fields(&self) -> RegistryBuildFields {
        RegistryBuildFields::default()
    }
}

/// In-memory mock build source for testing.
#[derive(Debug, Clone)]
pub struct MockBuildSource {
    pub rtl_version: Option<(u32, u32, u32)>,
    pub registry_fields: RegistryBuildFields,
}

impl MockBuildSource {
    pub fn new(major: u32, minor: u32, build: u32, ubr: u32, display_version: &str) -> Self {
        Self {
            rtl_version: Some((major, minor, build)),
            registry_fields: RegistryBuildFields {
                current_build: Some(build),
                ubr: Some(ubr),
                display_version: Some(display_version.to_string()),
                product_name: Some("Windows 11 Mock".to_string()),
            },
        }
    }
}

impl BuildSource for MockBuildSource {
    fn detect_rtl_version(&self) -> Option<(u32, u32, u32)> {
        self.rtl_version
    }

    fn detect_registry_fields(&self) -> RegistryBuildFields {
        self.registry_fields.clone()
    }
}

/// Detects the Windows build information using the supplied `BuildSource`.
pub fn detect_build_from_source(source: &dyn BuildSource) -> WindowsBuildInfo {
    let rtl = source.detect_rtl_version();
    let reg = source.detect_registry_fields();

    let (major, minor, build) = match (rtl, reg.current_build) {
        (Some((maj, min, b)), _) => (maj, min, b),
        (None, Some(b)) => (10, 0, b),
        (None, None) => (10, 0, 22631),
    };

    let ubr = reg.ubr.unwrap_or(0);
    let display_version = reg.display_version.unwrap_or_else(|| {
        if build >= 26100 {
            "24H2".to_string()
        } else if build >= 22631 {
            "23H2".to_string()
        } else if build >= 22621 {
            "22H2".to_string()
        } else if build >= 22000 {
            "21H2".to_string()
        } else {
            "Unknown".to_string()
        }
    });

    let edition = reg
        .product_name
        .unwrap_or_else(|| "Windows 11".to_string());

    WindowsBuildInfo {
        major,
        minor,
        build,
        ubr,
        display_version,
        edition,
    }
}

/// Detects the live Windows build and UBR on the host system.
pub fn detect_build() -> WindowsBuildInfo {
    detect_build_from_source(&LiveBuildSource)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_build_detection_23h2() {
        let mock = MockBuildSource::new(10, 0, 22631, 4387, "23H2");
        let info = detect_build_from_source(&mock);
        assert_eq!(info.major, 10);
        assert_eq!(info.minor, 0);
        assert_eq!(info.build, 22631);
        assert_eq!(info.ubr, 4387);
        assert_eq!(info.display_version, "23H2");
        assert!(info.is_windows_11());
        assert!(info.is_22h2_or_newer());
        assert!(!info.is_24h2_or_newer());
        assert_eq!(info.full_version_string(), "10.0.22631.4387 (23H2)");
    }

    #[test]
    fn test_mock_build_detection_24h2() {
        let mock = MockBuildSource::new(10, 0, 26100, 1742, "24H2");
        let info = detect_build_from_source(&mock);
        assert_eq!(info.build, 26100);
        assert!(info.is_24h2_or_newer());
    }

    #[test]
    fn test_missing_registry_derives_display_version() {
        let mock = MockBuildSource {
            rtl_version: Some((10, 0, 26100)),
            registry_fields: RegistryBuildFields::default(),
        };
        let info = detect_build_from_source(&mock);
        assert_eq!(info.build, 26100);
        assert_eq!(info.display_version, "24H2");
        assert_eq!(info.ubr, 0);
    }
}
