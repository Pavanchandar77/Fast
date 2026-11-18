//! Instant global search: a compact in-memory path index (Everything-style).
//!
//! * [`PathIndex`] stores every file and folder as a 20-byte node pointing at
//!   its parent plus its name in a shared byte arena, so 1M paths cost tens
//!   of MB instead of hundreds.
//! * [`matcher`] does prefix/substring/fuzzy matching with a 64-bit
//!   character-set prefilter, scanning all names in parallel.
//! * [`frecency`] ranks what the user actually opens.
//! * Index sources implement [`VolumeIndexer`]: the NTFS MFT + USN journal
//!   reader on Windows ([`mft`], [`usn`]) and a portable parallel walker
//!   ([`walker`]) for other file systems and Linux.

pub mod engine;
pub mod frecency;
pub mod matcher;
pub mod mft;
pub mod synthetic;
pub mod usn;
pub mod walker;

use crate::error::FsResult;
use crate::pool::CancelToken;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

pub use engine::{SearchEngine, SearchHit};
pub use matcher::Query;

/// Dense node identifier.
pub type NodeId = u32;

/// Parent value of root nodes.
pub const NO_PARENT: NodeId = u32::MAX;

/// Node flags.
pub mod flags {
    pub const DIR: u16 = 1;
    pub const DELETED: u16 = 2;
    pub const LINK: u16 = 4;
    pub const HIDDEN: u16 = 8;
    pub const ROOT: u16 = 16;
}

#[derive(Debug, Clone, Copy)]
struct Node {
    name_off: u32,
    fold_off: u32,
    parent: NodeId,
    name_len: u16,
    fold_len: u16,
    flags: u16,
}

/// Identity hasher: keys are already well-mixed 64-bit hashes.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) ^ u64::from(b) ^ (self.0 >> 56);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

type IdMap = HashMap<u64, NodeId, BuildHasherDefault<IdHasher>>;

/// A change to apply to an index (from a file watcher, the USN journal
/// translated to paths, or nx-fs's own file operations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexChange {
    Created { path: String, is_dir: bool },
    Removed { path: String },
    Renamed { from: String, to: String },
}

/// A source that can build and live-update an index.
pub trait VolumeIndexer: Send {
    /// Short name for diagnostics ("ntfs-mft", "walker").
    fn name(&self) -> &'static str;
    /// Build a complete index.
    fn build(&mut self, cancel: &CancelToken) -> FsResult<PathIndex>;
    /// Apply changes that happened since the last build/sync. Returns how
    /// many changes were applied.
    fn sync(&mut self, index: &mut PathIndex) -> FsResult<usize>;
}

/// Pick the fastest index source for a volume: the MFT/USN reader for NTFS
/// on Windows, the parallel walker everywhere else. The caller should fall
/// back to [`walker::WalkIndexer`] if the MFT build fails with
/// `PermissionDenied` (opening a volume handle needs elevation).
pub fn indexer_for(volume: &crate::volumes::Volume) -> Box<dyn VolumeIndexer> {
    #[cfg(windows)]
    {
        if volume.supports_usn() {
            if let Some(letter) = volume.root.chars().next() {
                return Box::new(crate::platform::windows::volume::NtfsIndexer::new(letter));
            }
        }
    }
    Box::new(walker::WalkIndexer::new(
        vec![std::path::PathBuf::from(&volume.root)],
        crate::walk::WalkOptions::default(),
    ))
}

/// Compact path store.
#[derive(Debug, Default)]
pub struct PathIndex {
    arena: Vec<u8>,
    nodes: Vec<Node>,
    masks: Vec<u64>,
    children: IdMap,
    /// Hash collisions within one parent (astronomically rare): (key, id).
    overflow: Vec<(u64, NodeId)>,
    roots: Vec<NodeId>,
    live: usize,
    garbage_bytes: usize,
}

