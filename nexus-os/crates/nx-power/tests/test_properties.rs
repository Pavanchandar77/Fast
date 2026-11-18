use nx_power::clipboard::{ClipboardKind, ClipboardStore, NewClipboardItem};
use nx_power::color::Color;
use nx_power::rename::{RenamePlanner, RenameRule};
use proptest::prelude::*;
use std::path::PathBuf;

proptest! {
    #[test]
    fn prop_color_hex_roundtrip(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
        let original = Color::rgb(r, g, b);
        let hex = original.to_hex_rgb();
        let parsed = Color::from_hex(&hex).unwrap();
        prop_assert_eq!(original, parsed);
    }

    #[test]
    fn prop_color_hex_rgba_roundtrip(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255, a in 0u8..=255) {
        let original = Color::rgba(r, g, b, a);
        let hex = original.to_hex_rgba();
        let parsed = Color::from_hex(&hex).unwrap();
        prop_assert_eq!(original, parsed);
    }

    #[test]
    fn prop_rename_undo_identity(count in 1usize..=20) {
        let files: Vec<PathBuf> = (0..count)
            .map(|i| PathBuf::from(format!("dir/file_{:03}.txt", i)))
            .collect();

        let rule = RenameRule::SequentialNumbering {
            prefix: "renamed_".to_string(),
            suffix: "_v1".to_string(),
            start: 1,
            step: 2,
            padding: 4,
        };

        let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
        prop_assert!(plan.is_valid());

        let undo = plan.generate_undo_plan();
        prop_assert!(undo.verify_restores_sources(&files));
        prop_assert_eq!(undo.actions.len(), files.len());
    }

    #[test]
    fn prop_clipboard_capacity_invariant(ops in prop::collection::vec(0u8..=2, 1..=50)) {
        let capacity = 5;
        let mut store = ClipboardStore::new(capacity);
        let mut next_item_idx = 0;

        for op in ops {
            match op {
                0 => {
                    // Push unpinned item
                    let item = NewClipboardItem {
                        kind: ClipboardKind::PlainText,
                        content: format!("item_{}", next_item_idx),
                        timestamp_ms: next_item_idx as u64,
                        source_app: None,
                        sensitive_flag: false,
                    };
                    next_item_idx += 1;
                    let _ = store.push(item);
                }
                1 => {
                    // Pin top item if any
                    if let Some(entry) = store.search("").first() {
                        let id = entry.id;
                        store.pin(id);
                    }
                }
                _ => {
                    // Unpin oldest item if any
                    if let Some(entry) = store.search("").last() {
                        let id = entry.id;
                        store.unpin(id);
                    }
                }
            }
            // Invariant: unpinned count must never exceed capacity
            prop_assert!(store.unpinned_count() <= capacity);
        }
    }
}
