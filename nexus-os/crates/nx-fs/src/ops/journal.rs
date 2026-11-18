//! Crash-safe op journal.
//!
//! Every file operation appends intent records *before* touching the disk
//! and `fsync`s them, so after a crash or power loss [`Journal::recover`]
//! can remove half-written temp files, put back files that were moved
//! aside for a replace, and restore the last completed op's undo record.
//!
//! Format: one record per line, tab-separated, fields escaped.
//! ```text
//! B  <job> <kind>                 job began
//! T  <job> <temp> <final>         temp file about to be written
//! R  <job> <target> <backup>      target about to be moved aside
//! S  <job> <undo step...>         a step completed (for undo)
//! E  <job> ok|cancelled|failed    job ended
//! ```

use super::undo::UndoStep;
use crate::error::{FsError, FsResult, IoContext};
use crate::path::to_native;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A parsed journal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Begin {
        job: u64,
        kind: String,
    },
    Temp {
        job: u64,
        temp: PathBuf,
        dest: PathBuf,
    },
    Backup {
        job: u64,
        target: PathBuf,
        backup: PathBuf,
    },
    Step {
        job: u64,
        step: UndoStep,
    },
    End {
        job: u64,
        outcome: String,
    },
}

impl Record {
    pub fn job(&self) -> u64 {
        match self {
            Record::Begin { job, .. }
            | Record::Temp { job, .. }
            | Record::Backup { job, .. }
            | Record::Step { job, .. }
            | Record::End { job, .. } => *job,
        }
    }
}

/// Escape a field (tab, newline, backslash; non-UTF-8 bytes as `\xNN`).
pub fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(s) => {
                push_escaped(&mut out, s);
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                if let Ok(s) = std::str::from_utf8(&bytes[i..i + good]) {
                    push_escaped(&mut out, s);
                }
                let bad = e.error_len().unwrap_or(bytes.len() - i - good);
                for b in &bytes[i + good..i + good + bad] {
                    out.push_str(&format!("\\x{b:02x}"));
                }
                i += good + bad;
            }
        }
    }
    out
}

fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
}

/// Inverse of [`escape`].
pub fn unescape(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        match b.get(i + 1)? {
            b'\\' => out.push(b'\\'),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b'x' => {
                let hex = s.get(i + 2..i + 4)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            _ => return None,
        }
        i += 2;
    }
    Some(out)
}

/// Path -> journal field.
pub fn path_field(p: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        escape(p.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        escape(p.to_string_lossy().as_bytes())
    }
}

/// Journal field -> path.
pub fn field_path(s: &str) -> Option<PathBuf> {
    let bytes = unescape(s)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes).ok().map(PathBuf::from)
    }
}

fn encode(r: &Record) -> String {
    match r {
        Record::Begin { job, kind } => format!("B\t{job}\t{}", escape(kind.as_bytes())),
        Record::Temp { job, temp, dest } => {
            format!("T\t{job}\t{}\t{}", path_field(temp), path_field(dest))
        }
        Record::Backup {
            job,
            target,
            backup,
        } => format!("R\t{job}\t{}\t{}", path_field(target), path_field(backup)),
        Record::Step { job, step } => format!("S\t{job}\t{}", step.encode()),
        Record::End { job, outcome } => format!("E\t{job}\t{}", escape(outcome.as_bytes())),
    }
}

fn decode(line: &str) -> Option<Record> {
    let mut f = line.split('\t');
    let tag = f.next()?;
    let job: u64 = f.next()?.parse().ok()?;
    let rec = match tag {
        "B" => Record::Begin {
            job,
            kind: String::from_utf8(unescape(f.next()?)?).ok()?,
        },
        "T" => Record::Temp {
            job,
            temp: field_path(f.next()?)?,
            dest: field_path(f.next()?)?,
        },
        "R" => Record::Backup {
            job,
            target: field_path(f.next()?)?,
            backup: field_path(f.next()?)?,
        },
        "S" => {
            let rest: Vec<&str> = f.collect();
            Record::Step {
                job,
                step: UndoStep::decode(&rest)?,
            }
        }
        "E" => Record::End {
            job,
            outcome: String::from_utf8(unescape(f.next()?)?).ok()?,
        },
        _ => return None,
    };
    Some(rec)
}

/// Parse a journal. A torn (partially written) *last* line is ignored;
/// garbage anywhere else is reported as corruption.
pub fn parse(text: &str, origin: &Path) -> FsResult<Vec<Record>> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match decode(line) {
            Some(r) => out.push(r),
            None if i + 1 == lines.len() => break, // torn tail: no trailing newline
            None => {
                return Err(FsError::JournalCorrupt {
                    path: origin.to_path_buf(),
                    line: i + 1,
                    reason: "unparseable record".into(),
                })
            }
        }
    }
    Ok(out)
}

