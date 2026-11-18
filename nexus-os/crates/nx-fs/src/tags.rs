//! Finder-style colour tags and sidebar favourites, persisted atomically.

use crate::error::{FsError, FsResult};
use crate::ops::journal::{field_path, path_field};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Built-in tag colours (custom names are allowed too).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tag {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Gray,
    Custom(String),
}

impl Tag {
    pub fn parse(s: &str) -> Tag {
        match s.to_ascii_lowercase().as_str() {
            "red" => Tag::Red,
            "orange" => Tag::Orange,
            "yellow" => Tag::Yellow,
            "green" => Tag::Green,
            "blue" => Tag::Blue,
            "purple" => Tag::Purple,
            "gray" | "grey" => Tag::Gray,
            _ => Tag::Custom(s.to_string()),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Tag::Red => "red",
            Tag::Orange => "orange",
            Tag::Yellow => "yellow",
            Tag::Green => "green",
            Tag::Blue => "blue",
            Tag::Purple => "purple",
            Tag::Gray => "gray",
            Tag::Custom(s) => s,
        }
    }
}

/// Tags per path plus an ordered favourites list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagStore {
    tags: BTreeMap<PathBuf, BTreeSet<Tag>>,
    favorites: Vec<PathBuf>,
}

impl TagStore {
    pub fn tag(&mut self, path: &Path, tag: Tag) -> bool {
        self.tags.entry(path.to_path_buf()).or_default().insert(tag)
    }

    pub fn untag(&mut self, path: &Path, tag: &Tag) -> bool {
        let Some(set) = self.tags.get_mut(path) else {
            return false;
        };
        let removed = set.remove(tag);
        if set.is_empty() {
            self.tags.remove(path);
        }
        removed
    }

    pub fn tags_of(&self, path: &Path) -> Vec<Tag> {
        self.tags
            .get(path)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// All paths carrying `tag` (sidebar "Tags" section).
    pub fn with_tag(&self, tag: &Tag) -> Vec<PathBuf> {
        self.tags
            .iter()
            .filter(|(_, s)| s.contains(tag))
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Keep tags attached when a file is moved/renamed (including children).
    pub fn rename(&mut self, from: &Path, to: &Path) {
        let moved: Vec<PathBuf> = self
            .tags
            .keys()
            .filter(|p| p.starts_with(from))
            .cloned()
            .collect();
        for old in moved {
            if let (Some(set), Ok(rest)) = (self.tags.remove(&old), old.strip_prefix(from)) {
                self.tags.insert(to.join(rest), set);
            }
        }
        for f in &mut self.favorites {
            if let Ok(rest) = f.strip_prefix(from) {
                *f = to.join(rest);
            }
        }
    }

    pub fn forget(&mut self, path: &Path) {
        self.tags.retain(|p, _| !p.starts_with(path));
    }

    pub fn favorites(&self) -> &[PathBuf] {
        &self.favorites
    }

    pub fn add_favorite(&mut self, path: &Path) -> bool {
        if self.favorites.iter().any(|f| f == path) {
            return false;
        }
        self.favorites.push(path.to_path_buf());
        true
    }

    pub fn remove_favorite(&mut self, path: &Path) -> bool {
        let n = self.favorites.len();
        self.favorites.retain(|f| f != path);
        n != self.favorites.len()
    }

    /// Reorder (drag in the sidebar). Out-of-range indices are clamped.
    pub fn move_favorite(&mut self, from: usize, to: usize) {
        if from < self.favorites.len() {
            let item = self.favorites.remove(from);
            let to = to.min(self.favorites.len());
            self.favorites.insert(to, item);
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# nx-fs tags v1\n");
        for f in &self.favorites {
            out.push_str(&format!("F\t{}\n", path_field(f)));
        }
        for (p, set) in &self.tags {
            for t in set {
                out.push_str(&format!(
                    "T\t{}\t{}\n",
                    path_field(p),
                    crate::ops::journal::escape(t.name().as_bytes())
                ));
            }
        }
        out
    }

    pub fn from_text(text: &str, origin: &Path) -> FsResult<Self> {
        let mut s = TagStore::default();
        for (i, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let bad = || FsError::StoreCorrupt {
                path: origin.to_path_buf(),
                line: i + 1,
            };
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["F", p] => {
                    s.favorites.push(field_path(p).ok_or_else(bad)?);
                }
                ["T", p, t] => {
                    let tag = crate::ops::journal::unescape(t)
                        .and_then(|b| String::from_utf8(b).ok())
                        .ok_or_else(bad)?;
                    s.tag(&field_path(p).ok_or_else(bad)?, Tag::parse(&tag));
                }
                _ => return Err(bad()),
            }
        }
        Ok(s)
    }

    pub fn save(&self, file: &Path) -> FsResult<()> {
        crate::index::frecency::write_atomic(file, self.to_text().as_bytes())
    }

    pub fn load(file: &Path) -> FsResult<Self> {
        match std::fs::read_to_string(file) {
            Ok(t) => Self::from_text(&t, file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(FsError::from_io(file, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_favorites_rename_and_persist() {
        let mut s = TagStore::default();
        assert!(s.tag(Path::new("/p/a.txt"), Tag::Red));
        assert!(!s.tag(Path::new("/p/a.txt"), Tag::parse("RED")));
        s.tag(Path::new("/p/sub/b.txt"), Tag::Custom("Work".into()));
        s.add_favorite(Path::new("/p"));
        s.add_favorite(Path::new("/q"));
        assert!(!s.add_favorite(Path::new("/q")));
        s.move_favorite(1, 0);
        assert_eq!(s.favorites()[0], PathBuf::from("/q"));
        s.rename(Path::new("/p"), Path::new("/r"));
        assert_eq!(s.tags_of(Path::new("/r/a.txt")), vec![Tag::Red]);
        assert_eq!(
            s.with_tag(&Tag::Custom("Work".into())),
            vec![PathBuf::from("/r/sub/b.txt")]
        );
        assert_eq!(s.favorites()[1], PathBuf::from("/r"));
        let t = tempfile::tempdir().expect("tempdir");
        let f = t.path().join("tags.tsv");
        s.save(&f).expect("save");
        assert_eq!(TagStore::load(&f).expect("load"), s);
        assert!(s.untag(Path::new("/r/a.txt"), &Tag::Red));
        assert!(s.tags_of(Path::new("/r/a.txt")).is_empty());
        assert!(TagStore::from_text("Z\tx\n", &f).is_err());
    }
}
