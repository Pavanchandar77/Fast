//! Parser for CapabilityAccessManager ConsentStore registry exports and fixture files.

use super::types::{AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus};
use crate::filetime::FileTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("Invalid .reg syntax on line {line}: {msg}")]
    InvalidRegSyntax { line: usize, msg: String },
    #[error("Invalid QWORD hex bytes: {0}")]
    InvalidHexBytes(String),
    #[error("JSON deserialization error: {0}")]
    JsonError(String),
}

/// A snapshot containing all parsed ConsentStore records.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ConsentStoreSnapshot {
    pub records: Vec<ConsentRecord>,
}

impl ConsentStoreSnapshot {
    pub fn new(records: Vec<ConsentRecord>) -> Self {
        Self { records }
    }

    /// Empty snapshot.
    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Find all records where access is currently active (stop is 0 or less than start).
    pub fn active_records(&self) -> Vec<&ConsentRecord> {
        self.records
            .iter()
            .filter(|r| r.is_currently_active())
            .collect()
    }

    /// Find active records for a specific capability (e.g. Webcam).
    pub fn active_by_capability(&self, cap: &CapabilityKind) -> Vec<&ConsentRecord> {
        self.records
            .iter()
            .filter(|r| &r.capability == cap && r.is_currently_active())
            .collect()
    }

    /// Find all records that were recently used within `threshold_ms` of `now_ms`.
    pub fn recently_used_records(&self, now_ms: u64, threshold_ms: u64) -> Vec<&ConsentRecord> {
        self.records
            .iter()
            .filter(|r| r.was_recently_used(now_ms, threshold_ms))
            .collect()
    }

    /// Find a record for a specific app and capability.
    pub fn find_record(&self, cap: &CapabilityKind, app: &AppIdentifier) -> Option<&ConsentRecord> {
        self.records
            .iter()
            .find(|r| &r.capability == cap && &r.app_id == app)
    }

    /// All records associated with an app across capabilities.
    pub fn records_for_app(&self, app: &AppIdentifier) -> Vec<&ConsentRecord> {
        self.records.iter().filter(|r| &r.app_id == app).collect()
    }
}

/// Helper struct for JSON fixture representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonConsentAppEntry {
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub last_used_time_start: Option<u64>,
    #[serde(default)]
    pub last_used_time_stop: Option<u64>,
    #[serde(default)]
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JsonCapabilityEntry {
    #[serde(default)]
    pub packaged: BTreeMap<String, JsonConsentAppEntry>,
    #[serde(default)]
    pub non_packaged: BTreeMap<String, JsonConsentAppEntry>,
}

/// Parse fixtures into `ConsentStoreSnapshot`.
pub struct ConsentStoreParser;

impl ConsentStoreParser {
    /// Parse JSON fixture structure.
    pub fn parse_json(json_str: &str) -> Result<ConsentStoreSnapshot, ParseError> {
        let root: BTreeMap<String, JsonCapabilityEntry> =
            serde_json::from_str(json_str).map_err(|e| ParseError::JsonError(e.to_string()))?;

        let mut records = Vec::new();

        for (cap_name, cap_entry) in root {
            let cap = CapabilityKind::from_key_name(&cap_name);

            // Packaged apps
            for (pfn, app_data) in cap_entry.packaged {
                let app_id = AppIdentifier::from_package_family_name(&pfn);
                records.push(ConsentRecord {
                    app_id,
                    capability: cap.clone(),
                    status: app_data
                        .value
                        .as_deref()
                        .map(ConsentStatus::from_value_str)
                        .unwrap_or(ConsentStatus::Unspecified),
                    last_used_start: app_data.last_used_time_start.map(FileTime::from_raw),
                    last_used_stop: app_data.last_used_time_stop.map(FileTime::from_raw),
                    attribution: app_data.attribution,
                });
            }

            // Non-packaged Win32 apps
            for (raw_key, app_data) in cap_entry.non_packaged {
                let app_id = AppIdentifier::from_non_packaged_key(&raw_key);
                records.push(ConsentRecord {
                    app_id,
                    capability: cap.clone(),
                    status: app_data
                        .value
                        .as_deref()
                        .map(ConsentStatus::from_value_str)
                        .unwrap_or(ConsentStatus::Unspecified),
                    last_used_start: app_data.last_used_time_start.map(FileTime::from_raw),
                    last_used_stop: app_data.last_used_time_stop.map(FileTime::from_raw),
                    attribution: app_data.attribution,
                });
            }
        }

        Ok(ConsentStoreSnapshot::new(records))
    }

