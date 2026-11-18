//! Platform-free parser for NTFS `USN_RECORD_V2` / `USN_RECORD_V3` buffers
//! as returned by `FSCTL_ENUM_USN_DATA` and `FSCTL_READ_USN_JOURNAL`.
//!
//! Everything is bounds-checked: a truncated or hostile buffer yields a
//! typed error, never a panic or an out-of-bounds read.

use crate::error::{FsError, FsResult};

/// USN reason flags we act on.
pub mod reason {
    pub const DATA_OVERWRITE: u32 = 0x0000_0001;
    pub const FILE_CREATE: u32 = 0x0000_0100;
    pub const FILE_DELETE: u32 = 0x0000_0200;
    pub const RENAME_OLD_NAME: u32 = 0x0000_1000;
    pub const RENAME_NEW_NAME: u32 = 0x0000_2000;
    pub const BASIC_INFO_CHANGE: u32 = 0x0000_8000;
    pub const CLOSE: u32 = 0x8000_0000;
}

/// `FILE_ATTRIBUTE_DIRECTORY`.
pub const ATTR_DIRECTORY: u32 = 0x10;
/// `FILE_ATTRIBUTE_HIDDEN`.
pub const ATTR_HIDDEN: u32 = 0x2;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub const ATTR_REPARSE: u32 = 0x400;

/// A file reference (64-bit on NTFS V2 records, 128-bit on V3/ReFS).
pub type FileRef = u128;

/// A decoded USN record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsnRecord {
    pub file_ref: FileRef,
    pub parent_ref: FileRef,
    pub usn: i64,
    pub reason: u32,
    pub attributes: u32,
    pub name: String,
}

impl UsnRecord {
    pub fn is_dir(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
}

/// Decode little-endian UTF-16 code units (a trailing odd byte is ignored).
pub(crate) fn le_u16s(b: &[u8]) -> Vec<u16> {
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect()
}

/// Decode big-endian UTF-16 code units.
pub(crate) fn be_u16s(b: &[u8]) -> Vec<u16> {
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes(*c))
        .collect()
}

fn rd_u16(b: &[u8], at: usize) -> FsResult<u16> {
    b.get(at..at + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| FsError::Malformed(format!("u16 at {at} out of bounds")))
}

fn rd_u32(b: &[u8], at: usize) -> FsResult<u32> {
    b.get(at..at + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| FsError::Malformed(format!("u32 at {at} out of bounds")))
}

fn rd_u64(b: &[u8], at: usize) -> FsResult<u64> {
    b.get(at..at + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| FsError::Malformed(format!("u64 at {at} out of bounds")))
}

fn rd_u128(b: &[u8], at: usize) -> FsResult<u128> {
    b.get(at..at + 16)
        .and_then(|s| s.try_into().ok())
        .map(u128::from_le_bytes)
        .ok_or_else(|| FsError::Malformed(format!("u128 at {at} out of bounds")))
}

/// Parse one record that starts at the beginning of `rec` (which must be
/// exactly `RecordLength` bytes).
pub fn parse_record(rec: &[u8]) -> FsResult<UsnRecord> {
    let major = rd_u16(rec, 4)?;
    let (file_ref, parent_ref, usn, reason, attributes, name_len, name_off) = match major {
        2 => (
            u128::from(rd_u64(rec, 8)?),
            u128::from(rd_u64(rec, 16)?),
            rd_u64(rec, 24)? as i64,
            rd_u32(rec, 40)?,
            rd_u32(rec, 52)?,
            rd_u16(rec, 56)?,
            rd_u16(rec, 58)?,
        ),
        3 => (
            rd_u128(rec, 8)?,
            rd_u128(rec, 24)?,
            rd_u64(rec, 40)? as i64,
            rd_u32(rec, 56)?,
            rd_u32(rec, 68)?,
            rd_u16(rec, 72)?,
            rd_u16(rec, 74)?,
        ),
        v => {
            return Err(FsError::Malformed(format!(
                "unsupported USN record version {v}"
            )))
        }
    };
    let (off, len) = (name_off as usize, name_len as usize);
    if len % 2 != 0 {
        return Err(FsError::Malformed("odd file name length".into()));
    }
    let raw = rec
        .get(off..off + len)
        .ok_or_else(|| FsError::Malformed("file name out of bounds".into()))?;
    let units = le_u16s(raw);
    let name = String::from_utf16_lossy(&units);
    Ok(UsnRecord {
        file_ref,
        parent_ref,
        usn,
        reason,
        attributes,
        name,
    })
}

/// Parse a buffer of back-to-back records (after the leading 8-byte
/// "next USN / next file reference" header has been stripped).
pub fn parse_records(buf: &[u8]) -> FsResult<Vec<UsnRecord>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= buf.len() {
        let len = rd_u32(buf, at)? as usize;
        if len == 0 {
            break;
        }
        if len < 60 || at + len > buf.len() {
            return Err(FsError::Malformed(format!(
                "record length {len} at {at} is invalid"
            )));
        }
        out.push(parse_record(&buf[at..at + len])?);
        // Records are 8-byte aligned.
        at += (len + 7) & !7;
    }
    Ok(out)
}

/// Split the 8-byte header off an `FSCTL_*` output buffer.
pub fn split_header(buf: &[u8]) -> FsResult<(u64, &[u8])> {
    let next = rd_u64(buf, 0)?;
    Ok((next, &buf[8..]))
}

