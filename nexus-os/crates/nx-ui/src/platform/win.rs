//! Win32 DWM Mica / Acrylic hooks.
//!
//! `DwmSetWindowAttribute` is applied with values from [`BackdropRequest`], so a
//! crash or process exit drops the backdrop with the window. Nothing here is
//! written to a journal because DWM attributes do not outlive the HWND.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWM_SYSTEMBACKDROP_TYPE,
    DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::UI::Controls::MARGINS;

use super::BackdropRequest;

/// Apply a system backdrop, immersive dark mode, rounded corners, and a
/// frame extension so Mica/Acrylic show through the client area.
///
/// `hwnd` is the raw Win32 window handle. `0` is rejected.
pub fn apply_hwnd(hwnd: isize, request: BackdropRequest) -> crate::Result<()> {
    if hwnd == 0 {
        return Err(crate::Error::Gpu("null hwnd".to_string()));
    }
    // Safety: the caller owns `hwnd` for the duration of these calls. Each
    // attribute pointer lives on this stack and is not retained by DWM.
    unsafe { apply_inner(HWND(hwnd as *mut core::ffi::c_void), request) }
}

unsafe fn apply_inner(hwnd: HWND, request: BackdropRequest) -> crate::Result<()> {
    let backdrop = DWM_SYSTEMBACKDROP_TYPE(request.system_backdrop_value());
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_SYSTEMBACKDROP_TYPE,
        std::ptr::from_ref(&backdrop).cast(),
        std::mem::size_of_val(&backdrop) as u32,
    )
    .map_err(|err| crate::Error::Gpu(format!("DWMWA_SYSTEMBACKDROP_TYPE: {err}")))?;

    let dark = request.dark_mode_value();
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        std::ptr::from_ref(&dark).cast(),
        std::mem::size_of_val(&dark) as u32,
    )
    .map_err(|err| crate::Error::Gpu(format!("DWMWA_USE_IMMERSIVE_DARK_MODE: {err}")))?;

    let corners = DWM_WINDOW_CORNER_PREFERENCE(request.corner_value());
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        std::ptr::from_ref(&corners).cast(),
        std::mem::size_of_val(&corners) as u32,
    )
    .map_err(|err| crate::Error::Gpu(format!("DWMWA_WINDOW_CORNER_PREFERENCE: {err}")))?;

    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    DwmExtendFrameIntoClientArea(hwnd, &margins)
        .map_err(|err| crate::Error::Gpu(format!("DwmExtendFrameIntoClientArea: {err}")))?;
    Ok(())
}
