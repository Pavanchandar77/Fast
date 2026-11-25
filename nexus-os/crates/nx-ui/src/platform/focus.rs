//! Bring a window to the foreground and show or hide it without blocking
//! the UI thread.
//!
//! Windows refuses `SetForegroundWindow` unless this process recently received
//! input. The sequence below follows PowerToys' `WindowHelpers.BringToForeground`
//! (MIT, Microsoft): a short `SendInput` nudge (with a timeout, so a stuck
//! input stack cannot hang the UI thread), then `SetForegroundWindow`, then
//! `AttachThreadInput` to the current foreground thread if that still failed.
//! Showing and hiding use `ShowWindowAsync` so the call never re-enters the
//! window procedure from a GPUI update (Pavan: a sync `ShowWindow`/`SetWindowPos`
//! during an update can deadlock or flicker).
//!
//! Copyright notice for the borrowed pattern: Copyright (c) Microsoft
//! Corporation, licensed under the MIT license. See THIRD-PARTY-NOTICES.md.

use std::time::Duration;

/// How long we wait for `SendInput` before giving up and trying
/// `SetForegroundWindow` anyway. PowerToys uses 500 ms for the same reason:
/// `SendInput` can stall inside the kernel.
#[allow(dead_code)]
pub const FOREGROUND_INPUT_TIMEOUT: Duration = Duration::from_millis(500);

/// Hooks used by [`bring_to_foreground_with`], so the sequence can be unit
/// tested without a real desktop.
#[cfg_attr(not(windows), allow(dead_code))]
pub trait ForegroundHooks {
    fn foreground(&self) -> isize;
    fn set_foreground(&self, hwnd: isize) -> bool;
    fn thread_of(&self, hwnd: isize) -> u32;
    /// Send a dummy input event so this process is the last-input process.
    /// Must not block the caller for longer than `timeout`.
    fn nudge_input(&self, timeout: Duration);
    fn attach_threads(&self, a: u32, b: u32, attach: bool);
}

/// Bring `hwnd` to the foreground. No-op when it already is, or off Windows.
pub fn bring_to_foreground(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    #[cfg(windows)]
    bring_to_foreground_with(hwnd, &Win32Hooks, FOREGROUND_INPUT_TIMEOUT);
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Restore a minimized window (or show a hidden one) and focus it.
pub fn restore_and_focus(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    #[cfg(windows)]
    {
        show_async(hwnd, ShowCmd::Restore);
        bring_to_foreground(hwnd);
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Hide a window without re-entering its procedure (hide-on-close, overlays).
pub fn hide_async(hwnd: isize) {
    show_async(hwnd, ShowCmd::Hide);
}

/// Show a hidden (but not minimized) window without activating it.
pub fn show_noactivate_async(hwnd: isize) {
    show_async(hwnd, ShowCmd::ShowNoActivate);
}

/// Commands for [`show_async`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowCmd {
    Hide,
    Show,
    ShowNoActivate,
    Restore,
    Minimize,
}

/// `ShowWindowAsync` on Windows so a GPUI update cannot re-enter `WndProc`.
pub fn show_async(hwnd: isize, cmd: ShowCmd) {
    if hwnd == 0 {
        return;
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            ShowWindowAsync, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
        };
        let sw = match cmd {
            ShowCmd::Hide => SW_HIDE,
            ShowCmd::Show => SW_SHOW,
            ShowCmd::ShowNoActivate => SW_SHOWNOACTIVATE,
            ShowCmd::Restore => SW_RESTORE,
            ShowCmd::Minimize => SW_MINIMIZE,
        };
        let _ = ShowWindowAsync(HWND(hwnd as *mut std::ffi::c_void), sw);
    }
    #[cfg(not(windows))]
    let _ = (hwnd, cmd);
}

/// The PowerToys sequence: skip if already foreground, nudge input, set
/// foreground, then attach threads if Windows still refused.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn bring_to_foreground_with(
    hwnd: isize,
    hooks: &dyn ForegroundHooks,
    timeout: Duration,
) -> bool {
    if hwnd == 0 {
        return false;
    }
    if hooks.foreground() == hwnd {
        return true;
    }
    hooks.nudge_input(timeout);
    if hooks.set_foreground(hwnd) && hooks.foreground() == hwnd {
        return true;
    }
    let ours = hooks.thread_of(hwnd);
    let theirs = hooks.thread_of(hooks.foreground());
    if ours != 0 && theirs != 0 && ours != theirs {
        hooks.attach_threads(ours, theirs, true);
        let ok = hooks.set_foreground(hwnd);
        hooks.attach_threads(ours, theirs, false);
        return ok;
    }
    hooks.set_foreground(hwnd)
}

