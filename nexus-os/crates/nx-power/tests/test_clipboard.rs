use nx_power::clipboard::{ClipboardFilter, ClipboardKind, ClipboardStore, NewClipboardItem};

fn make_text_item(content: &str, app: Option<&str>, sensitive: bool) -> NewClipboardItem {
    NewClipboardItem {
        kind: ClipboardKind::PlainText,
        content: content.to_string(),
        timestamp_ms: 1000,
        source_app: app.map(|s| s.to_string()),
        sensitive_flag: sensitive,
    }
}

#[test]
fn test_clipboard_ring_eviction() {
    let mut store = ClipboardStore::new(3);
    assert_eq!(store.capacity(), 3);

    let id1 = store.push(make_text_item("item 1", None, false)).unwrap();
    let id2 = store.push(make_text_item("item 2", None, false)).unwrap();
    let id3 = store.push(make_text_item("item 3", None, false)).unwrap();
    assert_eq!(store.len(), 3);

    // Pushing 4th item should evict oldest unpinned (item 1)
    let id4 = store.push(make_text_item("item 4", None, false)).unwrap();
    assert_eq!(store.len(), 3);
    assert!(store.get(id1).is_none());
    assert!(store.get(id2).is_some());
    assert!(store.get(id3).is_some());
    assert!(store.get(id4).is_some());
}

#[test]
fn test_clipboard_pin_survives_eviction() {
    let mut store = ClipboardStore::new(2);

    let id1 = store
        .push(make_text_item("pinned item", None, false))
        .unwrap();
    assert!(store.pin(id1));
    assert_eq!(store.pinned_count(), 1);

    let id2 = store
        .push(make_text_item("unpinned 1", None, false))
        .unwrap();
    let id3 = store
        .push(make_text_item("unpinned 2", None, false))
        .unwrap();
    // 1 pinned + 2 unpinned = 3 items total. Both unpinned fit in capacity 2.
    assert_eq!(store.len(), 3);
    assert!(store.get(id2).is_some());

    // Pushing a 3rd unpinned item exceeds unpinned capacity 2, evicting id2
    let id4 = store
        .push(make_text_item("unpinned 3", None, false))
        .unwrap();

    // Pinned item must survive even when unpinned items overflow capacity
    assert!(store.get(id1).is_some());
    assert!(store.get(id1).unwrap().pinned);
    // Oldest unpinned (id2) was evicted
    assert!(store.get(id2).is_none());
    assert!(store.get(id3).is_some());
    assert!(store.get(id4).is_some());
}

#[test]
fn test_clipboard_unpin_triggers_capacity() {
    let mut store = ClipboardStore::new(2);

    let id1 = store.push(make_text_item("item 1", None, false)).unwrap();
    store.pin(id1);
    let _id2 = store.push(make_text_item("item 2", None, false)).unwrap();
    let _id3 = store.push(make_text_item("item 3", None, false)).unwrap();

    // 1 pinned + 2 unpinned = 3 total
    assert_eq!(store.len(), 3);

    // Unpinning id1 means 3 unpinned > capacity 2, so oldest unpinned is evicted
    assert!(store.unpin(id1));
    assert_eq!(store.unpinned_count(), 2);
    assert_eq!(store.len(), 2);
}

#[test]
fn test_clipboard_deduplication() {
    let mut store = ClipboardStore::new(5);

    let mut item1 = make_text_item("duplicate content", Some("app"), false);
    item1.timestamp_ms = 100;
    let id1 = store.push(item1).unwrap();

    let mut item2 = make_text_item("duplicate content", Some("app"), false);
    item2.timestamp_ms = 200;
    let id2 = store.push(item2).unwrap();

    assert_eq!(id1, id2);
    assert_eq!(store.len(), 1);
    assert_eq!(store.get(id1).unwrap().timestamp_ms, 200);
}

