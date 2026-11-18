//! Deterministic synthetic trees for benchmarks and tests.

use super::{NodeId, PathIndex};
use crate::error::FsResult;

const WORDS: &[&str] = &[
    "src",
    "docs",
    "assets",
    "build",
    "photos",
    "music",
    "projects",
    "notes",
    "invoice",
    "report",
    "backup",
    "config",
    "Windows",
    "Program Files",
    "Users",
    "Downloads",
    "Desktop",
    "node_modules",
    "target",
    "cache",
    "vacation",
    "budget",
    "design",
    "nexus",
    "explorer",
    "kernel",
    "driver",
    "shader",
    "render",
    "window",
    "camera",
    "summer",
    "family",
];
const EXTS: &[&str] = &[
    "rs", "txt", "md", "jpg", "png", "pdf", "docx", "xlsx", "json", "toml", "dll", "exe", "mp3",
    "js", "ts",
];

/// xorshift64* PRNG (no deps, reproducible).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// Generate a realistic name (`budget_report 2024 (3).xlsx`-ish).
pub fn name(rng: &mut Rng, dir: bool, serial: usize) -> String {
    let a = WORDS[rng.below(WORDS.len())];
    let b = WORDS[rng.below(WORDS.len())];
    let sep = ["_", "-", " ", ""][rng.below(4)];
    if dir {
        format!("{a}{sep}{b}{}", serial % 97)
    } else {
        format!("{a}{sep}{b}_{serial}.{}", EXTS[rng.below(EXTS.len())])
    }
}

/// Build an index with exactly `n` nodes below one root: ~8% directories,
/// fan-out of ~12 entries per directory, depth up to ~7.
pub fn build_index(n: usize, seed: u64) -> FsResult<PathIndex> {
    let mut rng = Rng::new(seed);
    let mut ix = PathIndex::with_capacity(n + 1, n * 24);
    let root = ix.add_root("C:")?;
    let mut dirs: Vec<NodeId> = vec![root];
    let mut made = 0usize;
    while made < n {
        let is_dir = rng.below(100) < 8 || dirs.len() < 16;
        // Prefer recent directories so the tree gets depth.
        let pick = if rng.below(4) == 0 {
            rng.below(dirs.len())
        } else {
            dirs.len() - 1 - rng.below(dirs.len().min(64))
        };
        let parent = dirs[pick];
        let nm = name(&mut rng, is_dir, made);
        let id = ix.insert(parent, &nm, is_dir)?;
        if is_dir {
            dirs.push(id);
        }
        made += 1;
    }
    ix.shrink_to_fit();
    Ok(ix)
}

/// Generate `n` full synthetic paths (for builders that take strings).
pub fn paths(n: usize, seed: u64) -> FsResult<Vec<String>> {
    let ix = build_index(n, seed)?;
    Ok(ix
        .ids()
        .filter(|&i| ix.parent(i) != super::NO_PARENT)
        .map(|i| ix.full_path(i))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_sized() {
        let a = build_index(5_000, 7).expect("build");
        let b = build_index(5_000, 7).expect("build");
        assert_eq!(a.len(), b.len());
        assert!(a.len() > 4_900, "few name collisions: {}", a.len());
        assert_eq!(paths(100, 1).expect("paths"), paths(100, 1).expect("paths"));
    }
}
