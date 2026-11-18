use std::collections::BTreeSet;

pub const TELEMETRY_HOSTS: &[&str] = &[
    "activity.windows.com",
    "browser.pipe.aria.microsoft.com",
    "diagnostics.support.microsoft.com",
    "fe3.delivery.mp.microsoft.com",
    "feedback.microsoft.com",
    "mobile.pipe.aria.microsoft.com",
    "nexus.officeapps.live.com",
    "pipe.aria.microsoft.com",
    "settings-win.data.microsoft.com",
    "telecommand.telemetry.microsoft.com",
    "telemetry.microsoft.com",
    "v10.events.data.microsoft.com",
    "v10c.events.data.microsoft.com",
    "v20.events.data.microsoft.com",
    "watson.live.com",
    "watson.ppe.telemetry.microsoft.com",
    "watson.telemetry.microsoft.com",
];

pub const AD_AND_TRACKER_HOSTS: &[&str] = &[
    "adnxs.com",
    "adservice.google.com",
    "ads.linkedin.com",
    "ads.pubmatic.com",
    "advertising.apple.com",
    "analytics.yahoo.com",
    "c.bing.com",
    "clarity.ms",
    "doubleclick.net",
    "google-analytics.com",
    "metrics.data.hicloud.com",
    "scorecardresearch.com",
];

/// Parses hosts-file formatted text or simple domain lists into a deduplicated,
/// normalized, sorted list of hostnames.
///
/// Supported input forms:
/// - `0.0.0.0 domain.com`
/// - `127.0.0.1 domain.com`
/// - `::1 domain.com`
/// - `domain.com`
/// - Comments starting with `#` and inline `# comment`
pub fn parse_tracker_list(input: &str) -> Vec<String> {
    let mut set = BTreeSet::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Strip inline comments
        let without_comment = if let Some((before, _)) = trimmed.split_once('#') {
            before.trim()
        } else {
            trimmed
        };

        if without_comment.is_empty() {
            continue;
        }

        // Check if there is an IP prefix (e.g. 0.0.0.0 domain.com)
        let parts: Vec<&str> = without_comment.split_whitespace().collect();
        let domain_candidate = match parts.as_slice() {
            [ip, domain, ..]
                if *ip == "0.0.0.0"
                    || *ip == "127.0.0.1"
                    || *ip == "::1"
                    || *ip == "broadcasthost" =>
            {
                *domain
            }
            [domain] => *domain,
            _ => continue,
        };

        let normalized = domain_candidate.trim().to_ascii_lowercase();

        if is_valid_hostname(&normalized) {
            set.insert(normalized);
        }
    }

    set.into_iter().collect()
}

fn is_valid_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    // Filter out localhost and loopbacks
    if host == "localhost" || host == "localhost.localdomain" || host == "broadcasthost" {
        return false;
    }
    // Must contain at least one dot
    if !host.contains('.') {
        return false;
    }

    // Each label must be non-empty, <= 63 chars, and contain valid chars
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return false;
        }
    }

    true
}
