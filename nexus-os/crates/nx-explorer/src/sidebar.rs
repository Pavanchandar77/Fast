//! Sidebar: favourites (Quick access), recent folders and drives.

use nx_fs::volumes::Volume;
use std::path::{Path, PathBuf};

/// Sidebar section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Favorites,
    Recent,
    Drives,
}

/// One clickable row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    pub section: Section,
    pub label: String,
    pub path: PathBuf,
}

/// Max recent folders shown.
pub const MAX_RECENTS: usize = 8;

/// Sidebar model.
#[derive(Debug, Clone, Default)]
pub struct Sidebar {
    favorites: Vec<PathBuf>,
    recents: Vec<PathBuf>,
    volumes: Vec<Volume>,
    pub cursor: usize,
}

fn label_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

impl Sidebar {
    pub fn favorites(&self) -> &[PathBuf] {
        &self.favorites
    }

    pub fn set_favorites(&mut self, favs: Vec<PathBuf>) {
        self.favorites.clear();
        for f in favs {
            if !self.favorites.contains(&f) {
                self.favorites.push(f);
            }
        }
        self.clamp();
    }

    /// Returns false if it was already a favourite.
    pub fn add_favorite(&mut self, p: PathBuf) -> bool {
        if self.favorites.contains(&p) {
            return false;
        }
        self.favorites.push(p);
        true
    }

    pub fn remove_favorite(&mut self, p: &Path) -> bool {
        let before = self.favorites.len();
        self.favorites.retain(|f| f != p);
        self.clamp();
        before != self.favorites.len()
    }

    pub fn set_recents(&mut self, mut r: Vec<PathBuf>) {
        r.dedup();
        r.truncate(MAX_RECENTS);
        self.recents = r;
        self.clamp();
    }

    /// Record a visited folder (most recent first).
    pub fn visit(&mut self, p: &Path) {
        self.recents.retain(|r| r != p);
        self.recents.insert(0, p.to_path_buf());
        self.recents.truncate(MAX_RECENTS);
    }

    pub fn recents(&self) -> &[PathBuf] {
        &self.recents
    }

    pub fn set_volumes(&mut self, v: Vec<Volume>) {
        self.volumes = v;
        self.clamp();
    }

    /// All rows, in display order.
    pub fn items(&self) -> Vec<SidebarItem> {
        let fav = self.favorites.iter().map(|p| SidebarItem {
            section: Section::Favorites,
            label: label_of(p),
            path: p.clone(),
        });
        let rec = self.recents.iter().map(|p| SidebarItem {
            section: Section::Recent,
            label: label_of(p),
            path: p.clone(),
        });
        let drv = self.volumes.iter().map(|v| SidebarItem {
            section: Section::Drives,
            label: if v.label.is_empty() {
                v.root.clone()
            } else {
                format!("{} ({})", v.label, v.root.trim_end_matches(['\\', '/']))
            },
            path: PathBuf::from(&v.root),
        });
        fav.chain(rec).chain(drv).collect()
    }

    pub fn len(&self) -> usize {
        self.favorites.len() + self.recents.len() + self.volumes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn path_at(&self, n: usize) -> Option<PathBuf> {
        self.items().into_iter().nth(n).map(|i| i.path)
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.len();
        if n == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nx_fs::volumes::VolumeKind;

    #[test]
    fn sections_and_dedup() {
        let mut s = Sidebar::default();
        s.set_favorites(vec!["/home/me".into(), "/home/me".into(), "/srv".into()]);
        assert!(!s.add_favorite("/srv".into()));
        assert!(s.add_favorite("/tmp".into()));
        for i in 0..12 {
            s.visit(Path::new(&format!("/r{i}")));
        }
        s.visit(Path::new("/r5"));
        assert_eq!(s.recents().len(), MAX_RECENTS);
        assert_eq!(s.recents()[0], PathBuf::from("/r5"));
        s.set_volumes(vec![Volume {
            root: "C:\\".into(),
            label: "System".into(),
            kind: VolumeKind::Fixed,
            fs: "NTFS".into(),
        }]);
        let items = s.items();
        assert_eq!(items.len(), 3 + MAX_RECENTS + 1);
        assert_eq!(items[0].label, "me");
        assert_eq!(items.last().map(|i| i.label.as_str()), Some("System (C:)"));
        assert!(s.remove_favorite(Path::new("/srv")));
        s.cursor = 100;
        s.move_cursor(1);
        assert_eq!(s.cursor, s.len() - 1);
        s.move_cursor(-100);
        assert_eq!(s.path_at(s.cursor), Some(PathBuf::from("/home/me")));
    }
}
