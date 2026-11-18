//! Trash / Recycle Bin abstraction.
//!
//! On Windows [`default_trash`] returns the shell Recycle Bin backend
//! (`IFileOperation` with `FOFX_RECYCLEONDELETE`, restore via the
//! `$Recycle.Bin` metadata). Elsewhere a portable trash directory is used.

use crate::error::{FsError, FsResult, IoContext};
use crate::path::{to_native, unique_name};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a trashed item went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashedItem {
    pub original: PathBuf,
    /// Backend-specific location (trash path, or `$R` file on Windows).
    pub location: PathBuf,
}

/// A recycle bin.
pub trait Trash: Send + Sync {
    fn trash(&self, path: &Path) -> FsResult<TrashedItem>;
    fn restore(&self, item: &TrashedItem) -> FsResult<()>;
    fn name(&self) -> &'static str;
}

/// Portable trash: moves items into a directory (falls back to copy+delete
/// across volumes). Each item gets a `<name>.nxtrashinfo` sidecar with its
/// original path so a UI can list and restore it later.
#[derive(Debug, Clone)]
pub struct DirTrash {
    dir: PathBuf,
}

impl DirTrash {
    pub fn new(dir: impl Into<PathBuf>) -> FsResult<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(to_native(&dir)).at(&dir)?;
        Ok(DirTrash { dir })
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Trash for DirTrash {
    fn trash(&self, path: &Path) -> FsResult<TrashedItem> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| FsError::InvalidPath {
                path: path.to_path_buf(),
                reason: "cannot trash a root",
            })?;
        if self.dir.starts_with(path) {
            return Err(FsError::IntoItself {
                src: path.to_path_buf(),
                dest: self.dir.clone(),
            });
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = format!("{stamp}-{name}");
        let unique = unique_name(&base, |n| self.dir.join(n).exists());
        let location = self.dir.join(&unique);
        super::exec::move_path(path, &location, None)?;
        let info = self.dir.join(format!("{unique}.nxtrashinfo"));
        // Best effort: the sidecar only helps listing; restore uses the record.
        let _ = std::fs::write(to_native(&info), super::journal::path_field(path));
        Ok(TrashedItem {
            original: path.to_path_buf(),
            location,
        })
    }

    fn restore(&self, item: &TrashedItem) -> FsResult<()> {
        if std::fs::symlink_metadata(to_native(&item.original)).is_ok() {
            return Err(FsError::AlreadyExists {
                path: item.original.clone(),
            });
        }
        if let Some(parent) = item.original.parent() {
            std::fs::create_dir_all(to_native(parent)).at(parent)?;
        }
        super::exec::move_path(&item.location, &item.original, None)?;
        let mut info = item.location.clone().into_os_string();
        info.push(".nxtrashinfo");
        let _ = std::fs::remove_file(to_native(Path::new(&info)));
        Ok(())
    }

    fn name(&self) -> &'static str {
        "directory trash"
    }
}