/// Lower-case a name for matching. ASCII fast path; full Unicode otherwise.
pub fn fold(name: &str) -> std::borrow::Cow<'_, str> {
    if name.is_ascii() {
        if name.bytes().any(|b| b.is_ascii_uppercase()) {
            std::borrow::Cow::Owned(name.to_ascii_lowercase())
        } else {
            std::borrow::Cow::Borrowed(name)
        }
    } else {
        let lower = name.to_lowercase();
        if lower == name {
            std::borrow::Cow::Borrowed(name)
        } else {
            std::borrow::Cow::Owned(lower)
        }
    }
}

/// 64-bit character-set signature used to prefilter candidates.
pub fn char_mask(folded: &[u8]) -> u64 {
    let mut m = 0u64;
    for &c in folded {
        let bit = match c {
            b'a'..=b'z' => c - b'a',
            b'0'..=b'9' => 26 + (c - b'0'),
            _ => 36 + (c % 28),
        };
        m |= 1u64 << bit;
    }
    m
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn child_key(parent: NodeId, folded: &[u8]) -> u64 {
    let h = fnv1a(folded) ^ (u64::from(parent).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    // Final avalanche (splitmix64) so the identity hasher sees good bits.
    let mut z = h;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Split a path into components on both separators.
pub fn split_components(path: &str) -> impl Iterator<Item = &str> {
    path.split(['/', '\\']).filter(|c| !c.is_empty())
}

impl PathIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-allocate for `nodes` entries with `name_bytes` of names.
    pub fn with_capacity(nodes: usize, name_bytes: usize) -> Self {
        PathIndex {
            arena: Vec::with_capacity(name_bytes),
            nodes: Vec::with_capacity(nodes),
            masks: Vec::with_capacity(nodes),
            children: IdMap::with_capacity_and_hasher(nodes, Default::default()),
            ..Default::default()
        }
    }

    /// Number of live (non-deleted) nodes.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Total slots including tombstones (ids are `0..capacity_ids()`).
    pub fn capacity_ids(&self) -> usize {
        self.nodes.len()
    }

    /// Approximate heap bytes owned by the index.
    pub fn heap_bytes(&self) -> usize {
        self.arena.capacity()
            + self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.masks.capacity() * 8
            // hashbrown: one (u64, u32) slot (16 B) plus one control byte per bucket.
            + self.children.capacity() * 8 / 7 * 17
            + self.overflow.capacity() * 16
            + self.roots.capacity() * 4
    }

    /// Release spare capacity after a bulk build.
    pub fn shrink_to_fit(&mut self) {
        self.arena.shrink_to_fit();
        self.nodes.shrink_to_fit();
        self.masks.shrink_to_fit();
        self.overflow.shrink_to_fit();
    }

    /// Bytes of dead names left by renames/deletes.
    pub fn garbage_bytes(&self) -> usize {
        self.garbage_bytes
    }

    /// Add a root (a drive like `C:` or a directory like `/home/me`).
    pub fn add_root(&mut self, path: &str) -> FsResult<NodeId> {
        let trimmed = path.trim_end_matches(['/', '\\']);
        let name = if trimmed.is_empty() { path } else { trimmed };
        if let Some(id) = self.roots.iter().copied().find(|&r| self.name(r) == name) {
            return Ok(id);
        }
        let id = self.push_node(NO_PARENT, name, flags::DIR | flags::ROOT)?;
        self.roots.push(id);
        Ok(id)
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    fn push_node(&mut self, parent: NodeId, name: &str, fl: u16) -> FsResult<NodeId> {
        let too_big = || crate::FsError::Malformed("index exceeds 4G entries or bytes".into());
        let id = NodeId::try_from(self.nodes.len()).map_err(|_| too_big())?;
        if id == NO_PARENT {
            return Err(too_big());
        }
        let name_len = u16::try_from(name.len())
            .map_err(|_| crate::FsError::Malformed("name longer than 64 KiB".into()))?;
        let name_off = u32::try_from(self.arena.len()).map_err(|_| too_big())?;
        self.arena.extend_from_slice(name.as_bytes());
        let folded = fold(name);
        let (fold_off, fold_len) = if folded.as_ref() == name {
            (name_off, name_len)
        } else {
            let off = u32::try_from(self.arena.len()).map_err(|_| too_big())?;
            let len = u16::try_from(folded.len()).map_err(|_| too_big())?;
            self.arena.extend_from_slice(folded.as_bytes());
            (off, len)
        };
        let fb = folded.as_bytes();
        self.masks.push(char_mask(fb));
        self.nodes.push(Node {
            name_off,
            fold_off,
            parent,
            name_len,
            fold_len,
            flags: fl,
        });
        if parent != NO_PARENT {
            let key = child_key(parent, fb);
            if let Some(prev) = self.children.insert(key, id) {
                // Either a genuine collision (keep both) or a replaced duplicate.
                if self.fold_bytes(prev) == fb && self.nodes[prev as usize].parent == parent {
                    self.tombstone(prev);
                } else {
                    self.overflow.push((key, prev));
                }
            }
        }
        self.live += 1;
        Ok(id)
    }

    /// Insert `name` under `parent`; replaces an existing child of that name.
    pub fn insert(&mut self, parent: NodeId, name: &str, is_dir: bool) -> FsResult<NodeId> {
        if parent as usize >= self.nodes.len() || self.is_deleted(parent) {
            return Err(crate::FsError::Malformed(format!(
                "parent {parent} does not exist"
            )));
        }
        if let Some(existing) = self.child(parent, name) {
            // Upsert: keep the id (and the subtree) stable.
            if let Some(n) = self.nodes.get_mut(existing as usize) {
                if is_dir {
                    n.flags |= flags::DIR;
                } else {
                    n.flags &= !flags::DIR;
                }
            }
            if self.name(existing) != name {
                self.rename(existing, parent, name)?;
            }
            return Ok(existing);
        }
        let mut fl = if is_dir { flags::DIR } else { 0 };
        if name.starts_with('.') {
            fl |= flags::HIDDEN;
        }
        self.push_node(parent, name, fl)
    }

    /// Set extra flags (LINK, HIDDEN) on a node.
    pub fn set_flags(&mut self, id: NodeId, extra: u16) {
        if let Some(n) = self.nodes.get_mut(id as usize) {
            n.flags |= extra & !(flags::DELETED | flags::ROOT);
        }
    }

    fn tombstone(&mut self, id: NodeId) {
        if let Some(n) = self.nodes.get_mut(id as usize) {
            if n.flags & flags::DELETED == 0 {
                n.flags |= flags::DELETED;
                self.live -= 1;
                self.garbage_bytes += n.name_len as usize;
                self.masks[id as usize] = 0;
            }
        }
    }

    /// Look up a direct child by name (case-insensitive, like NTFS).
    pub fn child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let folded = fold(name);
        let fb = folded.as_bytes();
        let key = child_key(parent, fb);
        let hit = |id: NodeId| {
            let n = &self.nodes[id as usize];
            n.parent == parent && n.flags & flags::DELETED == 0 && self.fold_bytes(id) == fb
        };
        if let Some(&id) = self.children.get(&key) {
            if hit(id) {
                return Some(id);
            }
        }
        self.overflow
            .iter()
            .filter(|(k, _)| *k == key)
            .map(|&(_, id)| id)
            .find(|&id| hit(id))
    }

    /// Resolve a full path (`C:\a\b` or `/root/a/b`) to a node.
    pub fn resolve(&self, path: &str) -> Option<NodeId> {
        let (root, rest) = self.split_root(path)?;
        let mut cur = root;
        for comp in split_components(rest) {
            cur = self.child(cur, comp)?;
        }
        Some(cur)
    }

    /// Find the root that prefixes `path`; returns (root, remainder).
    fn split_root<'p>(&self, path: &'p str) -> Option<(NodeId, &'p str)> {
        let mut best: Option<(NodeId, usize)> = None;
        for &r in &self.roots {
            let name = self.name(r);
            let matches = path.len() >= name.len()
                && path.as_bytes()[..name.len()].eq_ignore_ascii_case(name.as_bytes())
                && (path.len() == name.len()
                    || matches!(path.as_bytes()[name.len()], b'/' | b'\\')
                    || name.ends_with(['/', '\\']));
            if matches && best.is_none_or(|(_, l)| name.len() > l) {
                best = Some((r, name.len()));
            }
        }
        best.map(|(r, l)| (r, &path[l..]))
    }

    /// Remove a node and everything below it. Returns how many nodes died.
    pub fn remove_subtree(&mut self, id: NodeId) -> usize {
        if id as usize >= self.nodes.len() || self.is_deleted(id) {
            return 0;
        }
        let mut dead = vec![false; self.nodes.len()];
        dead[id as usize] = true;
        // Repeat until stable: ids are mostly topologically ordered, so this
        // converges in one or two passes.
        loop {
            let mut changed = false;
            for i in 0..self.nodes.len() {
                let p = self.nodes[i].parent;
                if !dead[i]
                    && p != NO_PARENT
                    && dead[p as usize]
                    && self.nodes[i].flags & flags::DELETED == 0
                {
                    dead[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut n = 0;
        for (i, d) in dead.iter().enumerate() {
            if *d {
                let key = child_key(self.nodes[i].parent, self.fold_bytes(i as NodeId));
                if self.children.get(&key) == Some(&(i as NodeId)) {
                    self.children.remove(&key);
                }
                self.tombstone(i as NodeId);
                n += 1;
            }
        }
        self.roots.retain(|r| !dead[*r as usize]);
        n
    }

    /// Rename and/or move a node.
    pub fn rename(&mut self, id: NodeId, new_parent: NodeId, new_name: &str) -> FsResult<()> {
        if id as usize >= self.nodes.len()
            || self.is_deleted(id)
            || new_parent as usize >= self.nodes.len()
        {
            return Err(crate::FsError::Malformed(format!(
                "rename of unknown node {id}"
            )));
        }
        // Refuse to create a cycle.
        let mut cur = new_parent;
        while cur != NO_PARENT {
            if cur == id {
                return Err(crate::FsError::Malformed(
                    "rename would create a cycle".into(),
                ));
            }
            cur = self.nodes[cur as usize].parent;
        }
        if let Some(existing) = self.child(new_parent, new_name) {
            if existing != id {
                self.remove_subtree(existing);
            }
        }
        let old = self.nodes[id as usize];
        let old_key = child_key(old.parent, self.fold_bytes(id));
        if self.children.get(&old_key) == Some(&id) {
            self.children.remove(&old_key);
        }
        self.garbage_bytes += old.name_len as usize;
        let name_len = u16::try_from(new_name.len())
            .map_err(|_| crate::FsError::Malformed("name too long".into()))?;
        let name_off = u32::try_from(self.arena.len())
            .map_err(|_| crate::FsError::Malformed("arena full".into()))?;
        self.arena.extend_from_slice(new_name.as_bytes());
        let folded = fold(new_name).into_owned();
        let (fold_off, fold_len) = if folded == new_name {
            (name_off, name_len)
        } else {
            let off = u32::try_from(self.arena.len())
                .map_err(|_| crate::FsError::Malformed("arena full".into()))?;
            self.arena.extend_from_slice(folded.as_bytes());
            (
                off,
                u16::try_from(folded.len())
                    .map_err(|_| crate::FsError::Malformed("name too long".into()))?,
            )
        };
        let n = &mut self.nodes[id as usize];
        n.name_off = name_off;
        n.name_len = name_len;
        n.fold_off = fold_off;
        n.fold_len = fold_len;
        n.parent = new_parent;
        self.masks[id as usize] = char_mask(folded.as_bytes());
        self.children
            .insert(child_key(new_parent, folded.as_bytes()), id);
        Ok(())
    }

    /// Apply a path-level change. Unknown parents are created as needed for
    /// `Created`; unknown sources of `Removed`/`Renamed` are ignored.
    pub fn apply(&mut self, change: &IndexChange) -> FsResult<()> {
        match change {
            IndexChange::Created { path, is_dir } => {
                let (parent, name) = self.ensure_parent(path)?;
                if let Some(parent) = parent {
                    self.insert(parent, name, *is_dir)?;
                }
            }
            IndexChange::Removed { path } => {
                if let Some(id) = self.resolve(path) {
                    self.remove_subtree(id);
                }
            }
            IndexChange::Renamed { from, to } => {
                let Some(id) = self.resolve(from) else {
                    return Ok(());
                };
                let (parent, name) = self.ensure_parent(to)?;
                match parent {
                    Some(p) => self.rename(id, p, name)?,
                    None => {
                        self.remove_subtree(id);
                    }
                }
            }
        }
        Ok(())
    }

    /// Resolve (creating missing directories) the parent of `path`.
    fn ensure_parent<'p>(&mut self, path: &'p str) -> FsResult<(Option<NodeId>, &'p str)> {
        let Some((root, rest)) = self.split_root(path) else {
            return Ok((None, ""));
        };
        let comps: Vec<&str> = split_components(rest).collect();
        let Some((name, dirs)) = comps.split_last() else {
            return Ok((None, ""));
        };
        let mut cur = root;
        for d in dirs {
            cur = match self.child(cur, d) {
                Some(c) => c,
                None => self.insert(cur, d, true)?,
            };
        }
        Ok((Some(cur), name))
    }

    #[inline]
    pub fn is_deleted(&self, id: NodeId) -> bool {
        self.nodes
            .get(id as usize)
            .is_none_or(|n| n.flags & flags::DELETED != 0)
    }

    #[inline]
    pub fn is_dir(&self, id: NodeId) -> bool {
        self.nodes
            .get(id as usize)
            .is_some_and(|n| n.flags & flags::DIR != 0)
    }

    pub fn node_flags(&self, id: NodeId) -> u16 {
        self.nodes.get(id as usize).map_or(0, |n| n.flags)
    }

    #[inline]
    pub fn parent(&self, id: NodeId) -> NodeId {
        self.nodes.get(id as usize).map_or(NO_PARENT, |n| n.parent)
    }

    /// Original-case name.
    pub fn name(&self, id: NodeId) -> &str {
        let Some(n) = self.nodes.get(id as usize) else {
            return "";
        };
        let bytes = &self.arena[n.name_off as usize..n.name_off as usize + n.name_len as usize];
        std::str::from_utf8(bytes).unwrap_or_default()
    }

    #[inline]
    pub(crate) fn fold_bytes(&self, id: NodeId) -> &[u8] {
        let n = &self.nodes[id as usize];
        &self.arena[n.fold_off as usize..n.fold_off as usize + n.fold_len as usize]
    }

    #[inline]
    pub(crate) fn mask(&self, id: NodeId) -> u64 {
        self.masks[id as usize]
    }

    /// Depth (roots are 0).
    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = self.parent(id);
        while cur != NO_PARENT && d < 4096 {
            d += 1;
            cur = self.parent(cur);
        }
        d
    }

    /// Full path using the root's own separator style.
    pub fn full_path(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != NO_PARENT && parts.len() < 4096 {
            parts.push(self.name(cur));
            cur = self.parent(cur);
        }
        parts.reverse();
        let sep = match parts.first() {
            Some(r) if r.contains('\\') || r.ends_with(':') => '\\',
            _ => '/',
        };
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 && !out.ends_with(['/', '\\']) {
                out.push(sep);
            }
            out.push_str(p);
        }
        out
    }

    /// Iterate live node ids.
    pub fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.nodes.len() as NodeId).filter(|&i| !self.is_deleted(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sample() -> PathIndex {
        let mut ix = PathIndex::new();
        let r = ix.add_root("C:").expect("root");
        let users = ix.insert(r, "Users", true).expect("insert");
        let me = ix.insert(users, "Pavan", true).expect("insert");
        ix.insert(me, "Notes.TXT", false).expect("insert");
        ix.insert(me, "résumé.pdf", false).expect("insert");
        ix
    }

    #[test]
    fn insert_resolve_and_paths() {
        let ix = sample();
        assert_eq!(ix.len(), 5);
        let id = ix
            .resolve(r"C:\Users\Pavan\notes.txt")
            .expect("resolve case-insensitively");
        assert_eq!(ix.name(id), "Notes.TXT");
        assert_eq!(ix.full_path(id), r"C:\Users\Pavan\Notes.TXT");
        assert!(ix.resolve("C:/users/pavan/RÉSUMÉ.pdf").is_some());
        assert!(ix.resolve(r"C:\Users\nobody").is_none());
        assert!(ix.resolve(r"D:\x").is_none());
        assert_eq!(ix.depth(id), 3);
    }

    #[test]
    fn duplicate_insert_replaces() {
        let mut ix = sample();
        let p = ix.resolve(r"C:\Users\Pavan").expect("dir");
        ix.insert(p, "notes.txt", false).expect("insert");
        assert_eq!(ix.len(), 5, "replaced, not duplicated");
        let id = ix.resolve(r"C:\Users\Pavan\NOTES.txt").expect("found");
        assert_eq!(ix.name(id), "notes.txt");
    }

    #[test]
    fn remove_and_rename() {
        let mut ix = sample();
        let users = ix.resolve(r"C:\Users").expect("users");
        let me = ix.resolve(r"C:\Users\Pavan").expect("me");
        let root = ix.resolve("C:").expect("root");
        ix.rename(me, root, "Home").expect("rename");
        assert!(ix.resolve(r"C:\Home\Notes.txt").is_some());
        assert!(ix.resolve(r"C:\Users\Pavan").is_none());
        assert!(ix.rename(root, me, "loop").is_err(), "cycle refused");
        assert_eq!(ix.remove_subtree(users), 1);
        assert_eq!(ix.remove_subtree(me), 3);
        assert_eq!(ix.len(), 1);
        assert!(ix.garbage_bytes() > 0);
    }

    #[test]
    fn path_changes_apply() {
        let mut ix = PathIndex::new();
        ix.add_root("/home/me").expect("root");
        ix.apply(&IndexChange::Created {
            path: "/home/me/src/nx/lib.rs".into(),
            is_dir: false,
        })
        .expect("create");
        assert!(ix
            .resolve("/home/me/src/nx")
            .is_some_and(|id| ix.is_dir(id)));
        ix.apply(&IndexChange::Renamed {
            from: "/home/me/src".into(),
            to: "/home/me/code".into(),
        })
        .expect("rename");
        assert!(ix.resolve("/home/me/code/nx/lib.rs").is_some());
        ix.apply(&IndexChange::Removed {
            path: "/home/me/code".into(),
        })
        .expect("remove");
        assert_eq!(ix.len(), 1);
        // Outside every root: ignored, not an error.
        ix.apply(&IndexChange::Created {
            path: "/etc/passwd".into(),
            is_dir: false,
        })
        .expect("ignored");
        assert_eq!(ix.len(), 1);
    }

    #[test]
    fn masks_and_fold() {
        assert_eq!(fold("ABC"), "abc");
        assert_eq!(fold("ÄÖ"), "äö");
        let m = char_mask(b"abc");
        assert_eq!(m & char_mask(b"cab"), char_mask(b"cab"));
        assert_ne!(m & char_mask(b"z"), char_mask(b"z"));
        assert_eq!(std::mem::size_of::<Node>(), 20);
    }

    proptest! {
        #[test]
        fn every_inserted_path_resolves(paths in proptest::collection::vec(proptest::collection::vec("[a-dA-D]{1,3}", 1..5), 1..40)) {
            let mut ix = PathIndex::new();
            ix.add_root("/r").expect("root");
            for p in &paths {
                let s = format!("/r/{}", p.join("/"));
                ix.apply(&IndexChange::Created { path: s, is_dir: false }).expect("create");
            }
            for p in &paths {
                let s = format!("/r/{}", p.join("/"));
                let id = ix.resolve(&s);
                prop_assert!(id.is_some(), "missing {}", s);
                let id = id.unwrap_or_default();
                prop_assert!(ix.full_path(id).eq_ignore_ascii_case(&s));
            }
            // Live count equals number of distinct case-folded prefixes + root.
            let mut set = std::collections::HashSet::new();
            for p in &paths {
                for k in 1..=p.len() {
                    set.insert(p[..k].join("/").to_ascii_lowercase());
                }
            }
            prop_assert_eq!(ix.len(), set.len() + 1);
        }
    }
}
