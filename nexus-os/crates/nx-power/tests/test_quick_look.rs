use nx_power::quick_look::{QuickLookContent, QuickLookSession, QuickLookStatus};
use std::path::PathBuf;

#[test]
fn test_quick_look_session_lifecycle() {
    let mut session = QuickLookSession::new();
    assert!(!session.is_open());
    assert_eq!(*session.status(), QuickLookStatus::Closed);

    let path = PathBuf::from("C:\\nexus\\main.rs");
    session.open(path.clone(), vec![path.clone()]);
    assert!(session.is_open());
    assert_eq!(session.current_path(), Some(path.as_path()));

    // Set content
    session.set_content(QuickLookContent::Code {
        language: "rust".to_string(),
        lines: vec!["fn main() {}".to_string()],
        line_count: 1,
        truncated: false,
    });
    assert_eq!(*session.status(), QuickLookStatus::Active);

    // Close
    session.close();
    assert!(!session.is_open());
    assert_eq!(*session.status(), QuickLookStatus::Closed);
    assert!(session.current_path().is_none());
}

#[test]
fn test_quick_look_playlist_navigation() {
    let mut session = QuickLookSession::new();
    let f1 = PathBuf::from("1.txt");
    let f2 = PathBuf::from("2.txt");
    let f3 = PathBuf::from("3.txt");
    let playlist = vec![f1.clone(), f2.clone(), f3.clone()];

    session.open(f1.clone(), playlist);
    assert_eq!(session.current_path(), Some(f1.as_path()));

    // Next -> f2
    assert_eq!(session.next_file(), Some(f2.as_path()));
    // Next -> f3
    assert_eq!(session.next_file(), Some(f3.as_path()));
    // Next wraps -> f1
    assert_eq!(session.next_file(), Some(f1.as_path()));

    // Prev wraps -> f3
    assert_eq!(session.prev_file(), Some(f3.as_path()));
}

#[test]
fn test_quick_look_pinning() {
    let mut session = QuickLookSession::new();
    let path = PathBuf::from("doc.pdf");
    session.open(path.clone(), vec![path.clone()]);

    assert!(!session.is_pinned());
    session.toggle_pinned();
    assert!(session.is_pinned());

    // Normal close is ignored when pinned
    session.close();
    assert!(session.is_open());
    assert_eq!(*session.status(), QuickLookStatus::Pinned);

    // Force close terminates pinned session
    session.force_close();
    assert!(!session.is_open());
    assert!(!session.is_pinned());
}

#[test]
fn test_quick_look_zoom_controls() {
    let mut session = QuickLookSession::new();
    assert_eq!(session.zoom_level(), 1.0);

    session.zoom_in();
    assert!((session.zoom_level() - 1.25).abs() < f32::EPSILON);

    session.zoom_out();
    assert!((session.zoom_level() - 1.0).abs() < f32::EPSILON);

    // Zoom out to minimum clamp
    for _ in 0..10 {
        session.zoom_out();
    }
    assert_eq!(session.zoom_level(), QuickLookSession::MIN_ZOOM);

    // Zoom in to maximum clamp
    for _ in 0..20 {
        session.zoom_in();
    }
    assert_eq!(session.zoom_level(), QuickLookSession::MAX_ZOOM);

    session.reset_zoom();
    assert_eq!(session.zoom_level(), 1.0);
}

#[test]
fn test_quick_look_toggle() {
    let mut session = QuickLookSession::new();
    let path = PathBuf::from("image.png");
    let playlist = vec![path.clone()];

    // Toggle 1: open
    session.toggle(path.clone(), playlist.clone());
    assert!(session.is_open());

    // Toggle 2 on same path: close
    session.toggle(path.clone(), playlist);
    assert!(!session.is_open());
}
