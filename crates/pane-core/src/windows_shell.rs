//! What Pane's Windows code shares to talk to the shell, each defined once:
//! COM on the calling thread ([`Com`]), and the shell's image of an item
//! read as pixels ([`shell_image`], [`bitmap_pixels`]). System icons
//! (`system_icons`), application icons (`applications::icons`), the Start
//! menu's shortcuts, the system's file actions and elevated programs use
//! them.

use std::ffi::{OsStr, c_void};

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

use crate::system_icons::{ICON_SIZE, straight_rgba};
use crate::util::wide;

/// COM initialized on this thread, single-threaded, for as long as it is
/// held: the shell needs it to draw an item, read or open a shortcut,
/// enumerate the Apps folder, move a file to the Recycle Bin or ask for
/// elevation.
pub(crate) struct Com {
    /// Whether this guard initialized COM and must uninitialize it: not
    /// when the thread already had it in another mode.
    initialized: bool,
}

impl Com {
    /// COM on this thread; a thread that already has it as multithreaded
    /// keeps it as it is, usable.
    pub(crate) fn new() -> Result<Com, String> {
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

/// An image the system drew: straight RGBA, row by row, top down.
pub(crate) struct Pixels {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// The shell's image of the item `name` names (a path, or a parsing name
/// such as `shell:AppsFolder\<id>`), as Explorer shows it, icon only, at
/// [`ICON_SIZE`] pixels or larger. The calling thread must hold [`Com`].
pub(crate) fn shell_image(name: impl AsRef<OsStr>) -> Result<Pixels, String> {
    let name = name.as_ref();
    let wide_name = wide(name);
    let failed = |error: windows::core::Error| {
        format!("the shell has no image of {}: {error}", name.display())
    };
    let side = ICON_SIZE as i32;
    // SAFETY: plain COM calls on the interface the shell returns, on a
    // thread with COM initialized for their lifetime; `wide_name` outlives
    // the call reading it.
    let bitmap = unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide_name.as_ptr()), None).map_err(failed)?;
        factory
            .GetImage(
                SIZE { cx: side, cy: side },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .map_err(failed)?
    };
    // SAFETY: the bitmap is the shell's, given to this caller, and deleted
    // once read.
    let read = unsafe { bitmap_pixels(bitmap) };
    // SAFETY: as above; nothing uses it after.
    unsafe {
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
    }
    read
}

/// The width, height and straight RGBA pixels of `bitmap`.
///
/// # Safety
///
/// `bitmap` must be a valid bitmap handle.
pub(crate) unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Result<Pixels, String> {
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
        return Err("the system drew no bitmap".into());
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
        return Err("the system's bitmap could not be read".into());
    }
    Ok(Pixels {
        width: width as u32,
        height: height as u32,
        rgba: straight_rgba(&bgra),
    })
}
