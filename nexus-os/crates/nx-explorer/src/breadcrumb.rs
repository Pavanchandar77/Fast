//! Breadcrumb / address-bar path segmentation for Windows and Unix paths.

use std::path::PathBuf;

/// One clickable breadcrumb segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub path: PathBuf,
}

/// Split a path into breadcrumb segments.
///
/// * `C:\Users\me`        -> `C:` / `Users` / `me`
/// * `\\srv\share\docs`   -> `\\srv\share` / `docs`
/// * `\\?\C:\x` (long)    -> `C:` / `x`
/// * `/home/me`           -> `/` / `home` / `me`
pub fn crumbs(path: &str) -> Vec<Crumb> {
    let p = nx_fs::path::strip_extended_windows(path);
    let mut out = Vec::new();
    if let Some(rest) = p.strip_prefix(r"\\") {
        // UNC: the root is \\server\share.
        let mut parts = rest.split(['\\', '/']).filter(|s| !s.is_empty());
        let (Some(server), share) = (parts.next(), parts.next()) else {
            return out;
        };
        let mut acc = match share {
            Some(sh) => format!(r"\\{server}\{sh}"),
            None => format!(r"\\{server}"),
        };
        out.push(Crumb {
            label: acc.clone(),
            path: PathBuf::from(&acc),
        });
        for seg in parts {
            acc.push('\\');
            acc.push_str(seg);
            out.push(Crumb {
                label: seg.to_string(),
                path: PathBuf::from(&acc),
            });
        }
        return out;
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        let drive = p[..2].to_ascii_uppercase();
        let mut acc = format!("{drive}\\");
        out.push(Crumb {
            label: drive,
            path: PathBuf::from(&acc),
        });
        for seg in p[2..].split(['\\', '/']).filter(|s| !s.is_empty()) {
            if !acc.ends_with('\\') {
                acc.push('\\');
            }
            acc.push_str(seg);
            out.push(Crumb {
                label: seg.to_string(),
                path: PathBuf::from(&acc),
            });
        }
        return out;
    }
    if p.starts_with('/') {
        out.push(Crumb {
            label: "/".into(),
            path: PathBuf::from("/"),
        });
        let mut acc = String::new();
        for seg in p.split('/').filter(|s| !s.is_empty()) {
            acc.push('/');
            acc.push_str(seg);
            out.push(Crumb {
                label: seg.to_string(),
                path: PathBuf::from(&acc),
            });
        }
        return out;
    }
    // Relative or unknown: one crumb per segment.
    let mut acc = PathBuf::new();
    for seg in p.split(['\\', '/']).filter(|s| !s.is_empty()) {
        acc.push(seg);
        out.push(Crumb {
            label: seg.to_string(),
            path: acc.clone(),
        });
    }
    out
}

/// Collapse the middle of long breadcrumbs (Finder-style "…") so at most
/// `max` crumbs are shown; the first and the last `max - 2` are kept.
pub fn collapse(crumbs: &[Crumb], max: usize) -> Vec<Option<&Crumb>> {
    if crumbs.len() <= max || max < 3 {
        return crumbs.iter().map(Some).collect();
    }
    let tail = max - 2;
    let mut v = vec![crumbs.first(), None];
    v.extend(crumbs[crumbs.len() - tail..].iter().map(Some));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(p: &str) -> Vec<String> {
        crumbs(p).into_iter().map(|c| c.label).collect()
    }

    #[test]
    fn windows_unc_unix() {
        assert_eq!(labels(r"c:\Users\me\"), vec!["C:", "Users", "me"]);
        assert_eq!(
            crumbs(r"C:\Users\me")[2].path,
            PathBuf::from(r"C:\Users\me")
        );
        assert_eq!(crumbs(r"C:\")[0].path, PathBuf::from(r"C:\"));
        assert_eq!(labels(r"\\?\C:\deep\x"), vec!["C:", "deep", "x"]);
        assert_eq!(labels(r"\\nas\media\films"), vec![r"\\nas\media", "films"]);
        assert_eq!(
            crumbs(r"\\nas\media\films")[1].path,
            PathBuf::from(r"\\nas\media\films")
        );
        assert_eq!(labels("/home/me/src"), vec!["/", "home", "me", "src"]);
        assert_eq!(crumbs("/home/me")[2].path, PathBuf::from("/home/me"));
        assert_eq!(labels("rel/x"), vec!["rel", "x"]);
    }

    #[test]
    fn collapses_middle() {
        let c = crumbs("/a/b/c/d/e/f");
        let v = collapse(&c, 4);
        assert_eq!(v.len(), 4);
        assert!(v[1].is_none());
        assert_eq!(v[3].map(|c| c.label.as_str()), Some("f"));
        assert_eq!(collapse(&c, 10).len(), 7);
    }
}
