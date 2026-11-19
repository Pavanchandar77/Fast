//! Grep-style guard: `NtSuspendProcess` is not reachable unless the operator
//! set `allow_nt_suspend` (default false). Also checks every `SetWindowPos`
//! on a foreign window passes `SWP_ASYNCWINDOWPOS`.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut block = false;
    while let Some(c) = chars.next() {
        if block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block = false;
                out.push(' ');
            } else if c == '\n' {
                out.push('\n');
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block = true;
            continue;
        }
        out.push(c);
    }
    out
}

struct Function {
    name: String,
    body: String,
}

fn functions(src: &str) -> Vec<Function> {
    let mut fns = Vec::new();
    let mut current_name = String::from("<prelude>");
    let mut current = String::new();
    for line in src.lines() {
        if let Some(name) = fn_name(line) {
            fns.push(Function {
                name: std::mem::take(&mut current_name),
                body: std::mem::take(&mut current),
            });
            current_name = name.to_string();
        }
        current.push_str(line);
        current.push('\n');
    }
    fns.push(Function {
        name: current_name,
        body: current,
    });
    fns
}

fn fn_name(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
        return None;
    }
    let mut t = t;
    for prefix in ["pub(crate) ", "pub ", "unsafe "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest;
        }
    }
    // `pub unsafe fn` has both prefixes; strip a second time.
    for prefix in ["pub(crate) ", "pub ", "unsafe "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest;
        }
    }
    let t = t.strip_prefix("fn ")?;
    if t.starts_with('<') {
        return None;
    }
    let end = t.find(['(', '<']).unwrap_or(t.len());
    let name = t[..end].trim();
    if name.is_empty() || name.contains(' ') {
        None
    } else {
        Some(name)
    }
}

#[test]
fn nt_suspend_process_is_not_reachable_by_default() {
    let root = workspace_root();
    let crates = root.join("crates");
    let mut files = Vec::new();
    for name in [
        "os-core",
        "os-governor",
        "os-compositor",
        "os-session",
        "os-shield",
        "os-config",
        "os-hal",
        "os-vfs",
    ] {
        rust_files(&crates.join(name), &mut files);
    }

    let mut violations = Vec::new();
    for path in &files {
        let raw = fs::read_to_string(path).unwrap_or_default();
        let src = strip_comments(&raw);
        for func in functions(&src) {
            let rel = path.strip_prefix(&root).unwrap_or(path);
            if func.body.contains("NtSuspendProcess")
                && func.name != "NtSuspendProcess"
                && func.name != "suspend"
            {
                violations.push(format!(
                    "{} function `{}` references NtSuspendProcess",
                    rel.display(),
                    func.name
                ));
            }
            if func.body.contains(".suspend(") && func.name != "suspend_if_allowed" {
                violations.push(format!(
                    "{} function `{}` calls .suspend()",
                    rel.display(),
                    func.name
                ));
            }
            let creates_suspend = func
                .body
                .lines()
                .any(|line| line.contains("Mutation::Suspend") && !line.contains("=>"));
            if creates_suspend && !func.body.contains("allow_nt_suspend") {
                violations.push(format!(
                    "{} function `{}` builds Mutation::Suspend without allow_nt_suspend",
                    rel.display(),
                    func.name
                ));
            }
        }
    }

    let process = fs::read_to_string(crates.join("os-core/src/process.rs")).expect("process.rs");
    let process = strip_comments(&process);
    let allowed = functions(&process);
    let gate = allowed
        .iter()
        .find(|f| f.name == "suspend_if_allowed")
        .expect("suspend_if_allowed");
    assert!(
        gate.body.contains("if !allow_nt_suspend"),
        "suspend_if_allowed must return before NtSuspendProcess when the flag is false"
    );

    let mut schema = fs::read_to_string(crates.join("os-config/src/schema.rs")).unwrap_or_default();
    if let Ok(model) = fs::read_to_string(crates.join("nexus-model/src/config.rs")) {
        schema.push('\n');
        schema.push_str(&model);
    }
    assert!(
        schema.contains("allow_nt_suspend: false"),
        "GovernorConfig::default must set allow_nt_suspend: false"
    );
    assert!(
        schema.contains("app_nap_enabled: false"),
        "app nap must default off"
    );

    let toml_path = root.join("config/system.toml");
    let toml = fs::read_to_string(&toml_path).expect("system.toml");
    assert!(
        !toml.contains("allow_nt_suspend = true"),
        "shipped config must not opt into NtSuspendProcess"
    );
    assert!(
        toml.contains("app_nap_enabled = false"),
        "shipped config must ship app nap off"
    );

    assert!(
        violations.is_empty(),
        "NtSuspendProcess is reachable by default:\n{}",
        violations.join("\n")
    );
}

#[test]
fn set_window_pos_on_foreign_windows_is_async() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let mut violations = Vec::new();
    for path in &files {
        if path.components().any(|c| c.as_os_str() == "nexus-journal") {
            continue;
        }
        let raw = fs::read_to_string(path).unwrap_or_default();
        let src = strip_comments(&raw);
        for func in functions(&src) {
            if func.body.contains("SetWindowPos(") && !func.body.contains("SWP_ASYNCWINDOWPOS") {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                violations.push(format!("{} `{}`", rel.display(), func.name));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "SetWindowPos without SWP_ASYNCWINDOWPOS:\n{}",
        violations.join("\n")
    );
}
