use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT,
    VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
    WM_SYSKEYDOWN,
};
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    PanicFailSafe,
    CommandBar,
    Terminal,
    CloseWindow,
    ToggleFullscreen,
    ToggleFloating,
    FocusWorkspace(u8),
    MoveToWorkspace(u8),
    Custom(String),
}

#[derive(Clone, Copy)]
struct SendHhook(HHOOK);
unsafe impl Send for SendHhook {}
unsafe impl Sync for SendHhook {}

// Lock-free static sender - zero lock contention in WH_KEYBOARD_LL callback
static HOTKEY_SENDER: OnceLock<Sender<HotkeyAction>> = OnceLock::new();
static HOOK_HANDLE: parking_lot::Mutex<Option<SendHhook>> = parking_lot::Mutex::new(None);
static IS_RUNNING: AtomicBool = AtomicBool::new(false);

pub struct GlobalKeyboardHook {
    thread_handle: Option<JoinHandle<()>>,
    thread_id: u32,
}

impl GlobalKeyboardHook {
    pub fn start() -> anyhow::Result<(Self, Receiver<HotkeyAction>)> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let _ = HOTKEY_SENDER.set(tx);

        let (thread_id_tx, thread_id_rx) = crossbeam_channel::bounded(1);

        let handle = std::thread::spawn(move || {
            let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            let _ = thread_id_tx.send(tid);

            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(Self::low_level_keyboard_proc),
                    HMODULE::default(),
                    0,
                )
            };

            match hook {
                Ok(h) => {
                    *HOOK_HANDLE.lock() = Some(SendHhook(h));
                    IS_RUNNING.store(true, Ordering::SeqCst);
                    info!("Global WH_KEYBOARD_LL hook installed successfully");

                    // Standard message pump for the hook thread
                    let mut msg = MSG::default();
                    unsafe {
                        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
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
                }
            }
        });

        let thread_id = thread_id_rx.recv().unwrap_or(0);

        Ok((
            Self {
                thread_handle: Some(handle),
                thread_id,
            },
            rx,
        ))
    }

    /// Ultra-fast low-level keyboard callback.
    /// MUST complete in <0.01ms (10 microseconds) to strictly avoid Windows `LowLevelHooksTimeout` unhooking.
    /// Never performs heap allocations, locking, or blocking operations.
    unsafe extern "system" fn low_level_keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 && (w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize) {
            let kbd = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let vk = kbd.vkCode;

            // Direct bitwise async key state check (sub-microsecond)
            let is_down = |v: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| -> bool {
                (GetAsyncKeyState(v.0 as i32) as u16 & 0x8000) != 0
            };

            let ctrl = is_down(VK_CONTROL);
            let alt = is_down(VK_MENU);
            let shift = is_down(VK_SHIFT);
            let win = is_down(VK_LWIN) || is_down(VK_RWIN);

            // 1. Critical Fail-Safe Watchdog: Ctrl + Alt + Shift + Esc
            if ctrl && alt && shift && vk == VK_ESCAPE.0 as u32 {
                if let Some(sender) = HOTKEY_SENDER.get() {
                    let _ = sender.try_send(HotkeyAction::PanicFailSafe);
                }
                return LRESULT(1);
            }

            // 2. Command Bar: Super + Space
            if win && vk == VK_SPACE.0 as u32 {
                if let Some(sender) = HOTKEY_SENDER.get() {
                    let _ = sender.try_send(HotkeyAction::CommandBar);
                }
                return LRESULT(1);
            }

            // 3. Terminal: Super + Return
            if win && !shift && vk == VK_RETURN.0 as u32 {
                if let Some(sender) = HOTKEY_SENDER.get() {
                    let _ = sender.try_send(HotkeyAction::Terminal);
                }
                return LRESULT(1);
            }

            // 4. Close Window: Super + Q
            if win && vk == 'Q' as u32 {
                if let Some(sender) = HOTKEY_SENDER.get() {
                    let _ = sender.try_send(HotkeyAction::CloseWindow);
                }
                return LRESULT(1);
            }

            // 5. Workspaces 1-9: Super + [1-9] or Super + Shift + [1-9]
            if win && (0x31..=0x39).contains(&vk) {
                let ws_idx = (vk - 0x30) as u8;
                if let Some(sender) = HOTKEY_SENDER.get() {
                    if shift {
                        let _ = sender.try_send(HotkeyAction::MoveToWorkspace(ws_idx));
                    } else {
                        let _ = sender.try_send(HotkeyAction::FocusWorkspace(ws_idx));
                    }
                }
                return LRESULT(1);
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