#[cfg(windows)]
struct Win32Hooks;

#[cfg(windows)]
impl ForegroundHooks for Win32Hooks {
    fn foreground(&self) -> isize {
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize }
    }

    fn set_foreground(&self, hwnd: isize) -> bool {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void),
            )
            .as_bool()
        }
    }

    fn thread_of(&self, hwnd: isize) -> u32 {
        if hwnd == 0 {
            return 0;
        }
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void),
                None,
            )
        }
    }

    fn nudge_input(&self, timeout: Duration) {
        // Run SendInput off this thread: a stuck input stack must not hang
        // the UI (PowerToys Settings once did, AppHang in NtUserSendInput).
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("nexus-fg-nudge".into())
            .spawn(move || {
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
                    };
                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT::default(),
                        },
                    };
                    let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                let _ = tx.send(());
            });
        let _ = rx.recv_timeout(timeout);
    }

    fn attach_threads(&self, a: u32, b: u32, attach: bool) {
        unsafe {
            let _ = windows::Win32::System::Threading::AttachThreadInput(a, b, attach);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fake {
        fg: Cell<isize>,
        set_calls: Cell<u32>,
        nudged: Cell<bool>,
        attaches: Cell<u32>,
        threads: std::collections::HashMap<isize, u32>,
        refuse_until_attach: bool,
        attached: Cell<bool>,
    }

    impl ForegroundHooks for Fake {
        fn foreground(&self) -> isize {
            self.fg.get()
        }
        fn set_foreground(&self, hwnd: isize) -> bool {
            self.set_calls.set(self.set_calls.get() + 1);
            if self.refuse_until_attach && !self.attached.get() {
                return false;
            }
            self.fg.set(hwnd);
            true
        }
        fn thread_of(&self, hwnd: isize) -> u32 {
            self.threads.get(&hwnd).copied().unwrap_or(0)
        }
        fn nudge_input(&self, _timeout: Duration) {
            self.nudged.set(true);
        }
        fn attach_threads(&self, _a: u32, _b: u32, attach: bool) {
            self.attaches.set(self.attaches.get() + 1);
            self.attached.set(attach);
        }
    }

    #[test]
    fn already_foreground_is_a_no_op() {
        let fake = Fake {
            fg: Cell::new(7),
            set_calls: Cell::new(0),
            nudged: Cell::new(false),
            attaches: Cell::new(0),
            threads: std::collections::HashMap::new(),
            refuse_until_attach: false,
            attached: Cell::new(false),
        };
        assert!(bring_to_foreground_with(
            7,
            &fake,
            Duration::from_millis(10)
        ));
        assert!(!fake.nudged.get());
        assert_eq!(fake.set_calls.get(), 0);
    }

    #[test]
    fn zero_hwnd_is_ignored() {
        let fake = Fake {
            fg: Cell::new(1),
            set_calls: Cell::new(0),
            nudged: Cell::new(false),
            attaches: Cell::new(0),
            threads: std::collections::HashMap::new(),
            refuse_until_attach: false,
            attached: Cell::new(false),
        };
        assert!(!bring_to_foreground_with(
            0,
            &fake,
            Duration::from_millis(10)
        ));
        assert_eq!(fake.set_calls.get(), 0);
    }

    #[test]
    fn input_nudge_then_set_foreground() {
        let fake = Fake {
            fg: Cell::new(1),
            set_calls: Cell::new(0),
            nudged: Cell::new(false),
            attaches: Cell::new(0),
            threads: [(7, 10), (1, 11)].into_iter().collect(),
            refuse_until_attach: false,
            attached: Cell::new(false),
        };
        assert!(bring_to_foreground_with(
            7,
            &fake,
            Duration::from_millis(10)
        ));
        assert!(fake.nudged.get());
        assert_eq!(fake.fg.get(), 7);
        assert_eq!(fake.attaches.get(), 0);
    }

    #[test]
    fn attaches_threads_when_set_foreground_is_refused() {
        let fake = Fake {
            fg: Cell::new(1),
            set_calls: Cell::new(0),
            nudged: Cell::new(false),
            attaches: Cell::new(0),
            threads: [(7, 10), (1, 11)].into_iter().collect(),
            refuse_until_attach: true,
            attached: Cell::new(false),
        };
        assert!(bring_to_foreground_with(
            7,
            &fake,
            Duration::from_millis(10)
        ));
        assert!(fake.nudged.get());
        assert_eq!(fake.fg.get(), 7);
        // attach + detach
        assert_eq!(fake.attaches.get(), 2);
    }
}
