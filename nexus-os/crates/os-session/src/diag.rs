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
/// where the subscriber may be poisoned). Opens the log if nothing has yet
/// and syncs it to disk, because the process may be about to die.
pub fn raw_line(line: &str) {
    let need_open = LOG_FILE.lock().map(|s| s.is_none()).unwrap_or(false);
    if need_open {
        let _ = open_log_file();
    }
    let slot = match LOG_FILE.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut slot = slot;
    if let Some(f) = slot.as_mut() {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
        let _ = f.sync_all();
    }
}

static DIAG_MODE: AtomicBool = AtomicBool::new(false);
static FATAL_SHOWN: AtomicBool = AtomicBool::new(false);

/// `--diag`: console window plus verbose logging (Start menu "Nexus (diagnostic)").
pub fn parse_diag_mode<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|a| a == "--diag" || a == "/diag")
}

pub fn set_diag_mode(on: bool) {
    DIAG_MODE.store(on, Ordering::SeqCst);
}

pub fn diag_mode() -> bool {
    DIAG_MODE.load(Ordering::SeqCst)
}

/// Give a GUI-subsystem exe a console (for `--diag`). Returns true on success.
pub fn attach_console() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Console::{AllocConsole, SetConsoleTitleW};
        let ok = AllocConsole().is_ok();
        if ok {
            let _ = SetConsoleTitleW(windows::core::w!("Nexus (diagnostic)"));
        }
        ok
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Write `crash-<unix seconds>.txt` next to nexus.log and mirror it into the log.
pub fn write_crash(kind: &str, text: &str) -> Option<PathBuf> {
    raw_line(&format!("==== FATAL ({kind}) ====\n{text}"));
    let dir = log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("crash-{secs}.txt"));
    let body = format!(
        "Nexus {} crashed ({kind})\npid {}\nargs {:?}\n\n{text}\n\nFull log: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::args().collect::<Vec<_>>(),
        log_path().display()
    );
    let mut f = std::fs::File::create(&path).ok()?;
    f.write_all(body.as_bytes()).ok()?;
    let _ = f.sync_all();
    Some(path)
}

/// Tell the user, once per process: a message box on Windows (skipped in
/// `--safe`), stderr always (visible in `--diag`).
pub fn show_message(title: &str, text: &str, error: bool) {
    eprintln!("{title}: {text}");
    if safe_mode() {
        return;
    }
    // The box runs on its own thread: it has its own message queue, so it
    // never re-enters the (possibly panicking) UI thread's window procedures.
    #[cfg(windows)]
    {
        let (title, text) = (title.to_string(), text.to_string());
        let shown = std::thread::Builder::new()
            .name("nexus-fatal-box".into())
            .spawn(move || unsafe {
                use windows::core::HSTRING;
                use windows::Win32::UI::WindowsAndMessaging::{
                    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
                    MB_TOPMOST,
                };
                let icon = if error {
                    MB_ICONERROR
                } else {
                    MB_ICONINFORMATION
                };
                let _ = MessageBoxW(
                    None,
                    &HSTRING::from(text),
                    &HSTRING::from(title),
                    MB_OK | icon | MB_TOPMOST | MB_SETFOREGROUND,
                );
            });
        if let Ok(handle) = shown {
            let _ = handle.join();
        }
    }
    #[cfg(not(windows))]
    let _ = error;
}

/// A fatal stop: log it, write a crash file, and show the reason with the log
/// path (once). Nexus never exits without saying why.
pub fn fatal(kind: &str, reason: &str) {
    let crash = write_crash(kind, reason);
    if FATAL_SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut text = format!("Nexus stopped: {reason}\n\nLog: {}", log_path().display());
    if let Some(c) = crash {
        text.push_str(&format!("\nCrash report: {}", c.display()));
    }
    text.push_str(
        "\n\nThe Windows taskbar has been restored. Start menu > Nexus > \"Nexus (diagnostic)\" shows a live log.",
    );
    show_message("Nexus", &text, true);
    if diag_mode() {
        pause_console();
    }
}

/// In `--diag`, keep the console open until Enter so the log can be read.
pub fn pause_console() {
    static PAUSED: AtomicBool = AtomicBool::new(false);
    if diag_mode() && !PAUSED.swap(true, Ordering::SeqCst) {
        eprintln!("\nPress Enter to close this window.");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}

/// Last-chance handler for native crashes (access violations and other SEH
/// exceptions nobody caught): log, crash file, message box.
pub fn install_crash_filter() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::{
            SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
        };
        unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
            let (code, addr) = unsafe {
                info.as_ref()
                    .and_then(|p| p.ExceptionRecord.as_ref())
                    .map(|r| (r.ExceptionCode.0 as u32, r.ExceptionAddress as usize))
                    .unwrap_or((0, 0))
            };
            let bt = std::backtrace::Backtrace::force_capture();
            os_core::recovery::restore_best_effort();
            os_core::hollow_shell::HollowShellManager::restore_taskbar();
            fatal(
                "native exception",
                &format!("unhandled exception 0x{code:08X} at 0x{addr:X}\n{bt}"),
            );
            1 // EXCEPTION_EXECUTE_HANDLER: terminate now, the report is written
        }
        SetUnhandledExceptionFilter(Some(filter));
    }
}

/// Chain a hook in front of the current panic hook that records the panic
/// (thread, location, message, backtrace) in the log file. A panic that will
/// end the process (main thread, or one that cannot unwind) also writes a
/// crash file and shows the reason. Panics inside `nx_shell::guard` are
/// caught and only logged.
pub fn install_panic_logging() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!("PANIC on thread '{thread}': {info}\n{bt}");
        raw_line(&text);
        previous(info);
        let caught = nx_shell::guard::in_guard();
        let msg = info.to_string();
        if !caught && (thread == "main" || msg.contains("cannot unwind")) {
            fatal("panic", &format!("panic on thread '{thread}': {info}"));
        }
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
    fn diag_flag() {
        assert!(parse_diag_mode(["x".to_string(), "--diag".to_string()]));
        assert!(parse_diag_mode(["/diag".to_string()]));
        assert!(!parse_diag_mode(["--safe".to_string()]));
    }

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
