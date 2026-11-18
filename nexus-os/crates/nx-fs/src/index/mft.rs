//! Build and live-update a [`PathIndex`] from NTFS MFT / USN records.
//!
//! The pure part ([`MftTree`]) turns records keyed by file reference into a
//! path index and applies journal records idempotently. It is fully tested
//! on Linux with synthetic records. The Windows part (`NtfsIndexer`, in
//! `platform::windows::volume`) only does the `DeviceIoControl` I/O.

use super::usn::{reason, FileRef, UsnRecord, ATTR_DIRECTORY, ATTR_HIDDEN, ATTR_REPARSE};
use super::{flags, NodeId, PathIndex};
use crate::error::FsResult;
use std::collections::HashMap;

/// NTFS root directory file reference (MFT record 5, sequence 5).
pub const NTFS_ROOT_REF: FileRef = 0x0005_0000_0000_0005;

/// Mask of the MFT record number inside a 64-bit file reference.
const RECORD_MASK: u128 = 0x0000_ffff_ffff_ffff;

/// Largest MFT record number kept in the dense table (beyond: hash map).
const DENSE_MAX: u64 = 1 << 25;
const NONE: NodeId = NodeId::MAX;

/// File reference -> node mapping (kept by the live indexer between syncs).
///
/// NTFS references are a dense 48-bit MFT record number plus a 16-bit
/// sequence number, so (like Everything) we index a flat table by record
/// number: 6 bytes per MFT record instead of ~32 for a hash map. Wide
/// (ReFS / V3) references go to a small map.
#[derive(Debug, Default)]
pub struct MftRefs {
    dense: Vec<NodeId>,
    seqs: Vec<u16>,
    wide: HashMap<FileRef, NodeId>,
    root: NodeId,
    /// Highest USN applied (resume point for the journal reader).
    pub last_usn: i64,
}

impl MftRefs {
    fn slot(r: FileRef) -> Option<(usize, u16)> {
        if r >> 64 != 0 {
            return None;
        }
        let rec = (r & RECORD_MASK) as u64;
        (rec < DENSE_MAX).then_some((rec as usize, (r >> 48) as u16))
    }

    pub fn get(&self, r: FileRef) -> Option<NodeId> {
        match Self::slot(r) {
            Some((rec, seq)) => match (self.dense.get(rec), self.seqs.get(rec)) {
                (Some(&id), Some(&s)) if id != NONE && s == seq => Some(id),
                _ => None,
            },
            None => self.wide.get(&r).copied(),
        }
    }

    pub fn insert(&mut self, r: FileRef, id: NodeId) {
        match Self::slot(r) {
            Some((rec, seq)) => {
                if rec >= self.dense.len() {
                    let n = (rec + 1).max(self.dense.len() * 5 / 4);
                    self.dense.resize(n, NONE);
                    self.seqs.resize(n, 0);
                }
                self.dense[rec] = id;
                self.seqs[rec] = seq;
            }
            None => {
                self.wide.insert(r, id);
            }
        }
    }

    pub fn remove(&mut self, r: FileRef) -> Option<NodeId> {
        let id = self.get(r)?;
        match Self::slot(r) {
            Some((rec, _)) => self.dense[rec] = NONE,
            None => {
                self.wide.remove(&r);
            }
        }
        Some(id)
    }

    fn reserve_dense(&mut self, max_rec: usize) {
        if max_rec < DENSE_MAX as usize {
            self.dense = vec![NONE; max_rec + 1];
            self.seqs = vec![0; max_rec + 1];
        }
    }
}

/// An index plus the reference map that keeps it live.
#[derive(Debug, Default)]
pub struct MftTree {
    pub index: PathIndex,
    pub refs: MftRefs,
}

