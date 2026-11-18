//! Clipboard-read alerts and auto-clear of sensitive copies as a pure store.
//!
//! Detects secrets, tokens, passwords, and credit-card-like numbers, validates
//! checksums (Luhn algorithm), enforces app exclusions, and schedules timed auto-clears
//! via reversible command objects.

use crate::consent_store::AppIdentifier;
use crate::permissions::DryRunPlan;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Categories of sensitive information detected in clipboard copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SensitiveKind {
    ApiToken,
    CreditCard,
    SocialSecurityNumber,
    PrivateKey,
    HighEntropyPassword,
}

impl SensitiveKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ApiToken => "API Secret Token",
            Self::CreditCard => "Credit Card Number",
            Self::SocialSecurityNumber => "Social Security Number",
            Self::PrivateKey => "Private Key",
            Self::HighEntropyPassword => "Password / Secret",
        }
    }

    /// Default recommended auto-clear timeout in milliseconds.
    pub fn default_timeout_ms(&self) -> u64 {
        match self {
            Self::CreditCard => 30_000,           // 30 seconds
            Self::SocialSecurityNumber => 45_000, // 45 seconds
            Self::ApiToken => 60_000,             // 60 seconds
            Self::PrivateKey => 60_000,           // 60 seconds
            Self::HighEntropyPassword => 60_000,  // 60 seconds
        }
    }
}

impl fmt::Display for SensitiveKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Alert severity for unauthorized or sensitive clipboard reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

/// Event generated when an app reads sensitive content from the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardReadAlert {
    pub alert_id: u64,
    pub reading_app: AppIdentifier,
    pub sensitive_kind: SensitiveKind,
    pub redacted_snippet: String,
    pub severity: AlertSeverity,
    pub read_timestamp_ms: u64,
    pub source_app: Option<AppIdentifier>,
}

/// Reversible, dry-run-able command object to clear sensitive clipboard data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardClearCommand {
    pub command_id: u64,
    pub content_hash: u64,
    pub sensitive_kind: SensitiveKind,
    pub reason: String,
    pub scheduled_at_ms: u64,
    pub timeout_ms: u64,
    pub backup_redacted: String,
}

impl ClipboardClearCommand {
    /// Invert to a restore command (undo clear).
    pub fn reverse(&self) -> ClipboardRestoreCommand {
        ClipboardRestoreCommand {
            command_id: self.command_id.wrapping_add(1),
            content_hash: self.content_hash,
            reason: format!("Restore clipboard cleared by: {}", self.reason),
            timestamp_ms: self.scheduled_at_ms + self.timeout_ms,
        }
    }

    /// Dry run plan.
    pub fn dry_run(&self) -> DryRunPlan {
        DryRunPlan {
            target_description: format!(
                "Auto-clear sensitive clipboard ({}) after {}s timeout",
                self.sensitive_kind,
                self.timeout_ms / 1000
            ),
            target_registry_key: None,
            old_value: format!("<sensitive:{}>", self.sensitive_kind),
            new_value: "<empty>".into(),
            is_reversible: true,
            safety_notes: "Clipboard memory is zeroed. History item is flagged as cleared.".into(),
        }
    }
}

/// Reversible command to restore previously cleared clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardRestoreCommand {
    pub command_id: u64,
    pub content_hash: u64,
    pub reason: String,
    pub timestamp_ms: u64,
}

/// Details of a sensitive copy detected on clipboard write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitiveCopyDetected {
    pub kind: SensitiveKind,
    pub redacted_snippet: String,
    pub auto_clear_timeout_ms: u64,
}

/// Pure pattern detector and validator.
pub struct SensitivePatternDetector {
    openai_re: Regex,
    github_re: Regex,
    jwt_re: Regex,
    aws_re: Regex,
    private_key_re: Regex,
    ssn_re: Regex,
    credit_card_re: Regex,
    uuid_re: Regex,
}

impl Default for SensitivePatternDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SensitivePatternDetector {
    pub fn new() -> Self {
        Self {
            openai_re: Regex::new(r"\bsk-[a-zA-Z0-9_-]{20,}\b").unwrap(),
            github_re: Regex::new(r"\b(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,}\b").unwrap(),
            jwt_re: Regex::new(
                r"\beyJ[a-zA-Z0-9_-]{10,}\.eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b",
            )
            .unwrap(),
            aws_re: Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
            private_key_re: Regex::new(r"-----BEGIN [A-Z ]+PRIVATE KEY-----").unwrap(),
            ssn_re: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
            credit_card_re: Regex::new(r"\b(?:\d[ -]?){13,19}\b").unwrap(),
            uuid_re: Regex::new(
                r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
            )
            .unwrap(),
        }
    }

