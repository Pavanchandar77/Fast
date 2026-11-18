use nx_power::alt_tab::{AltTabState, AltTabSwitcher, AltTabWindow};
use nx_power::types::WindowId;

#[test]
fn test_alt_tab_lifecycle_and_mru() {
    let mut switcher = AltTabSwitcher::new();
    assert_eq!(switcher.state(), AltTabState::Idle);

    let w1 = AltTabWindow::new(WindowId::new(1), "Browser", "chrome.exe");
    let w2 = AltTabWindow::new(WindowId::new(2), "Editor", "code.exe");
    let w3 = AltTabWindow::new(WindowId::new(3), "Terminal", "pwsh.exe");

    switcher.upsert_window(w1);
    switcher.upsert_window(w2);
    switcher.upsert_window(w3);
    assert_eq!(switcher.windows().len(), 3);

    // Initial press Alt+Tab: activates switcher, selects index 1 (Editor)
    switcher.press_alt_tab();
    assert!(switcher.is_active());
    assert_eq!(switcher.selected_index(), Some(1));
    assert_eq!(switcher.selected_window().unwrap().id, WindowId::new(2));

    // Next press Alt+Tab: advances to index 2 (Terminal)
    switcher.press_alt_tab();
    assert_eq!(switcher.selected_index(), Some(2));
    assert_eq!(switcher.selected_window().unwrap().id, WindowId::new(3));

    // Next press Alt+Tab: wraps back to index 0 (Browser)
    switcher.press_alt_tab();
    assert_eq!(switcher.selected_index(), Some(0));

    // Step prev (Shift+Tab): wraps backwards to index 2
    switcher.step_prev();
    assert_eq!(switcher.selected_index(), Some(2));

    // Release Alt: commits selection of Terminal (id 3)
    let chosen = switcher.release_alt().unwrap();
    assert_eq!(chosen, WindowId::new(3));
    assert_eq!(switcher.state(), AltTabState::Idle);

    // Terminal must now be at MRU index 0
    assert_eq!(switcher.windows()[0].id, WindowId::new(3));
}

#[test]
fn test_alt_tab_cancel() {
    let mut switcher = AltTabSwitcher::new();
    switcher.upsert_window(AltTabWindow::new(WindowId::new(10), "W1", "app1"));
    switcher.upsert_window(AltTabWindow::new(WindowId::new(20), "W2", "app2"));

    switcher.press_alt_tab();
    assert_eq!(switcher.selected_index(), Some(1));

    // Cancel dismisses without changing MRU order
    switcher.cancel();
    assert_eq!(switcher.state(), AltTabState::Idle);
    assert_eq!(switcher.windows()[0].id, WindowId::new(10));
}

#[test]
fn test_alt_tab_window_removal() {
    let mut switcher = AltTabSwitcher::new();
    let win1 = WindowId::new(100);
    let win2 = WindowId::new(200);

    switcher.upsert_window(AltTabWindow::new(win1, "W1", "app"));
    switcher.upsert_window(AltTabWindow::new(win2, "W2", "app"));

    switcher.press_alt_tab(); // selected_index: 1 (win2)

    // Remove selected window
    let removed = switcher.remove_window(win2);
    assert!(removed.is_some());
    assert_eq!(switcher.windows().len(), 1);
    assert_eq!(switcher.selected_index(), Some(0));
}
