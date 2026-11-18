//! Rule engine: critical process protection, exact exclude matching, and whitelisting.

use crate::types::Pid;
use std::collections::HashSet;

/// Hardcoded Windows NT critical processes that must NEVER be demoted, throttled, or suspended.
pub const CRITICAL_SYSTEM_PROCESSES: &[&str] = &[
    "csrss.exe",
    "dwm.exe",
    "lsass.exe",
    "smss.exe",
    "winlogon.exe",
    "services.exe",
    "svchost.exe",
    "audiodg.exe",
    "system",
    "registry",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
    "conhost.exe",
    "explorer.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "spoolsv.exe",
    "securityhealthservice.exe",
    "runtimebroker.exe",
    "ntoskrnl.exe",
    "wininit.exe",
];

/// Nexus-OS shell binaries that must never be demoted or starved.
pub const NEXUS_CORE_PROCESSES: &[&str] = &[
    "nexus-os.exe",
    "nexus-compositor.exe",
    "nexus-governor.exe",
    "nexus-vfs.exe",
    "nexus-watchdog.exe",
    "nexus-shield.exe",
];

/// Known game executables and launch patterns for auto Game Mode detection.
pub const KNOWN_GAME_EXECUTABLES: &[&str] = &[
    "valorant.exe",
    "cs2.exe",
    "dota2.exe",
    "r5apex.exe",
    "fortniteclient-win64-shipping.exe",
    "overwatch.exe",
    "cyberpunk2077.exe",
    "witcher3.exe",
    "eldenring.exe",
    "starfield.exe",
    "baldursgate3.exe",
    "bg3.exe",
    "bg3_dx11.exe",
    "steam.exe",
    "steamwebhelper.exe",
];