    /// Parse standard Windows `.reg` export text file.
    pub fn parse_reg_file(reg_text: &str) -> Result<ConsentStoreSnapshot, ParseError> {
        let mut current_key: Option<String> = None;
        let mut current_values: BTreeMap<String, String> = BTreeMap::new();
        let mut key_sections: Vec<(String, BTreeMap<String, String>)> = Vec::new();

        for (idx, line) in reg_text.lines().enumerate() {
            let line_num = idx + 1;
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }

            // Check header
            if trimmed.starts_with("Windows Registry Editor") {
                continue;
            }

            // Section key: [HKEY_CURRENT_USER\...]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(key) = current_key.take() {
                    key_sections.push((key, std::mem::take(&mut current_values)));
                }
                let key_name = &trimmed[1..trimmed.len() - 1];
                current_key = Some(key_name.to_string());
                continue;
            }

            // Key value: "Name"="Value" or "Name"=hex(b):... or "Name"=dword:... or "Name"=qword:...
            if let Some((name_part, val_part)) = trimmed.split_once('=') {
                let name = name_part.trim().trim_matches('"');
                let val = val_part.trim();
                current_values.insert(name.to_string(), val.to_string());
            } else if !trimmed.is_empty() && current_key.is_none() {
                return Err(ParseError::InvalidRegSyntax {
                    line: line_num,
                    msg: format!("Unexpected line outside section: {trimmed}"),
                });
            }
        }

        if let Some(key) = current_key {
            key_sections.push((key, current_values));
        }

        let mut records = Vec::new();

        for (key_path, values) in key_sections {
            // Find if this key is under ConsentStore
            if let Some(record) = Self::parse_consent_key_path(&key_path, &values)? {
                records.push(record);
            }
        }

        Ok(ConsentStoreSnapshot::new(records))
    }

    /// Parse a single registry key path and its values if it belongs to ConsentStore.
    fn parse_consent_key_path(
        key_path: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Option<ConsentRecord>, ParseError> {
        // Looking for path containing ConsentStore\<capability>
        // Key path can be:
        // ...\CapabilityAccessManager\ConsentStore\<cap>\NonPackaged\<app_key>
        // or
        // ...\CapabilityAccessManager\ConsentStore\<cap>\<package_family_name>
        let parts: Vec<&str> = key_path.split('\\').collect();
        let consent_pos = parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("ConsentStore"));

        let Some(pos) = consent_pos else {
            return Ok(None);
        };

        // Capability is right after ConsentStore
        if pos + 1 >= parts.len() {
            // Root ConsentStore key itself
            return Ok(None);
        }

        let cap_str = parts[pos + 1];
        let capability = CapabilityKind::from_key_name(cap_str);

        // Subkey under capability
        if pos + 2 >= parts.len() {
            // It's the capability root key (e.g. ConsentStore\webcam)
            return Ok(None);
        }

        let next_part = parts[pos + 2];
        let app_id = if next_part.eq_ignore_ascii_case("NonPackaged") {
            if pos + 3 < parts.len() {
                let raw_app = parts[pos + 3..].join("\\");
                AppIdentifier::from_non_packaged_key(&raw_app)
            } else {
                return Ok(None);
            }
        } else {
            AppIdentifier::from_package_family_name(next_part)
        };

        // Extract values
        let status = values
            .get("Value")
            .map(|s| ConsentStatus::from_value_str(s.trim_matches('"')))
            .unwrap_or(ConsentStatus::Unspecified);

        let last_used_start = values
            .get("LastUsedTimeStart")
            .map(|v| Self::parse_qword_value(v))
            .transpose()?
            .map(FileTime::from_raw);

        let last_used_stop = values
            .get("LastUsedTimeStop")
            .map(|v| Self::parse_qword_value(v))
            .transpose()?
            .map(FileTime::from_raw);

        let attribution = values
            .get("Attribution")
            .map(|s| s.trim_matches('"').to_string());

        Ok(Some(ConsentRecord {
            app_id,
            capability,
            status,
            last_used_start,
            last_used_stop,
            attribution,
        }))
    }

    /// Parse registry QWORD value from hex(b):..., qword:..., or decimal strings.
    fn parse_qword_value(raw: &str) -> Result<u64, ParseError> {
        let raw = raw.trim();

        if let Some(hex_part) = raw.strip_prefix("hex(b):") {
            // Little-endian bytes separated by comma: "hex(b):00,12,34,56,78,9a,bc,01"
            let byte_strs: Vec<&str> = hex_part.split(',').map(|s| s.trim()).collect();
            if byte_strs.len() != 8 {
                return Err(ParseError::InvalidHexBytes(format!(
                    "Expected 8 bytes for QWORD, got {}: {}",
                    byte_strs.len(),
                    raw
                )));
            }

            let mut bytes = [0u8; 8];
            for (i, b_str) in byte_strs.iter().enumerate() {
                bytes[i] = u8::from_str_radix(b_str, 16).map_err(|_| {
                    ParseError::InvalidHexBytes(format!("Invalid byte '{b_str}' in {raw}"))
                })?;
            }

            Ok(u64::from_le_bytes(bytes))
        } else if let Some(qword_part) = raw.strip_prefix("qword:") {
            u64::from_str_radix(qword_part, 16)
                .map_err(|e| ParseError::InvalidHexBytes(e.to_string()))
        } else if let Some(dword_part) = raw.strip_prefix("dword:") {
            u64::from_str_radix(dword_part, 16)
                .map_err(|e| ParseError::InvalidHexBytes(e.to_string()))
        } else {
            raw.parse::<u64>()
                .map_err(|e| ParseError::InvalidHexBytes(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_qword_formats() {
        // hex(b) little-endian: 01,00,00,00,00,00,00,00 is 1
        let val1 = ConsentStoreParser::parse_qword_value("hex(b):01,00,00,00,00,00,00,00").unwrap();
        assert_eq!(val1, 1);

        // qword hex
        let val2 = ConsentStoreParser::parse_qword_value("qword:00000000000000ff").unwrap();
        assert_eq!(val2, 255);

        // dword hex
        let val3 = ConsentStoreParser::parse_qword_value("dword:00000010").unwrap();
        assert_eq!(val3, 16);

        // decimal
        let val4 = ConsentStoreParser::parse_qword_value("123456").unwrap();
        assert_eq!(val4, 123456);

        // invalid hex length
        let err = ConsentStoreParser::parse_qword_value("hex(b):01,02,03");
        assert!(err.is_err());
    }

    #[test]
    fn parse_non_packaged_deep_path() {
        let reg = r#"
[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\NonPackaged\C:#Users#Alex#AppData#Local#Programs#Signal#Signal.exe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,10,20,30,40,50,60,01
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00
"#;
        let snap = ConsentStoreParser::parse_reg_file(reg).unwrap();
        assert_eq!(snap.records.len(), 1);
        let rec = &snap.records[0];
        assert_eq!(rec.app_id.display_name(), "Signal");
        assert_eq!(rec.capability, CapabilityKind::Webcam);
        assert!(rec.is_currently_active());
        if let AppIdentifier::NonPackaged {
            executable_path, ..
        } = &rec.app_id
        {
            assert_eq!(
                executable_path,
                "C:\\Users\\Alex\\AppData\\Local\\Programs\\Signal\\Signal.exe"
            );
        } else {
            panic!("Expected NonPackaged");
        }
    }

    #[test]
    fn parse_empty_and_comments_only() {
        let reg = r#"
Windows Registry Editor Version 5.00
; This is a comment
# Another comment

"#;
        let snap = ConsentStoreParser::parse_reg_file(reg).unwrap();
        assert!(snap.records.is_empty());
    }

    #[test]
    fn parse_duration_and_active_transitions() {
        let now_ms = 1_700_000_000_000u64;
        let start_ft = FileTime::from_unix_millis(now_ms - 20_000);
        let stop_ft = FileTime::from_unix_millis(now_ms - 5_000);

        let rec_active = ConsentRecord {
            app_id: AppIdentifier::from_desktop_path("C:\\App\\app.exe"),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(start_ft),
            last_used_stop: None,
            attribution: None,
        };
        assert!(rec_active.is_currently_active());
        assert_eq!(rec_active.session_duration_ms(now_ms), 20_000);

        let rec_stopped = ConsentRecord {
            app_id: AppIdentifier::from_desktop_path("C:\\App\\app.exe"),
            capability: CapabilityKind::Webcam,
            status: ConsentStatus::Allow,
            last_used_start: Some(start_ft),
            last_used_stop: Some(stop_ft),
            attribution: None,
        };
        assert!(!rec_stopped.is_currently_active());
        assert_eq!(rec_stopped.session_duration_ms(now_ms), 15_000);
    }
}
