//! Memory of a 1M-path index: exact heap accounting plus process RSS delta.
//! Not a criterion bench (memory is not a timing); prints a table.

use nx_fs::index::mft::{MftTree, NTFS_ROOT_REF};
use nx_fs::index::synthetic::build_index;
use nx_fs::index::usn::{UsnRecord, ATTR_DIRECTORY};
use nx_fs::index::NO_PARENT;
use std::time::Instant;

fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

fn mb(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn main() {
    // `cargo bench` passes --bench; ignore arguments.
    const N: usize = 1_000_000;
    let before = rss_bytes().unwrap_or(0);
    let t = Instant::now();
    let ix = build_index(N, 42).expect("build");
    let build = t.elapsed();
    let after = rss_bytes().unwrap_or(0);
    let names: usize = ix.ids().map(|i| ix.name(i).len()).sum();
    println!(
        "index_memory: nodes={} avg_name_bytes={:.1}",
        ix.len(),
        names as f64 / ix.len() as f64
    );
    println!(
        "index_memory: build_time_ms={:.1}",
        build.as_secs_f64() * 1e3
    );
    println!(
        "index_memory: heap_bytes={} ({:.1} MiB)",
        ix.heap_bytes(),
        mb(ix.heap_bytes() as u64)
    );
    println!(
        "index_memory: rss_delta={} ({:.1} MiB)",
        after.saturating_sub(before),
        mb(after.saturating_sub(before))
    );
    println!(
        "index_memory: bytes_per_path={:.1}",
        ix.heap_bytes() as f64 / ix.len() as f64
    );

    let records: Vec<UsnRecord> = ix
        .ids()
        .filter(|&i| ix.parent(i) != NO_PARENT)
        .map(|i| {
            let parent = ix.parent(i);
            UsnRecord {
                file_ref: u128::from(i) + 100,
                parent_ref: if ix.parent(parent) == NO_PARENT {
                    NTFS_ROOT_REF
                } else {
                    u128::from(parent) + 100
                },
                usn: i64::from(i),
                reason: 0,
                attributes: if ix.is_dir(i) { ATTR_DIRECTORY } else { 0x20 },
                name: ix.name(i).to_string(),
            }
        })
        .collect();
    drop(ix);
    let t = Instant::now();
    let tree = MftTree::build("C:", records).expect("mft");
    println!(
        "index_memory: mft_build_time_ms={:.1}",
        t.elapsed().as_secs_f64() * 1e3
    );
    println!(
        "index_memory: mft_index_heap={:.1} MiB mft_ref_map={:.1} MiB total_live={:.1} MiB",
        mb(tree.index.heap_bytes() as u64),
        mb(tree.refs.map_bytes() as u64),
        mb((tree.index.heap_bytes() + tree.refs.map_bytes()) as u64)
    );
}
