//! Crash-safe on-disk index persistence with CRC32 checksums and versioning.

use super::{char_mask, child_key, flags, IdMap, Node, NodeId, PathIndex, NO_PARENT};
use crate::error::{FsError, FsResult};
use crc32fast::Hasher;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

pub const INDEX_MAGIC: &[u8; 4] = b"NXIX";
pub const INDEX_VERSION: u32 = 1;
const HEADER_SIZE: usize = 48;
const NODE_RECORD_SIZE: usize = 20;

// Maximum sanity limits to prevent OOM on malicious or garbage files
const MAX_ROOTS: usize = 1024;
const MAX_NODES: usize = 100_000_000;
const MAX_ARENA_BYTES: usize = 1_000_000_000;

impl PathIndex {
    /// Save index to a writer with compact binary format and CRC32 checksum.
    pub fn save_to<W: Write>(&self, w: &mut W) -> FsResult<()> {
        let roots_count = u32::try_from(self.roots.len())
            .map_err(|_| FsError::Malformed("roots count overflow".into()))?;
        let nodes_count = u32::try_from(self.nodes.len())
            .map_err(|_| FsError::Malformed("nodes count overflow".into()))?;
        let arena_len = u32::try_from(self.arena.len())
            .map_err(|_| FsError::Malformed("arena bytes overflow".into()))?;
        let live_count = self.live as u64;
        let garbage_bytes = self.garbage_bytes as u64;

        // Compute CRC32 of payload (roots + nodes + arena)
        let mut hasher = Hasher::new();

        // 1. Roots buffer
        let mut roots_bytes = Vec::with_capacity(self.roots.len() * 4);
        for &r in &self.roots {
            roots_bytes.extend_from_slice(&r.to_le_bytes());
        }
        hasher.update(&roots_bytes);

        // 2. Nodes buffer (20 bytes per node)
        let mut nodes_bytes = Vec::with_capacity(self.nodes.len() * NODE_RECORD_SIZE);
        for n in &self.nodes {
            nodes_bytes.extend_from_slice(&n.name_off.to_le_bytes());
            nodes_bytes.extend_from_slice(&n.fold_off.to_le_bytes());
            nodes_bytes.extend_from_slice(&n.parent.to_le_bytes());
            nodes_bytes.extend_from_slice(&n.name_len.to_le_bytes());
            nodes_bytes.extend_from_slice(&n.fold_len.to_le_bytes());
            nodes_bytes.extend_from_slice(&n.flags.to_le_bytes());
            nodes_bytes.extend_from_slice(&0u16.to_le_bytes()); // padding
        }
        hasher.update(&nodes_bytes);

        // 3. Arena
        hasher.update(&self.arena);

        let checksum = hasher.finalize();

        // Write Header (48 bytes)
        let mut header = [0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(INDEX_MAGIC);
        header[4..8].copy_from_slice(&INDEX_VERSION.to_le_bytes());
        header[8..12].copy_from_slice(&0u32.to_le_bytes()); // flags
        header[12..16].copy_from_slice(&roots_count.to_le_bytes());
        header[16..20].copy_from_slice(&nodes_count.to_le_bytes());
        header[20..24].copy_from_slice(&arena_len.to_le_bytes());
        header[24..32].copy_from_slice(&live_count.to_le_bytes());
        header[32..40].copy_from_slice(&garbage_bytes.to_le_bytes());
        header[40..44].copy_from_slice(&checksum.to_le_bytes());
        header[44..48].copy_from_slice(&0u32.to_le_bytes()); // reserved

        w.write_all(&header)
            .map_err(|e| FsError::from_io("index", e))?;
        w.write_all(&roots_bytes)
            .map_err(|e| FsError::from_io("index", e))?;
        w.write_all(&nodes_bytes)
            .map_err(|e| FsError::from_io("index", e))?;
        w.write_all(&self.arena)
            .map_err(|e| FsError::from_io("index", e))?;

        Ok(())
    }

    /// Crash-safe atomic write to disk: writes to temp file, flushes/syncs, then renames.
    pub fn save_file(&self, path: &Path) -> FsResult<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let temp_path = parent.join(format!(".tmp_index_{}", std::process::id()));

        {
            let file = File::create(&temp_path).map_err(|e| FsError::from_io(&temp_path, e))?;
            let mut writer = BufWriter::with_capacity(256 * 1024, file);
            self.save_to(&mut writer)?;
            writer
                .flush()
                .map_err(|e| FsError::from_io(&temp_path, e))?;
            let file = writer
                .into_inner()
                .map_err(|e| FsError::from_io(&temp_path, e.into_error()))?;
            file.sync_all()
                .map_err(|e| FsError::from_io(&temp_path, e))?;
        }

        std::fs::rename(&temp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            FsError::from_io(path, e)
        })?;

