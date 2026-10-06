//! System icons on Windows: the shell draws the item at a path (a
//! document, a folder, a program, a shortcut, or a packaged application by
//! its `shell:AppsFolder\<id>`) as Explorer shows it, icon only, through
//! `IShellItemImageFactory`; Pane reads the bitmap's pixels and keeps them
//! as a PNG.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::core::PCWSTR;

use super::{ICON_SIZE, SystemIcon, straight_rgba};

/// COM initialized on this thread while it is held, as the shell needs.
struct Com {
    /// Whether this guard initialized COM and must uninitialize it: not
    /// when the thread already had it in another mode.
    initialized: bool,
}

impl Com {
    fn new() -> Result<Com, String> {
        // SAFETY: no reserved pointer; paired with CoUninitialize in `drop`.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result == RPC_E_CHANGED_MODE {
            // Already initialized as multithreaded: usable as it is.
            return Ok(Com { initialized: false });
        }
        result
            .ok()
            .map_err(|error| format!("cannot start COM: {error}"))?;
        Ok(Com { initialized: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: paired with the successful CoInitializeEx in `new`.
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) fn icon(path: &Path) -> Result<SystemIcon, String> {
    let _com = Com::new()?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let failed = |error: windows::core::Error| format!("no icon for {}: {error}", path.display());
    let side = ICON_SIZE as i32;
    // SAFETY: plain COM calls on the interface the shell returns, on a
    // thread with COM initialized for their lifetime; `wide` outlives the
    // call reading it.
    let bitmap = unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(failed)?;
        factory
            .GetImage(
                SIZE { cx: side, cy: side },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .map_err(failed)?
    };
    // SAFETY: the bitmap is the shell's, given to this caller, and deleted
    // once read.
    let read = unsafe { pixels(bitmap) };
    // SAFETY: as above; nothing uses it after.
    unsafe {
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
    }
    let (width, height, bgra) = read?;
    let rgba = straight_rgba(&bgra);
    crate::icons::encode_png(width, height, &rgba)
        .map(SystemIcon::Png)
        .ok_or_else(|| format!("no icon for {}: its pixels make no image", path.display()))
}

/// The width, height and top-down 32-bit BGRA pixels of `bitmap`.
///
/// # Safety
///
/// `bitmap` must be a valid bitmap handle.
unsafe fn pixels(bitmap: HBITMAP) -> Result<(u32, u32, Vec<u8>), String> {
    let mut info = BITMAP::default();
    // SAFETY: `info` is a BITMAP of the size given.
    let filled = unsafe {
        GetObjectW(
            HGDIOBJ::from(bitmap),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut info as *mut BITMAP).cast::<c_void>()),
        )
    };
    let (width, height) = (info.bmWidth, info.bmHeight.abs());
    if filled == 0 || width <= 0 || height <= 0 {
        return Err("the shell drew no bitmap".into());
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: rows top-down.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: a memory DC of the screen's, deleted below; `bgra` holds
    // every row GetDIBits writes at 32 bits a pixel.
    let lines = unsafe {
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast::<c_void>()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        lines
    };
    if lines <= 0 {
        return Err("the shell's bitmap could not be read".into());
    }
    Ok((width as u32, height as u32, bgra))
}
