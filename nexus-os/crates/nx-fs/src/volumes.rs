//! Drives / volumes for the sidebar.

/// Kind of volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
    Optical,
    RamDisk,
    Unknown,
}

/// One mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Mount point (`C:\` or `/`).
    pub root: String,
    pub label: String,
    pub kind: VolumeKind,
    /// File system name (`NTFS`, `exFAT`, `ext4`, ...).
    pub fs: String,
}

impl Volume {
    /// NTFS volumes can use the MFT/USN indexer; everything else the walker.
    pub fn supports_usn(&self) -> bool {
        self.fs.eq_ignore_ascii_case("NTFS") || self.fs.eq_ignore_ascii_case("ReFS")
    }
}

/// Parse `/proc/self/mounts` content into user-relevant volumes.
pub fn parse_proc_mounts(text: &str) -> Vec<Volume> {
    const PSEUDO: &[&str] = &[
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "securityfs",
        "debugfs",
        "tracefs",
        "mqueue",
        "pstore",
        "bpf",
        "autofs",
        "configfs",
        "fusectl",
        "hugetlbfs",
        "binfmt_misc",
        "overlay",
        "nsfs",
        "squashfs",
        "rpc_pipefs",
    ];
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let dev = f.next()?;
            let mnt = f.next()?.replace("\\040", " ");
            let fs = f.next()?;
            if PSEUDO.contains(&fs)
                || mnt.starts_with("/proc")
                || mnt.starts_with("/sys")
                || mnt.starts_with("/dev")
                || mnt.starts_with("/run")
            {
                return None;
            }
            let kind = match fs {
                "nfs" | "nfs4" | "cifs" | "smb3" | "sshfs" | "fuse.sshfs" => VolumeKind::Network,
                "tmpfs" | "ramfs" => VolumeKind::RamDisk,
                "iso9660" | "udf" => VolumeKind::Optical,
                "vfat" | "exfat" if mnt.starts_with("/media") || mnt.starts_with("/run/media") => {
                    VolumeKind::Removable
                }
                _ => VolumeKind::Fixed,
            };
            if kind == VolumeKind::RamDisk && mnt != "/tmp" {
                return None;
            }
            let label = mnt
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(dev)
                .to_string();
            Some(Volume {
                root: mnt.clone(),
                label,
                kind,
                fs: fs.to_string(),
            })
        })
        .collect()
}

/// List volumes on this machine.
pub fn list_volumes() -> Vec<Volume> {
    #[cfg(windows)]
    {
        crate::platform::windows::volume::list_volumes()
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/proc/self/mounts")
            .map(|t| parse_proc_mounts(&t))
            .unwrap_or_else(|_| {
                vec![Volume {
                    root: "/".into(),
                    label: "/".into(),
                    kind: VolumeKind::Fixed,
                    fs: String::new(),
                }]
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mounts() {
        let t = "/dev/sda1 / ext4 rw 0 0\nproc /proc proc rw 0 0\n/dev/sdb1 /media/usb\\040stick exfat rw 0 0\nsrv:/x /mnt/nas nfs4 rw 0 0\ntmpfs /run/user tmpfs rw 0 0\n";
        let v = parse_proc_mounts(t);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].root, "/media/usb stick");
        assert_eq!(v[1].kind, VolumeKind::Removable);
        assert_eq!(v[2].kind, VolumeKind::Network);
        assert!(!v[0].supports_usn());
        assert!(!list_volumes().is_empty());
    }
}
