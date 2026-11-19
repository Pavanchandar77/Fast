//! Low-level keyboard hook.
//!
//! The fail-safe chord does two things and then returns to the hook chain:
//! it sets an atomic flag and posts `WM_APP + 0x15` to the main thread.
//! It does not restore windows, allocate, or swallow the key. The watchdog
//! registers the same chord with `RegisterHotKey`, which only sees the key
//! when this hook calls `CallNextHookEx`.
//!
//! Every other binding comes from the config-driven `nx-wm` map. A chord
//! that is not in that map falls through, including the Windows shortcuts
//! the shell must not steal.

use crossbeam_channel::{Receiver, Sender};
use nexus_model::hotkeys::Direction;
use os_config::HotkeyChord;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use tracing::{error, info};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, WH_KEYBOARD_LL,
    WM_APP, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

pub const FAIL_SAFE_MSG: u32 = WM_APP + 0x15;

pub use nexus_model::hotkeys::HotkeyAction;

#[derive(Clone, Copy)]
struct SendHhook(HHOOK);
unsafe impl Send for SendHhook {}
unsafe impl Sync for SendHhook {}

static HOTKEY_SENDER: OnceLock<Sender<HotkeyAction>> = OnceLock::new();
static HOOK_HANDLE: parking_lot::Mutex<Option<SendHhook>> = parking_lot::Mutex::new(None);
static IS_RUNNING: AtomicBool = AtomicBool::new(false);
static FS_CTRL: AtomicBool = AtomicBool::new(false);
static FS_ALT: AtomicBool = AtomicBool::new(false);
static FS_SHIFT: AtomicBool = AtomicBool::new(false);
static FS_WIN: AtomicBool = AtomicBool::new(false);
static FS_VK: AtomicU32 = AtomicU32::new(0);
static FAIL_SAFE: AtomicBool = AtomicBool::new(false);
static MAIN_TID: AtomicU32 = AtomicU32::new(0);
static KEYBINDINGS: parking_lot::RwLock<Option<nx_wm::KeybindingMap>> =
    parking_lot::RwLock::new(None);

pub struct GlobalKeyboardHook {
    thread_handle: Option<JoinHandle<()>>,
    thread_id: u32,
}

fn arm_fail_safe(chord: HotkeyChord) {
    FS_CTRL.store(chord.ctrl, Ordering::SeqCst);
    FS_ALT.store(chord.alt, Ordering::SeqCst);
    FS_SHIFT.store(chord.shift, Ordering::SeqCst);
    FS_WIN.store(chord.win, Ordering::SeqCst);
    FS_VK.store(u32::from(chord.vk), Ordering::SeqCst);
}