        Ok(())
    }

    /// Load index from reader, validating magic, version, CRC32, and all internal offsets.
    pub fn load_from<R: Read>(r: &mut R) -> FsResult<Self> {
        let mut header = [0u8; HEADER_SIZE];
        r.read_exact(&mut header)
            .map_err(|e| FsError::Malformed(format!("truncated index header: {e}")))?;

        if &header[0..4] != INDEX_MAGIC {
            return Err(FsError::Malformed("invalid index magic".into()));
        }

        let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if version != INDEX_VERSION {
            return Err(FsError::Malformed(format!(
                "unsupported index version {version}, expected {INDEX_VERSION}"
            )));
        }

        let roots_count =
            u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
        let nodes_count =
            u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;
        let arena_len =
            u32::from_le_bytes([header[20], header[21], header[22], header[23]]) as usize;
        let live_count = u64::from_le_bytes([
            header[24], header[25], header[26], header[27], header[28], header[29], header[30],
            header[31],
        ]) as usize;
        let garbage_bytes = u64::from_le_bytes([
            header[32], header[33], header[34], header[35], header[36], header[37], header[38],
            header[39],
        ]) as usize;
        let expected_checksum =
            u32::from_le_bytes([header[40], header[41], header[42], header[43]]);

        if roots_count > MAX_ROOTS || nodes_count > MAX_NODES || arena_len > MAX_ARENA_BYTES {
            return Err(FsError::Malformed(
                "index dimensions exceed maximum limits".into(),
            ));
        }

        // Read payload
        let roots_byte_len = roots_count
            .checked_mul(4)
            .ok_or_else(|| FsError::Malformed("roots size overflow".into()))?;
        let nodes_byte_len = nodes_count
            .checked_mul(NODE_RECORD_SIZE)
            .ok_or_else(|| FsError::Malformed("nodes size overflow".into()))?;

        let mut roots_bytes = vec![0u8; roots_byte_len];
        r.read_exact(&mut roots_bytes)
            .map_err(|e| FsError::Malformed(format!("truncated roots: {e}")))?;

        let mut nodes_bytes = vec![0u8; nodes_byte_len];
        r.read_exact(&mut nodes_bytes)
            .map_err(|e| FsError::Malformed(format!("truncated nodes: {e}")))?;

        let mut arena = vec![0u8; arena_len];
        r.read_exact(&mut arena)
            .map_err(|e| FsError::Malformed(format!("truncated arena: {e}")))?;

        // Verify CRC32 checksum
        let mut hasher = Hasher::new();
        hasher.update(&roots_bytes);
        hasher.update(&nodes_bytes);
        hasher.update(&arena);
        let actual_checksum = hasher.finalize();

        if actual_checksum != expected_checksum {
            return Err(FsError::Malformed(format!(
                "checksum mismatch: expected {expected_checksum:08x}, got {actual_checksum:08x}"
            )));
        }

        // Parse roots
        let mut roots = Vec::with_capacity(roots_count);
        for chunk in roots_bytes.as_chunks::<4>().0 {
            let r = u32::from_le_bytes(*chunk);
            if r as usize >= nodes_count {
                return Err(FsError::Malformed("root node id out of bounds".into()));
            }
            roots.push(r);
        }

        // Parse nodes and validate bounds
        let mut nodes = Vec::with_capacity(nodes_count);
        let mut masks = Vec::with_capacity(nodes_count);
        let mut children = IdMap::with_capacity_and_hasher(nodes_count, Default::default());
        let mut overflow = Vec::new();

        for (id_idx, chunk) in nodes_bytes
            .as_chunks::<NODE_RECORD_SIZE>()
            .0
            .iter()
            .enumerate()
        {
            let id = id_idx as NodeId;
            let name_off = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let fold_off = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let parent = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
            let name_len = u16::from_le_bytes([chunk[12], chunk[13]]);
            let fold_len = u16::from_le_bytes([chunk[14], chunk[15]]);
            let node_flags = u16::from_le_bytes([chunk[16], chunk[17]]);

            // Validate arena bounds
            let name_end = (name_off as usize)
                .checked_add(name_len as usize)
                .ok_or_else(|| FsError::Malformed("name offset overflow".into()))?;
            let fold_end = (fold_off as usize)
                .checked_add(fold_len as usize)
                .ok_or_else(|| FsError::Malformed("fold offset overflow".into()))?;

            if name_end > arena_len || fold_end > arena_len {
                return Err(FsError::Malformed(
                    "node string bounds exceed arena length".into(),
                ));
            }

            // Validate parent
            if parent != NO_PARENT && parent as usize >= nodes_count {
                return Err(FsError::Malformed("node parent id out of bounds".into()));
            }

            // UTF-8 validation
            if std::str::from_utf8(&arena[name_off as usize..name_end]).is_err() {
                return Err(FsError::Malformed("invalid UTF-8 in node name".into()));
            }

            nodes.push(Node {
                name_off,
                fold_off,
                parent,
                name_len,
                fold_len,
                flags: node_flags,
            });

            // Reconstruct mask
            let fb = &arena[fold_off as usize..fold_end];
            let mask = if node_flags & flags::DELETED == 0 {
                char_mask(fb)
            } else {
                0
            };
            masks.push(mask);

            // Reconstruct children map for live nodes
            if parent != NO_PARENT && (node_flags & flags::DELETED == 0) {
                let key = child_key(parent, fb);
                if let Some(prev) = children.insert(key, id) {
                    overflow.push((key, prev));
                }
            }
        }

        Ok(PathIndex {
            arena,
            nodes,
            masks,
            children,
            overflow,
            roots,
            live: live_count,
            garbage_bytes,
        })
    }

    /// Load index from file. Returns error if corrupt, missing, or truncated.
    pub fn load_file(path: &Path) -> FsResult<Self> {
        let file = File::open(path).map_err(|e| FsError::from_io(path, e))?;
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        Self::load_from(&mut reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> PathIndex {
        let mut idx = PathIndex::new();
        let r = idx.add_root("C:").expect("root");
        let users = idx.insert(r, "Users", true).expect("users");
        let pavan = idx.insert(users, "Pavan", true).expect("pavan");
        idx.insert(pavan, "Notes.txt", false).expect("notes");
        idx.insert(pavan, "Project.rs", false).expect("project");
        idx
    }

    #[test]
    fn index_round_trip() {
        let original = sample_index();
        let mut buf = Vec::new();
        original.save_to(&mut buf).expect("save_to");

        let mut slice = &buf[..];
        let loaded = PathIndex::load_from(&mut slice).expect("load_from");

        assert_eq!(loaded.len(), original.len());
        let id = loaded
            .resolve(r"C:\Users\Pavan\Notes.txt")
            .expect("resolve");
        assert_eq!(loaded.name(id), "Notes.txt");
        assert_eq!(loaded.full_path(id), r"C:\Users\Pavan\Notes.txt");
        assert!(loaded.resolve(r"C:\Users\Pavan\Project.rs").is_some());
        assert!(loaded.resolve(r"C:\Users\Pavan\Missing.doc").is_none());
    }

    #[test]
    fn file_save_and_load_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("index.nxix");

        let original = sample_index();
        original.save_file(&file_path).expect("save_file");
        assert!(file_path.exists());

        let loaded = PathIndex::load_file(&file_path).expect("load_file");
        assert_eq!(loaded.len(), original.len());
        assert!(loaded.resolve(r"C:\Users\Pavan\Notes.txt").is_some());
    }

    #[test]
    fn corruption_detected_truncated() {
        let original = sample_index();
        let mut buf = Vec::new();
        original.save_to(&mut buf).expect("save_to");

        // Truncate to less than header
        let mut truncated = &buf[..20];
        assert!(PathIndex::load_from(&mut truncated).is_err());

        // Truncate halfway through payload
        let mut truncated_payload = &buf[..buf.len() - 10];
        assert!(PathIndex::load_from(&mut truncated_payload).is_err());
    }

    #[test]
    fn corruption_detected_bad_magic() {
        let original = sample_index();
        let mut buf = Vec::new();
        original.save_to(&mut buf).expect("save_to");

        buf[0] = b'B';
        buf[1] = b'A';
        buf[2] = b'D';
        buf[3] = b'!';
        let mut slice = &buf[..];
        assert!(PathIndex::load_from(&mut slice).is_err());
    }

    #[test]
    fn corruption_detected_crc_mismatch() {
        let original = sample_index();
        let mut buf = Vec::new();
        original.save_to(&mut buf).expect("save_to");

        // Flip a bit in the payload (arena region at the end)
        let last = buf.len() - 1;
        buf[last] ^= 0x55;

        let mut slice = &buf[..];
        let res = PathIndex::load_from(&mut slice);
        assert!(res.is_err(), "must detect bit flip via CRC32");
        let err_msg = format!("{}", res.err().unwrap());
        assert!(err_msg.contains("checksum mismatch"));
    }
}
