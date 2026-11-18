use crate::error::{PowerError, PowerResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Format or kind of clipboard content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardKind {
    PlainText,
    RichText,
    Image {
        width: u32,
        height: u32,
        format: String,
        bytes_len: usize,
    },
    FileList(Vec<String>),
}

/// An entry stored in the clipboard history ring buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: u64,
    pub kind: ClipboardKind,
    pub content: String,
    pub timestamp_ms: u64,
    pub source_app: Option<String>,
    pub pinned: bool,
    pub sensitive_flag: bool,
}

/// Incoming candidate to be added to clipboard history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewClipboardItem {
    pub kind: ClipboardKind,
    pub content: String,
    pub timestamp_ms: u64,
    pub source_app: Option<String>,
    pub sensitive_flag: bool,
}

/// Privacy exclusion rule preventing secrets or sensitive data from entering history.
#[derive(Debug, Clone)]
pub enum ExclusionRule {
    /// Blocks entries whose source app contains this substring (case-insensitive).
    SourceAppSubstring(String),
    /// Blocks entries whose content matches this regular expression.
    ContentPattern(Regex, String),
    /// Blocks any entry flagged with CF_CLIPBOARD_VIEWER_IGNORE or equivalent.
    HonorSensitiveFlag,
    /// Blocks entries exceeding a maximum length.
    MaxLength(usize),
}

impl ExclusionRule {
    pub fn matches(&self, item: &NewClipboardItem) -> Option<String> {
        match self {
            Self::SourceAppSubstring(sub) => {
                if let Some(app) = &item.source_app {
                    if app.to_ascii_lowercase().contains(&sub.to_ascii_lowercase()) {
                        return Some(format!("Source app '{}' matches exclusion '{}'", app, sub));
                    }
                }
                None
            }
            Self::ContentPattern(re, desc) => {
                if re.is_match(&item.content) {
                    Some(format!("Content matches pattern rule: {}", desc))
                } else {
                    None
                }
            }
            Self::HonorSensitiveFlag => {
                if item.sensitive_flag {
                    Some("Item is flagged with CF_CLIPBOARD_VIEWER_IGNORE / sensitive".to_string())
                } else {
                    None
                }
            }
            Self::MaxLength(max) => {
                if item.content.len() > *max {
                    Some(format!(
                        "Content length {} exceeds max {}",
                        item.content.len(),
                        max
                    ))
                } else {
                    None
                }
            }
        }
    }
}

