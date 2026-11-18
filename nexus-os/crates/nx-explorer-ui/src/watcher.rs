//! Folder watcher: live filesystem updates with debounced, coalesced diffs (no full reloads).

use crossbeam_channel::{bounded, Receiver, Sender};
use nx_explorer::action::DirDiff;
use nx_fs::entry::{DirEntry, EntryKind};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Raw change event emitted by a watcher backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawWatcherEvent {
    Created(PathBuf),
    Removed(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

/// Internal coalesced state for an individual path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoalescedKind {
    Created,
    Removed,
    Modified,
    Renamed { from: PathBuf },
}

/// Debouncer that collects rapid filesystem bursts and coalesces them into minimal diffs.
#[derive(Debug, Default)]
pub struct WatcherDebouncer {
    pending: HashMap<PathBuf, CoalescedKind>,
    last_event: Option<Instant>,
    debounce_duration: Duration,
}

impl WatcherDebouncer {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            pending: HashMap::new(),
            last_event: None,
            debounce_duration: Duration::from_millis(debounce_ms),
        }
    }

    pub fn push(&mut self, event: RawWatcherEvent) {
        self.last_event = Some(Instant::now());
        match event {
            RawWatcherEvent::Created(path) => match self.pending.get(&path) {
                Some(CoalescedKind::Removed) => {
                    self.pending.insert(path, CoalescedKind::Modified);
                }
                Some(CoalescedKind::Created) => {}
                _ => {
                    self.pending.insert(path, CoalescedKind::Created);
                }
            },
            RawWatcherEvent::Removed(path) => match self.pending.get(&path) {
                Some(CoalescedKind::Created) => {
                    // Created and then immediately deleted: cancels out!
                    self.pending.remove(&path);
                }
                _ => {
                    self.pending.insert(path, CoalescedKind::Removed);
                }
            },
            RawWatcherEvent::Modified(path) => match self.pending.get(&path) {
                Some(CoalescedKind::Created) => {}
                Some(CoalescedKind::Renamed { .. }) => {}
                _ => {
                    self.pending.insert(path, CoalescedKind::Modified);
                }
            },
            RawWatcherEvent::Renamed { from, to } => {
                if let Some(CoalescedKind::Created) = self.pending.remove(&from) {
                    self.pending.insert(to, CoalescedKind::Created);
                } else {
                    self.pending.insert(to, CoalescedKind::Renamed { from });
                }
            }
        }
    }

    pub fn should_flush(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        if let Some(last) = self.last_event {
            last.elapsed() >= self.debounce_duration
        } else {
            false
        }
    }

    /// Flush coalesced events into path-level changes.
    pub fn flush_coalesced(&mut self) -> Vec<(PathBuf, CoalescedKind)> {
        self.last_event = None;
        self.pending.drain().collect()
    }

    /// Build `DirDiff`s by inspecting disk metadata for existing paths.
    pub fn flush_diffs(&mut self, _watched_dir: &Path) -> Vec<DirDiff> {
        let items = self.flush_coalesced();
        let mut diffs = Vec::with_capacity(items.len());

        for (path, kind) in items {
            let Some(name) = path.file_name() else {
                continue;
            };
            let name_os = name.to_os_string();

            match kind {
                CoalescedKind::Created => {
                    if let Ok(entry) = build_entry(&path) {
                        diffs.push(DirDiff::Created(entry));
                    }
                }
                CoalescedKind::Removed => {
                    diffs.push(DirDiff::Removed(name_os));
                }
                CoalescedKind::Modified => {
                    if let Ok(entry) = build_entry(&path) {
                        diffs.push(DirDiff::Updated(entry));
                    }
                }
                CoalescedKind::Renamed { from } => {
                    if let Some(from_name) = from.file_name() {
                        if let Ok(entry) = build_entry(&path) {
                            diffs.push(DirDiff::Renamed {
                                from: from_name.to_os_string(),
                                to: entry,
                            });
                        }
                    }
                }
            }
        }

        diffs
    }
}

fn build_entry(path: &Path) -> Result<DirEntry, std::io::Error> {
    let meta = std::fs::symlink_metadata(path)?;
    let name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(""));
    let kind = if meta.is_dir() {
        EntryKind::Dir
    } else if meta.file_type().is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    let size = if meta.is_dir() { 0 } else { meta.len() };
    let mut entry = DirEntry::new(name, kind, size);
    entry.modified = meta.modified().ok();
    Ok(entry)
}

/// Live directory watcher running on a background worker thread.
pub struct FolderWatcher {
    stop_flag: Arc<AtomicBool>,
    rx: Receiver<Vec<DirDiff>>,
    current_dir: PathBuf,
}

impl FolderWatcher {
    /// Watch a directory using `notify` on portable platforms or `ReadDirectoryChangesW` on Windows.
    pub fn watch(dir: PathBuf) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = bounded(64);

