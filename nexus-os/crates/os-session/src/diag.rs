//! Startup diagnostics: a log file next to the user's data, panic capture,
//! and the `--safe` switch.
//!
//! Release builds have no console, so without this file a real-PC failure
//! leaves no trace. The log lives at `%LOCALAPPDATA%\Nexus\logs\nexus.log`
//! (`$XDG_STATE_HOME`/`~/.local/state/nexus/logs` elsewhere) and rolls over
//! to `nexus.old.log` past 4 MB.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static SAFE_MODE: AtomicBool = AtomicBool::new(false);
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

const ROLL_BYTES: u64 = 4 * 1024 * 1024;

/// Directory holding `nexus.log`.
pub fn log_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("Nexus").join("logs");
        }
    }
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state).join("nexus").join("logs");
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("nexus")
            .join("logs");
    }
    std::env::temp_dir().join("nexus-logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join("nexus.log")
}

/// `--safe` on the command line or `NEXUS_SAFE=1`: no motion overlay, no
/// foreign-window effects, no global keyboard hook, no window manager tiling.
pub fn parse_safe_mode<I: IntoIterator<Item = String>>(args: I, env: Option<&str>) -> bool {
    args.into_iter().any(|a| a == "--safe" || a == "/safe")
        || matches!(env, Some(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn set_safe_mode(on: bool) {
    SAFE_MODE.store(on, Ordering::SeqCst);
}

pub fn safe_mode() -> bool {
    SAFE_MODE.load(Ordering::SeqCst)
}

/// Open (and roll) the log file. Returns its path on success.
pub fn open_log_file() -> Option<PathBuf> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("nexus.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROLL_BYTES) {
        let _ = std::fs::rename(&path, dir.join("nexus.old.log"));
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    if let Ok(mut slot) = LOG_FILE.lock() {
        *slot = Some(file);
    }
    Some(path)
}

/// `tracing` writer that appends to the log file (and mirrors to stderr).
#[derive(Clone, Copy, Default)]
pub struct LogWriter;

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Ok(mut slot) = LOG_FILE.lock() {
            if let Some(f) = slot.as_mut() {
                let _ = f.write_all(buf);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Ok(mut slot) = LOG_FILE.lock() {
            if let Some(f) = slot.as_mut() {
                let _ = f.flush();
            }
        }
        Ok(())
    }
}

/// Append one raw line, bypassing `tracing` (used from the panic hook,
/// where the subscriber may be poisoned).
pub fn raw_line(line: &str) {
    if let Ok(mut slot) = LOG_FILE.lock() {
        if let Some(f) = slot.as_mut() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

/// Chain a hook in front of the current panic hook that records the panic
/// (thread, location, message, backtrace) in the log file.
pub fn install_panic_logging() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        raw_line(&format!("PANIC on thread '{thread}': {info}\n{bt}"));
        previous(info);
    }));
}

/// Run `f`, logging how long it took when it is slow (>= `warn_ms`).
pub fn timed<T>(what: &str, warn_ms: u128, f: impl FnOnce() -> T) -> T {
    let t = std::time::Instant::now();
    let out = f();
    let ms = t.elapsed().as_millis();
    if ms >= warn_ms {
        tracing::warn!("slow: {what} took {ms} ms");
    } else {
        tracing::debug!("{what} took {ms} ms");
    }
    out
}

/// Log how long one shell window took to open (nexus.log startup profile).
pub fn window_opened(name: &str, since: std::time::Instant) {
    let ms = since.elapsed().as_millis();
    if ms >= 120 {
        tracing::warn!("startup: window '{name}' took {ms} ms to open");
    } else {
        tracing::info!("startup: window '{name}' opened in {ms} ms");
    }
    if std::env::var_os("NEXUS_STARTUP_TRACE").is_some() {
        eprintln!("STARTUP_WINDOW {name} {ms}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_mode_flag_and_env() {
        assert!(parse_safe_mode(vec!["x".into(), "--safe".into()], None));
        assert!(parse_safe_mode(Vec::<String>::new(), Some("1")));
        assert!(!parse_safe_mode(vec!["--dry-run".into()], Some("0")));
    }

    #[test]
    fn log_lives_under_nexus_logs() {
        let p = log_path();
        assert!(p.ends_with("nexus.log"));
        assert!(p.parent().is_some_and(|d| d.ends_with("logs")));
    }
}
