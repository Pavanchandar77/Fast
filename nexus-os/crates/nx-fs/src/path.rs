//! Path helpers: Windows long-path (`\\?\`) handling and display utilities.
//!
//! The string-level functions are platform-free so they are unit-tested on
//! Linux; [`to_native`] applies them only on Windows.

use std::path::{Path, PathBuf};

/// Maximum classic Win32 path length (`MAX_PATH`).
pub const MAX_PATH: usize = 260;

/// Convert a Windows path string to its extended-length form.
///
/// * `C:\dir\file`        -> `\\?\C:\dir\file`
/// * `\\server\share\x`   -> `\\?\UNC\server\share\x`
/// * already `\\?\...` or `\\.\...` device paths are returned unchanged
/// * relative paths are returned unchanged (they cannot be prefixed)
///
/// Forward slashes are normalised to backslashes because the `\\?\` prefix
/// disables the Win32 normalisation that would otherwise do it.
pub fn to_extended_windows(path: &str) -> String {
    if path.starts_with(r"\\?\") || path.starts_with(r"\\.\") {
        return path.to_string();
    }
    let normalized = path.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\") {
        return format!(r"\\?\UNC\{rest}");
    }
    let b = normalized.as_bytes();
    let is_drive_abs = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if is_drive_abs {
        format!(r"\\?\{}", collapse_dots(&normalized))
    } else {
        normalized
    }
}

/// Strip an extended-length prefix for display (`\\?\C:\x` -> `C:\x`,
/// `\\?\UNC\srv\s` -> `\\srv\s`).
pub fn strip_extended_windows(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// Resolve `.` and `..` components lexically (required before adding `\\?\`).
fn collapse_dots(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut iter = path.split('\\');
    let drive = iter.next().unwrap_or_default();
    for part in iter {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut out = String::with_capacity(path.len());
    out.push_str(drive);
    out.push('\\');
    out.push_str(&parts.join("\\"));
    if path.ends_with('\\') && !parts.is_empty() {
        out.push('\\');
    }
    out
}

/// True when a path would exceed `MAX_PATH` without the long-path prefix.
pub fn needs_extended(path: &str) -> bool {
    path.encode_utf16().count() >= MAX_PATH - 12
}

/// Convert a path to the form native APIs should receive.
///
/// On Windows absolute paths are always converted to extended-length form so
/// deep trees (node_modules, ...) never fail with `ERROR_PATH_NOT_FOUND`.
/// Elsewhere the path is returned as-is.
pub fn to_native(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        match path.to_str() {
            Some(s) => PathBuf::from(to_extended_windows(s)),
            None => path.to_path_buf(),
        }
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// Split a file name into (stem, extension) the way Finder/Explorer do for
/// "keep both" renames: `archive.tar.gz` -> (`archive.tar`, `.gz`),
/// `.bashrc` -> (`.bashrc`, ``).
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i..]),
    }
}

/// Produce a unique name in a directory: `name (2).ext`, `name (3).ext`, ...
///
/// `exists` is consulted for every candidate; it is a closure so this stays
/// pure and testable. Existing ` (n)` suffixes are continued, not nested.
pub fn unique_name(name: &str, mut exists: impl FnMut(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let (stem, ext) = split_name(name);
    let (base, start) = parse_counter(stem);
    let mut n = start.max(2);
    loop {
        let candidate = format!("{base} ({n}){ext}");
        if !exists(&candidate) {
            return candidate;
        }
        n = n.saturating_add(1);
    }
}

/// `report (4)` -> (`report`, 5); `report` -> (`report`, 2).
fn parse_counter(stem: &str) -> (&str, u32) {
    if let Some(open) = stem.rfind(" (") {
        if let Some(inner) = stem[open + 2..].strip_suffix(')') {
            if let Ok(n) = inner.parse::<u32>() {
                return (&stem[..open], n.saturating_add(1));
            }
        }
    }
    (stem, 2)
}

/// True if `child` is `parent` or lies inside it (lexical check).
pub fn is_within(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}

/// Windows reserves these device names in every directory.
pub fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0')
}

/// Validate a user-entered file name (rename / new folder).
pub fn validate_file_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("name is empty");
    }
    if name == "." || name == ".." {
        return Err("name is reserved");
    }
    if name.chars().any(|c| {
        matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 32
    }) {
        return Err("name contains a character that is not allowed");
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("name cannot end with a space or a dot");
    }
    if is_reserved_windows_name(name) {
        return Err("name is reserved by Windows");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn extended_paths() {
        assert_eq!(to_extended_windows(r"C:\a\b"), r"\\?\C:\a\b");
        assert_eq!(to_extended_windows("C:/a/b"), r"\\?\C:\a\b");
        assert_eq!(to_extended_windows(r"C:\a\..\b\.\c"), r"\\?\C:\b\c");
        assert_eq!(
            to_extended_windows(r"\\srv\share\x"),
            r"\\?\UNC\srv\share\x"
        );
        assert_eq!(to_extended_windows(r"\\?\C:\already"), r"\\?\C:\already");
        assert_eq!(to_extended_windows(r"\\.\C:"), r"\\.\C:");
        assert_eq!(to_extended_windows(r"rel\path"), r"rel\path");
    }

    #[test]
    fn strip_extended() {
        assert_eq!(strip_extended_windows(r"\\?\C:\x"), r"C:\x");
        assert_eq!(strip_extended_windows(r"\\?\UNC\srv\s"), r"\\srv\s");
        assert_eq!(strip_extended_windows(r"C:\x"), r"C:\x");
    }

    #[test]
    fn long_path_detection() {
        let deep = format!(r"C:\{}", "a\\".repeat(200));
        assert!(needs_extended(&deep));
        assert!(!needs_extended(r"C:\short"));
    }

    #[test]
    fn unique_names() {
        let taken = ["a.txt", "a (2).txt", "b", "c (4).log"];
        let ex = |n: &str| taken.contains(&n);
        assert_eq!(unique_name("a.txt", ex), "a (3).txt");
        assert_eq!(unique_name("b", ex), "b (2)");
        assert_eq!(unique_name("new.txt", ex), "new.txt");
        assert_eq!(unique_name("c (4).log", ex), "c (5).log");
        assert_eq!(unique_name(".bashrc", |n| n == ".bashrc"), ".bashrc (2)");
    }

    #[test]
    fn reserved_and_validation() {
        assert!(is_reserved_windows_name("CON"));
        assert!(is_reserved_windows_name("com1.txt"));
        assert!(!is_reserved_windows_name("COM0"));
        assert!(!is_reserved_windows_name("console"));
        assert!(validate_file_name("ok name.txt").is_ok());
        assert!(validate_file_name("bad:name").is_err());
        assert!(validate_file_name("trail.").is_err());
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("nul").is_err());
    }

    proptest! {
        #[test]
        fn unique_name_never_collides(existing in proptest::collection::hash_set("[a-c]{1,2}( \\([2-5]\\))?(\\.txt)?", 0..20), name in "[a-c]{1,2}(\\.txt)?") {
            let out = unique_name(&name, |n| existing.contains(n));
            prop_assert!(!existing.contains(&out));
        }

        #[test]
        fn extended_is_idempotent(s in "[A-Z]:(\\\\[a-z]{1,5}){1,6}") {
            let once = to_extended_windows(&s);
            prop_assert_eq!(to_extended_windows(&once), once.clone());
            prop_assert!(once.starts_with(r"\\?\"));
        }
    }
}
