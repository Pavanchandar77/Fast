use nx_power::rename::{CaseType, RenameConflict, RenamePlanner, RenameRule};
use std::path::PathBuf;

#[test]
fn test_sequential_numbering() {
    let files = vec![
        PathBuf::from("photos/img_a.jpg"),
        PathBuf::from("photos/img_b.jpg"),
        PathBuf::from("photos/img_c.jpg"),
    ];

    let rule = RenameRule::SequentialNumbering {
        prefix: "photo_".to_string(),
        suffix: "".to_string(),
        start: 1,
        step: 1,
        padding: 3,
    };

    let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
    assert!(plan.is_valid());
    assert_eq!(plan.actions.len(), 3);

    assert_eq!(
        plan.actions[0].target,
        PathBuf::from("photos/photo_001.jpg")
    );
    assert_eq!(
        plan.actions[1].target,
        PathBuf::from("photos/photo_002.jpg")
    );
    assert_eq!(
        plan.actions[2].target,
        PathBuf::from("photos/photo_003.jpg")
    );
}

#[test]
fn test_regex_replace() {
    let files = vec![
        PathBuf::from("/docs/report_2026_final.pdf"),
        PathBuf::from("/docs/summary_2026_draft.pdf"),
    ];

    let rule = RenameRule::RegexReplace {
        pattern: r"(\w+)_(\d{4})_(\w+)".to_string(),
        replacement: "$2_$1_$3".to_string(),
    };

    let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
    assert!(plan.is_valid());
    assert_eq!(
        plan.actions[0].target,
        PathBuf::from("/docs/2026_report_final.pdf")
    );
    assert_eq!(
        plan.actions[1].target,
        PathBuf::from("/docs/2026_summary_draft.pdf")
    );
}

#[test]
fn test_prefix_suffix_and_extension() {
    let files = vec![PathBuf::from("log.txt")];

    let rules = vec![
        RenameRule::PrefixSuffix {
            prefix: "backup_".to_string(),
            suffix: "_old".to_string(),
        },
        RenameRule::ChangeExtension {
            new_extension: "bak".to_string(),
        },
    ];

    let plan = RenamePlanner::plan(&files, &rules).unwrap();
    assert!(plan.is_valid());
    assert_eq!(plan.actions[0].target, PathBuf::from("backup_log_old.bak"));
}

#[test]
fn test_case_transformations() {
    let test_cases = [
        ("hello_world", CaseType::Uppercase, "HELLO_WORLD"),
        ("HELLO_WORLD", CaseType::Lowercase, "hello_world"),
        ("hello world", CaseType::TitleCase, "Hello World"),
        ("hello_world", CaseType::CamelCase, "helloWorld"),
        ("HelloWorld", CaseType::SnakeCase, "hello_world"),
        ("HelloWorld", CaseType::KebabCase, "hello-world"),
    ];

    for (input, case_type, expected) in test_cases {
        let files = vec![PathBuf::from(format!("{}.txt", input))];
        let plan = RenamePlanner::plan(&files, &[RenameRule::ChangeCase { case_type }]).unwrap();
        assert_eq!(
            plan.actions[0].target,
            PathBuf::from(format!("{}.txt", expected))
        );
    }
}

#[test]
fn test_collision_detection_within_batch() {
    let files = vec![PathBuf::from("doc1.txt"), PathBuf::from("doc2.txt")];

    // Renaming both to same constant name
    let rule = RenameRule::FindReplace {
        find: "doc2".to_string(),
        replace: "doc1".to_string(),
        case_sensitive: true,
    };

    let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
    assert!(!plan.is_valid());
    assert_eq!(plan.conflicts.len(), 1);

    match &plan.conflicts[0] {
        RenameConflict::CollisionWithBatch { target, sources } => {
            assert_eq!(target, &PathBuf::from("doc1.txt"));
            assert_eq!(sources.len(), 2);
        }
        _ => panic!("Expected CollisionWithBatch conflict"),
    }
}

#[test]
fn test_invalid_character_detection() {
    let files = vec![PathBuf::from("safe_name.txt")];

    let rule = RenameRule::PrefixSuffix {
        prefix: "bad:name?".to_string(),
        suffix: "".to_string(),
    };

    let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
    assert!(!plan.is_valid());
    assert_eq!(plan.conflicts.len(), 1);

    match &plan.conflicts[0] {
        RenameConflict::InvalidCharacters { invalid_char, .. } => {
            assert_eq!(*invalid_char, ':');
        }
        _ => panic!("Expected InvalidCharacters conflict"),
    }
}

#[test]
fn test_rename_dry_run_and_undo_identity() {
    let files = vec![
        PathBuf::from("a.txt"),
        PathBuf::from("b.txt"),
        PathBuf::from("c.txt"),
    ];

    let rule = RenameRule::PrefixSuffix {
        prefix: "v2_".to_string(),
        suffix: "".to_string(),
    };

    let plan = RenamePlanner::plan(&files, &[rule]).unwrap();
    assert!(plan.is_valid());

    // Dry run
    let report = plan.dry_run();
    assert_eq!(report.total_files, 3);
    assert_eq!(report.changed_count, 3);
    assert_eq!(report.unchanged_count, 0);
    assert_eq!(report.conflict_count, 0);

    // Undo plan must restore original source paths
    let undo = plan.generate_undo_plan();
    assert_eq!(undo.actions.len(), 3);
    assert!(undo.verify_restores_sources(&files));

    for (i, action) in undo.actions.iter().enumerate() {
        assert_eq!(action.source, plan.actions[i].target);
        assert_eq!(action.target, files[i]);
    }
}
