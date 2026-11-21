//! The directory entry model shared by listing, search and the explorer.

use std::ffi::OsString;
use std::time::SystemTime;

/// What kind of object an entry is. Links are never followed implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    /// Symbolic link (file or directory).
    Symlink,
    /// NTFS junction / mount point (reparse point that is not a symlink).
    Junction,
    /// Devices, sockets, FIFOs, unknown reparse points.
    Other,
}

impl EntryKind {
    /// Directories sort first and can be navigated into.
    pub fn is_dir(self) -> bool {
        matches!(self, EntryKind::Dir)
    }
    /// Links and junctions are rendered with an alias badge and never recursed.
    pub fn is_link(self) -> bool {
        matches!(self, EntryKind::Symlink | EntryKind::Junction)
    }
}

/// Windows-style attribute flags, also synthesised on other platforms
/// (dot-files are reported as hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attributes(pub u32);

impl Attributes {
    pub const READONLY: u32 = 0x1;
    pub const HIDDEN: u32 = 0x2;
    pub const SYSTEM: u32 = 0x4;
    pub const ARCHIVE: u32 = 0x20;
    pub const REPARSE_POINT: u32 = 0x400;
    pub const COMPRESSED: u32 = 0x800;
    pub const OFFLINE: u32 = 0x1000;
    pub const ENCRYPTED: u32 = 0x4000;
    /// Cloud placeholder that is not downloaded (OneDrive "online-only").
    pub const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

    pub fn has(self, flag: u32) -> bool {
        self.0 & flag != 0
    }
    pub fn hidden(self) -> bool {
        self.has(Self::HIDDEN)
    }
    pub fn system(self) -> bool {
        self.has(Self::SYSTEM)
    }
    pub fn readonly(self) -> bool {
        self.has(Self::READONLY)
    }
    /// Opening this file would trigger a network download; previews must not touch it.
    pub fn is_cloud_placeholder(self) -> bool {
        self.has(Self::RECALL_ON_DATA_ACCESS) || self.has(Self::OFFLINE)
    }
}

/// One row of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Exact on-disk name (may not be valid Unicode).
    pub name: OsString,
    /// Lossy UTF-8 name used for display, sorting and type-to-select.
    pub display_name: String,
    pub kind: EntryKind,
    /// Logical size in bytes (0 for directories).
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub attributes: Attributes,
    /// For symlinks and junctions, whether the resolved target is a directory.
    pub target_is_dir: bool,
}

impl DirEntry {
    /// Build an entry, deriving `display_name` and dot-file hiddenness.
    pub fn new(name: OsString, kind: EntryKind, size: u64) -> Self {
        let display_name = name.to_string_lossy().into_owned();
        let mut attributes = Attributes::default();
        if display_name.starts_with('.') {
            attributes.0 |= Attributes::HIDDEN;
        }
        DirEntry {
            name,
            display_name,
            kind,
            size,
            modified: None,
            created: None,
            attributes,
            target_is_dir: false,
        }
    }

    /// Lower-cased extension without the dot (`"rs"`), empty for none/dirs.
    pub fn extension(&self) -> String {
        if self.kind.is_dir() {
            return String::new();
        }
        let (_, ext) = crate::path::split_name(&self.display_name);
        ext.trim_start_matches('.').to_ascii_lowercase()
    }

    /// Hidden or system entries are filtered unless "show hidden" is on.
    pub fn is_hidden(&self) -> bool {
        self.attributes.hidden() || self.attributes.system()
    }

    /// True when this entry should be shown and opened as a folder.
    /// Follows symlink/junction metadata rather than treating every link as a file.
    pub fn is_folder(&self) -> bool {
        matches!(self.kind, EntryKind::Dir | EntryKind::Junction) || self.target_is_dir
    }
}

/// Convert a Windows FILETIME (100 ns ticks since 1601-01-01) to `SystemTime`.
/// Returns `None` for 0 (unset) and for values before 1970.
pub fn filetime_to_system(ticks: u64) -> Option<SystemTime> {
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    if ticks == 0 {
        return None;
    }
    let since_unix = ticks.checked_sub(UNIX_EPOCH_TICKS)?;
    let d = std::time::Duration::new(
        since_unix / 10_000_000,
        ((since_unix % 10_000_000) * 100) as u32,
    );
    SystemTime::UNIX_EPOCH.checked_add(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion() {
        assert_eq!(filetime_to_system(0), None);
        assert_eq!(
            filetime_to_system(116_444_736_000_000_000),
            Some(SystemTime::UNIX_EPOCH)
        );
        let t = filetime_to_system(116_444_736_000_000_000 + 15_000_000).expect("time");
        assert_eq!(
            t.duration_since(SystemTime::UNIX_EPOCH).ok(),
            Some(std::time::Duration::from_millis(1500))
        );
        assert_eq!(filetime_to_system(5), None);
    }

    #[test]
    fn dotfiles_are_hidden_and_extensions_lowercase() {
        let e = DirEntry::new(".git".into(), EntryKind::Dir, 0);
        assert!(e.is_hidden());
        assert_eq!(e.extension(), "");
        let f = DirEntry::new("Photo.JPG".into(), EntryKind::File, 10);
        assert!(!f.is_hidden());
        assert_eq!(f.extension(), "jpg");
        assert!(Attributes(Attributes::OFFLINE).is_cloud_placeholder());
        assert!(EntryKind::Junction.is_link());
    }

    #[test]
    fn symlink_to_directory_is_a_folder() {
        let root =
            std::env::temp_dir().join(format!("nexus-symlink-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("real-dir")).unwrap();
        let link = root.join("alias-dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("real-dir"), &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(root.join("real-dir"), &link).unwrap();
        let entries = crate::listing::list_dir(&root).expect("list");
        let alias = entries
            .iter()
            .find(|e| e.display_name == "alias-dir")
            .expect("alias");
        assert!(alias.kind.is_link() || alias.kind == EntryKind::Dir);
        assert!(
            alias.is_folder(),
            "symlink to a directory must display as a folder"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
