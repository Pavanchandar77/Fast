//! Shell thumbnails via `IThumbnailCache` (the same cache Explorer uses, so
//! thumbnails Explorer already generated are free).

use super::{wide, win_err, ComGuard};
use crate::error::{FsError, FsResult};
use crate::preview::{Thumbnail, ThumbnailProvider};
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    ISharedBitmap, IShellItem, IThumbnailCache, LocalThumbnailCache, SHCreateItemFromParsingName,
    WTS_EXTRACT, WTS_SCALETOREQUESTEDSIZE,
};

/// `IThumbnailCache`-backed provider.
#[derive(Debug, Default)]
pub struct ShellThumbnails;

impl ThumbnailProvider for ShellThumbnails {
    fn thumbnail(&self, path: &Path, size: u32) -> FsResult<Option<Thumbnail>> {
        let _com = ComGuard::init();
        let w = wide(path);
        // SAFETY: COM initialised; every pointer passed is valid for the call;
        // the HBITMAP is owned by the ISharedBitmap which outlives its use.
        unsafe {
            let cache: IThumbnailCache =
                CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| win_err(path, &e))?;
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)
                .map_err(|e| win_err(path, &e))?;
            let mut shared: Option<ISharedBitmap> = None;
            if cache
                .GetThumbnail(
                    &item,
                    size,
                    WTS_EXTRACT | WTS_SCALETOREQUESTEDSIZE,
                    Some(&mut shared),
                    None,
                    None,
                )
                .is_err()
            {
                // No thumbnail handler for this type.
                return Ok(None);
            }
            let Some(shared) = shared else {
                return Ok(None);
            };
            let hbmp = shared.GetSharedBitmap().map_err(|e| win_err(path, &e))?;
            let mut bm = BITMAP::default();
            let got = GetObjectW(
                HGDIOBJ(hbmp.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut bm as *mut BITMAP).cast()),
            );
            if got == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
                return Ok(None);
            }
            let (wpx, hpx) = (bm.bmWidth as u32, bm.bmHeight.unsigned_abs());
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: wpx as i32,
                    biHeight: -(hpx as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut px = vec![0u8; (wpx as usize) * (hpx as usize) * 4];
            let dc = GetDC(HWND::default());
            let lines = GetDIBits(
                dc,
                hbmp,
                0,
                hpx,
                Some(px.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            ReleaseDC(HWND::default(), dc);
            if lines == 0 {
                return Err(FsError::Os {
                    op: "GetDIBits",
                    code: 0,
                    path: path.to_path_buf(),
                });
            }
            for p in px.as_chunks_mut::<4>().0 {
                p.swap(0, 2); // BGRA -> RGBA
            }
            Ok(Some(Thumbnail {
                width: wpx,
                height: hpx,
                rgba: px,
            }))
        }
    }
}
