//! Volumes and the NTFS MFT + USN journal indexer.

use super::{wide_str, win_err};
use crate::error::{FsError, FsResult};
use crate::index::mft::{MftRefs, MftTree};
use crate::index::usn::{parse_records, split_header};
use crate::index::{PathIndex, VolumeIndexer};
use crate::pool::CancelToken;
use crate::volumes::{Volume, VolumeKind};
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_HANDLE_EOF, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0,
    READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0,
};
use windows::Win32::System::IO::DeviceIoControl;

/// Enumerate drive letters with type, label and file system.
pub(crate) fn list_volumes() -> Vec<Volume> {
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = char::from(b'A' + i as u8);
        let root = format!("{letter}:\\");
        let w = wide_str(&root);
        // SAFETY: NUL-terminated root path.
        let kind = match unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) } {
            2 => VolumeKind::Removable,
            3 => VolumeKind::Fixed,
            4 => VolumeKind::Network,
            5 => VolumeKind::Optical,
            6 => VolumeKind::RamDisk,
            _ => VolumeKind::Unknown,
        };
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        // SAFETY: buffers are valid for their lengths. Empty optical drives fail; we keep them.
        let ok = unsafe {
            GetVolumeInformationW(
                PCWSTR(w.as_ptr()),
                Some(&mut label),
                None,
                None,
                None,
                Some(&mut fs),
            )
        }
        .is_ok();
        let s = |b: &[u16]| {
            String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
        };
        let label = if ok { s(&label) } else { String::new() };
        out.push(Volume {
            root: root.clone(),
            label: if label.is_empty() {
                format!("Local Disk ({letter}:)")
            } else {
                label
            },
            kind,
            fs: if ok { s(&fs) } else { String::new() },
        });
    }
    out
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Everything-style indexer for one NTFS volume. Needs administrator rights
/// (volume handle); callers fall back to the walker when `build` fails with
/// `PermissionDenied`.
pub struct NtfsIndexer {
    letter: char,
    journal_id: u64,
    next_usn: i64,
    refs: Option<MftRefs>,
    buf: Vec<u8>,
}

impl NtfsIndexer {
    pub fn new(letter: char) -> Self {
        NtfsIndexer {
            letter: letter.to_ascii_uppercase(),
            journal_id: 0,
            next_usn: 0,
            refs: None,
            buf: vec![0u8; 1 << 20],
        }
    }

    fn device(&self) -> PathBuf {
        PathBuf::from(format!(r"\\.\{}:", self.letter))
    }

    fn open(&self) -> FsResult<Handle> {
        let dev = self.device();
        let w = wide_str(&dev.to_string_lossy());
        // SAFETY: NUL-terminated device path; no template handle.
        let h = unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                HANDLE::default(),
            )
        }
        .map_err(|e| win_err(&dev, &e))?;
        Ok(Handle(h))
    }

    fn query(&self, h: &Handle) -> FsResult<USN_JOURNAL_DATA_V0> {
        let mut jd = USN_JOURNAL_DATA_V0::default();
        let mut got = 0u32;
        // SAFETY: output buffer is a valid USN_JOURNAL_DATA_V0.
        unsafe {
            DeviceIoControl(
                h.0,
                FSCTL_QUERY_USN_JOURNAL,
                None,
                0,
                Some((&mut jd as *mut USN_JOURNAL_DATA_V0).cast()),
                std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
                Some(&mut got),
                None,
            )
        }
        .map_err(|e| win_err(&self.device(), &e))?;
        Ok(jd)
    }
}

impl VolumeIndexer for NtfsIndexer {
    fn name(&self) -> &'static str {
        "ntfs-mft"
    }

    fn build(&mut self, cancel: &CancelToken) -> FsResult<PathIndex> {
        let h = self.open()?;
        let jd = self.query(&h)?;
        let mut med = MFT_ENUM_DATA_V0 {
            StartFileReferenceNumber: 0,
            LowUsn: 0,
            HighUsn: jd.NextUsn,
        };
        let mut records = Vec::new();
        loop {
            if cancel.is_cancelled() {
                return Err(FsError::Cancelled);
            }
            let mut got = 0u32;
            // SAFETY: input/output buffers are valid for the given sizes.
            let r = unsafe {
                DeviceIoControl(
                    h.0,
                    FSCTL_ENUM_USN_DATA,
                    Some((&med as *const MFT_ENUM_DATA_V0).cast()),
                    std::mem::size_of::<MFT_ENUM_DATA_V0>() as u32,
                    Some(self.buf.as_mut_ptr().cast()),
                    self.buf.len() as u32,
                    Some(&mut got),
                    None,
                )
            };
            if let Err(e) = r {
                if e.code() == ERROR_HANDLE_EOF.to_hresult() {
                    break;
                }
                return Err(win_err(&self.device(), &e));
            }
            let data = self
                .buf
                .get(..got as usize)
                .ok_or_else(|| FsError::Malformed("short USN buffer".into()))?;
            let (next, body) = split_header(data)?;
            records.extend(parse_records(body)?);
            if next == med.StartFileReferenceNumber || body.is_empty() {
                break;
            }
            med.StartFileReferenceNumber = next;
        }
        let tree = MftTree::build(&format!("{}:", self.letter), records)?;
        let (index, refs) = tree.into_parts();
        self.journal_id = jd.UsnJournalID;
        self.next_usn = jd.NextUsn;
        self.refs = Some(refs);
        Ok(index)
    }

    fn sync(&mut self, index: &mut PathIndex) -> FsResult<usize> {
        let Some(mut refs) = self.refs.take() else {
            return Err(FsError::Unsupported("sync before build"));
        };
        let result = (|| {
            let h = self.open()?;
            let mut applied = 0;
            loop {
                let rd = READ_USN_JOURNAL_DATA_V0 {
                    StartUsn: self.next_usn,
                    ReasonMask: 0xffff_ffff,
                    ReturnOnlyOnClose: 0,
                    Timeout: 0,
                    BytesToWaitFor: 0,
                    UsnJournalID: self.journal_id,
                };
                let mut got = 0u32;
                // SAFETY: input/output buffers are valid for the given sizes.
                unsafe {
                    DeviceIoControl(
                        h.0,
                        FSCTL_READ_USN_JOURNAL,
                        Some((&rd as *const READ_USN_JOURNAL_DATA_V0).cast()),
                        std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                        Some(self.buf.as_mut_ptr().cast()),
                        self.buf.len() as u32,
                        Some(&mut got),
                        None,
                    )
                }
                // ERROR_JOURNAL_ENTRY_DELETED etc.: the caller must rebuild.
                .map_err(|e| win_err(&self.device(), &e))?;
                let data = self
                    .buf
                    .get(..got as usize)
                    .ok_or_else(|| FsError::Malformed("short USN buffer".into()))?;
                let (next, body) = split_header(data)?;
                let recs = parse_records(body)?;
                applied += refs.apply_all(index, &recs)?;
                let next = next as i64;
                if recs.is_empty() || next == self.next_usn {
                    self.next_usn = next.max(self.next_usn);
                    break;
                }
                self.next_usn = next;
            }
            Ok(applied)
        })();
        self.refs = Some(refs);
        result
    }
}

/// Root path of the volume containing `p` (`C:\`), if it has a drive letter.
pub fn drive_letter(p: &Path) -> Option<char> {
    let s = crate::path::strip_extended_windows(&p.to_string_lossy());
    let b = s.as_bytes();
    (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
        .then(|| char::from(b[0]).to_ascii_uppercase())
}
