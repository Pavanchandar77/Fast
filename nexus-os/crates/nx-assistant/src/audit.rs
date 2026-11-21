//! Append-only assistant audit log (JSON lines) next to the privacy audit data.
//! Every proposed, confirmed, declined, blocked, or executed action lands here.

use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;

/// `%LOCALAPPDATA%\Nexus` on Windows, `$XDG_DATA_HOME/Nexus` (or
/// `~/.local/share/Nexus`) elsewhere.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NEXUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(l).join("Nexus");
        }
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join("Nexus");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local").join("share").join("Nexus")
}

pub fn audit_path() -> PathBuf {
    data_dir().join("audit").join("assistant.jsonl")
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct AuditRecord<'a> {
    pub ts: u64,
    pub event: &'a str,
    pub tool: &'a str,
    pub detail: &'a str,
    pub network: bool,
    pub via: &'a str,
}

pub fn log(event: &str, tool: &str, detail: &str, network: bool, via: &str) {
    let rec = AuditRecord {
        ts: crate::exec::unix_secs(),
        event,
        tool,
        detail,
        network,
        via,
    };
    let path = audit_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(&rec) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Last `n` audit lines (newest last), for the Privacy view.
pub fn tail(n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(audit_path()).unwrap_or_default();
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}