/// Extracts the basename of an executable path without heap allocation.
#[inline]
pub fn extract_basename(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// Normalizes an executable name for exact matching:
/// - Extracts the file basename if a path was supplied.
/// - Converts to lowercase.
/// - Trims whitespace.
pub fn normalize_process_name(name: &str) -> String {
    extract_basename(name).to_ascii_lowercase()
}

/// Helper that strips `.exe` if present (case-insensitive) without heap allocation.
#[inline]
pub fn strip_exe_suffix(s: &str) -> &str {
    if s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case(".exe") {
        &s[..s.len() - 4]
    } else {
        s
    }
}

/// Checks if a process name matches a specific rule pattern.
///
/// Safety invariant: Does NOT perform arbitrary substring search!
/// - "cl.exe" does NOT match "clean.exe", "client.exe", or "eclipse.exe".
/// - "rustc.exe" does NOT match "trustcenter.exe".
/// - "spotify" matches "spotify.exe" and "spotify".
/// - Wildcards (e.g. "nexus-*") only match prefix or suffix as denoted by `*`.
pub fn matches_rule(process_name: &str, rule: &str) -> bool {
    let proc_base = extract_basename(process_name);
    let rule_base = extract_basename(rule);

    let proc_stem = strip_exe_suffix(proc_base);
    let rule_stem = strip_exe_suffix(rule_base);

    // Exact match (case-insensitive)
    if proc_base.eq_ignore_ascii_case(rule_base) || proc_stem.eq_ignore_ascii_case(rule_stem) {
        return true;
    }

    // Explicit wildcard handling
    if rule_base.contains('*') {
        if rule_base.starts_with('*') && rule_base.ends_with('*') && rule_base.len() > 2 {
            let inner = &rule_base[1..rule_base.len() - 1];
            let proc_lower = proc_base.to_ascii_lowercase();
            return proc_lower.contains(&inner.to_ascii_lowercase());
        } else if let Some(prefix) = rule_stem.strip_suffix('*') {
            if proc_stem.len() >= prefix.len()
                && proc_stem[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                return true;
            }
        } else if let Some(prefix) = rule_base.strip_suffix('*') {
            if (proc_base.len() >= prefix.len()
                && proc_base[..prefix.len()].eq_ignore_ascii_case(prefix))
                || (proc_stem.len() >= prefix.len()
                    && proc_stem[..prefix.len()].eq_ignore_ascii_case(prefix))
            {
                return true;
            }
        } else if let Some(suffix) = rule_base.strip_prefix('*') {
            if proc_base.len() >= suffix.len()
                && proc_base[proc_base.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            {
                return true;
            }
        } else if let Some(suffix) = rule_stem.strip_prefix('*') {
            if proc_stem.len() >= suffix.len()
                && proc_stem[proc_stem.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            {
                return true;
            }
        }
    }

    false
}

/// Governor rules engine controlling process classification and protections.
#[derive(Debug, Clone)]
pub struct GovernorRules {
    critical_processes: HashSet<String>,
    exclude_rules: Vec<String>,
    high_priority_whitelist: Vec<String>,
    game_executables: HashSet<String>,
}

impl Default for GovernorRules {
    fn default() -> Self {
        Self::new(
            &[
                "spotify.exe",
                "discord.exe",
                "slack.exe",
                "obs64.exe",
                "steam.exe",
                "cargo.exe",
                "rustc.exe",
                "cl.exe",
            ],
            &[] as &[&str],
        )
    }
}

impl GovernorRules {
    /// Creates a new ruleset with specified exclusions and High-priority whitelists.
    pub fn new<S1: AsRef<str>, S2: AsRef<str>>(
        exclude_rules: &[S1],
        high_priority_whitelist: &[S2],
    ) -> Self {
        let mut critical_processes = HashSet::new();
        for &proc in CRITICAL_SYSTEM_PROCESSES {
            critical_processes.insert(normalize_process_name(proc));
            critical_processes.insert(strip_exe_suffix(&normalize_process_name(proc)).to_string());
        }
        for &proc in NEXUS_CORE_PROCESSES {
            critical_processes.insert(normalize_process_name(proc));
            critical_processes.insert(strip_exe_suffix(&normalize_process_name(proc)).to_string());
        }

        let mut game_executables = HashSet::new();
        for &game in KNOWN_GAME_EXECUTABLES {
            game_executables.insert(normalize_process_name(game));
            game_executables.insert(strip_exe_suffix(&normalize_process_name(game)).to_string());
        }

        let excludes = exclude_rules
            .iter()
            .map(|r| r.as_ref().to_string())
            .collect();

        let whitelist = high_priority_whitelist
            .iter()
            .map(|w| w.as_ref().to_string())
            .collect();

        Self {
            critical_processes,
            exclude_rules: excludes,
            high_priority_whitelist: whitelist,
            game_executables,
        }
    }

    /// Checks if a PID and name denote a protected critical system/Nexus process.
    pub fn is_critical(&self, pid: Pid, name: &str) -> bool {
        if pid.is_system_kernel() {
            return true;
        }

        let base = extract_basename(name);
        let stem = strip_exe_suffix(base);

        if self.critical_processes.contains(base) || self.critical_processes.contains(stem) {
            return true;
        }

        if base.bytes().any(|b| b.is_ascii_uppercase()) {
            let lower = base.to_ascii_lowercase();
            let stem_lower = strip_exe_suffix(&lower);
            self.critical_processes.contains(&lower) || self.critical_processes.contains(stem_lower)
        } else {
            false
        }
    }

    /// Checks if a process is excluded from governance (demotion, EcoQoS, rate caps).
    pub fn is_excluded(&self, pid: Pid, name: &str) -> bool {
        if self.is_critical(pid, name) {
            return true;
        }

        self.exclude_rules
            .iter()
            .any(|rule| matches_rule(name, rule))
    }

    /// Checks if a process is on the explicit whitelist allowed to receive HIGH priority boost.
    pub fn is_high_priority_whitelisted(&self, name: &str) -> bool {
        self.high_priority_whitelist
            .iter()
            .any(|rule| matches_rule(name, rule))
    }

    /// Checks if a process is a known game executable.
    pub fn is_known_game(&self, name: &str) -> bool {
        let base = extract_basename(name);
        let stem = strip_exe_suffix(base);

        if self.game_executables.contains(base) || self.game_executables.contains(stem) {
            return true;
        }

        if base.bytes().any(|b| b.is_ascii_uppercase()) {
            let lower = base.to_ascii_lowercase();
            let stem_lower = strip_exe_suffix(&lower);
            self.game_executables.contains(&lower) || self.game_executables.contains(stem_lower)
        } else {
            false
        }
    }

    /// Add an exclude rule at runtime.
    pub fn add_exclude_rule(&mut self, rule: impl Into<String>) {
        self.exclude_rules.push(rule.into());
    }

    /// Add a high priority whitelist entry at runtime.
    pub fn add_high_priority_whitelist(&mut self, rule: impl Into<String>) {
        self.high_priority_whitelist.push(rule.into());
    }
}
