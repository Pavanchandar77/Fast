use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use tracing::{error, info};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
    WM_SYSKEYDOWN,
};

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
static KEYBINDINGS: parking_lot::RwLock<Option<nx_wm::KeybindingMap>> =
    parking_lot::RwLock::new(None);

pub struct GlobalKeyboardHook {
    thread_handle: Option<JoinHandle<()>>,
    thread_id: u32,
}

impl GlobalKeyboardHook {
    pub fn start() -> anyhow::Result<(Self, Receiver<HotkeyAction>)> {
        Self::start_with_config(&os_config::schema::KeybindingsConfig::default())
    }

    pub fn start_with_config(
        config: &os_config::schema::KeybindingsConfig,
    ) -> anyhow::Result<(Self, Receiver<HotkeyAction>)> {
        let map = nx_wm::KeybindingMap::load_from_config(&config.mod_key, &config.bindings)
            .unwrap_or_default();
        *KEYBINDINGS.write() = Some(map);

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

    pub fn update_keybindings(config: &os_config::schema::KeybindingsConfig) {
        if let Ok(map) = nx_wm::KeybindingMap::load_from_config(&config.mod_key, &config.bindings) {
            *KEYBINDINGS.write() = Some(map);
            info!("Hotkeys reloaded from configuration");
        }
    }

    /// Global unhook function called on panic, exit, or fail-safe
    pub fn unhook_global() {
        let mut h_guard = HOOK_HANDLE.lock();
        if let Some(h) = h_guard.take() {
            let _ = unsafe { UnhookWindowsHookEx(h.0) };
        }
        IS_RUNNING.store(false, Ordering::SeqCst);
    }

    /// Ultra-fast low-level keyboard callback.
    /// MUST complete in <0.01ms (10 microseconds) to strictly avoid Windows `LowLevelHooksTimeout` unhooking.
    /// Never performs heap allocations or blocking operations.
    unsafe extern "system" fn low_level_keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 && (w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize)
        {
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
                os_core::hollow_shell::HollowShellManager::restore_taskbar();
                crate::gpu::top_bar::TopBar::unregister_global();
                Self::unhook_global();
                std::process::exit(0);
            }

            // 2. Config-driven keybinding lookup with passthrough for Windows reserved shortcuts
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
                    let hotkey_action = match action {
                        nx_wm::Action::PanicFailSafe => HotkeyAction::PanicFailSafe,
                        nx_wm::Action::CommandBar => HotkeyAction::CommandBar,
                        nx_wm::Action::Terminal => HotkeyAction::Terminal,
                        nx_wm::Action::CloseWindow => HotkeyAction::CloseWindow,
                        nx_wm::Action::ToggleFullscreen => HotkeyAction::ToggleFullscreen,
                        nx_wm::Action::ToggleFloating => HotkeyAction::ToggleFloating,
                        nx_wm::Action::FocusWorkspace(ws) => HotkeyAction::FocusWorkspace(ws),
                        nx_wm::Action::MoveToWorkspace(ws) => HotkeyAction::MoveToWorkspace(ws),
                        nx_wm::Action::Execute(cmd) => HotkeyAction::Custom(format!("exec:{cmd}")),
                        nx_wm::Action::Custom(s) => HotkeyAction::Custom(s),
                        other => HotkeyAction::Custom(format!("{other:?}")),
                    };

                    if let Some(sender) = HOTKEY_SENDER.get() {
                        let _ = sender.try_send(hotkey_action);
                    }

                    // Win-key-up masking: cancels Windows Start Menu popup when Super was part of the combination
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
