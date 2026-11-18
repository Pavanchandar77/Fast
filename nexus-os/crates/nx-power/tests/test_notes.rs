use nx_power::notes::{NoteColor, NotesStore};

#[test]
fn test_notes_crud_and_pin() {
    let mut store = NotesStore::new();

    let id = store.create_note(
        "Shopping List",
        "- Milk\n- Bread\n- Eggs",
        vec!["personal".to_string()],
        NoteColor::Yellow,
        1000,
    );
    assert_eq!(store.len(), 1);

    let note = store.get(id).unwrap();
    assert_eq!(note.title, "Shopping List");
    assert!(!note.pinned);

    // Pin note
    store.set_pinned(id, true).unwrap();
    assert!(store.get(id).unwrap().pinned);
    assert_eq!(store.list_pinned().len(), 1);
}

#[test]
fn test_notes_update_and_revert_identity() {
    let mut store = NotesStore::new();

    let id = store.create_note(
        "Initial Title",
        "Initial Content",
        vec![],
        NoteColor::Blue,
        100,
    );

    // Update
    let revert = store
        .update_note(id, "Updated Title", "Updated Content", 200)
        .unwrap();

    let updated = store.get(id).unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.content, "Updated Content");
    assert_eq!(updated.updated_at_ms, 200);

    // Revert restores initial state
    revert.revert(&mut store).unwrap();
    let reverted = store.get(id).unwrap();
    assert_eq!(reverted.title, "Initial Title");
    assert_eq!(reverted.content, "Initial Content");
    assert_eq!(reverted.updated_at_ms, 100);
}

#[test]
fn test_notes_delete_and_undelete() {
    let mut store = NotesStore::new();

    let id = store.create_note(
        "Secret note",
        "Top secret body",
        vec![],
        NoteColor::Pink,
        50,
    );
    assert_eq!(store.len(), 1);

    // Delete
    let undelete = store.delete_note(id).unwrap();
    assert_eq!(store.len(), 0);
    assert!(store.get(id).is_none());

    // Undelete resurrects note
    undelete.revert(&mut store).unwrap();
    assert_eq!(store.len(), 1);
    let note = store.get(id).unwrap();
    assert_eq!(note.title, "Secret note");
}

#[test]
fn test_notes_search_and_tag_filter() {
    let mut store = NotesStore::new();

    let id1 = store.create_note(
        "Rust Memory Safety",
        "Ownership and borrowing rules guarantee fearless concurrency",
        vec!["rust".to_string(), "tech".to_string()],
        NoteColor::Green,
        10,
    );
    let _id2 = store.create_note(
        "Grocery store run",
        "Buy apples and oranges",
        vec!["personal".to_string()],
        NoteColor::Yellow,
        20,
    );
    let id3 = store.create_note(
        "Nexus OS design",
        "Shell performance benchmarks and Rust windows integration",
        vec!["tech".to_string(), "nexus".to_string()],
        NoteColor::Purple,
        30,
    );

    store.set_pinned(id3, true).unwrap();

    // Search query "rust"
    let rust_notes = store.search("rust");
    assert_eq!(rust_notes.len(), 2); // id1 and id3

    // Pinned notes sort first
    assert_eq!(rust_notes[0].id, id3);
    assert_eq!(rust_notes[1].id, id1);

    // Tag filtering
    let tech_notes = store.filter_by_tag("tech");
    assert_eq!(tech_notes.len(), 2);
    assert!(tech_notes.iter().any(|n| n.id == id1));
    assert!(tech_notes.iter().any(|n| n.id == id3));
}
