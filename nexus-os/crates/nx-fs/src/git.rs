//! Optional git status badges.
//!
//! Runs `git status --porcelain=v1 -z` (only if git is installed and the
//! folder is inside a work tree) and maps entries to badges. The parser is
//! pure and tested; nothing here is required for the explorer to work.

use crate::error::{FsError, FsResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Badge shown next to a file/folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitBadge {
    // Ordered by severity: folders show their most severe child.
    Clean,
    Ignored,
    Untracked,
    Added,
    Renamed,
    Modified,
    Deleted,
    Conflict,
}

fn badge(x: u8, y: u8) -> GitBadge {
    match (x, y) {
        (b'?', b'?') => GitBadge::Untracked,
        (b'!', b'!') => GitBadge::Ignored,
        (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => GitBadge::Conflict,
        (b'R', _) | (_, b'R') => GitBadge::Renamed,
        (b'A', _) => GitBadge::Added,
        (b'D', _) | (_, b'D') => GitBadge::Deleted,
        _ => GitBadge::Modified,
    }
}

/// Parse `git status --porcelain=v1 -z` output into repo-relative paths.
pub fn parse_porcelain_z(out: &[u8]) -> FsResult<Vec<(PathBuf, GitBadge)>> {
    let mut v = Vec::new();
    let mut parts = out.split(|&b| b == 0).filter(|p| !p.is_empty());
    while let Some(rec) = parts.next() {
        if rec.len() < 4 || rec[2] != b' ' {
            return Err(FsError::Malformed("bad porcelain record".into()));
        }
        let b = badge(rec[0], rec[1]);
        let path = String::from_utf8_lossy(&rec[3..]).into_owned();
        if matches!(rec[0], b'R' | b'C') {
            // Rename/copy records are followed by the original path.
            let _ = parts.next();
        }
        v.push((PathBuf::from(path), b));
    }
    Ok(v)
}

/// Badges for direct children of `dir` (folders take the "worst" badge of
/// anything below them, like VS Code / Finder plugins).
pub fn badges_for_children(
    repo_root: &Path,
    dir: &Path,
    entries: &[(PathBuf, GitBadge)],
) -> HashMap<String, GitBadge> {
    let rel = dir.strip_prefix(repo_root).unwrap_or(Path::new(""));
    let mut out: HashMap<String, GitBadge> = HashMap::new();
    for (p, b) in entries {
        let Ok(rest) = p.strip_prefix(rel) else {
            continue;
        };
        let Some(first) = rest.components().next() else {
            continue;
        };
        let name = first.as_os_str().to_string_lossy().into_owned();
        let slot = out.entry(name).or_insert(*b);
        if *b > *slot {
            *slot = *b;
        }
    }
    out
}

/// Find the repository root of `dir` (walks up looking for `.git`).
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Run git for `dir`. `Unsupported` if git is missing or not a repo.
pub fn status(dir: &Path) -> FsResult<(PathBuf, Vec<(PathBuf, GitBadge)>)> {
    let root = repo_root(dir).ok_or(FsError::Unsupported("not a git work tree"))?;
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=normal"])
        .output()
        .map_err(|_| FsError::Unsupported("git is not installed"))?;
    if !out.status.success() {
        return Err(FsError::Unsupported("git status failed"));
    }
    Ok((root, parse_porcelain_z(&out.stdout)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_and_rolls_up() {
        let raw = b" M src/lib.rs\0?? new.txt\0R  b.rs\0a.rs\0UU src/conflict.rs\0A  docs/x.md\0";
        let v = parse_porcelain_z(raw).expect("parse");
        assert_eq!(v.len(), 5);
        assert_eq!(v[0], (PathBuf::from("src/lib.rs"), GitBadge::Modified));
        assert_eq!(v[1].1, GitBadge::Untracked);
        assert_eq!(v[2], (PathBuf::from("b.rs"), GitBadge::Renamed));
        let kids = badges_for_children(Path::new("/repo"), Path::new("/repo"), &v);
        assert_eq!(
            kids.get("src"),
            Some(&GitBadge::Conflict),
            "folder shows worst child"
        );
        assert_eq!(kids.get("docs"), Some(&GitBadge::Added));
        assert!(parse_porcelain_z(b"xx\0").is_err());
    }
}
