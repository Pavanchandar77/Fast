use nx_privacy::trackers::{parse_tracker_list, AD_AND_TRACKER_HOSTS, TELEMETRY_HOSTS};

#[test]
fn test_parse_hosts_format_with_ips() {
    let raw = r#"
# Sample hosts file
0.0.0.0 telemetry.microsoft.com
127.0.0.1 adservice.google.com
::1 tracking.example.com
"#;
    let list = parse_tracker_list(raw);
    assert_eq!(
        list,
        vec![
            "adservice.google.com".to_string(),
            "telemetry.microsoft.com".to_string(),
            "tracking.example.com".to_string(),
        ]
    );
}

#[test]
fn test_parse_plain_domain_list() {
    let raw = r#"
doubleclick.net
scorecardresearch.com
adnxs.com
"#;
    let list = parse_tracker_list(raw);
    assert_eq!(
        list,
        vec![
            "adnxs.com".to_string(),
            "doubleclick.net".to_string(),
            "scorecardresearch.com".to_string(),
        ]
    );
}

#[test]
fn test_parse_ignores_comments_and_inline_comments() {
    let raw = r#"
# Full line comment
   # Indented comment
0.0.0.0 tracker.com # inline comment
clarity.ms # another inline comment
"#;
    let list = parse_tracker_list(raw);
    assert_eq!(
        list,
        vec!["clarity.ms".to_string(), "tracker.com".to_string()]
    );
}

#[test]
fn test_parse_filters_invalid_hostnames_and_localhost() {
    let raw = r#"
127.0.0.1 localhost
::1 localhost
0.0.0.0 broadcasthost
0.0.0.0 invalid_name!@#
nodots
-hyphenstart.com
valid-tracker.com
"#;
    let list = parse_tracker_list(raw);
    assert_eq!(list, vec!["valid-tracker.com".to_string()]);
}

#[test]
fn test_parse_normalizes_case_and_deduplicates() {
    let raw = r#"
0.0.0.0 Tracker.COM
0.0.0.0 tracker.com
TRACKER.COM
"#;
    let list = parse_tracker_list(raw);
    assert_eq!(list, vec!["tracker.com".to_string()]);
}

#[test]
fn test_shipped_telemetry_hosts_are_valid() {
    assert!(!TELEMETRY_HOSTS.is_empty());
    for host in TELEMETRY_HOSTS {
        assert!(host.contains('.'), "Host must have dot: {host}");
        assert_eq!(host.to_ascii_lowercase(), *host, "Must be lowercase");
    }
}

#[test]
fn test_shipped_ad_hosts_are_valid() {
    assert!(!AD_AND_TRACKER_HOSTS.is_empty());
    for host in AD_AND_TRACKER_HOSTS {
        assert!(host.contains('.'), "Host must have dot: {host}");
        assert_eq!(host.to_ascii_lowercase(), *host, "Must be lowercase");
    }
}
