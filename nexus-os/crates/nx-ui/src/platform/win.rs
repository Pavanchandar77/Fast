//! Win32 DWM Mica / Acrylic hooks routed through `nx-compat` capability probing.
//!
//! `DwmSetWindowAttribute` is applied with probed fallbacks from [`BackdropRequest`], so a
//! crash or process exit drops the backdrop with the window. Nothing here is
//! written to a journal because DWM attributes do not outlive the HWND.

use super::BackdropRequest;

/// Apply a system backdrop, immersive dark mode, rounded corners, and a
/// frame extension so Mica/Acrylic show through the client area.
///
/// `hwnd` is the raw Win32 window handle. `0` is rejected.
pub fn apply_hwnd(hwnd: isize, request: BackdropRequest) -> crate::Result<()> {
    if hwnd == 0 {
        return Err(crate::Error::Gpu("null hwnd".to_string()));
    }

    nx_compat::dispatch::safe_apply_dwm_attributes(
        hwnd,
        request.system_backdrop_value() as u32,
        request.dark_mode_value() != 0,
        request.corner_value() != 0,
        true,
    )
    .map_err(|err| crate::Error::Gpu(format!("safe_apply_dwm_attributes: {err}")))
}
