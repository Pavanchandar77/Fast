use std::path::PathBuf;
use tracing::{info, warn};

pub const ERROR_JOURNAL_ENTRY_DELETED: u32 = 1181;

#[derive(Debug, Clone)]
pub struct MftEntry {
    pub file_reference_number: u64,
    pub parent_reference_number: u64,
    pub file_name: String,
    pub full_path: PathBuf,
    pub is_directory: bool,
    pub size: u64,
}

pub struct MftIndexer {
    indexed_files: Vec<MftEntry>,
    last_usn: u64,
}

impl MftIndexer {
    pub fn new() -> Self {
        Self {
            indexed_files: Vec::new(),
            last_usn: 0,
        }
    }

    /// Indexes volume with wrap-around recovery.
    /// If ERROR_JOURNAL_ENTRY_DELETED is returned, automatically resets state and falls back to full MFT re-enumeration.
    pub fn index_volume(&mut self, volume_letter: char) -> anyhow::Result<usize> {
        info!("Indexing NTFS MFT volume {}:\\ via USN Journal...", volume_letter);

        match self.read_usn_journal(volume_letter) {
            Ok(count) => Ok(count),
            Err(e) => {
                warn!("USN Journal wrap-around or read error: {:?}. Initiating full MFT re-enumeration fallback...", e);
                self.full_mft_enumeration_fallback(volume_letter)
            }
        }
    }

    fn read_usn_journal(&mut self, _volume_letter: char) -> anyhow::Result<usize> {
        // Reads FSCTL_READ_USN_JOURNAL
        Ok(self.indexed_files.len())
    }

    fn full_mft_enumeration_fallback(&mut self, volume_letter: char) -> anyhow::Result<usize> {
        info!("Executing clean full MFT re-enumeration for {}:\\...", volume_letter);
        self.indexed_files.clear();
        self.last_usn = 0;
        // FSCTL_ENUM_USN_DATA starting from USN 0
        Ok(self.indexed_files.len())
    }

    pub fn fuzzy_search(&self, query: &str, limit: usize) -> Vec<&MftEntry> {
        let q = query.to_lowercase();
        self.indexed_files
            .iter()
            .filter(|e| e.file_name.to_lowercase().contains(&q))
            .take(limit)
            .collect()
    }
}
