use nx_power::remap::{KeyChord, KeyCode, KeyModifiers, RemapTable, RemapTarget};

#[test]
fn test_basic_remap() {
    let mut table = RemapTable::new(false);

    let src = KeyChord::plain(KeyCode::CapsLock);
    let target = RemapTarget::Key(KeyCode::LeftCtrl);

    let id = table.add_rule(src, target.clone(), None).unwrap();
    assert_eq!(table.rules().len(), 1);

    let resolved = table.resolve_chord(src, None);
    assert_eq!(resolved, Some(target));

    // Remove rule
    assert!(table.remove_rule(id));
    assert_eq!(table.rules().len(), 0);
    assert_eq!(table.resolve_chord(src, None), None);
}

#[test]
fn test_app_specific_priority() {
    let mut table = RemapTable::new(false);

    let chord = KeyChord::new(KeyModifiers::CTRL, KeyCode::KeyP);

    // Global rule
    let global_target = RemapTarget::Key(KeyCode::F1);
    table.add_rule(chord, global_target.clone(), None).unwrap();

    // App-specific rule for Photoshop
    let app_target = RemapTarget::Key(KeyCode::F2);
    table
        .add_rule(chord, app_target.clone(), Some("photoshop.exe".to_string()))
        .unwrap();

    // Resolving under photoshop.exe gets app_target
    assert_eq!(
        table.resolve_chord(chord, Some("photoshop.exe")),
        Some(app_target)
    );

    // Resolving under code.exe gets global_target
    assert_eq!(
        table.resolve_chord(chord, Some("code.exe")),
        Some(global_target)
    );
}

#[test]
fn test_system_shortcut_protection() {
    let mut table = RemapTable::new(false); // default: do not allow system override

    let protected = [
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyL), // Win+L
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyX), // Win+X
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyR), // Win+R
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyE), // Win+E
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyD), // Win+D
        KeyChord::new(KeyModifiers::WIN, KeyCode::KeyI), // Win+I
        KeyChord::new(KeyModifiers::ALT, KeyCode::Tab),  // Alt+Tab
    ];

    for chord in protected {
        let res = table.add_rule(chord, RemapTarget::Disabled, None);
        assert!(
            res.is_err(),
            "Protected shortcut {:?} should be guarded",
            chord
        );
    }

    // Now test with override allowed
    let mut override_table = RemapTable::new(true);
    let win_e = KeyChord::new(KeyModifiers::WIN, KeyCode::KeyE);
    let res = override_table.add_rule(win_e, RemapTarget::Key(KeyCode::F12), None);
    assert!(res.is_ok());
}

#[test]
fn test_circular_remap_detection() {
    let mut table = RemapTable::new(false);

    let chord_a = KeyChord::plain(KeyCode::KeyA);
    let chord_b = KeyChord::plain(KeyCode::KeyB);

    // Self loop
    let self_loop = table.add_rule(chord_a, RemapTarget::Chord(chord_a), None);
    assert!(self_loop.is_err());

    // A -> B
    table
        .add_rule(chord_a, RemapTarget::Chord(chord_b), None)
        .unwrap();

    // B -> A (creates cycle A -> B -> A)
    let cycle = table.add_rule(chord_b, RemapTarget::Chord(chord_a), None);
    assert!(cycle.is_err());
}
