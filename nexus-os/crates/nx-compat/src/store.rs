//! Build history persistence in state directory / journal dir.
//! Tracks last-seen Windows build and detects build/UBR changes between startups.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::build::WindowsBuildInfo;
use crate::error::{CompatError, CompatResult};

/// Status of the Windows build compared to the recorded last-seen build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStatus {
    /// First time Nexus-OS has run on this system or state dir.
    FreshInstall(WindowsBuildInfo),
    /// Windows build and UBR are identical to last-seen.
    Unchanged(WindowsBuildInfo),
    /// Windows build or UBR has changed (Windows Update applied).
    BuildChanged {
        previous: WindowsBuildInfo,
        current: WindowsBuildInfo,
    },
}

impl BuildStatus {
    pub fn is_changed(&self) -> bool {
        matches!(self, Self::BuildChanged { .. })
    }

    pub fn current(&self) -> &WindowsBuildInfo {
        match self {
            Self::FreshInstall(c) | Self::Unchanged(c) | Self::BuildChanged { current: c, .. } => c,
        }
    }
}

/// Store for reading and atomically writing the last-seen Windows build metadata.
#[derive(Debug, Clone)]
pub struct BuildHistoryStore {
    state_dir: PathBuf,
    history_file: PathBuf,
}

impl BuildHistoryStore {
    pub const BUILD_FILE_NAME: &'static str = "last_seen_build.json";

    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        let dir = state_dir.as_ref().to_path_buf();
        let history_file = dir.join(Self::BUILD_FILE_NAME);
        Self {
            state_dir: dir,
            history_file,
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn history_file(&self) -> &Path {
        &self.history_file
    }

    /// Loads the last-seen build info if present on disk.
    pub fn load_last_seen(&self) -> CompatResult<Option<WindowsBuildInfo>> {
        if !self.history_file.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.history_file)
            .map_err(|e| CompatError::Io(format!("Failed to read build history: {e}")))?;

        let info: WindowsBuildInfo = serde_json::from_str(&content)
            .map_err(|e| CompatError::Serialization(format!("Corrupt build history: {e}")))?;

        Ok(Some(info))
    }

    /// Atomically writes the build info to disk using temp-file + rename.
    pub fn save_build(&self, info: &WindowsBuildInfo) -> CompatResult<()> {
        if !self.state_dir.exists() {
            fs::create_dir_all(&self.state_dir)
                .map_err(|e| CompatError::Io(format!("Failed to create state dir: {e}")))?;
        }

        let serialized = serde_json::to_string_pretty(info)
            .map_err(|e| CompatError::Serialization(e.to_string()))?;

        let temp_path = self.state_dir.join(format!(
            "{}.tmp.{}",
            Self::BUILD_FILE_NAME,
            std::process::id()
        ));

        {
            let mut file = File::create(&temp_path)
                .map_err(|e| CompatError::Io(format!("Failed to create temp build file: {e}")))?;
            file.write_all(serialized.as_bytes())
                .map_err(|e| CompatError::Io(format!("Failed to write temp build file: {e}")))?;
            file.sync_all()
                .map_err(|e| CompatError::Io(format!("Failed to sync temp build file: {e}")))?;
        }

        fs::rename(&temp_path, &self.history_file)
            .map_err(|e| CompatError::Io(format!("Failed to rename build history file: {e}")))?;

        Ok(())
    }

    /// Compares the current build with the last-seen build, records the current build,
    /// and returns the `BuildStatus`.
    pub fn check_and_record(&self, current: &WindowsBuildInfo) -> CompatResult<BuildStatus> {
        let last_seen = self.load_last_seen()?;

        let status = match last_seen {
            None => {
                self.save_build(current)?;
                BuildStatus::FreshInstall(current.clone())
            }
            Some(prev) => {
                if prev.build == current.build && prev.ubr == current.ubr {
                    BuildStatus::Unchanged(current.clone())
                } else {
                    self.save_build(current)?;
                    BuildStatus::BuildChanged {
                        previous: prev,
                        current: current.clone(),
                    }
                }
            }
        };

        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_store_fresh_unchanged_changed_cycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = BuildHistoryStore::new(temp_dir.path());

        let b1 = WindowsBuildInfo::new(10, 0, 22631, 4387, "23H2", "Windows 11 Pro");

        // 1. Fresh install
        let s1 = store.check_and_record(&b1).unwrap();
        assert_eq!(s1, BuildStatus::FreshInstall(b1.clone()));
        assert!(!s1.is_changed());

        // 2. Unchanged startup
        let s2 = store.check_and_record(&b1).unwrap();
        assert_eq!(s2, BuildStatus::Unchanged(b1.clone()));
        assert!(!s2.is_changed());

        // 3. Minor Windows Update (UBR bump: 4387 -> 4460)
        let b2 = WindowsBuildInfo::new(10, 0, 22631, 4460, "23H2", "Windows 11 Pro");
        let s3 = store.check_and_record(&b2).unwrap();
        assert!(s3.is_changed());
        match s3 {
            BuildStatus::BuildChanged { previous, current } => {
                assert_eq!(previous, b1);
                assert_eq!(current, b2);
            }
            _ => panic!("Expected BuildChanged"),
        }

        // 4. Feature Update (23H2 -> 24H2, build 26100)
        let b3 = WindowsBuildInfo::new(10, 0, 26100, 1742, "24H2", "Windows 11 Pro");
        let s4 = store.check_and_record(&b3).unwrap();
        assert!(s4.is_changed());
        match s4 {
            BuildStatus::BuildChanged { previous, current } => {
                assert_eq!(previous, b2);
                assert_eq!(current, b3);
            }
            _ => panic!("Expected BuildChanged"),
        }
    }
}