/// Filter criteria for querying clipboard history.
#[derive(Debug, Clone, Default)]
pub struct ClipboardFilter {
    pub query: Option<String>,
    pub pinned_only: bool,
    pub kind: Option<ClipboardKindFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKindFilter {
    Text,
    Image,
    Files,
}

/// Ring-buffer store for clipboard history with privacy filtering, search, and pin protection.
#[derive(Debug, Clone)]
pub struct ClipboardStore {
    capacity: usize,
    items: VecDeque<ClipboardEntry>,
    next_id: u64,
    exclusion_rules: Vec<ExclusionRule>,
}

impl ClipboardStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            items: VecDeque::new(),
            next_id: 1,
            exclusion_rules: Vec::new(),
        }
    }

    /// Creates a store pre-configured with standard security and privacy exclusion rules:
    /// - Password managers (1Password, Bitwarden, KeePass, LastPass, Dashlane, Enpass)
    /// - Sensitive OS privacy flags (CF_CLIPBOARD_VIEWER_IGNORE)
    /// - Private keys (PEM headers)
    /// - Credit card numbers
    /// - JWT tokens
    /// - API keys / bearer tokens
    pub fn with_default_exclusions(capacity: usize) -> Self {
        let mut store = Self::new(capacity);

        // Source app exclusions
        let password_managers = [
            "1password",
            "bitwarden",
            "keepass",
            "lastpass",
            "dashlane",
            "enpass",
            "authenticator",
        ];
        for pm in password_managers {
            store.add_exclusion_rule(ExclusionRule::SourceAppSubstring(pm.to_string()));
        }

        // Sensitive flags
        store.add_exclusion_rule(ExclusionRule::HonorSensitiveFlag);

        // Private key pattern
        if let Ok(re) = Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----") {
            store.add_exclusion_rule(ExclusionRule::ContentPattern(
                re,
                "Private Key PEM".to_string(),
            ));
        }

        // Credit cards (13-19 digits with optional spaces or dashes)
        if let Ok(re) = Regex::new(r"\b(?:\d{4}[ -]?){3}\d{4}\b|\b\d{13,19}\b") {
            store.add_exclusion_rule(ExclusionRule::ContentPattern(
                re,
                "Credit Card Number".to_string(),
            ));
        }

        // JWT tokens
        if let Ok(re) = Regex::new(r"\beyJ[a-zA-Z0-9_-]+\.eyJ[a-zA-Z0-9_-]+\.[a-zA-Z0-9_-]+\b") {
            store.add_exclusion_rule(ExclusionRule::ContentPattern(re, "JWT Token".to_string()));
        }

        // Bearer / API secrets
        if let Ok(re) = Regex::new(r"(?i)\b(?:bearer\s+[a-zA-Z0-9_\-\.]{20,}|AKIA[0-9A-Z]{16})\b") {
            store.add_exclusion_rule(ExclusionRule::ContentPattern(
                re,
                "API / Bearer Secret".to_string(),
            ));
        }

        store
    }

    pub fn add_exclusion_rule(&mut self, rule: ExclusionRule) {
        self.exclusion_rules.push(rule);
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn unpinned_count(&self) -> usize {
        self.items.iter().filter(|i| !i.pinned).count()
    }

    pub fn pinned_count(&self) -> usize {
        self.items.iter().filter(|i| i.pinned).count()
    }

    /// Push a new clipboard item into history.
    ///
    /// Validates item against all exclusion rules. If rejected, returns `Err(PowerError::ClipboardRejected)`.
    /// If identical to the most recent item, updates the timestamp instead of duplicating.
    /// Enforces the ring buffer capacity: evicts oldest unpinned item if unpinned count exceeds capacity.
    pub fn push(&mut self, item: NewClipboardItem) -> PowerResult<u64> {
        // 1. Check exclusion rules
        for rule in &self.exclusion_rules {
            if let Some(reason) = rule.matches(&item) {
                return Err(PowerError::ClipboardRejected(reason));
            }
        }

        // 2. Deduplication check against the most recent entry
        if let Some(front) = self.items.front_mut() {
            if front.kind == item.kind && front.content == item.content {
                front.timestamp_ms = item.timestamp_ms;
                front.source_app = item.source_app;
                return Ok(front.id);
            }
        }

        // 3. Insert new entry at the front
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let entry = ClipboardEntry {
            id,
            kind: item.kind,
            content: item.content,
            timestamp_ms: item.timestamp_ms,
            source_app: item.source_app,
            pinned: false,
            sensitive_flag: item.sensitive_flag,
        };

        self.items.push_front(entry);

        // 4. Ring buffer eviction: evict oldest unpinned if over capacity
        self.enforce_capacity();

        Ok(id)
    }

    fn enforce_capacity(&mut self) {
        while self.unpinned_count() > self.capacity {
            // Find the oldest unpinned item from the back
            if let Some(pos) = self.items.iter().rposition(|i| !i.pinned) {
                self.items.remove(pos);
            } else {
                break;
            }
        }
    }

    pub fn pin(&mut self, id: u64) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.pinned = true;
            true
        } else {
            false
        }
    }

    pub fn unpin(&mut self, id: u64) -> bool {
        let mut changed = false;
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.pinned = false;
            changed = true;
        }
        if changed {
            self.enforce_capacity();
        }
        changed
    }

    pub fn delete(&mut self, id: u64) -> Option<ClipboardEntry> {
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            self.items.remove(pos)
        } else {
            None
        }
    }

    pub fn clear_unpinned(&mut self) -> usize {
        let prev_len = self.items.len();
        self.items.retain(|i| i.pinned);
        prev_len.saturating_sub(self.items.len())
    }

    pub fn clear_all(&mut self) {
        self.items.clear();
    }

    pub fn get(&self, id: u64) -> Option<&ClipboardEntry> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn search(&self, query: &str) -> Vec<&ClipboardEntry> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.items.iter().collect();
        }
        self.items
            .iter()
            .filter(|i| {
                i.content.to_ascii_lowercase().contains(&q)
                    || i.source_app
                        .as_ref()
                        .map(|s| s.to_ascii_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .collect()
    }

    pub fn filter(&self, filter: &ClipboardFilter) -> Vec<&ClipboardEntry> {
        self.items
            .iter()
            .filter(|i| {
                if filter.pinned_only && !i.pinned {
                    return false;
                }
                if let Some(ref kind_filter) = filter.kind {
                    match (kind_filter, &i.kind) {
                        (
                            ClipboardKindFilter::Text,
                            ClipboardKind::PlainText | ClipboardKind::RichText,
                        ) => {}
                        (ClipboardKindFilter::Image, ClipboardKind::Image { .. }) => {}
                        (ClipboardKindFilter::Files, ClipboardKind::FileList(_)) => {}
                        _ => return false,
                    }
                }
                if let Some(ref q) = filter.query {
                    let needle = q.trim().to_ascii_lowercase();
                    if !needle.is_empty() {
                        let text_match = i.content.to_ascii_lowercase().contains(&needle);
                        let app_match = i
                            .source_app
                            .as_ref()
                            .map(|s| s.to_ascii_lowercase().contains(&needle))
                            .unwrap_or(false);
                        if !text_match && !app_match {
                            return false;
                        }
                    }
                }
                true
            })
            .collect()
    }
}
