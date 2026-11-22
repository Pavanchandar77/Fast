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

/// Drop the Windows caption and accent border so Nexus draws its own rounded
/// chrome and clicks land in the client area.
///
/// Extending the DWM frame (`DwmExtendFrameIntoClientArea` with -1 margins)
/// sheets the accent-colored caption over the whole window. On a short island
/// that is a flat blue bar. On every other window it is the blue focus outline,
/// and the non-client band eats the clicks meant for the buttons inside.
pub fn present_client_window(hwnd: isize) -> crate::Result<()> {
    if hwnd == 0 {
        return Err(crate::Error::Gpu("null hwnd".to_string()));
    }
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWINDOWATTRIBUTE,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_BORDER, WS_CAPTION, WS_CLIPCHILDREN, WS_DLGFRAME,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    let hwnd_obj = HWND(hwnd as *mut c_void);
    unsafe {
        // Borderless, but keep the system menu and the min/max boxes.
        // Stripping WS_SYSMENU makes SW_MINIMIZE, SW_MAXIMIZE, and WM_CLOSE
        // no-ops. That is the same reason a popup with no system menu cannot
        // be closed from its own buttons. Win32 borderless apps (and the
        // Windhawk genie mod) leave those bits on and only remove the caption.
        let style = GetWindowLongW(hwnd_obj, GWL_STYLE) as u32;
        let strip = WS_CAPTION.0 | WS_THICKFRAME.0 | WS_BORDER.0 | WS_DLGFRAME.0;
        let keep = WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0 | WS_CLIPCHILDREN.0;
        let next = (style & !strip) | keep;
        if next != style {
            SetWindowLongW(hwnd_obj, GWL_STYLE, next as i32);
        }
        // DWMWA_COLOR_NONE. Stops the accent color painting the border and caption.
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWINDOWATTRIBUTE(34),
            std::ptr::from_ref(&none).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWINDOWATTRIBUTE(35),
            std::ptr::from_ref(&none).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        // DWMWCP_DONOTROUND. Windows' own rounded frame is the blue ring.
        let corners = DWM_WINDOW_CORNER_PREFERENCE(1);
        let _ = DwmSetWindowAttribute(
            hwnd_obj,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corners).cast(),
            std::mem::size_of_val(&corners) as u32,
        );
        let _ = SetWindowPos(
            hwnd_obj,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
    // No frame extension. The client stays the client, so GPUI receives the click.
    nx_compat::dispatch::safe_apply_dwm_attributes(hwnd, 1, true, false, false)
        .map_err(|err| crate::Error::Gpu(format!("safe_apply_dwm_attributes: {err}")))?;
    paint_transparent(hwnd_obj);
    Ok(())
}

/// Ask DWM for a per-pixel transparent client. Without this, the rounded
/// window's padding composites as a solid dark rectangle.
fn paint_transparent(hwnd: windows::Win32::Foundation::HWND) {
    #[repr(C)]
    struct AccentPolicy {
        accent_state: u32,
        accent_flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }
    #[repr(C)]
    struct Data {
        attrib: u32,
        pv_data: *mut std::ffi::c_void,
        cb_data: usize,
    }
    unsafe {
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::w;
        let Some(user32) = LoadLibraryW(w!("user32.dll")).ok() else {
            return;
        };
        type SetFn =
            unsafe extern "system" fn(windows::Win32::Foundation::HWND, *mut Data) -> BOOL;
        let Some(sym) = GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute"))
        else {
            return;
        };
        let set: SetFn = std::mem::transmute(sym);
        // ACCENT_ENABLE_TRANSPARENTGRADIENT. Alpha 0 so the corners stay clear.
        let accent = AccentPolicy {
            accent_state: 2,
            accent_flags: 2,
            gradient_color: 0,
            animation_id: 0,
        };
        let mut data = Data {
            attrib: 0x13,
            pv_data: std::ptr::from_ref(&accent) as *mut std::ffi::c_void,
            cb_data: std::mem::size_of::<AccentPolicy>(),
        };
        let _ = set(hwnd, &mut data);
    }
}
