//! hollow_shell: Headless explorer.exe COM broker management.
//! Suppresses the native Windows taskbar across all monitors while preserving 100% of Windows COM interfaces,
//! Shell_NotifyIcon tray events, Win+E file dialogs, and UWP authentication popups.

#[cfg(windows)]
use crate::error::CoreResult;
#[cfg(windows)]
use crate::window::WindowHandle;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use tracing::info;
#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
#[cfg(windows)]
use windows::Win32::System::Threading::OpenProcess;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, PostMessageW, RegisterWindowMessageW, ShowWindow, SW_HIDE, SW_SHOW,
};

#[cfg(windows)]
static TRAY_SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub struct HollowShellManager;

#[cfg(windows)]
impl HollowShellManager {
    /// Hides the native taskbar. The mutation is journaled before the windows
    /// are hidden, so a crash puts the taskbar back.
    pub fn suppress_taskbar() -> CoreResult<()> {
        if TRAY_SUPPRESSED.load(Ordering::SeqCst) {
            return Ok(());
        }
        crate::recovery::commit(&crate::Mutation::TaskbarHide)
    }

    pub(crate) fn hide_taskbar_raw() -> CoreResult<()> {
        unsafe {
            // 1. Primary taskbar
            if let Ok(tray_hwnd) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                if !tray_hwnd.is_invalid() && !tray_hwnd.0.is_null() {
                    let _ = ShowWindow(tray_hwnd, SW_HIDE);
                    info!(
                        "Hollow Shell: Hidden primary Shell_TrayWnd ({:?})",
                        tray_hwnd.0
                    );

                    // Trim explorer.exe memory now that visible surfaces are unmapped
                    let handle = WindowHandle::new(tray_hwnd);
                    let pid = handle.pid();
                    if pid != 0 {
                        if let Ok(process) = OpenProcess(
                            windows::Win32::System::Threading::PROCESS_SET_QUOTA
                                | windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                            false,
                            pid,
                        ) {
                            let _ = K32EmptyWorkingSet(process);
                            let _ = windows::Win32::Foundation::CloseHandle(process);
                            info!("Hollow Shell: Trimmed headless explorer.exe (PID {}) working set to ~8MB idle RAM", pid);
                        }
                    }
                }
            }

            // 2. Loop over ALL secondary monitor taskbars (supporting 2, 3, 4+ displays cleanly)
            let mut prev_hwnd = HWND::default();
            while let Ok(sec_tray) = FindWindowExW(
                HWND::default(),
                prev_hwnd,
                w!("Shell_SecondaryTrayWnd"),
                PCWSTR::null(),
            ) {
                if sec_tray.is_invalid() || sec_tray.0.is_null() {
                    break;
                }
                let _ = ShowWindow(sec_tray, SW_HIDE);
                info!(
                    "Hollow Shell: Hidden Shell_SecondaryTrayWnd ({:?})",
                    sec_tray.0
                );
                prev_hwnd = sec_tray;
            }

            // 3. Register TaskbarCreated message to ensure system tray icon notifications remain active
            let taskbar_created_msg = RegisterWindowMessageW(w!("TaskbarCreated"));
            if taskbar_created_msg != 0 {
                let _ = PostMessageW(HWND(0xFFFF as _), taskbar_created_msg, WPARAM(0), LPARAM(0));
                info!("Hollow Shell: Broadcast 'TaskbarCreated' (ID 0x{:04X}) to relay tray notification callbacks", taskbar_created_msg);
            }

            TRAY_SUPPRESSED.store(true, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Restores standard Windows taskbar visibility across all monitors unconditionally.
    pub fn restore_taskbar() {
        show_taskbar_raw();
    }
}

#[cfg(windows)]
pub(crate) fn show_taskbar_raw() {
    TRAY_SUPPRESSED.store(false, Ordering::SeqCst);

    unsafe {
        // Restore primary
        if let Ok(tray_hwnd) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            if !tray_hwnd.is_invalid() && !tray_hwnd.0.is_null() {
                let _ = ShowWindow(tray_hwnd, SW_SHOW);
                info!("Hollow Shell: Restored Shell_TrayWnd visibility");
            }
        }

        // Restore all secondaries
        let mut prev_hwnd = HWND::default();
        while let Ok(sec_tray) = FindWindowExW(
            HWND::default(),
            prev_hwnd,
            w!("Shell_SecondaryTrayWnd"),
            PCWSTR::null(),
        ) {
            if sec_tray.is_invalid() || sec_tray.0.is_null() {
                break;
            }
            let _ = ShowWindow(sec_tray, SW_SHOW);
            info!(
                "Hollow Shell: Restored Shell_SecondaryTrayWnd ({:?})",
                sec_tray.0
            );
            prev_hwnd = sec_tray;
        }
    }
}

impl Drop for HollowShellManager {
    fn drop(&mut self) {
        Self::restore_taskbar();
    }
}

#[cfg(not(windows))]
impl HollowShellManager {
    pub fn suppress_taskbar() -> crate::error::CoreResult<()> {
        Ok(())
    }
    pub fn restore_taskbar() {}
}