    /// Analyze clipboard text and return detected sensitive kind, if any.
    pub fn detect(&self, text: &str) -> Option<(SensitiveKind, String)> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 1. Exclude UUIDs
        if self.uuid_re.is_match(trimmed) {
            return None;
        }

        // 2. Private Keys
        if self.private_key_re.is_match(trimmed) {
            return Some((
                SensitiveKind::PrivateKey,
                "-----BEGIN PRIVATE KEY...[REDACTED]-----".into(),
            ));
        }

        // 3. API Tokens
        if let Some(m) = self.openai_re.find(trimmed) {
            return Some((SensitiveKind::ApiToken, Self::redact_token(m.as_str())));
        }
        if let Some(m) = self.github_re.find(trimmed) {
            return Some((SensitiveKind::ApiToken, Self::redact_token(m.as_str())));
        }
        if let Some(m) = self.aws_re.find(trimmed) {
            return Some((SensitiveKind::ApiToken, Self::redact_token(m.as_str())));
        }
        if self.jwt_re.is_match(trimmed) {
            return Some((SensitiveKind::ApiToken, "eyJ...[JWT REDACTED]".into()));
        }

        // 4. SSN
        for m in self.ssn_re.find_iter(trimmed) {
            let ssn = m.as_str();
            if Self::is_valid_ssn(ssn) {
                let redacted = format!("***-**-{}", &ssn[ssn.len() - 4..]);
                return Some((SensitiveKind::SocialSecurityNumber, redacted));
            }
        }

        // 5. Credit Cards (with Luhn check)
        for m in self.credit_card_re.find_iter(trimmed) {
            let raw_match = m.as_str();
            let digits: String = raw_match.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 13 && digits.len() <= 19 && Self::luhn_check(&digits) {
                let last4 = &digits[digits.len() - 4..];
                let redacted = format!("****-****-****-{}", last4);
                return Some((SensitiveKind::CreditCard, redacted));
            }
        }

        // 6. High-Entropy Password heuristic
        if Self::is_high_entropy_password(trimmed) {
            return Some((
                SensitiveKind::HighEntropyPassword,
                "********[PASSWORD REDACTED]".into(),
            ));
        }

        None
    }

    /// Validate US SSN parts according to SSA rules (no 000/666/9xx area, no 00 group, no 0000 serial).
    pub fn is_valid_ssn(ssn: &str) -> bool {
        let parts: Vec<&str> = ssn.split('-').collect();
        if parts.len() != 3 {
            return false;
        }
        let (area, group, serial) = (parts[0], parts[1], parts[2]);
        if area == "000" || area == "666" || area.starts_with('9') {
            return false;
        }
        if group == "00" || serial == "0000" {
            return false;
        }
        true
    }

    /// Luhn algorithm (mod 10 checksum) for credit cards.
    pub fn luhn_check(digits: &str) -> bool {
        let mut sum = 0u32;
        let mut alternate = false;

        for ch in digits.chars().rev() {
            let Some(mut d) = ch.to_digit(10) else {
                return false;
            };

            if alternate {
                d *= 2;
                if d > 9 {
                    d -= 9;
                }
            }

            sum += d;
            alternate = !alternate;
        }

        sum.is_multiple_of(10)
    }

    /// Redact a token: show prefix and last 3 chars.
    fn redact_token(tok: &str) -> String {
        if tok.len() <= 8 {
            "****[REDACTED]".into()
        } else {
            let prefix_len = tok.find('_').map(|i| i + 1).unwrap_or(4).min(6);
            let prefix = &tok[..prefix_len];
            let suffix = &tok[tok.len() - 3..];
            format!("{}...{}", prefix, suffix)
        }
    }

    /// Heuristic for passwords.
    fn is_high_entropy_password(s: &str) -> bool {
        // Explicit password prefix like "password: " or "pwd="
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("password=")
            || lower.starts_with("password: ")
            || lower.starts_with("pwd=")
        {
            return true;
        }

        // Length between 12 and 64, no spaces, mixed character classes
        if s.len() < 12 || s.len() > 64 || s.contains(' ') || s.contains('\n') || s.contains('\t') {
            return false;
        }

        let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = s.chars().any(|c| c.is_ascii_lowercase());
        let has_digit = s.chars().any(|c| c.is_ascii_digit());
        let has_special = s.chars().any(|c| c.is_ascii_punctuation());

        let class_count = [has_upper, has_lower, has_digit, has_special]
            .iter()
            .filter(|&&b| b)
            .count();

        // 3 or more character classes and length >= 14
        class_count >= 3 && s.len() >= 14
    }
}

