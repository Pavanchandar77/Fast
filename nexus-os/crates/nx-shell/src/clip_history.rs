//! Clipboard history: a background watcher records text you copy (skipping
//! anything a password manager marks private), Spotlight lists it under the
//! `cb` prefix (Ctrl+Alt+V opens it), and picking an entry puts it back on
//! the clipboard so Ctrl+V pastes it. History stays in memory only.

use nx_power::{ClipboardKind, ClipboardStore, NewClipboardItem};
use std::sync::{Mutex, OnceLock};

/// Spotlight prefix that switches to clipboard history.
pub const PREFIX: &str = "cb";
const CAPACITY: usize = 50;
/// Longest text kept per entry (bytes); bigger copies are skipped.
const MAX_BYTES: usize = 64 * 1024;

fn store() -> &'static Mutex<ClipboardStore> {
    static STORE: OnceLock<Mutex<ClipboardStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ClipboardStore::with_default_exclusions(CAPACITY)))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record one copied text. Returns false when it was empty, too big or
/// rejected by an exclusion rule.
pub fn record(text: &str, source_app: Option<String>) -> bool {
    if text.trim().is_empty() || text.len() > MAX_BYTES {
        return false;
    }
    let Ok(mut store) = store().lock() else {
        return false;
    };
    store
        .push(NewClipboardItem {
            kind: ClipboardKind::PlainText,
            content: text.to_string(),
            timestamp_ms: now_ms(),
            source_app,
            sensitive_flag: false,
        })
        .is_ok()
}

/// Newest-first (id, text).
pub fn entries(limit: usize) -> Vec<(u64, String)> {
    matching("", limit)
}

/// Newest-first (id, text) whose text contains `query` (case-insensitive).
pub fn matching(query: &str, limit: usize) -> Vec<(u64, String)> {
    let Ok(store) = store().lock() else {
        return Vec::new();
    };
    store
        .search(query)
        .into_iter()
        .take(limit)
        .map(|e| (e.id, e.content.clone()))
        .collect()
}

/// One-line preview of a clip for a result row.
pub fn preview(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 90 {
        let cut: String = line.chars().take(89).collect();
        format!("{cut}…")
    } else {
        line
    }
}

/// Text of a history id (`clip:<n>`).
pub fn text_of(id: &str) -> Option<String> {
    let n: u64 = id.strip_prefix("clip:")?.parse().ok()?;
    let store = store().lock().ok()?;
    store.get(n).map(|e| e.content.clone())
}

/// Put a history entry back on the clipboard (it moves to the top).
pub fn copy_back(id: &str) -> bool {
    let Some(text) = text_of(id) else {
        return false;
    };
    if let (Some(n), Ok(mut store)) = (
        id.strip_prefix("clip:").and_then(|n| n.parse::<u64>().ok()),
        store().lock(),
    ) {
        store.delete(n);
    }
    record(&text, None);
    set_clipboard_text(&text)
}

#[cfg(not(windows))]
fn set_clipboard_text(_text: &str) -> bool {
    false
}

#[cfg(windows)]
fn set_clipboard_text(text: &str) -> bool {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = (|| -> Option<()> {
            let mem: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let dst = GlobalLock(mem) as *mut u16;
            if dst.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_UNICODETEXT, HANDLE(mem.0)).ok()?;
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

/// Start the watcher thread (Windows). Polls the clipboard sequence number,
/// so it costs nothing while you are not copying.
pub fn start_watcher() {
    #[cfg(windows)]
    {
        let _ = std::thread::Builder::new()
            .name("nexus-clipboard".into())
            .spawn(|| {
                let mut last = 0u32;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(350));
                    let seq = unsafe {
                        windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                    };
                    if seq == last {
                        continue;
                    }
                    last = seq;
                    if let Some(text) = read_clipboard_text() {
                        record(&text, None);
                    }
                }
            });
    }
}

#[cfg(windows)]
fn read_clipboard_text() -> Option<String> {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        // Password managers and secure fields opt out of history.
        for fmt in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("Clipboard Viewer Ignore"),
        ] {
            let id = RegisterClipboardFormatW(fmt);
            if id != 0 && IsClipboardFormatAvailable(id).is_ok() {
                return None;
            }
        }
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let text = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            let mem = HGLOBAL(handle.0);
            let size = GlobalSize(mem) / 2;
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() || size == 0 || size > MAX_BYTES {
                let _ = GlobalUnlock(mem);
                return None;
            }
            let slice = std::slice::from_raw_parts(ptr, size);
            let end = slice.iter().position(|&c| c == 0).unwrap_or(size);
            let s = String::from_utf16_lossy(&slice[..end]);
            let _ = GlobalUnlock(mem);
            Some(s)
        })();
        let _ = CloseClipboard();
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_dedupes_previews_and_resolves() {
        assert!(!record("   ", None));
        assert!(record("hello world", None));
        assert!(record("hello world", None));
        assert!(record("second\nline", None));
        let all = entries(10);
        assert_eq!(all[0].1, "second\nline");
        assert_eq!(all.iter().filter(|e| e.1 == "hello world").count(), 1);
        let id = format!("clip:{}", all[0].0);
        assert_eq!(text_of(&id).as_deref(), Some("second\nline"));
        assert_eq!(preview("a\n  b\tc"), "a b c");
        assert!(preview(&"x".repeat(200)).ends_with('…'));
    }
}