/// Serialise a V2 record (used by tests and fixtures; mirrors the on-disk layout).
pub fn encode_v2(r: &UsnRecord) -> Vec<u8> {
    let name: Vec<u16> = r.name.encode_utf16().collect();
    let name_bytes = name.len() * 2;
    let len = 60 + name_bytes;
    let padded = (len + 7) & !7;
    let mut b = vec![0u8; padded];
    b[0..4].copy_from_slice(&(len as u32).to_le_bytes());
    b[4..6].copy_from_slice(&2u16.to_le_bytes());
    b[8..16].copy_from_slice(&(r.file_ref as u64).to_le_bytes());
    b[16..24].copy_from_slice(&(r.parent_ref as u64).to_le_bytes());
    b[24..32].copy_from_slice(&r.usn.to_le_bytes());
    b[40..44].copy_from_slice(&r.reason.to_le_bytes());
    b[52..56].copy_from_slice(&r.attributes.to_le_bytes());
    b[56..58].copy_from_slice(&(name_bytes as u16).to_le_bytes());
    b[58..60].copy_from_slice(&60u16.to_le_bytes());
    for (i, u) in name.iter().enumerate() {
        b[60 + i * 2..62 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A V2 record captured from `fsutil usn readjournal` layout:
    /// file 0x0001_0000_0000_1234 in root (0x0005_0000_0000_0005),
    /// name "a.txt", reason FILE_CREATE|CLOSE.
    const FIXTURE_V2: &[u8] = &[
        0x48, 0, 0, 0, 2, 0, 0, 0, // len=72, v2.0
        0x34, 0x12, 0, 0, 0, 0, 1, 0, // FRN
        5, 0, 0, 0, 0, 0, 5, 0, // parent FRN
        0x00, 0x10, 0, 0, 0, 0, 0, 0, // usn 0x1000
        0, 0, 0, 0, 0, 0, 0, 0, // timestamp
        0, 1, 0, 0x80, // reason
        0, 0, 0, 0, // source info
        0, 0, 0, 0, // security id
        0x20, 0, 0, 0, // attributes ARCHIVE
        10, 0, 60, 0, // name len 10, off 60
        b'a', 0, b'.', 0, b't', 0, b'x', 0, b't', 0, // "a.txt"
        0, 0, // padding to 72
    ];

    #[test]
    fn parses_fixture_v2() {
        let r = parse_record(FIXTURE_V2).expect("parse");
        assert_eq!(r.file_ref, 0x0001_0000_0000_1234);
        assert_eq!(r.parent_ref, 0x0005_0000_0000_0005);
        assert_eq!(r.usn, 0x1000);
        assert_eq!(r.reason, reason::FILE_CREATE | reason::CLOSE);
        assert_eq!(r.name, "a.txt");
        assert!(!r.is_dir());
        let mut buf = 0x2000u64.to_le_bytes().to_vec();
        buf.extend_from_slice(FIXTURE_V2);
        buf.extend_from_slice(FIXTURE_V2);
        let (next, body) = split_header(&buf).expect("header");
        assert_eq!(next, 0x2000);
        assert_eq!(parse_records(body).expect("records").len(), 2);
    }

    #[test]
    fn parses_v3() {
        let mut b = vec![0u8; 88];
        b[0..4].copy_from_slice(&88u32.to_le_bytes());
        b[4..6].copy_from_slice(&3u16.to_le_bytes());
        b[8..24].copy_from_slice(&(7u128 << 70 | 9).to_le_bytes());
        b[24..40].copy_from_slice(&5u128.to_le_bytes());
        b[56..60].copy_from_slice(&reason::FILE_DELETE.to_le_bytes());
        b[68..72].copy_from_slice(&ATTR_DIRECTORY.to_le_bytes());
        b[72..74].copy_from_slice(&4u16.to_le_bytes());
        b[74..76].copy_from_slice(&76u16.to_le_bytes());
        b[76..80].copy_from_slice(&[b'd', 0, b'r', 0]);
        let r = parse_record(&b).expect("v3");
        assert_eq!(r.file_ref, 7u128 << 70 | 9);
        assert!(r.is_dir());
        assert_eq!(r.name, "dr");
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_record(&FIXTURE_V2[..40]).is_err());
        let mut bad = FIXTURE_V2.to_vec();
        bad[58] = 250; // name offset beyond record
        assert!(parse_record(&bad).is_err());
        let mut v9 = FIXTURE_V2.to_vec();
        v9[4] = 9;
        assert!(parse_record(&v9).is_err());
        let mut long = FIXTURE_V2.to_vec();
        long[0] = 0xff;
        assert!(parse_records(&long).is_err());
        assert!(split_header(&[1, 2]).is_err());
    }

    proptest! {
        #[test]
        fn encode_parse_roundtrip(fr in any::<u64>(), pr in any::<u64>(), usn in any::<i64>(), rs in any::<u32>(), attrs in any::<u32>(), name in "\\PC{0,40}") {
            let r = UsnRecord { file_ref: fr.into(), parent_ref: pr.into(), usn, reason: rs, attributes: attrs, name };
            let enc = encode_v2(&r);
            let back = parse_records(&enc).expect("parse");
            prop_assert_eq!(back, vec![r]);
        }

        #[test]
        fn never_panics_on_garbage(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            let _ = parse_records(&bytes);
            let _ = parse_record(&bytes);
        }
    }
}