/// Pure store managing clipboard sensitivity, read alerts, and auto-clear scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardPrivacyStore {
    current_hash: Option<u64>,
    copied_at_ms: u64,
    source_app: Option<AppIdentifier>,
    sensitive_kind: Option<SensitiveKind>,
    redacted_snippet: Option<String>,
    auto_clear_timeout_ms: Option<u64>,
    is_cleared: bool,
    whitelisted_apps: HashSet<String>,
    next_alert_id: u64,
    next_command_id: u64,
    alerts_history: Vec<ClipboardReadAlert>,
}

impl Default for ClipboardPrivacyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardPrivacyStore {
    pub fn new() -> Self {
        let mut whitelisted = HashSet::new();
        // Whitelist common password managers and trusted system shells
        whitelisted.insert("1password".into());
        whitelisted.insert("bitwarden".into());
        whitelisted.insert("keepass".into());
        whitelisted.insert("keepassxc".into());
        whitelisted.insert("nexus-os".into());

        Self {
            current_hash: None,
            copied_at_ms: 0,
            source_app: None,
            sensitive_kind: None,
            redacted_snippet: None,
            auto_clear_timeout_ms: None,
            is_cleared: false,
            whitelisted_apps: whitelisted,
            next_alert_id: 1,
            next_command_id: 1,
            alerts_history: Vec::new(),
        }
    }

    /// Add an app display name to the exclusion whitelist.
    pub fn add_whitelisted_app(&mut self, app_name: &str) {
        self.whitelisted_apps.insert(app_name.to_ascii_lowercase());
    }

    /// Check if an app is whitelisted.
    pub fn is_app_whitelisted(&self, app: &AppIdentifier) -> bool {
        self.whitelisted_apps
            .contains(&app.display_name().to_ascii_lowercase())
    }

    /// Simple string hash helper for pure model tracking.
    fn hash_text(text: &str) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    /// Record a copy operation to the clipboard.
    pub fn record_copy(
        &mut self,
        text: &str,
        source_app: Option<AppIdentifier>,
        now_ms: u64,
    ) -> Option<SensitiveCopyDetected> {
        let detector = SensitivePatternDetector::new();
        let hash = Self::hash_text(text);

        self.current_hash = Some(hash);
        self.copied_at_ms = now_ms;
        self.source_app = source_app.clone();
        self.is_cleared = false;

        // Check if source app is whitelisted password manager
        let is_source_whitelisted = source_app
            .as_ref()
            .map(|a| self.is_app_whitelisted(a))
            .unwrap_or(false);

        if let Some((kind, redacted)) = detector.detect(text) {
            let timeout = kind.default_timeout_ms();
            self.sensitive_kind = Some(kind);
            self.redacted_snippet = Some(redacted.clone());
            self.auto_clear_timeout_ms = Some(timeout);

            Some(SensitiveCopyDetected {
                kind,
                redacted_snippet: redacted,
                auto_clear_timeout_ms: timeout,
            })
        } else if is_source_whitelisted {
            // Even if general pattern didn't match, copy from password manager is marked sensitive
            let kind = SensitiveKind::HighEntropyPassword;
            let timeout = kind.default_timeout_ms();
            let redacted: String = "********[PASSWORD MANAGER]".into();
            self.sensitive_kind = Some(kind);
            self.redacted_snippet = Some(redacted.clone());
            self.auto_clear_timeout_ms = Some(timeout);

            Some(SensitiveCopyDetected {
                kind,
                redacted_snippet: redacted,
                auto_clear_timeout_ms: timeout,
            })
        } else {
            self.sensitive_kind = None;
            self.redacted_snippet = None;
            self.auto_clear_timeout_ms = None;
            None
        }
    }