impl MftTree {
    /// Build from an MFT enumeration. Records may arrive in any order
    /// (children before parents); orphans attach to the volume root, and
    /// corrupt parent cycles are broken instead of looping forever.
    pub fn build(volume_root: &str, records: Vec<UsnRecord>) -> FsResult<MftTree> {
        let names: usize = records.iter().map(|r| r.name.len()).sum();
        let mut index = PathIndex::with_capacity(records.len() + 1, names + names / 4);
        let root = index.add_root(volume_root)?;
        let mut tree = MftTree {
            index,
            refs: MftRefs {
                root,
                ..MftRefs::default()
            },
        };
        let max_rec = records
            .iter()
            .filter_map(|r| MftRefs::slot(r.file_ref))
            .map(|(rec, _)| rec)
            .max()
            .unwrap_or(0);
        tree.refs.reserve_dense(max_rec.max(5));
        tree.refs.insert(NTFS_ROOT_REF, root);

        let mut pos: HashMap<FileRef, usize> = HashMap::with_capacity(records.len());
        for (i, r) in records.iter().enumerate() {
            if r.file_ref != NTFS_ROOT_REF {
                pos.insert(r.file_ref, i);
            }
            tree.refs.last_usn = tree.refs.last_usn.max(r.usn);
        }
        // 0 = unvisited, 1 = on stack, 2 = done.
        let mut state = vec![0u8; records.len()];
        let mut stack: Vec<usize> = Vec::new();
        for start in 0..records.len() {
            if state[start] != 0 || records[start].file_ref == NTFS_ROOT_REF {
                continue;
            }
            stack.push(start);
            while let Some(&i) = stack.last() {
                let r = &records[i];
                let parent_pos = pos.get(&r.parent_ref).copied();
                match parent_pos {
                    Some(p) if state[p] == 0 => {
                        state[i] = 1;
                        state[p] = 1;
                        stack.push(p);
                        continue;
                    }
                    _ => {}
                }
                stack.pop();
                if state[i] == 2 {
                    continue;
                }
                // Parent is done, missing (-> root), or on the stack (cycle -> root).
                let parent = tree.refs.get(r.parent_ref).unwrap_or(root);
                let id = tree.index.insert(parent, &r.name, r.is_dir())?;
                tree.index.set_flags(id, attr_flags(r.attributes));
                tree.refs.insert(r.file_ref, id);
                state[i] = 2;
            }
        }
        tree.index.shrink_to_fit();
        Ok(tree)
    }

    /// Apply one journal record to the index (see [`MftRefs::apply`]).
    pub fn apply(&mut self, r: &UsnRecord) -> FsResult<bool> {
        self.refs.apply(&mut self.index, r)
    }

    pub fn apply_all(&mut self, records: &[UsnRecord]) -> FsResult<usize> {
        self.refs.apply_all(&mut self.index, records)
    }

    pub fn last_usn(&self) -> i64 {
        self.refs.last_usn
    }

    /// Split into the index and the live reference map.
    pub fn into_parts(self) -> (PathIndex, MftRefs) {
        (self.index, self.refs)
    }

    /// MFT record number of a 64-bit reference (diagnostics).
    pub fn record_number(file_ref: FileRef) -> u64 {
        (file_ref & RECORD_MASK) as u64
    }
}

impl MftRefs {
    /// Apply one journal record. Idempotent: replaying records is harmless.
    /// Returns true if the index changed.
    pub fn apply(&mut self, index: &mut PathIndex, r: &UsnRecord) -> FsResult<bool> {
        self.last_usn = self.last_usn.max(r.usn);
        if r.reason & reason::FILE_DELETE != 0 {
            if let Some(id) = self.remove(r.file_ref) {
                index.remove_subtree(id);
                return Ok(true);
            }
            return Ok(false);
        }
        if r.reason & reason::RENAME_OLD_NAME != 0 && r.reason & reason::RENAME_NEW_NAME == 0 {
            // The new-name record that follows carries the destination.
            return Ok(false);
        }
        if r.reason & (reason::FILE_CREATE | reason::RENAME_NEW_NAME) == 0 {
            if r.reason & reason::BASIC_INFO_CHANGE != 0 {
                if let Some(id) = self.get(r.file_ref) {
                    index.set_flags(id, attr_flags(r.attributes));
                }
            }
            return Ok(false);
        }
        let parent = self.get(r.parent_ref).unwrap_or(self.root);
        match self.get(r.file_ref) {
            Some(id) if !index.is_deleted(id) => {
                if index.parent(id) == parent && index.name(id) == r.name {
                    return Ok(false);
                }
                index.rename(id, parent, &r.name)?;
            }
            _ => {
                let id = index.insert(parent, &r.name, r.attributes & ATTR_DIRECTORY != 0)?;
                index.set_flags(id, attr_flags(r.attributes));
                self.insert(r.file_ref, id);
            }
        }
        Ok(true)
    }

