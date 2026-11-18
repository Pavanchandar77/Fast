//! Live registry reader for Windows CapabilityAccessManager ConsentStore.
//!
//! Windows access is placed strictly behind `#[cfg(windows)]`. On non-Windows platforms
//! (e.g. Linux CI/test environments), a pure in-memory or fixture reader is provided.

use super::parser::ConsentStoreSnapshot;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("Registry access failed: {0}")]
    Registry(String),
    #[error("Platform unsupported")]
    UnsupportedPlatform,
}

/// ConsentStore reader abstraction.
pub struct ConsentStoreReader;

#[cfg(windows)]
mod win_reader {
    use super::super::types::{AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus};
    use super::*;
    use crate::filetime::FileTime;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ, REG_QWORD, REG_SZ, REG_VALUE_TYPE,
    };

    const CONSENT_BASE_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";

    impl ConsentStoreReader {
        /// Read live consent records from the Windows registry.
        pub fn read_live() -> Result<ConsentStoreSnapshot, ReaderError> {
            let mut records = Vec::new();

            for cap in [
                CapabilityKind::Webcam,
                CapabilityKind::Microphone,
                CapabilityKind::Location,
            ] {
                let subpath = format!("{}\\{}", CONSENT_BASE_PATH, cap.registry_key_name());
                let _ = Self::read_capability_key(HKEY_CURRENT_USER, &subpath, &cap, &mut records);
            }

            Ok(ConsentStoreSnapshot::new(records))
        }

        fn to_wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        fn from_wide(slice: &[u16]) -> String {
            let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
            String::from_utf16_lossy(&slice[..len])
        }

        fn read_capability_key(
            root: HKEY,
            subpath: &str,
            cap: &CapabilityKind,
            out: &mut Vec<ConsentRecord>,
        ) -> Result<(), ReaderError> {
            let wide_path = Self::to_wide(subpath);
            let mut hkey = HKEY::default();

            let res =
                unsafe { RegOpenKeyExW(root, PCWSTR(wide_path.as_ptr()), 0, KEY_READ, &mut hkey) };

            if res != WIN32_ERROR(0) {
                return Ok(());
            }

            // Enumerate subkeys (Packaged apps + "NonPackaged" directory)
            let mut index = 0u32;
            let mut name_buf = [0u16; 512];

            loop {
                let mut name_len = name_buf.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        hkey,
                        index,
                        windows::core::PWSTR(name_buf.as_mut_ptr()),
                        &mut name_len,
                        None,
                        windows::core::PWSTR::null(),
                        None,
                        None,
                    )
                };

                if status != WIN32_ERROR(0) {
                    break;
                }

                let subkey_name = Self::from_wide(&name_buf[..name_len as usize]);
                index += 1;

                if subkey_name.eq_ignore_ascii_case("NonPackaged") {
                    // Read desktop apps under NonPackaged
                    let np_path = format!("{}\\{}", subpath, subkey_name);
                    let _ = Self::read_non_packaged_apps(root, &np_path, cap, out);
                } else {
                    // Packaged app
                    let app_id = AppIdentifier::from_package_family_name(&subkey_name);
                    let app_path = format!("{}\\{}", subpath, subkey_name);
                    if let Some(record) =
                        Self::read_app_values(root, &app_path, app_id, cap.clone())
                    {
                        out.push(record);
                    }
                }
            }

            unsafe {
                let _ = RegCloseKey(hkey);
            }

            Ok(())
        }

        fn read_non_packaged_apps(
            root: HKEY,
            np_path: &str,
            cap: &CapabilityKind,
            out: &mut Vec<ConsentRecord>,
        ) -> Result<(), ReaderError> {
            let wide_path = Self::to_wide(np_path);
            let mut hkey = HKEY::default();

            let res =
                unsafe { RegOpenKeyExW(root, PCWSTR(wide_path.as_ptr()), 0, KEY_READ, &mut hkey) };

            if res != WIN32_ERROR(0) {
                return Ok(());
            }

            let mut index = 0u32;
            let mut name_buf = [0u16; 512];

            loop {
                let mut name_len = name_buf.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        hkey,
                        index,
                        windows::core::PWSTR(name_buf.as_mut_ptr()),
                        &mut name_len,
                        None,
                        windows::core::PWSTR::null(),
                        None,
                        None,
                    )
                };

                if status != WIN32_ERROR(0) {
                    break;
                }

                let subkey_name = Self::from_wide(&name_buf[..name_len as usize]);
                index += 1;

                let app_id = AppIdentifier::from_non_packaged_key(&subkey_name);
                let app_path = format!("{}\\{}", np_path, subkey_name);
                if let Some(record) = Self::read_app_values(root, &app_path, app_id, cap.clone()) {
                    out.push(record);
                }
            }

            unsafe {
                let _ = RegCloseKey(hkey);
            }

            Ok(())
        }

        fn read_app_values(
            root: HKEY,
            app_path: &str,
            app_id: AppIdentifier,
            capability: CapabilityKind,
        ) -> Option<ConsentRecord> {
            let wide_path = Self::to_wide(app_path);
            let mut hkey = HKEY::default();

            let res =
                unsafe { RegOpenKeyExW(root, PCWSTR(wide_path.as_ptr()), 0, KEY_READ, &mut hkey) };

            if res != WIN32_ERROR(0) {
                return None;
            }

            let status_val = Self::query_string_value(hkey, "Value")
                .map(|s| ConsentStatus::from_value_str(&s))
                .unwrap_or(ConsentStatus::Unspecified);

            let last_start =
                Self::query_qword_value(hkey, "LastUsedTimeStart").map(FileTime::from_raw);
            let last_stop =
                Self::query_qword_value(hkey, "LastUsedTimeStop").map(FileTime::from_raw);
            let attribution = Self::query_string_value(hkey, "Attribution");

            unsafe {
                let _ = RegCloseKey(hkey);
            }

            Some(ConsentRecord {
                app_id,
                capability,
                status: status_val,
                last_used_start: last_start,
                last_used_stop: last_stop,
                attribution,
            })
        }

        fn query_string_value(hkey: HKEY, value_name: &str) -> Option<String> {
            let wide_name = Self::to_wide(value_name);
            let mut val_type = REG_VALUE_TYPE(0);
            let mut data_len = 0u32;

            let res = unsafe {
                RegQueryValueExW(
                    hkey,
                    PCWSTR(wide_name.as_ptr()),
                    None,
                    Some(&mut val_type),
                    None,
                    Some(&mut data_len),
                )
            };

            if res != WIN32_ERROR(0) || val_type != REG_SZ || data_len == 0 {
                return None;
            }

            let mut buf = vec![0u16; (data_len as usize / 2) + 1];
            let res = unsafe {
                RegQueryValueExW(
                    hkey,
                    PCWSTR(wide_name.as_ptr()),
                    None,
                    Some(&mut val_type),
                    Some(buf.as_mut_ptr() as *mut _),
                    Some(&mut data_len),
                )
            };

            if res == WIN32_ERROR(0) {
                Some(Self::from_wide(&buf))
            } else {
                None
            }
        }

        fn query_qword_value(hkey: HKEY, value_name: &str) -> Option<u64> {
            let wide_name = Self::to_wide(value_name);
            let mut val_type = REG_VALUE_TYPE(0);
            let mut val = 0u64;
            let mut data_len = std::mem::size_of::<u64>() as u32;

            let res = unsafe {
                RegQueryValueExW(
                    hkey,
                    PCWSTR(wide_name.as_ptr()),
                    None,
                    Some(&mut val_type),
                    Some(&mut val as *mut u64 as *mut _),
                    Some(&mut data_len),
                )
            };

            if res == WIN32_ERROR(0) && (val_type == REG_QWORD || data_len == 8) {
                Some(val)
            } else {
                None
            }
        }
    }
}

#[cfg(not(windows))]
impl ConsentStoreReader {
    /// Pure Linux fallback: returns an empty snapshot or error on request.
    pub fn read_live() -> Result<ConsentStoreSnapshot, ReaderError> {
        Ok(ConsentStoreSnapshot::empty())
    }
}