    /// Record an app reading the clipboard and emit an alert if sensitive.
    pub fn record_read(
        &mut self,
        reader_app: AppIdentifier,
        now_ms: u64,
    ) -> Option<ClipboardReadAlert> {
        if self.is_cleared {
            return None;
        }

        // If source app is same as reader app, or reader is whitelisted, skip alert
        if self.is_app_whitelisted(&reader_app) {
            return None;
        }
        if let Some(src) = &self.source_app {
            if src == &reader_app {
                return None;
            }
        }

        // If sensitive content is on clipboard, emit alert
        if let (Some(kind), Some(snippet)) = (&self.sensitive_kind, &self.redacted_snippet) {
            let severity = match kind {
                SensitiveKind::CreditCard | SensitiveKind::SocialSecurityNumber => {
                    AlertSeverity::Critical
                }
                _ => AlertSeverity::Warning,
            };

            let alert = ClipboardReadAlert {
                alert_id: self.next_alert_id,
                reading_app: reader_app,
                sensitive_kind: *kind,
                redacted_snippet: snippet.clone(),
                severity,
                read_timestamp_ms: now_ms,
                source_app: self.source_app.clone(),
            };
            self.next_alert_id += 1;
            self.alerts_history.push(alert.clone());
            Some(alert)
        } else {
            None
        }
    }

    /// Check if timed auto-clear has elapsed and return reversible command.
    pub fn check_auto_clear(&mut self, now_ms: u64) -> Option<ClipboardClearCommand> {
        if self.is_cleared {
            return None;
        }

        let timeout = self.auto_clear_timeout_ms?;
        let hash = self.current_hash?;
        let kind = self.sensitive_kind?;
        let snippet = self.redacted_snippet.clone().unwrap_or_default();

        if now_ms >= self.copied_at_ms.saturating_add(timeout) {
            self.is_cleared = true;
            let cmd = ClipboardClearCommand {
                command_id: self.next_command_id,
                content_hash: hash,
                sensitive_kind: kind,
                reason: format!("Auto-clear expired for sensitive {}", kind),
                scheduled_at_ms: self.copied_at_ms,
                timeout_ms: timeout,
                backup_redacted: snippet,
            };
            self.next_command_id += 1;
            Some(cmd)
        } else {
            None
        }
    }

    /// Apply an undo/restore command to re-enable clipboard state in pure model.
    pub fn apply_restore(&mut self, cmd: &ClipboardRestoreCommand) {
        if let Some(hash) = self.current_hash {
            if hash == cmd.content_hash {
                self.is_cleared = false;
            }
        }
    }

    /// Recent read alerts.
    pub fn alerts(&self) -> &[ClipboardReadAlert] {
        &self.alerts_history
    }

