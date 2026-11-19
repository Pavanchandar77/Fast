//! Grep-style guard: system mutations (registry, services, power, process
//! priority/affinity, privacy toggles, system files) must go through the
//! never-brick journal and the Platform trait. Direct Win32 / reg.exe /
//! sc.exe calls outside the os-core Platform and Win32Mutator adapters are
//! treated as bypasses.

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

fn is_allowlisted(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.contains("/tests/") || s.contains("/benches/") || s.contains("/examples/") {
        return true;
    }
    matches!(
        s.as_str(),
        "crates/os-core/src/effects.rs"
            | "crates/os-core/src/platform.rs"
            | "crates/os-core/src/process.rs"
            | "crates/os-core/src/jobs.rs"
            | "crates/os-core/src/window.rs"
            | "crates/os-core/src/hollow_shell.rs"
            | "crates/os-core/src/ntdll.rs"
            | "crates/os-core/src/recovery.rs"
    )
}

const MUTATION_MARKERS: &[&str] = &[
    "reg.exe",
    "sc.exe",
    "schtasks.exe",
    "SetPriorityClass(",
    "SetProcessAffinityMask(",
    "K32EmptyWorkingSet(",
    "HWND_TOPMOST",
    "PowerSetActiveScheme",
    "ChangeServiceConfig",
];

#[test]
fn system_mutations_do_not_bypass_journal_and_platform() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);

    let mut violations = Vec::new();
    for path in &files {
        if is_allowlisted(path, &root) {
            continue;
        }
        let raw = fs::read_to_string(path).unwrap_or_default();
        let src = strip_comments(&raw);
        for marker in MUTATION_MARKERS {
            if src.contains(marker) {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                violations.push(format!("{} contains `{marker}`", rel.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "system mutations must go through the journal and Platform trait:\n{}",
        violations.join("\n")
    );
}

#[test]
fn platform_trait_covers_required_mutation_surface() {
    let platform = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../nexus-model/src/platform.rs");
    let src = fs::read_to_string(&platform).expect("platform.rs");
    for method in [
        "fn set_process_priority",
        "fn set_process_affinity",
        "fn set_power_profile",
        "fn set_registry_value",
        "fn set_service_state",
        "fn set_privacy_toggle",
        "fn write_system_file",
        "fn set_always_on_top",
        "fn empty_process_working_set",
    ] {
        assert!(src.contains(method), "Platform trait missing `{method}`");
    }

    let mutation = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/mutation.rs");
    let src = fs::read_to_string(&mutation).expect("mutation.rs");
    for kind in [
        "ProcessPriority",
        "ProcessAffinity",
        "PowerPlan",
        "RegistryValue",
        "ServiceState",
        "PrivacyToggle",
        "SystemFile",
        "AlwaysOnTop",
        "EcoQos",
    ] {
        assert!(
            src.contains(&format!("{kind} {{")) || src.contains(&format!("{kind},")),
            "journal Mutation missing `{kind}`"
        );
    }
}
