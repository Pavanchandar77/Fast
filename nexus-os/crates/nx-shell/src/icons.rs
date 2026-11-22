//! Real taskbar icons at 32px, the size the dock draws them. Stretching a
//! smaller bitmap up to a big dock tile is what made the icons blurry.

/// Load the icon Windows shows for `path` (an exe or a pinned shortcut).
/// Bytes are tightly packed BGRA. `None` when the shell has no icon.
pub fn app_icon_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(windows)]
    {
        let icon = load_icon(path)?;
        // 128px source, drawn at 32 logical px. A 32px bitmap gets stretched
        // by the display scale and looks soft.
        let image = hicon_to_bgra(icon, 128);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
        }
        image
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn load_icon(path: &str) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHDefExtractIconW, SHGetFileInfoW, SHGFI_ICON, SHFILEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::HICON;

    // A pinned shortcut's own icon includes the little arrow. Draw the
    // target program's icon instead.
    let source = if path.to_ascii_lowercase().ends_with(".lnk") {
        exe_path_from_lnk(path).unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(&source)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        // 32px is the dock's draw size. Extract at that size so DrawIconEx
        // does not stretch a 16px shell icon.
        let mut located = SHFILEINFOW::default();
        let found = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut located),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            windows::Win32::UI::Shell::SHGFI_ICONLOCATION,
        );
        if found != 0 {
            let end = located
                .szDisplayName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(located.szDisplayName.len());
            if end > 0 {
                let mut file: Vec<u16> = located.szDisplayName[..end].to_vec();
                file.push(0);
                let mut large = HICON(std::ptr::null_mut());
                if SHDefExtractIconW(
                    PCWSTR(file.as_ptr()),
                    located.iIcon,
                    0,
                    Some(&mut large),
                    None,
                    128,
                )
                .is_ok()
                    && !large.0.is_null()
                {
                    return Some(large);
                }
            }
        }
        let mut large = HICON(std::ptr::null_mut());
        if SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut large), None, 128).is_ok()
            && !large.0.is_null()
        {
            return Some(large);
        }
        let mut info = SHFILEINFOW::default();
        let got = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON,
        );
        if got != 0 && !info.hIcon.0.is_null() {
            return Some(info.hIcon);
        }
        None
    }
}

#[cfg(windows)]
fn exe_path_from_lnk(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut i = 0;
    while i + 8 <= bytes.len() {
        let is_exe = bytes[i] == b'.'
            && bytes[i + 1] == 0
            && bytes[i + 2].eq_ignore_ascii_case(&b'e')
            && bytes[i + 3] == 0
            && bytes[i + 4].eq_ignore_ascii_case(&b'x')
            && bytes[i + 5] == 0
            && bytes[i + 6].eq_ignore_ascii_case(&b'e')
            && bytes[i + 7] == 0;
        if is_exe {
            let mut start = i;
            while start >= 2 {
                let prev = u16::from_le_bytes([bytes[start - 2], bytes[start - 1]]);
                if prev == 0 || prev < 32 {
                    break;
                }
                start -= 2;
            }
            let mut chars = Vec::new();
            let mut k = start;
            while k + 1 < i + 8 {
                let ch = u16::from_le_bytes([bytes[k], bytes[k + 1]]);
                if ch == 0 {
                    break;
                }
                chars.push(ch);
                k += 2;
            }
            if let Ok(found) = String::from_utf16(&chars) {
                if std::path::Path::new(&found).is_file() {
                    return Some(found);
                }
            }
        }
        i += 2;
    }
    None
}

#[cfg(windows)]
fn hicon_to_bgra(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    size: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

    let size = size.clamp(16, 256);
    unsafe {
        let screen = GetDC(HWND::default());
        let mem = CreateCompatibleDC(screen);
        if mem.0.is_null() {
            ReleaseDC(HWND::default(), screen);
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(
            mem,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            windows::Win32::Foundation::HANDLE::default(),
            0,
        ) else {
            let _ = DeleteDC(mem);
            ReleaseDC(HWND::default(), screen);
            return None;
        };
        let old = SelectObject(mem, dib);
        let drawn = DrawIconEx(
            mem,
            0,
            0,
            icon,
            size as i32,
            size as i32,
            0,
            None,
            DI_NORMAL,
        )
        .is_ok();
        let mut buf = vec![0u8; (size * size * 4) as usize];
        let lines = if drawn {
            GetDIBits(
                mem,
                dib,
                0,
                size,
                Some(buf.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        SelectObject(mem, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), screen);
        if lines != size as i32 {
            return None;
        }
        let mut opaque = false;
        for px in buf.chunks_exact_mut(4) {
            if px[3] != 0 {
                opaque = true;
                break;
            }
        }
        if !opaque {
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        let colored = buf.chunks_exact(4).any(|px| px[0] | px[1] | px[2] != 0);
        colored.then_some((size, size, buf))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn explorer_icon_is_a_real_picture() {
        if !cfg!(windows) {
            return;
        }
        let Some((w, h, bytes)) = super::app_icon_rgba(r"C:\Windows\explorer.exe") else {
            panic!("explorer.exe has no icon");
        };
        assert_eq!((w, h), (128, 128), "dock icons are stored sharper than they are drawn");
        assert_eq!(bytes.len(), (w * h * 4) as usize);
        assert!(
            bytes.chunks_exact(4).any(|px| px[0] | px[1] | px[2] != 0),
            "icon was blank"
        );
    }
}