        let thread_dir = dir.clone();
        let thread_stop = Arc::clone(&stop_flag);

        std::thread::Builder::new()
            .name("nx-folder-watcher".into())
            .spawn(move || {
                run_watcher_loop(thread_dir, tx, thread_stop);
            })
            .ok();

        Self {
            stop_flag,
            rx,
            current_dir: dir,
        }
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// Try to receive pending debounced diffs non-blockingly.
    pub fn poll_diffs(&self) -> Option<Vec<DirDiff>> {
        self.rx.try_recv().ok()
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn run_watcher_loop(dir: PathBuf, tx: Sender<Vec<DirDiff>>, stop: Arc<AtomicBool>) {
    // Windows backend: ReadDirectoryChangesW with debounce
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let mut debouncer = WatcherDebouncer::new(50);
    let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();
    wide.push(0);

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            HANDLE::default(),
        )
    };

    let Ok(h) = handle else {
        return;
    };

    let mut buffer = [0u8; 4096];

    while !stop.load(Ordering::SeqCst) {
        let mut bytes_returned = 0u32;
        let success = unsafe {
            ReadDirectoryChangesW(
                h,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
                Some(&mut bytes_returned),
                None,
                None,
            )
        };

        if success.is_ok() && bytes_returned > 0 {
            // Parse notifications
            let mut offset = 0;
            loop {
                let info =
                    unsafe { &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };
                let name_len = (info.FileNameLength / 2) as usize;
                let name_slice =
                    unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
                let name = String::from_utf16_lossy(name_slice);
                let full = dir.join(name);

                // Action
                match info.Action.0 {
                    1 => debouncer.push(RawWatcherEvent::Created(full)),
                    2 => debouncer.push(RawWatcherEvent::Removed(full)),
                    3 => debouncer.push(RawWatcherEvent::Modified(full)),
                    4 | 5 => debouncer.push(RawWatcherEvent::Modified(full)),
                    _ => {}
                }

                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
                if offset >= buffer.len() {
                    break;
                }
            }
        }

        // Flush if debounced period elapsed
        if debouncer.should_flush() {
            let diffs = debouncer.flush_diffs(&dir);
            if !diffs.is_empty() {
                let _ = tx.send(diffs);
            }
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    unsafe {
        let _ = CloseHandle(h);
    }
}

#[cfg(not(windows))]
fn run_watcher_loop(dir: PathBuf, tx: Sender<Vec<DirDiff>>, stop: Arc<AtomicBool>) {
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    let mut debouncer = WatcherDebouncer::new(50);
    let (event_tx, event_rx) = bounded(256);

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(_) => return,
    };

    if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
        return;
    }

    while !stop.load(Ordering::SeqCst) {
        while let Ok(event) = event_rx.try_recv() {
            for path in event.paths {
                match event.kind {
                    EventKind::Create(_) => debouncer.push(RawWatcherEvent::Created(path)),
                    EventKind::Remove(_) => debouncer.push(RawWatcherEvent::Removed(path)),
                    EventKind::Modify(_) => debouncer.push(RawWatcherEvent::Modified(path)),
                    _ => {}
                }
            }
        }

        if debouncer.should_flush() {
            let diffs = debouncer.flush_diffs(&dir);
            if !diffs.is_empty() {
                let _ = tx.send(diffs);
            }
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debouncer_coalesces_rapid_modifications() {
        let mut debouncer = WatcherDebouncer::new(0);
        let path = PathBuf::from("/test/file.txt");

        debouncer.push(RawWatcherEvent::Created(path.clone()));
        debouncer.push(RawWatcherEvent::Modified(path.clone()));
        debouncer.push(RawWatcherEvent::Modified(path.clone()));

        let flushed = debouncer.flush_coalesced();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0], (path, CoalescedKind::Created));
    }

    #[test]
    fn debouncer_cancels_create_then_delete() {
        let mut debouncer = WatcherDebouncer::new(0);
        let path = PathBuf::from("/test/temp.tmp");

        debouncer.push(RawWatcherEvent::Created(path.clone()));
        debouncer.push(RawWatcherEvent::Removed(path.clone()));

        let flushed = debouncer.flush_coalesced();
        assert!(flushed.is_empty(), "create then remove must cancel out");
    }

    #[test]
    fn debouncer_handles_renames() {
        let mut debouncer = WatcherDebouncer::new(0);
        let from = PathBuf::from("/test/old.txt");
        let to = PathBuf::from("/test/new.txt");

        debouncer.push(RawWatcherEvent::Renamed {
            from: from.clone(),
            to: to.clone(),
        });
        debouncer.push(RawWatcherEvent::Modified(to.clone()));

        let flushed = debouncer.flush_coalesced();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0], (to, CoalescedKind::Renamed { from }));
    }
}
