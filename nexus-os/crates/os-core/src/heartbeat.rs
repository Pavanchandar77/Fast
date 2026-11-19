//! Named manual-reset event the shell pulses from its main loop.
//!
//! The watchdog waits on `Local\NexusOsHeartbeat`. A silent event means the
//! main thread is no longer running the frame loop.

use std::sync::OnceLock;
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, ResetEvent, SetEvent, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

use crate::error::CoreResult;

/// How long the watchdog waits after the last pulse before it treats the shell as hung.
pub const STALE_AFTER: Duration = Duration::from_secs(15);

const NAME: windows::core::PCWSTR = w!("Local\\NexusOsHeartbeat");

static HEARTBEAT: OnceLock<isize> = OnceLock::new();

fn bits_to_handle(bits: isize) -> HANDLE {
    HANDLE(bits as *mut std::ffi::c_void)
}

/// Create the event and pulse once so a watchdog that is already waiting sees us.
pub fn start() -> CoreResult<()> {
    let handle = unsafe { CreateEventW(None, true, false, NAME)? };
    unsafe {
        ResetEvent(handle)?;
        SetEvent(handle)?;
    }
    let _ = HEARTBEAT.set(handle.0 as isize);
    Ok(())
}

/// Signal the watchdog. Call this from the main thread's frame loop only.
pub fn pulse() {
    let Some(bits) = HEARTBEAT.get().copied() else {
        return;
    };
    let _ = unsafe { SetEvent(bits_to_handle(bits)) };
}

/// Watchdog side. The handle is closed on drop.
pub struct Heartbeat {
    handle: HANDLE,
}

impl Heartbeat {
    pub fn open_existing() -> CoreResult<Self> {
        let handle = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                false,
                NAME,
            )?
        };
        Ok(Self { handle })
    }

    pub fn raw(&self) -> HANDLE {
        self.handle
    }

    pub fn reset(&self) -> CoreResult<()> {
        unsafe { ResetEvent(self.handle)? };
        Ok(())
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}