#[test]
fn test_privacy_exclusion_password_managers() {
    let mut store = ClipboardStore::with_default_exclusions(10);

    let pms = [
        "1Password.exe",
        "Bitwarden",
        "KeePassXC",
        "LastPass",
        "Dashlane",
        "Enpass",
    ];

    for pm in pms {
        let item = make_text_item("secret_password_123", Some(pm), false);
        let res = store.push(item);
        assert!(res.is_err(), "Password manager {} should be blocked", pm);
    }
    assert_eq!(store.len(), 0);
}

#[test]
fn test_privacy_exclusion_sensitive_flag() {
    let mut store = ClipboardStore::with_default_exclusions(10);

    let item = make_text_item("sensitive clipboard data", Some("notepad.exe"), true);
    let res = store.push(item);
    assert!(res.is_err());
    assert_eq!(store.len(), 0);
}

#[test]
fn test_privacy_exclusion_private_key() {
    let mut store = ClipboardStore::with_default_exclusions(10);

    let priv_key =
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0...\n-----END RSA PRIVATE KEY-----";
    let res = store.push(make_text_item(priv_key, Some("terminal.exe"), false));
    assert!(res.is_err());
    assert_eq!(store.len(), 0);
}

#[test]
fn test_privacy_exclusion_credit_cards() {
    let mut store = ClipboardStore::with_default_exclusions(10);

    let cards = [
        "4532-1488-2931-4829",
        "4532 1488 2931 4829",
        "4532148829314829",
    ];

    for card in cards {
        let res = store.push(make_text_item(card, None, false));
        assert!(res.is_err(), "Card {} should be blocked", card);
    }
    assert_eq!(store.len(), 0);
}

#[test]
fn test_privacy_exclusion_jwt_tokens() {
    let mut store = ClipboardStore::with_default_exclusions(10);

    let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let res = store.push(make_text_item(jwt, None, false));
    assert!(res.is_err());
    assert_eq!(store.len(), 0);
}

#[test]
fn test_clipboard_search_and_filter() {
    let mut store = ClipboardStore::new(10);

    let id1 = store
        .push(make_text_item(
            "Rust programming language",
            Some("chrome.exe"),
            false,
        ))
        .unwrap();
    let id2 = store
        .push(make_text_item(
            "Nexus-OS shell architecture",
            Some("code.exe"),
            false,
        ))
        .unwrap();
    let _id3 = store
        .push(make_text_item(
            "Daily shopping list",
            Some("notes.exe"),
            false,
        ))
        .unwrap();

    store.pin(id2);

    // Search query
    let rust_results = store.search("rust");
    assert_eq!(rust_results.len(), 1);
    assert_eq!(rust_results[0].id, id1);

    // Search by app name
    let code_results = store.search("code.exe");
    assert_eq!(code_results.len(), 1);
    assert_eq!(code_results[0].id, id2);

    // Filter pinned only
    let filter_pinned = ClipboardFilter {
        query: None,
        pinned_only: true,
        kind: None,
    };
    let pinned_results = store.filter(&filter_pinned);
    assert_eq!(pinned_results.len(), 1);
    assert_eq!(pinned_results[0].id, id2);
}

#[test]
fn test_clipboard_delete_and_clear() {
    let mut store = ClipboardStore::new(5);

    let id1 = store.push(make_text_item("clip 1", None, false)).unwrap();
    let id2 = store.push(make_text_item("clip 2", None, false)).unwrap();
    store.pin(id1);

    // Delete single
    let deleted = store.delete(id2);
    assert!(deleted.is_some());
    assert_eq!(deleted.unwrap().id, id2);
    assert_eq!(store.len(), 1);

    // Add unpinned and clear unpinned
    store.push(make_text_item("clip 3", None, false)).unwrap();
    assert_eq!(store.len(), 2);
    let cleared = store.clear_unpinned();
    assert_eq!(cleared, 1);
    assert_eq!(store.len(), 1);
    assert!(store.get(id1).is_some());

    // Clear all
    store.clear_all();
    assert_eq!(store.len(), 0);
}
