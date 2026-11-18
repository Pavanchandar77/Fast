use nx_power::volume::{AudioSession, AudioSessionId, SetMutePlan, SetVolumePlan, VolumeStore};

#[test]
fn test_volume_store_sessions() {
    let mut store = VolumeStore::new();

    let s1 = AudioSession::new("spotify", 1234, "Spotify", 0.75, false);
    let s2 = AudioSession::new("discord", 5678, "Discord", 0.50, true);

    store.upsert_session(s1);
    store.upsert_session(s2);

    assert_eq!(store.list_sessions().len(), 2);
    assert_eq!(
        store
            .get_session(&AudioSessionId("spotify".to_string()))
            .unwrap()
            .volume,
        0.75
    );

    let removed = store.remove_session(&AudioSessionId("spotify".to_string()));
    assert!(removed.is_some());
    assert_eq!(store.list_sessions().len(), 1);
}

#[test]
fn test_set_volume_plan_dry_run_and_revert_identity() {
    let mut store = VolumeStore::new();
    let id = AudioSessionId("chrome".to_string());
    store.upsert_session(AudioSession::new("chrome", 100, "Chrome", 0.60, false));

    let mut plan = SetVolumePlan::new(&store, &id, 0.90).unwrap();

    // Dry run
    let dry_run = plan.dry_run();
    assert_eq!(dry_run.session_id, id);
    assert!((dry_run.old_volume - 0.60).abs() < f32::EPSILON);
    assert!((dry_run.new_volume - 0.90).abs() < f32::EPSILON);
    assert!(!dry_run.is_no_op);

    // Verify store was not modified during dry-run
    assert!((store.get_session(&id).unwrap().volume - 0.60).abs() < f32::EPSILON);

    // Apply
    let revert = plan.apply(&mut store).unwrap();
    assert!((store.get_session(&id).unwrap().volume - 0.90).abs() < f32::EPSILON);

    // Revert restores previous volume
    revert.revert(&mut store).unwrap();
    assert!((store.get_session(&id).unwrap().volume - 0.60).abs() < f32::EPSILON);
}

#[test]
fn test_set_mute_plan_apply_and_revert() {
    let mut store = VolumeStore::new();
    let id = AudioSessionId("game".to_string());
    store.upsert_session(AudioSession::new("game", 200, "Game", 1.0, false));

    let mut plan = SetMutePlan::new(&store, &id, true).unwrap();
    let dry_run = plan.dry_run();
    assert!(!dry_run.old_mute);
    assert!(dry_run.new_mute);

    // Apply mute
    let revert = plan.apply(&mut store).unwrap();
    assert!(store.get_session(&id).unwrap().is_muted);

    // Revert unmutes
    revert.revert(&mut store).unwrap();
    assert!(!store.get_session(&id).unwrap().is_muted);
}

#[test]
fn test_volume_bounds_validation() {
    let mut store = VolumeStore::new();
    let id = AudioSessionId("app".to_string());
    store.upsert_session(AudioSession::new("app", 300, "App", 0.5, false));

    assert!(SetVolumePlan::new(&store, &id, -0.1).is_err());
    assert!(SetVolumePlan::new(&store, &id, 1.05).is_err());
    assert!(SetVolumePlan::new(&store, &id, 0.0).is_ok());
    assert!(SetVolumePlan::new(&store, &id, 1.0).is_ok());
}
