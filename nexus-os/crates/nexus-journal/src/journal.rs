//! Atomic, fsynced, corruption-tolerant journal file.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::crc::crc32;
use crate::mutation::{decode_payload, encode_payload, Mutation};
use crate::mutator::SystemMutator;

const MAGIC: &[u8; 4] = b"NXJR";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 16;
const REC_MAGIC: &[u8; 4] = b"NXR1";
/// A corrupt length must not allocate unbounded memory.
const MAX_PAYLOAD: usize = 1024 * 1024;

#[derive(Debug)]
pub enum JournalError {
    Io(io::Error),
    /// Magic matches but the version is not one this build understands.
    /// The file is left untouched.
    UnsupportedVersion(u32),
    Apply(String),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "journal io: {e}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "journal version {v} is not supported")
            }
            Self::Apply(msg) => write!(f, "mutation failed: {msg}"),
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for JournalError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub reverted: usize,
    pub failures: Vec<String>,
}

/// Outstanding mutations, oldest first.
///
/// The process holds an exclusive advisory lock on a sibling `*.lock` file
/// for the lifetime of this value so two Nexus processes cannot interleave
/// rewrites. The data file itself is replaced atomically and is not held open.
pub struct Journal {
    path: PathBuf,
    /// Held open so the advisory lock survives until drop.
    _lock: File,
    entries: Vec<Mutation>,
}

impl Journal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let lock = lock_exclusive(&path)?;
        let (entries, dropped) = if path.exists() {
            load_file(&path)?
        } else {
            (Vec::new(), false)
        };
        let mut journal = Self {
            path,
            _lock: lock,
            entries,
        };
        if dropped || !journal.path.exists() {
            // Rewrite a clean file so the next crash does not re-read junk.
            // Unsupported versions never reach here: load_file returns Err.
            journal.atomic_store()?;
        }
        Ok(journal)
    }

    /// Parse a journal without taking the lock. Used when the owning process
    /// is already inside a panic and cannot re-lock its own file.
    pub fn read_durable(path: &Path) -> Result<Vec<Mutation>, JournalError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let (entries, _) = load_file(path)?;
        Ok(entries)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entries(&self) -> &[Mutation] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Durably append `mutation`. Returns only after the new file is fsynced
    /// and renamed into place. Callers apply the mutation after this returns.
    pub fn record(&mut self, mutation: &Mutation) -> Result<(), JournalError> {
        self.entries.push(mutation.clone());
        if let Err(e) = self.atomic_store() {
            self.entries.pop();
            return Err(e);
        }
        Ok(())
    }

    pub fn remove_last_equal(&mut self, mutation: &Mutation) -> Result<bool, JournalError> {
        let Some(idx) = self.entries.iter().rposition(|m| m == mutation) else {
            return Ok(false);
        };
        let removed = self.entries.remove(idx);
        if let Err(e) = self.atomic_store() {
            self.entries.insert(idx, removed);
            return Err(e);
        }
        Ok(true)
    }

    /// Revert every entry, newest first, and shrink the durable file after
    /// each successful step so a crash mid-restore retries only what remains.
    pub fn restore_all<M: SystemMutator>(
        &mut self,
        system: &mut M,
    ) -> Result<RestoreReport, JournalError> {
        let mut report = RestoreReport {
            reverted: 0,
            failures: Vec::new(),
        };
        while !self.entries.is_empty() {
            let Some(mutation) = self.entries.pop() else {
                break;
            };
            let revert_result = system.revert(&mutation);
            if let Err(e) = self.atomic_store() {
                self.entries.push(mutation);
                return Err(e);
            }
            match revert_result {
                Ok(()) => report.reverted += 1,
                Err(e) => report.failures.push(format!("{mutation:?}: {e}")),
            }
        }
        Ok(report)
    }

    fn atomic_store(&mut self) -> Result<(), JournalError> {
        atomic_write(&self.path, &encode_file(&self.entries))
    }
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    PathBuf::from(lock)
}