/// What recovery did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Jobs that never wrote an `E` record.
    pub interrupted_jobs: Vec<u64>,
    pub temps_removed: Vec<PathBuf>,
    pub backups_restored: Vec<PathBuf>,
    pub backups_discarded: Vec<PathBuf>,
    /// Undo steps of the last finished job (cleared by a later undo job).
    pub last_undo: Option<(u64, Vec<UndoStep>)>,
    pub max_job: u64,
}

/// The journal file.
pub struct Journal {
    path: PathBuf,
    file: File,
}

impl Journal {
    /// Open (creating) the journal, recover from any previous crash, and
    /// compact it down to the last completed job.
    pub fn open(path: &Path) -> FsResult<(Journal, RecoveryReport)> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(to_native(dir)).at(dir)?;
        }
        let text = match std::fs::read(to_native(path)) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(FsError::from_io(path, e)),
        };
        let records = parse(&text, path)?;
        let report = recover(&records);
        // Compact: keep only the last completed job (its undo record).
        let mut keep = String::from("# nx-fs op journal v1\n");
        if let Some((job, steps)) = &report.last_undo {
            keep.push_str(&encode(&Record::Begin {
                job: *job,
                kind: "restored".into(),
            }));
            keep.push('\n');
            for s in steps {
                keep.push_str(&encode(&Record::Step {
                    job: *job,
                    step: s.clone(),
                }));
                keep.push('\n');
            }
            keep.push_str(&encode(&Record::End {
                job: *job,
                outcome: "ok".into(),
            }));
            keep.push('\n');
        }
        crate::index::frecency::write_atomic(path, keep.as_bytes())?;
        let file = OpenOptions::new()
            .append(true)
            .open(to_native(path))
            .at(path)?;
        Ok((
            Journal {
                path: path.to_path_buf(),
                file,
            },
            report,
        ))
    }

    /// Append and fsync one record.
    pub fn append(&mut self, r: &Record) -> FsResult<()> {
        let mut line = encode(r);
        line.push('\n');
        self.file.write_all(line.as_bytes()).at(&self.path)?;
        self.file.sync_data().at(&self.path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Apply recovery actions for interrupted jobs. Pure in the sense that it
/// only touches paths named in the records.
pub fn recover(records: &[Record]) -> RecoveryReport {
    let mut report = RecoveryReport::default();
    let mut jobs: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
    for r in records {
        jobs.entry(r.job()).or_default().push(r);
        report.max_job = report.max_job.max(r.job());
    }
    for (job, recs) in &jobs {
        let ended = recs.iter().find_map(|r| match r {
            Record::End { outcome, .. } => Some(outcome.as_str()),
            _ => None,
        });
        // Leftover temps are always garbage (completed temps were renamed).
        for r in recs {
            if let Record::Temp { temp, .. } = r {
                if std::fs::symlink_metadata(to_native(temp)).is_ok() && remove_any(temp).is_ok() {
                    report.temps_removed.push(temp.clone());
                }
            }
        }
        if ended.is_none() {
            report.interrupted_jobs.push(*job);
            for r in recs {
                if let Record::Backup { target, backup, .. } = r {
                    if std::fs::symlink_metadata(to_native(backup)).is_err() {
                        continue;
                    }
                    if std::fs::symlink_metadata(to_native(target)).is_err() {
                        // Crash between "move aside" and "put new in place": undo the move.
                        if std::fs::rename(to_native(backup), to_native(target)).is_ok() {
                            report.backups_restored.push(target.clone());
                        }
                    } else if remove_any(backup).is_ok() {
                        // The replacement landed (atomic rename); the old copy is obsolete.
                        report.backups_discarded.push(backup.clone());
                    }
                }
            }
        } else {
            let is_undo = recs
                .iter()
                .any(|r| matches!(r, Record::Begin { kind, .. } if kind == "undo"));
            if is_undo {
                // The previous record was consumed by this undo.
                report.last_undo = None;
                continue;
            }
            let steps: Vec<UndoStep> = recs
                .iter()
                .filter_map(|r| match r {
                    Record::Step { step, .. } => Some(step.clone()),
                    _ => None,
                })
                .collect();
            if !steps.is_empty() {
                report.last_undo = Some((*job, steps));
            }
        }
    }
    report
}

pub(crate) fn remove_any(p: &Path) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(to_native(p))?;
    if md.is_dir() {
        std::fs::remove_dir_all(to_native(p))
    } else {
        std::fs::remove_file(to_native(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn roundtrip_records() {
        let recs = vec![
            Record::Begin {
                job: 1,
                kind: "copy".into(),
            },
            Record::Temp {
                job: 1,
                temp: "/a/.x.nxpart-1".into(),
                dest: "/a/x\twith tab".into(),
            },
            Record::Backup {
                job: 1,
                target: "/a/y".into(),
                backup: "/a/.y.nxold-1".into(),
            },
            Record::Step {
                job: 1,
                step: UndoStep::Created("/a/x".into()),
            },
            Record::Step {
                job: 1,
                step: UndoStep::Moved {
                    from: "/b".into(),
                    to: "/c\\d".into(),
                },
            },
            Record::End {
                job: 1,
                outcome: "ok".into(),
            },
        ];
        let text: String = recs.iter().map(|r| encode(r) + "\n").collect();
        assert_eq!(parse(&text, Path::new("j")).expect("parse"), recs);
    }

    #[test]
    fn torn_tail_is_tolerated_but_middle_garbage_is_not() {
        let good = encode(&Record::Begin {
            job: 3,
            kind: "move".into(),
        });
        let torn = format!("{good}\nS\t3\tMov");
        assert_eq!(parse(&torn, Path::new("j")).expect("torn ok").len(), 1);
        let bad = format!("{good}\n???\n{good}\n");
        assert!(matches!(
            parse(&bad, Path::new("j")),
            Err(FsError::JournalCorrupt { line: 2, .. })
        ));
    }

    #[test]
    fn recovery_cleans_temps_and_restores_backups() {
        let t = tempfile::tempdir().expect("tempdir");
        let temp = t.path().join(".a.nxpart-9");
        std::fs::write(&temp, b"half").expect("write");
        let target = t.path().join("b.txt");
        let backup = t.path().join(".b.txt.nxold-9");
        std::fs::write(&backup, b"original").expect("write");
        let target2 = t.path().join("c.txt");
        let backup2 = t.path().join(".c.txt.nxold-9");
        std::fs::write(&target2, b"new").expect("write");
        std::fs::write(&backup2, b"old").expect("write");
        let recs = vec![
            Record::Begin {
                job: 8,
                kind: "copy".into(),
            },
            Record::Step {
                job: 8,
                step: UndoStep::Created(t.path().join("done")),
            },
            Record::End {
                job: 8,
                outcome: "ok".into(),
            },
            Record::Begin {
                job: 9,
                kind: "copy".into(),
            },
            Record::Temp {
                job: 9,
                temp: temp.clone(),
                dest: t.path().join("a"),
            },
            Record::Backup {
                job: 9,
                target: target.clone(),
                backup: backup.clone(),
            },
            Record::Backup {
                job: 9,
                target: target2.clone(),
                backup: backup2.clone(),
            },
        ];
        let rep = recover(&recs);
        assert_eq!(rep.interrupted_jobs, vec![9]);
        assert_eq!(rep.temps_removed, vec![temp.clone()]);
        assert!(!temp.exists());
        assert_eq!(std::fs::read(&target).expect("restored"), b"original");
        assert_eq!(rep.backups_discarded, vec![backup2.clone()]);
        assert!(!backup2.exists());
        assert_eq!(
            rep.last_undo.as_ref().map(|(j, s)| (*j, s.len())),
            Some((8, 1))
        );
        assert_eq!(rep.max_job, 9);
    }

    #[test]
    fn open_compacts_and_appends() {
        let t = tempfile::tempdir().expect("tempdir");
        let p = t.path().join("j/ops.journal");
        {
            let (mut j, rep) = Journal::open(&p).expect("open");
            assert!(rep.last_undo.is_none());
            j.append(&Record::Begin {
                job: 1,
                kind: "copy".into(),
            })
            .expect("append");
            j.append(&Record::Step {
                job: 1,
                step: UndoStep::Created("/x".into()),
            })
            .expect("append");
            j.append(&Record::End {
                job: 1,
                outcome: "ok".into(),
            })
            .expect("append");
            j.append(&Record::Begin {
                job: 2,
                kind: "delete".into(),
            })
            .expect("append");
        }
        let (_j, rep) = Journal::open(&p).expect("reopen");
        assert_eq!(rep.interrupted_jobs, vec![2]);
        assert_eq!(
            rep.last_undo,
            Some((1, vec![UndoStep::Created("/x".into())]))
        );
        let text = std::fs::read_to_string(&p).expect("read");
        assert!(!text.contains("delete"), "compacted");
    }

    proptest! {
        #[test]
        fn escape_roundtrips(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let e = escape(&bytes);
            prop_assert!(!e.contains('\t') && !e.contains('\n'));
            prop_assert_eq!(unescape(&e), Some(bytes));
        }
    }
}