    /// Apply a batch; returns how many records changed the index.
    pub fn apply_all(&mut self, index: &mut PathIndex, records: &[UsnRecord]) -> FsResult<usize> {
        let mut n = 0;
        for r in records {
            if self.apply(index, r)? {
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn node_for(&self, file_ref: FileRef) -> Option<NodeId> {
        self.get(file_ref)
    }

    /// Extra heap used by the reference map.
    pub fn map_bytes(&self) -> usize {
        self.dense.capacity() * 4 + self.seqs.capacity() * 2 + self.wide.capacity() * 8 / 7 * 33
    }
}

fn attr_flags(attrs: u32) -> u16 {
    let mut f = 0;
    if attrs & ATTR_HIDDEN != 0 {
        f |= flags::HIDDEN;
    }
    if attrs & ATTR_REPARSE != 0 {
        f |= flags::LINK;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::matcher::Query;
    use crate::index::{engine::SearchOptions, frecency::FrecencyStore, SearchEngine};
    use proptest::prelude::*;

    fn rec(fr: u128, parent: u128, name: &str, dir: bool, reason: u32, usn: i64) -> UsnRecord {
        UsnRecord {
            file_ref: fr,
            parent_ref: parent,
            usn,
            reason,
            attributes: if dir { ATTR_DIRECTORY } else { 0x20 },
            name: name.into(),
        }
    }

    #[test]
    fn builds_out_of_order_with_orphans_and_cycles() {
        let records = vec![
            rec(30, 20, "deep.txt", false, 0, 3),
            rec(20, 10, "sub", true, 0, 2),
            rec(10, NTFS_ROOT_REF, "Users", true, 0, 1),
            rec(40, 999, "orphan.log", false, 0, 4),
            // corrupt cycle 50 <-> 51
            rec(50, 51, "c1", true, 0, 5),
            rec(51, 50, "c2", true, 0, 6),
        ];
        let t = MftTree::build("C:", records).expect("build");
        assert!(t.index.resolve(r"C:\Users\sub\deep.txt").is_some());
        assert!(t.index.resolve(r"C:\orphan.log").is_some());
        assert_eq!(t.index.len(), 7);
        assert_eq!(t.last_usn(), 6);
        assert_eq!(MftTree::record_number(0x0005_0000_0000_0005), 5);
    }

    #[test]
    fn journal_create_rename_delete() {
        let mut t =
            MftTree::build("C:", vec![rec(10, NTFS_ROOT_REF, "docs", true, 0, 1)]).expect("build");
        let create = rec(
            11,
            10,
            "draft.md",
            false,
            reason::FILE_CREATE | reason::CLOSE,
            10,
        );
        assert!(t.apply(&create).expect("apply"));
        assert!(!t.apply(&create).expect("replay"), "idempotent");
        assert!(t.index.resolve(r"C:\docs\draft.md").is_some());
        // rename = old-name record (ignored) + new-name record
        assert!(!t
            .apply(&rec(11, 10, "draft.md", false, reason::RENAME_OLD_NAME, 11))
            .expect("old"));
        assert!(t
            .apply(&rec(
                11,
                NTFS_ROOT_REF,
                "final.md",
                false,
                reason::RENAME_NEW_NAME | reason::CLOSE,
                12
            ))
            .expect("new"));
        assert!(t.index.resolve(r"C:\docs\draft.md").is_none());
        assert!(t.index.resolve(r"C:\final.md").is_some());
        // deleting a directory removes its subtree
        t.apply(&rec(12, 10, "inner", false, reason::FILE_CREATE, 13))
            .expect("create");
        assert!(t
            .apply(&rec(
                10,
                NTFS_ROOT_REF,
                "docs",
                true,
                reason::FILE_DELETE | reason::CLOSE,
                14
            ))
            .expect("delete"));
        assert!(t.index.resolve(r"C:\docs").is_none());
        assert!(!t
            .apply(&rec(77, 10, "x", false, reason::FILE_DELETE, 15))
            .expect("unknown delete"));
        assert_eq!(t.last_usn(), 15);
        let e = SearchEngine::new(std::mem::take(&mut t.index), FrecencyStore::default(), 0);
        let hits = e.search(&Query::parse("final"), &SearchOptions::default());
        assert_eq!(hits[0].path, r"C:\final.md");
    }

    #[test]
    fn refs_dense_wide_and_sequence_checked() {
        let mut m = MftRefs::default();
        m.insert(0x0003_0000_0000_0010, 7);
        assert_eq!(m.get(0x0003_0000_0000_0010), Some(7));
        assert_eq!(
            m.get(0x0004_0000_0000_0010),
            None,
            "reused record with a new sequence"
        );
        m.insert(1u128 << 80, 9);
        assert_eq!(m.get(1u128 << 80), Some(9));
        assert_eq!(m.remove(0x0003_0000_0000_0010), Some(7));
        assert_eq!(m.get(0x0003_0000_0000_0010), None);
        assert_eq!(m.remove(1u128 << 80), Some(9));
    }

    proptest! {
        #[test]
        fn any_order_builds_same_paths(perm in Just((0..12usize).collect::<Vec<_>>()).prop_shuffle()) {
            // A small fixed tree, enumerated in a random order.
            let base: Vec<UsnRecord> = (0..12u128).map(|i| {
                let parent = if i < 3 { NTFS_ROOT_REF } else { 100 + i / 3 - 1 };
                rec(100 + i, parent, &format!("n{i}"), i < 6, 0, i as i64)
            }).collect();
            let shuffled: Vec<UsnRecord> = perm.iter().map(|&i| base[i].clone()).collect();
            let a = MftTree::build("C:", base.clone()).expect("a");
            let b = MftTree::build("C:", shuffled).expect("b");
            let mut pa: Vec<String> = a.index.ids().map(|i| a.index.full_path(i)).collect();
            let mut pb: Vec<String> = b.index.ids().map(|i| b.index.full_path(i)).collect();
            pa.sort();
            pb.sort();
            prop_assert_eq!(pa, pb);
        }
    }
}
