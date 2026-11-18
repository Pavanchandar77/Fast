use std::collections::BTreeSet;

use crate::error::ShieldError;
use crate::plan::MutationStep;
use crate::system::SystemBackend;

pub const BLOCK_START_MARKER: &str = "# === BEGIN NEXUS SHIELD BLOCKLIST (DO NOT EDIT) ===";
pub const BLOCK_END_MARKER: &str = "# === END NEXUS SHIELD BLOCKLIST ===";

pub struct HostsPlanner;

impl HostsPlanner {
    /// Inspects the current hosts file and produces a planned mutation step
    /// if and only if changes are required. Idempotent: returns Ok(None) if
    /// the block already matches the requested domains.
    pub fn plan_update(
        backend: &impl SystemBackend,
        domains: &[String],
    ) -> Result<Option<MutationStep>, ShieldError> {
        let current_content = backend.read_hosts_file()?;
        let (new_content, changed) = Self::generate_updated_content(&current_content, domains);

        if !changed {
            return Ok(None);
        }

        Ok(Some(MutationStep::UpdateHosts {
            new_content,
            domains_count: domains.len(),
            description: format!("Update hosts file blocklist with {} domains", domains.len()),
        }))
    }

    /// Plans the clean removal of the Nexus Shield managed block, restoring
    /// all user entries without touching them.
    pub fn plan_removal(backend: &impl SystemBackend) -> Result<Option<MutationStep>, ShieldError> {
        let current_content = backend.read_hosts_file()?;
        let (new_content, changed) = Self::generate_removed_content(&current_content);

        if !changed {
            return Ok(None);
        }

        Ok(Some(MutationStep::UpdateHosts {
            new_content,
            domains_count: 0,
            description: "Remove Nexus Shield managed block from hosts file".into(),
        }))
    }

    /// Generates updated hosts file content.
    /// Returns `(updated_content, changed)`.
    pub fn generate_updated_content(current: &str, domains: &[String]) -> (String, bool) {
        let is_crlf = current.contains("\r\n");
        let newline = if is_crlf { "\r\n" } else { "\n" };

        let mut dedup_domains: Vec<String> = domains
            .iter()
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        dedup_domains.sort();

        // Build the block content
        let mut block = String::new();
        block.push_str(BLOCK_START_MARKER);
        block.push_str(newline);
        for domain in &dedup_domains {
            block.push_str("0.0.0.0 ");
            block.push_str(domain);
            block.push_str(newline);
        }
        block.push_str(BLOCK_END_MARKER);
        block.push_str(newline);

        // Check if an existing managed block is present
        if let Some(start_idx) = current.find(BLOCK_START_MARKER) {
            if let Some(end_marker_rel) = current[start_idx..].find(BLOCK_END_MARKER) {
                let end_idx = start_idx + end_marker_rel + BLOCK_END_MARKER.len();

                // Advance past trailing newline
                let end_with_newline = if current[end_idx..].starts_with("\r\n") {
                    end_idx + 2
                } else if current[end_idx..].starts_with('\n') {
                    end_idx + 1
                } else {
                    end_idx
                };

                let before = &current[..start_idx];
                let after = &current[end_with_newline..];

                let mut result = String::with_capacity(before.len() + block.len() + after.len());
                result.push_str(before);
                result.push_str(&block);
                result.push_str(after);

                let changed = result != current;
                return (result, changed);
            }
        }

        // No existing block: append to the end
        let mut result = current.to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push_str(newline);
        }
        if !result.is_empty() && !result.ends_with(&format!("{newline}{newline}")) {
            result.push_str(newline);
        }
        result.push_str(&block);

        let changed = result != current;
        (result, changed)
    }

    /// Strips the managed block completely from the hosts content.
    pub fn generate_removed_content(current: &str) -> (String, bool) {
        if let Some(start_idx) = current.find(BLOCK_START_MARKER) {
            if let Some(end_marker_rel) = current[start_idx..].find(BLOCK_END_MARKER) {
                let end_idx = start_idx + end_marker_rel + BLOCK_END_MARKER.len();

                let end_with_newline = if current[end_idx..].starts_with("\r\n") {
                    end_idx + 2
                } else if current[end_idx..].starts_with('\n') {
                    end_idx + 1
                } else {
                    end_idx
                };

                let before = &current[..start_idx];
                let after = &current[end_with_newline..];

                let mut result = format!(
                    "{}{}",
                    before.trim_end(),
                    if after.is_empty() { "\n" } else { "\n\n" }
                );
                if !after.is_empty() {
                    result.push_str(after.trim_start());
                }

                if current.contains("\r\n") {
                    result = result.replace('\n', "\r\n");
                }

                return (result, true);
            }
        }

        (current.to_string(), false)
    }
}
