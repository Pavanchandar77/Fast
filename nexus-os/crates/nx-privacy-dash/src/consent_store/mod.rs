//! Windows CapabilityAccessManager ConsentStore parser, reader, and data types.

pub mod parser;
pub mod reader;
pub mod types;

pub use parser::{ConsentStoreParser, ConsentStoreSnapshot, ParseError};
pub use reader::{ConsentStoreReader, ReaderError};
pub use types::{AppIdentifier, CapabilityKind, ConsentRecord, ConsentStatus};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filetime::FileTime;

    const SAMPLE_REG_FIXTURE: &str = r#"
Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam]
"Value"="Allow"

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\Microsoft.WindowsCamera_8wekyb3d8bbwe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,40,a2,88,85,9a,bc,01
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\NonPackaged\C:#Program Files#Zoom#bin#Zoom.exe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,50,4b,a2,82,9a,bc,01
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\C:#Program Files#Discord#app-1.0.9000#Discord.exe]
"Value"="Allow"
"LastUsedTimeStart"=hex(b):00,00,00,10,20,30,40,01
"LastUsedTimeStop"=hex(b):00,00,00,50,60,70,80,01

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location\NonPackaged\C:#Program Files#Google#Chrome#Application#chrome.exe]
"Value"="Deny"
"LastUsedTimeStart"=hex(b):00,00,00,00,00,00,00,00
"LastUsedTimeStop"=hex(b):00,00,00,00,00,00,00,00
"#;

    #[test]
    fn parse_reg_fixture_success() {
        let snapshot = ConsentStoreParser::parse_reg_file(SAMPLE_REG_FIXTURE).unwrap();
        assert_eq!(snapshot.records.len(), 4);

        // Active webcam apps
        let active_webcams = snapshot.active_by_capability(&CapabilityKind::Webcam);
        assert_eq!(active_webcams.len(), 2);

        // Check packaged app
        let camera_app = active_webcams
            .iter()
            .find(|r| r.app_id.is_packaged())
            .unwrap();
        assert_eq!(camera_app.status, ConsentStatus::Allow);
        assert!(camera_app.is_currently_active());
        assert_eq!(camera_app.app_id.display_name(), "WindowsCamera");

        // Check non-packaged app
        let zoom_app = active_webcams
            .iter()
            .find(|r| !r.app_id.is_packaged())
            .unwrap();
        assert_eq!(zoom_app.app_id.display_name(), "Zoom");
        if let AppIdentifier::NonPackaged {
            executable_path,
            binary_name,
            ..
        } = &zoom_app.app_id
        {
            assert_eq!(executable_path, "C:\\Program Files\\Zoom\\bin\\Zoom.exe");
            assert_eq!(binary_name, "Zoom.exe");
        } else {
            panic!("Expected NonPackaged app");
        }

        // Check microphone inactive app
        let discord_app = snapshot
            .find_record(
                &CapabilityKind::Microphone,
                &AppIdentifier::from_desktop_path(
                    "C:\\Program Files\\Discord\\app-1.0.9000\\Discord.exe",
                ),
            )
            .unwrap();
        assert_eq!(discord_app.status, ConsentStatus::Allow);
        assert!(!discord_app.is_currently_active());

        // Check location denied app
        let chrome_app = snapshot
            .find_record(
                &CapabilityKind::Location,
                &AppIdentifier::from_desktop_path(
                    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
                ),
            )
            .unwrap();
        assert_eq!(chrome_app.status, ConsentStatus::Deny);
        assert!(!chrome_app.is_currently_active());
    }

    #[test]
    fn parse_json_fixture_success() {
        let json_data = r#"{
            "webcam": {
                "packaged": {
                    "Microsoft.WindowsCamera_8wekyb3d8bbwe": {
                        "value": "Allow",
                        "last_used_time_start": 133456789012345678,
                        "last_used_time_stop": 0
                    }
                },
                "non_packaged": {
                    "C:#Program Files#OBS-Studio#bin#64bit#obs64.exe": {
                        "value": "Allow",
                        "last_used_time_start": 133456789012345678,
                        "last_used_time_stop": 0,
                        "attribution": "OBS Studio Broadcaster"
                    }
                }
            },
            "microphone": {
                "non_packaged": {
                    "C:#Program Files#Audacity#audacity.exe": {
                        "value": "Deny",
                        "last_used_time_start": 133456000000000000,
                        "last_used_time_stop": 133456001000000000
                    }
                }
            }
        }"#;

        let snapshot = ConsentStoreParser::parse_json(json_data).unwrap();
        assert_eq!(snapshot.records.len(), 3);

        let active_webcams = snapshot.active_by_capability(&CapabilityKind::Webcam);
        assert_eq!(active_webcams.len(), 2);

        let obs = snapshot
            .records
            .iter()
            .find(|r| r.app_id.display_name() == "obs64")
            .unwrap();
        assert_eq!(obs.attribution.as_deref(), Some("OBS Studio Broadcaster"));
        assert!(obs.is_currently_active());
    }

    #[test]
    fn recently_used_threshold_calculation() {
        let now_ms = 100_000u64;
        let five_sec_ago_ms = now_ms - 5_000;
        let twenty_sec_ago_ms = now_ms - 20_000;

        let recent_stop = FileTime::from_unix_millis(five_sec_ago_ms);
        let old_stop = FileTime::from_unix_millis(twenty_sec_ago_ms);

        let record_recent = ConsentRecord {
            app_id: AppIdentifier::from_desktop_path("C:\\App\\recent.exe"),
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(five_sec_ago_ms - 10_000)),
            last_used_stop: Some(recent_stop),
            attribution: None,
        };

        let record_old = ConsentRecord {
            app_id: AppIdentifier::from_desktop_path("C:\\App\\old.exe"),
            capability: CapabilityKind::Microphone,
            status: ConsentStatus::Allow,
            last_used_start: Some(FileTime::from_unix_millis(twenty_sec_ago_ms - 10_000)),
            last_used_stop: Some(old_stop),
            attribution: None,
        };

        let snapshot = ConsentStoreSnapshot::new(vec![record_recent, record_old]);

        // Within 10 seconds: only the recent one qualifies
        let recent = snapshot.recently_used_records(now_ms, 10_000);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].app_id.display_name(), "recent");

        // Within 30 seconds: both qualify
        let all_recent = snapshot.recently_used_records(now_ms, 30_000);
        assert_eq!(all_recent.len(), 2);
    }
}