    /// Whether sensitive data is currently stored on clipboard.
    pub fn is_sensitive_active(&self) -> bool {
        !self.is_cleared && self.sensitive_kind.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn_check_validation() {
        // Valid Visa test numbers (standard test card numbers)
        assert!(SensitivePatternDetector::luhn_check("4532015112830366"));
        assert!(SensitivePatternDetector::luhn_check("49927398716"));

        // Invalid numbers (differ by 1 digit)
        assert!(!SensitivePatternDetector::luhn_check("4532015112830367"));
        assert!(!SensitivePatternDetector::luhn_check("1234567812345678"));
    }

    #[test]
    fn sensitive_pattern_detects_tokens_and_cards() {
        let detector = SensitivePatternDetector::new();

        // OpenAI token
        let text_openai = "Here is my key: sk-abcdefghijklmnopqrstuvwxyz1234567890 for testing";
        let res = detector.detect(text_openai);
        assert!(res.is_some());
        let (kind, snippet) = res.unwrap();
        assert_eq!(kind, SensitiveKind::ApiToken);
        assert!(snippet.starts_with("sk-"));

        // GitHub token
        let text_gh = "ghp_1234567890abcdefghijklmnopqrstuvwxyzAB";
        let (kind_gh, snippet_gh) = detector.detect(text_gh).unwrap();
        assert_eq!(kind_gh, SensitiveKind::ApiToken);
        assert!(snippet_gh.starts_with("ghp_"));

        // Credit Card
        let text_cc = "Payment card: 4532-0151-1283-0366 expires 12/28";
        let (kind_cc, snippet_cc) = detector.detect(text_cc).unwrap();
        assert_eq!(kind_cc, SensitiveKind::CreditCard);
        assert_eq!(snippet_cc, "****-****-****-0366");

        // SSN
        let text_ssn = "My SSN is 123-45-6789";
        let (kind_ssn, snippet_ssn) = detector.detect(text_ssn).unwrap();
        assert_eq!(kind_ssn, SensitiveKind::SocialSecurityNumber);
        assert_eq!(snippet_ssn, "***-**-6789");

        // Non-sensitive regular text
        assert!(detector.detect("Hello world this is normal text").is_none());
    }

    #[test]
    fn clipboard_store_alert_and_auto_clear_lifecycle() {
        let mut store = ClipboardPrivacyStore::new();
        let now_ms = 1_000_000;

        let src = AppIdentifier::from_desktop_path("C:\\App\\notes.exe");
        let reader = AppIdentifier::from_desktop_path("C:\\Untrusted\\snoop.exe");

        // 1. Copy sensitive credit card
        let copy_det = store.record_copy("Card 4532015112830366", Some(src.clone()), now_ms);
        assert!(copy_det.is_some());
        assert_eq!(copy_det.unwrap().kind, SensitiveKind::CreditCard);
        assert!(store.is_sensitive_active());

        // 2. Snoop app reads clipboard -> generates alert
        let alert = store.record_read(reader.clone(), now_ms + 1_000);
        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert_eq!(alert.reading_app, reader);
        assert_eq!(alert.sensitive_kind, SensitiveKind::CreditCard);
        assert_eq!(alert.severity, AlertSeverity::Critical);

        // 3. Auto-clear check before timeout -> None
        assert!(store.check_auto_clear(now_ms + 10_000).is_none());

        // 4. Auto-clear check after 30s timeout -> emits reversible command
        let clear_cmd = store.check_auto_clear(now_ms + 30_000);
        assert!(clear_cmd.is_some());
        let clear_cmd = clear_cmd.unwrap();
        assert_eq!(clear_cmd.sensitive_kind, SensitiveKind::CreditCard);
        assert!(!store.is_sensitive_active());

        // Test reversibility
        let restore_cmd = clear_cmd.reverse();
        assert_eq!(restore_cmd.content_hash, clear_cmd.content_hash);
        store.apply_restore(&restore_cmd);
        assert!(store.is_sensitive_active());
    }

    #[test]
    fn whitelisted_app_exclusion() {
        let mut store = ClipboardPrivacyStore::new();
        let now_ms = 1_000_000;
        let bitwarden =
            AppIdentifier::from_desktop_path("C:\\Program Files\\Bitwarden\\Bitwarden.exe");

        store.record_copy(
            "sk-abcdefghijklmnopqrstuvwxyz1234567890",
            Some(bitwarden.clone()),
            now_ms,
        );

        // Bitwarden reading clipboard does not trigger alert
        assert!(store.record_read(bitwarden, now_ms + 500).is_none());
    }

    #[test]
    fn aws_and_jwt_and_pem_detection() {
        let detector = SensitivePatternDetector::new();

        // AWS
        let (kind_aws, snippet_aws) = detector
            .detect("export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE")
            .unwrap();
        assert_eq!(kind_aws, SensitiveKind::ApiToken);
        assert!(snippet_aws.contains("AKIA"));

        // JWT
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let (kind_jwt, snippet_jwt) = detector.detect(jwt).unwrap();
        assert_eq!(kind_jwt, SensitiveKind::ApiToken);
        assert!(snippet_jwt.contains("JWT"));

        // PEM
        let pem =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0...\n-----END RSA PRIVATE KEY-----";
        let (kind_pem, snippet_pem) = detector.detect(pem).unwrap();
        assert_eq!(kind_pem, SensitiveKind::PrivateKey);
        assert!(snippet_pem.contains("PRIVATE KEY"));
    }

    #[test]
    fn invalid_ssn_rejection() {
        let detector = SensitivePatternDetector::new();
        // 000 area code is invalid
        assert!(detector.detect("SSN: 000-12-3456").is_none());
        // 666 area code is invalid
        assert!(detector.detect("SSN: 666-12-3456").is_none());
        // 900+ area code is invalid
        assert!(detector.detect("SSN: 923-12-3456").is_none());
        // 00 group is invalid
        assert!(detector.detect("SSN: 123-00-3456").is_none());
        // 0000 serial is invalid
        assert!(detector.detect("SSN: 123-45-0000").is_none());
    }

    #[test]
    fn password_heuristics() {
        let detector = SensitivePatternDetector::new();
        let (kind1, _) = detector.detect("password=SuperSecretP@ss123").unwrap();
        assert_eq!(kind1, SensitiveKind::HighEntropyPassword);

        let (kind2, _) = detector.detect("K8#mQ9$zL2!vX5").unwrap();
        assert_eq!(kind2, SensitiveKind::HighEntropyPassword);
    }
}