fn lock_exclusive(path: &Path) -> Result<File, JournalError> {
    let lock_path = lock_path_for(path);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;
    file.lock()?;
    Ok(file)
}

fn encode_file(entries: &[Mutation]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_LEN + entries.len() * 32);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    let header_crc = crc32(&buf);
    buf.extend_from_slice(&header_crc.to_le_bytes());
    for entry in entries {
        let payload = encode_payload(entry);
        buf.extend_from_slice(REC_MAGIC);
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&crc32(&payload).to_le_bytes());
        buf.extend_from_slice(&payload);
    }
    buf
}

fn load_file(path: &Path) -> Result<(Vec<Mutation>, bool), JournalError> {
    let bytes = fs::read(path)?;
    parse_bytes(&bytes)
}

/// Returns `(entries, dropped_anything)`.
fn parse_bytes(bytes: &[u8]) -> Result<(Vec<Mutation>, bool), JournalError> {
    if bytes.is_empty() {
        return Ok((Vec::new(), false));
    }
    if bytes.len() >= 4 && &bytes[..4] == MAGIC {
        if bytes.len() < HEADER_LEN {
            // Torn header. Nothing trusted.
            return Ok((Vec::new(), true));
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != VERSION {
            return Err(JournalError::UnsupportedVersion(version));
        }
        let stored_crc = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let header_ok = stored_crc == crc32(&bytes[..12]);
        let (entries, dropped_records) = parse_records(&bytes[HEADER_LEN..]);
        return Ok((entries, !header_ok || dropped_records));
    }
    // No magic: scan the whole buffer for records (shifted/corrupt file).
    let (entries, _) = parse_records(bytes);
    Ok((entries, true))
}

/// Walk records. A bad CRC or a truncated tail is dropped; scanning resumes
/// at the next `NXR1` magic so one corrupt record does not discard the rest.
fn parse_records(mut bytes: &[u8]) -> (Vec<Mutation>, bool) {
    let mut entries = Vec::new();
    let mut dropped = false;
    while !bytes.is_empty() {
        let Some(rel) = find_magic(bytes) else {
            if !bytes.is_empty() {
                dropped = true;
            }
            break;
        };
        if rel > 0 {
            dropped = true;
            bytes = &bytes[rel..];
        }
        if bytes.len() < 12 {
            dropped = true;
            break;
        }
        let len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let stored_crc = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if len > MAX_PAYLOAD || bytes.len() < 12 + len {
            dropped = true;
            bytes = &bytes[1..];
            continue;
        }
        let payload = &bytes[12..12 + len];
        if crc32(payload) != stored_crc {
            dropped = true;
            bytes = &bytes[1..];
            continue;
        }
        match decode_payload(payload) {
            Some(m) => {
                entries.push(m);
                bytes = &bytes[12 + len..];
            }
            None => {
                dropped = true;
                bytes = &bytes[1..];
            }
        }
    }
    (entries, dropped)
}

fn find_magic(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == REC_MAGIC)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), JournalError> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp = tmp_path(path);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    atomic_rename(&tmp, path)?;
    if let Some(dir) = parent {
        sync_dir(dir)?;
    }
    let _ = fs::remove_file(&tmp);
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[cfg(unix)]
fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    let from_w = wide(from);
    let to_w = wide(to);
    // MOVEFILE_REPLACE_EXISTING makes the replace atomic on NTFS.
    let ok = unsafe {
        MoveFileExW(
            from_w.as_ptr(),
            to_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        Err(io::Error::from_raw_os_error(err as i32))
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    let file = File::open(dir)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;
    use crate::mutation::{Mutation, PriorityClass, Rect};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("nxjr-{}-{nanos}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            Self(scratch())
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn priority(pid: u32, previous: PriorityClass, new: PriorityClass) -> Mutation {
        Mutation::ProcessPriority { pid, previous, new }
    }

    #[test]
    fn record_then_reopen_preserves_order() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let first = priority(1, PriorityClass::Normal, PriorityClass::AboveNormal);
        let second = Mutation::TaskbarHide;
        {
            let mut j = Journal::open(&path).unwrap();
            j.record(&first).unwrap();
            j.record(&second).unwrap();
        }
        let j = Journal::open(&path).unwrap();
        assert_eq!(j.entries(), &[first, second]);
    }

    #[test]
    fn restore_all_is_reverse_and_clears_the_file() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let mut system = FakeSystem::clean();
        let original = system.clone();
        let m1 = priority(1, PriorityClass::Normal, PriorityClass::AboveNormal);
        let m2 = priority(1, PriorityClass::AboveNormal, PriorityClass::High);
        {
            let mut j = Journal::open(&path).unwrap();
            j.record(&m1).unwrap();
            system.apply(&m1).unwrap();
            j.record(&m2).unwrap();
            system.apply(&m2).unwrap();
            assert_eq!(system.priorities.get(&1), Some(&PriorityClass::High));
            let report = j.restore_all(&mut system).unwrap();
            assert_eq!(report.reverted, 2);
            assert!(report.failures.is_empty());
            assert!(j.is_empty());
        }
        assert_eq!(system, original);
        let j = Journal::open(&path).unwrap();
        assert!(j.is_empty());
    }

    #[test]
    fn crash_after_record_before_apply_still_restores() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let mut system = FakeSystem::clean();
        let original = system.clone();
        let applied = priority(2, PriorityClass::Normal, PriorityClass::AboveNormal);
        let not_applied = Mutation::WindowPosition {
            hwnd: 1,
            previous: Rect {
                x: 10,
                y: 10,
                width: 200,
                height: 100,
            },
            new: Rect {
                x: 30,
                y: 40,
                width: 400,
                height: 300,
            },
        };
        {
            let mut j = Journal::open(&path).unwrap();
            j.record(&applied).unwrap();
            system.apply(&applied).unwrap();
            j.record(&not_applied).unwrap();
            // Simulated crash: apply never runs, journal is dropped.
        }
        {
            let mut j = Journal::open(&path).unwrap();
            j.restore_all(&mut system).unwrap();
        }
        assert_eq!(system, original);
    }

    #[test]
    fn torn_tail_is_ignored() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let kept = Mutation::Suspend { pid: 4 };
        {
            let mut j = Journal::open(&path).unwrap();
            j.record(&kept).unwrap();
        }
        {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(b"NXR1\x40\x00").unwrap();
            file.sync_all().unwrap();
        }
        let j = Journal::open(&path).unwrap();
        assert_eq!(j.entries(), &[kept]);
    }

    #[test]
    fn flipped_checksum_drops_only_that_record() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let first = Mutation::TaskbarHide;
        let second = Mutation::Suspend { pid: 8 };
        {
            let mut j = Journal::open(&path).unwrap();
            j.record(&first).unwrap();
            j.record(&second).unwrap();
        }
        let mut bytes = fs::read(&path).unwrap();
        // Flip a payload byte near the end so the second record's CRC fails
        // and the first record, which sits in front of it, still parses.
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&path, &bytes).unwrap();
        let j = Journal::open(&path).unwrap();
        assert_eq!(j.entries(), &[first]);
    }

    #[test]
    fn unknown_version_is_refused_and_preserved() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        let mut bytes = encode_file(&[]);
        bytes[4] = 99;
        // Fixing the CRC is unnecessary: version is checked first.
        fs::write(&path, &bytes).unwrap();
        match Journal::open(&path) {
            Err(JournalError::UnsupportedVersion(99)) => {}
            Err(err) => panic!("unexpected journal error: {err}"),
            Ok(_) => panic!("unknown version was opened"),
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn garbage_file_opens_as_empty() {
        let dir = Scratch::new();
        let path = dir.path("state.nxjr");
        fs::write(&path, b"this is not a journal").unwrap();
        let j = Journal::open(&path).unwrap();
        assert!(j.is_empty());
    }
}