/// Default trash for this platform. `fallback_dir` is used where no system
/// recycle bin is available.
pub fn default_trash(fallback_dir: &Path) -> FsResult<Arc<dyn Trash>> {
    #[cfg(windows)]
    {
        let _ = fallback_dir;
        Ok(Arc::new(
            crate::platform::windows::recycle::RecycleBin::new(),
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(Arc::new(DirTrash::new(fallback_dir)?))
    }
}

/// Parse a Windows `$I` Recycle Bin metadata file (versions 1 and 2).
/// Returns (original path, size, deletion FILETIME). Platform-free so it is
/// tested on Linux.
pub fn parse_recycle_info(b: &[u8]) -> FsResult<(String, u64, u64)> {
    let rd = |at: usize| -> FsResult<u64> {
        b.get(at..at + 8)
            .and_then(|s| s.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or_else(|| FsError::Malformed("$I file truncated".into()))
    };
    let version = rd(0)?;
    let size = rd(8)?;
    let time = rd(16)?;
    let units: Vec<u16> = match version {
        1 => b
            .get(24..24 + 520)
            .map(crate::index::usn::le_u16s)
            .ok_or_else(|| FsError::Malformed("$I v1 truncated".into()))?,
        2 => {
            let n = b
                .get(24..28)
                .and_then(|s| s.try_into().ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| FsError::Malformed("$I v2 truncated".into()))?
                as usize;
            b.get(28..28 + n * 2)
                .map(crate::index::usn::le_u16s)
                .ok_or_else(|| FsError::Malformed("$I v2 path truncated".into()))?
        }
        v => return Err(FsError::Malformed(format!("unknown $I version {v}"))),
    };
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Ok((String::from_utf16_lossy(&units[..end]), size, time))
}

/// `$RABC123.txt` -> `$IABC123.txt` (the metadata file beside a recycled item).
pub fn recycle_info_path(r_path: &Path) -> Option<PathBuf> {
    let name = r_path.file_name()?.to_str()?;
    let rest = name.strip_prefix("$R")?;
    Some(r_path.with_file_name(format!("$I{rest}")))
}

/// `$IABC123.txt` -> `$RABC123.txt`.
pub fn recycle_data_path(i_path: &Path) -> Option<PathBuf> {
    let name = i_path.file_name()?.to_str()?;
    let rest = name.strip_prefix("$I")?;
    Some(i_path.with_file_name(format!("$R{rest}")))
}

/// Among parsed `$I` files `(path, original, deletion time)`, pick the most
/// recent one whose original path matches (case-insensitive, like NTFS).
pub fn pick_recycled(candidates: &[(PathBuf, String, u64)], original: &str) -> Option<PathBuf> {
    let want = original.trim_start_matches(r"\\?\").to_lowercase();
    candidates
        .iter()
        .filter(|(_, o, _)| o.to_lowercase() == want)
        .max_by_key(|(_, _, t)| *t)
        .map(|(p, _, _)| p.clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn recycle_paths_and_pick() {
        let r = Path::new("/rb/S-1/$RAB12.txt");
        let i = recycle_info_path(r).expect("i");
        assert_eq!(i, PathBuf::from("/rb/S-1/$IAB12.txt"));
        assert_eq!(recycle_data_path(&i).as_deref(), Some(r));
        assert!(recycle_info_path(Path::new("/x/plain")).is_none());
        let c = vec![
            (PathBuf::from("$I1"), r"C:\a.txt".to_string(), 10),
            (PathBuf::from("$I2"), r"c:\A.TXT".to_string(), 20),
            (PathBuf::from("$I3"), r"C:\b.txt".to_string(), 30),
        ];
        assert_eq!(
            pick_recycled(&c, r"\\?\C:\a.txt"),
            Some(PathBuf::from("$I2"))
        );
        assert_eq!(pick_recycled(&c, r"C:\zzz"), None);
    }

    use super::*;

    #[test]
    fn dir_trash_roundtrip() {
        let t = tempfile::tempdir().expect("tempdir");
        let trash = DirTrash::new(t.path().join(".trash")).expect("trash");
        let f = t.path().join("doc.txt");
        std::fs::write(&f, b"keep me").expect("write");
        let item = trash.trash(&f).expect("trash");
        assert!(!f.exists());
        assert!(item.location.exists());
        std::fs::write(&f, b"squatter").expect("write");
        assert!(matches!(
            trash.restore(&item),
            Err(FsError::AlreadyExists { .. })
        ));
        std::fs::remove_file(&f).expect("rm");
        trash.restore(&item).expect("restore");
        assert_eq!(std::fs::read(&f).expect("read"), b"keep me");
        assert!(
            trash.trash(t.path()).is_err(),
            "trashing the trash's parent is refused"
        );
    }

    #[test]
    fn parses_recycle_info_v2() {
        let path: Vec<u16> = r"C:\Users\p\a.txt".encode_utf16().chain([0]).collect();
        let mut b = Vec::new();
        b.extend_from_slice(&2u64.to_le_bytes());
        b.extend_from_slice(&1234u64.to_le_bytes());
        b.extend_from_slice(&99u64.to_le_bytes());
        b.extend_from_slice(&(path.len() as u32).to_le_bytes());
        for u in &path {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(
            parse_recycle_info(&b).expect("v2"),
            (r"C:\Users\p\a.txt".to_string(), 1234, 99)
        );
        assert!(parse_recycle_info(&b[..20]).is_err());
        let mut v1 = vec![0u8; 24 + 520];
        v1[0] = 1;
        v1[24] = b'D';
        v1[26] = b':';
        assert_eq!(parse_recycle_info(&v1).expect("v1").0, "D:");
    }
}