/// Drain the calling thread's queue. Returns true when the fail-safe fired.
/// Calling this once creates the thread message queue, which `PostThreadMessageW` needs.
pub fn pump_messages() -> bool {
    let mut saw = FAIL_SAFE.load(Ordering::SeqCst);
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
            if msg.message == FAIL_SAFE_MSG {
                saw = true;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    saw
}

fn to_hotkey(action: nx_wm::Action) -> HotkeyAction {
    match action {
        nx_wm::Action::PanicFailSafe => HotkeyAction::PanicFailSafe,
        nx_wm::Action::CommandBar => HotkeyAction::CommandBar,
        nx_wm::Action::Terminal => HotkeyAction::Terminal,
        nx_wm::Action::CloseWindow => HotkeyAction::CloseWindow,
        nx_wm::Action::ToggleFullscreen => HotkeyAction::ToggleFullscreen,
        nx_wm::Action::ToggleFloating => HotkeyAction::ToggleFloating,
        nx_wm::Action::FocusWorkspace(ws) => HotkeyAction::FocusWorkspace(ws),
        nx_wm::Action::MoveToWorkspace(ws) => HotkeyAction::MoveToWorkspace(ws),
        nx_wm::Action::FocusDirection(dir) => {
            let mapped = match dir {
                nx_wm::Direction::Left => Direction::Left,
                nx_wm::Direction::Right => Direction::Right,
                nx_wm::Direction::Up => Direction::Up,
                nx_wm::Direction::Down => Direction::Down,
            };
            HotkeyAction::FocusDirection(mapped)
        }
        nx_wm::Action::Execute(cmd) => HotkeyAction::Custom(format!("exec:{cmd}")),
        nx_wm::Action::Custom(s) => HotkeyAction::Custom(s),
        other => HotkeyAction::Custom(format!("{other:?}")),
    }
}

impl GlobalKeyboardHook {
    /// Install the hook. `Err` means `SetWindowsHookExW` failed; the caller
    /// must restore any journaled state and abort startup.
    ///
    /// Call this on the thread that will pump messages. Bindings come from
    /// the default keybinding table.
    pub fn start(chord: HotkeyChord) -> anyhow::Result<(Self, Receiver<HotkeyAction>)> {
        Self::start_with_config(chord, &os_config::schema::KeybindingsConfig::default())
    }

    pub fn start_with_config(
        chord: HotkeyChord,
        config: &os_config::schema::KeybindingsConfig,
    ) -> anyhow::Result<(Self, Receiver<HotkeyAction>)> {
        arm_fail_safe(chord);
        let main_tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
        MAIN_TID.store(main_tid, Ordering::SeqCst);
        let _ = pump_messages();

        let map = nx_wm::KeybindingMap::load_from_config(&config.mod_key, &config.bindings)
            .unwrap_or_default();
        *KEYBINDINGS.write() = Some(map);

        let (tx, rx) = crossbeam_channel::unbounded();
        let _ = HOTKEY_SENDER.set(tx);

        let (installed_tx, installed_rx) = crossbeam_channel::bounded(1);
        let handle = std::thread::spawn(move || {
            let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(Self::low_level_keyboard_proc),
                    windows::Win32::Foundation::HMODULE::default(),
                    0,
                )
            };
            match hook {
                Ok(h) => {
                    *HOOK_HANDLE.lock() = Some(SendHhook(h));
                    IS_RUNNING.store(true, Ordering::SeqCst);
                    let _ = installed_tx.send(Ok(tid));
                    info!("Global WH_KEYBOARD_LL hook installed successfully");
                    let mut msg = MSG::default();
                    unsafe {
                        while windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                            &mut msg,
                            HWND::default(),
                            0,
                            0,
                        )
                        .as_bool()
                        {
                            if msg.message == WM_QUIT {
                                break;
                            }
                            DispatchMessageW(&msg);
                        }
                    }
                    let mut h_guard = HOOK_HANDLE.lock();
                    if let Some(h) = h_guard.take() {
                        let _ = unsafe { UnhookWindowsHookEx(h.0) };
                    }
                    IS_RUNNING.store(false, Ordering::SeqCst);
                }
                Err(err) => {
                    error!("Failed to install WH_KEYBOARD_LL hook: {:?}", err);
                    let _ = installed_tx.send(Err(err));
                }
            }
        });

        match installed_rx.recv() {
            Ok(Ok(thread_id)) => Ok((
                Self {
                    thread_handle: Some(handle),
                    thread_id,
                },
                rx,
            )),
            Ok(Err(err)) => {
                let _ = handle.join();
                anyhow::bail!("WH_KEYBOARD_LL failed to install: {err}")
            }
            Err(_) => {
                let _ = handle.join();
                anyhow::bail!("WH_KEYBOARD_LL thread exited before the hook was installed")
            }
        }
    }

    pub fn update_keybindings(config: &os_config::schema::KeybindingsConfig) {
        if let Ok(map) = nx_wm::KeybindingMap::load_from_config(&config.mod_key, &config.bindings) {
            *KEYBINDINGS.write() = Some(map);
            info!("Hotkeys reloaded from configuration");
        }
    }

    pub fn unhook_global() {
        let mut h_guard = HOOK_HANDLE.lock();
        if let Some(h) = h_guard.take() {
            let _ = unsafe { UnhookWindowsHookEx(h.0) };
        }
        IS_RUNNING.store(false, Ordering::SeqCst);
    }

    /// Fail-safe path: set a flag, post to the main thread, call the next hook.
    /// Bound chords are dispatched from the config map. Nothing else is swallowed.
    unsafe extern "system" fn low_level_keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 && (w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize)
        {
            let kbd = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let vk = kbd.vkCode;

            let is_down = |v: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| -> bool {
                (GetAsyncKeyState(v.0 as i32) as u16 & 0x8000) != 0
            };

            let ctrl = is_down(VK_CONTROL);
            let alt = is_down(VK_MENU);
            let shift = is_down(VK_SHIFT);
            let win = is_down(VK_LWIN) || is_down(VK_RWIN);

            let fail_vk = FS_VK.load(Ordering::SeqCst);
            let fail_safe = fail_vk != 0
                && vk == fail_vk
                && ctrl == FS_CTRL.load(Ordering::SeqCst)
                && alt == FS_ALT.load(Ordering::SeqCst)
                && shift == FS_SHIFT.load(Ordering::SeqCst)
                && win == FS_WIN.load(Ordering::SeqCst);
            if fail_safe {
                FAIL_SAFE.store(true, Ordering::SeqCst);
                let tid = MAIN_TID.load(Ordering::SeqCst);
                if tid != 0 {
                    let _ = PostThreadMessageW(tid, FAIL_SAFE_MSG, WPARAM(0), LPARAM(0));
                }
                return CallNextHookEx(HHOOK::default(), n_code, w_param, l_param);
            }

            if let Some(key) = nx_wm::Key::from_vk(vk) {
                let modifiers = nx_wm::Modifiers {
                    super_key: win,
                    alt,
                    ctrl,
                    shift,
                };

                let matched_action = {
                    let guard = KEYBINDINGS.read();
                    guard
                        .as_ref()
                        .and_then(|map| map.lookup(modifiers, key).cloned())
                };

                if let Some(action) = matched_action {
                    let hotkey_action = to_hotkey(action);
                    if let Some(sender) = HOTKEY_SENDER.get() {
                        let _ = sender.try_send(hotkey_action);
                    }

                    // Cancels the Windows Start Menu when Super was part of the chord.
                    if win {
                        unsafe {
                            windows::Win32::UI::Input::KeyboardAndMouse::keybd_event(
                                0,
                                0,
                                windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                                0,
                            );
                        }
                    }

                    return LRESULT(1);
                }
            }
        }

        CallNextHookEx(HHOOK::default(), n_code, w_param, l_param)
    }
}

impl Drop for GlobalKeyboardHook {
    fn drop(&mut self) {
        if self.thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
