//! The clipboard on Windows: a clipboard format listener
//! (`AddClipboardFormatListener`) on a message-only window of a thread of
//! Pane's own (`threads::windows::MessageThread`), which receives
//! `WM_CLIPBOARDUPDATE` after every change of the clipboard, whichever
//! application made it. No permission is needed.
//!
//! On each change the thread, and only it, opens the clipboard and reads, in
//! this order: the formats applications use to say that a clipboard monitor
//! or clipboard history must not keep what they copied
//! (`ExcludeClipboardContentFromMonitorProcessing`, the older `Clipboard
//! Viewer Ignore`, `CanIncludeInClipboardHistory` and
//! `CanUploadToCloudClipboard` as a DWORD of 0); then, only if none of them
//! forbids it, what was copied: the files File Explorer copied (`CF_HDROP`,
//! every path), else the text (`CF_UNICODETEXT`), else an image — the
//! application's own PNG (the registered `PNG` format, as browsers and
//! Office put it), else the bitmap (`CF_DIBV5`, `CF_DIB`, 24 or 32 bits a
//! pixel) made into a PNG (#167); and the full path of the program of the
//! process whose window owns the clipboard. What is kept is decided by
//! `clipboard::accept`, the same on every system. A change is read once: if
//! reading it fails (another program holds the clipboard open), the thread
//! tries again shortly, a few times, before giving up on that change.
//!
//! For a paste into the application that was in front before Pane (#253),
//! the module also keeps what the clipboard holds — every format held in
//! ordinary memory — and puts it back afterwards, tagged as a concealed
//! copy is, so no history keeps the restore. What is held as a graphics
//! handle (`CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`, `CF_PENDATA`,
//! `CF_ENHMETAFILE`, and any private format whose data is not clipboard
//! memory) cannot be copied as bytes and is lost; a format the application
//! renders only when asked has it render, and keeps what it answers; and
//! nothing past 64 MiB is kept.
//!
//! Reading can wait on the program that copied (a program that renders its
//! data only when asked), however long it takes. Dropping the watch never
//! waits on it for more than a moment: it tells the thread to stop and
//! leaves it to end on its own after [`STOP_WAIT`]; Pane closed the sink's
//! fence before, so what such a late read returns is dropped. A panic while
//! handling a change is caught in the window procedure and logged, and the
//! listener goes on listening.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ::windows::Win32::Foundation::{
    CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use ::windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use ::windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use ::windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetWindowThreadProcessId, KillTimer, SetTimer, WM_CLIPBOARDUPDATE, WM_TIMER,
};
use ::windows::core::{PCWSTR, PWSTR, w};

use super::ignoring::IGNORED;
use super::{
    ClipboardSystem, Content, CopiedImage, MAX_FILES, MAX_IMAGE_BYTES, MAX_IMAGE_PIXELS,
    MAX_TEXT_BYTES, Markers, Observation, Sink, Watch,
};
use crate::system::{CONCEALED_MARKERS, Clip};
use crate::threads::windows::{MessageThread, Window, WindowClass, stop_sent};

/// `CF_UNICODETEXT`: text as UTF-16, ending with a NUL.
const CF_UNICODETEXT: u32 = 13;

/// `CF_HDROP`: the files copied in File Explorer, as a `DROPFILES` header
/// and a list of paths.
const CF_HDROP: u32 = 15;

/// `CF_DIB`: a bitmap, as a `BITMAPINFOHEADER` and its pixels.
const CF_DIB: u32 = 8;

/// `CF_DIBV5`: a bitmap, as a `BITMAPV5HEADER` (with its colour masks) and
/// its pixels.
const CF_DIBV5: u32 = 17;

/// The largest bitmap header Pane reads (`BITMAPV5HEADER`), with room for
/// the three colour masks an older header's bit fields follow it with.
const DIB_HEADER_BYTES: usize = 124 + 12;

/// The registered format that tells File Explorer what pasting files does,
/// and its value for copying them (`DROPEFFECT_COPY`) rather than moving.
const PREFERRED_DROP_EFFECT: (&str, u32) = ("Preferred DropEffect", 1);

/// The listener's window, which receives the clipboard's changes.
static LISTENER_CLASS: WindowClass = WindowClass::new("PaneClipboardListener", listener_procedure);
/// The window that owns what Pane puts on the clipboard.
static WRITER_CLASS: WindowClass = WindowClass::new("PaneClipboardWriter", plain_procedure);

/// How often, and how long apart, Pane tries to open the clipboard while
/// another application holds it open.
const OPEN_TRIES: u32 = 10;
const OPEN_WAIT: Duration = Duration::from_millis(20);

/// The timer that has the listener read a change again after reading it
/// failed; how long after, and how often at most.
const RETRY_TIMER: usize = 1;
const RETRY_WAIT_MS: u32 = 250;
const READ_TRIES: u32 = 5;

/// How long dropping the watch waits for the listener to end.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(1);

/// The system's clipboard on Windows.
pub struct WindowsClipboard;

impl ClipboardSystem for WindowsClipboard {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String> {
        let thread = MessageThread::spawn(
            "pane-clipboard",
            move || start_listening(sink),
            // The listener gets no thread messages but its stop.
            |_, _| {},
            stop_listening,
        )
        .map_err(|problem| format!("Pane could not watch the clipboard: {problem}"))?;
        Ok(Watch::new(Listening(thread)))
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        let owner = WRITER_CLASS.message_window()?;
        write(&owner, text, &[])
        // The clipboard is closed, then the window destroyed: what was put
        // on the clipboard stays there.
    }

    fn write_image(&self, png: &[u8]) -> Result<(), String> {
        let owner = WRITER_CLASS.message_window()?;
        write_image(&owner, png)
    }

    fn write_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        let owner = WRITER_CLASS.message_window()?;
        write_formats(
            &owner,
            &[(CF_HDROP, drop_list(paths))],
            &[PREFERRED_DROP_EFFECT],
        )
    }
}

/// Puts the image `png` on the clipboard, owned by `owner`, a window of
/// this thread: as the PNG itself (the registered `PNG` format, which
/// browsers and Office read) and as a bitmap (`CF_DIB`, 32 bits a pixel,
/// from which Windows makes the other bitmap formats), for every other
/// application.
fn write_image(owner: &Window, png: &[u8]) -> Result<(), String> {
    let mut formats = Vec::new();
    if let Some(dib) = png_to_dib(png) {
        formats.push((CF_DIB, dib));
    }
    // SAFETY: a valid NUL-terminated wide string.
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if png_format != 0 {
        formats.push((png_format, png.to_vec()));
    }
    if formats.is_empty() {
        return Err("the kept image could not be read".into());
    }
    write_formats(owner, &formats, &[])
}

/// The listener thread, until dropped.
struct Listening(MessageThread);

impl Drop for Listening {
    fn drop(&mut self) {
        if !self.0.stop(Some(STOP_WAIT)) {
            log(
                "Pane stopped watching the clipboard while a read was still waiting on the program that copied; it ends on its own",
            );
        }
    }
}

/// What the listener's window procedure uses, on the listener thread.
struct Listener {
    sink: Arc<dyn Sink>,
    formats: Formats,
    /// The clipboard's sequence number when it was last read in full (or
    /// given up on): a change reported twice is read once.
    read_through: u32,
    /// How many times reading the latest change failed.
    failures: u32,
}

thread_local! {
    static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) };
}

/// Writes `message` to standard error, if there is one, and to Pane's log
/// (see `crate::diagnostics`); never what was copied. It cannot panic.
fn log(message: &str) {
    crate::diagnostics::report_line(message);
}

/// The registered formats that carry an application's markers.
#[derive(Clone, Copy)]
struct Formats {
    exclude: u32,
    viewer_ignore: u32,
    history: u32,
    cloud: u32,
    /// `PNG`, the format browsers and Office put a copied image in.
    png: u32,
}

impl Formats {
    fn register() -> Formats {
        // SAFETY: each name is a valid NUL-terminated wide string.
        unsafe {
            Formats {
                exclude: RegisterClipboardFormatW(w!(
                    "ExcludeClipboardContentFromMonitorProcessing"
                )),
                viewer_ignore: RegisterClipboardFormatW(w!("Clipboard Viewer Ignore")),
                history: RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
                png: RegisterClipboardFormatW(w!("PNG")),
            }
        }
    }
}

/// On the listener thread: makes its window and starts listening, reporting
/// to `sink`.
fn start_listening(sink: Arc<dyn Sink>) -> Result<(Window, Option<HWND>), String> {
    let window = LISTENER_CLASS.message_window()?;
    // SAFETY: `window` is this thread's own window.
    unsafe { AddClipboardFormatListener(window.handle()) }.map_err(|error| error.message())?;
    LISTENER.with(|listener| {
        *listener.borrow_mut() = Some(Listener {
            sink,
            formats: Formats::register(),
            // SAFETY: no arguments.
            read_through: unsafe { GetClipboardSequenceNumber() },
            failures: 0,
        });
    });
    let handle = window.handle();
    Ok((window, Some(handle)))
}

/// On the listener thread, as it ends: stops listening and lets go of the
/// sink; the window is destroyed as it is dropped.
fn stop_listening(window: Window) {
    // SAFETY: `window` is this thread's own window.
    let _ = unsafe { RemoveClipboardFormatListener(window.handle()) };
    LISTENER.with(|listener| listener.borrow_mut().take());
}

/// Reads the clipboard's latest change, unless it was read already, and
/// reports it; if reading fails, tries again later.
fn observe(window: HWND, listener: &mut Listener) {
    // SAFETY: no arguments.
    let changed_to = unsafe { GetClipboardSequenceNumber() };
    if changed_to == listener.read_through {
        return;
    }
    // A simulated copy's window is armed (#262): the change it carries is
    // the target's copy, which is not Pane's and carries no marker it
    // could be given, or the restore that follows. It is skipped as a
    // marked copy is, so no history keeps it; the read that armed the
    // window is still running, and the next change is read as usual.
    if IGNORED.is_ignoring(crate::util::now_ms()) {
        listener.read_through = changed_to;
        return;
    }
    let ticket = listener.sink.reading();
    match read(window, listener.formats) {
        Ok((sequence, observation)) => {
            listener.read_through = sequence;
            listener.failures = 0;
            listener.sink.observed(ticket, observation);
        }
        Err(problem) => {
            listener.failures += 1;
            if listener.failures < READ_TRIES {
                // SAFETY: this thread's own window; the timer is killed
                // when it fires.
                unsafe { SetTimer(Some(window), RETRY_TIMER, RETRY_WAIT_MS, None) };
                log(&format!(
                    "Pane could not read the clipboard, and tries again: {problem}"
                ));
            } else {
                // SAFETY: no arguments.
                listener.read_through = unsafe { GetClipboardSequenceNumber() };
                listener.failures = 0;
                log(&format!(
                    "Pane could not read the clipboard, and skips this change: {problem}"
                ));
            }
        }
    }
}

extern "system" fn listener_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A panic must not unwind into Windows, which would end Pane.
    std::panic::catch_unwind(|| handle(window, message, wparam, lparam)).unwrap_or_else(|_| {
        log("Pane's clipboard listener failed on a change; it goes on listening");
        LRESULT(0)
    })
}

fn handle(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if stop_sent(message) {
        return LRESULT(0);
    }
    let changed = match message {
        WM_CLIPBOARDUPDATE => true,
        WM_TIMER if wparam.0 == RETRY_TIMER => {
            // SAFETY: this thread's own window and timer.
            let _ = unsafe { KillTimer(Some(window), RETRY_TIMER) };
            true
        }
        _ => false,
    };
    if !changed {
        // SAFETY: the arguments are those this procedure was called with.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    LISTENER.with(|listener| {
        // Not borrowed already: nothing a read waits on calls back into it.
        if let Ok(mut listener) = listener.try_borrow_mut()
            && let Some(listener) = listener.as_mut()
        {
            observe(window, listener);
        }
    });
    LRESULT(0)
}

extern "system" fn plain_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the arguments are those this procedure was called with.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The clipboard, opened by a window of this thread until dropped.
struct OpenedClipboard;

impl OpenedClipboard {
    /// Opens the clipboard for `owner`, a window of this thread, trying
    /// again for a moment while another application holds it open.
    fn by(owner: HWND) -> Result<OpenedClipboard, String> {
        let mut last = String::new();
        for attempt in 0..OPEN_TRIES {
            if attempt > 0 {
                std::thread::sleep(OPEN_WAIT);
            }
            // SAFETY: `owner` is a window of this thread.
            match unsafe { OpenClipboard(Some(owner)) } {
                Ok(()) => return Ok(OpenedClipboard),
                Err(error) => last = error.message(),
            }
        }
        Err(format!(
            "another application keeps the clipboard open ({last})"
        ))
    }
}

impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        // SAFETY: the clipboard is open by this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Whether `format` is on the clipboard.
fn available(format: u32) -> bool {
    // SAFETY: a plain value.
    format != 0 && unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// The bytes of `format`'s global memory on the open clipboard, at most
/// `limit` of them, if it has any.
fn bytes(format: u32, limit: usize) -> Option<Vec<u8>> {
    if !available(format) {
        return None;
    }
    // SAFETY: the clipboard is open; the handle stays the clipboard's.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let memory = HGLOBAL(handle.0);
    // SAFETY: a clipboard format with global memory, locked while read and
    // read within its size.
    unsafe {
        let size = GlobalSize(memory).min(limit);
        let data = GlobalLock(memory);
        if data.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(memory);
        Some(bytes)
    }
}

/// A DWORD format as a yes (not 0) or no (0), if it is on the clipboard.
fn flag(format: u32) -> Option<bool> {
    let bytes = bytes(format, 4)?;
    let value: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(value) != 0)
}

/// Opens the clipboard and reads its markers, then what was copied only if
/// they allow it, and its owner; with the sequence number of what it read.
fn read(window: HWND, formats: Formats) -> Result<(u32, Observation), String> {
    let _open = OpenedClipboard::by(window)?;
    // SAFETY: no arguments. While the clipboard is open, nothing changes it.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let markers = Markers {
        exclude_from_monitoring: available(formats.exclude) || available(formats.viewer_ignore),
        include_in_history: flag(formats.history),
        upload_to_cloud: flag(formats.cloud),
    };
    let content = if !markers.allow() {
        Content::Withheld
    } else {
        copied_files()
            .or_else(copied_text)
            .or_else(|| copied_image(formats.png))
            .unwrap_or(Content::Other)
    };
    let observation = Observation {
        content,
        markers,
        source: owner_program(),
    };
    Ok((sequence, observation))
}

/// The text on the open clipboard (`CF_UNICODETEXT`), if it has any. One
/// unit more than Pane keeps is read, which is enough to know it is too
/// long: UTF-8 never has fewer bytes than UTF-16 has units.
fn copied_text() -> Option<Content> {
    let bytes = bytes(CF_UNICODETEXT, (MAX_TEXT_BYTES + 1) * 2)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    Some(Content::Text(String::from_utf16_lossy(&units)))
}

/// The files File Explorer copied onto the open clipboard (`CF_HDROP`),
/// every one in order, if it holds any; more than [`MAX_FILES`] are not
/// read.
fn copied_files() -> Option<Content> {
    if !available(CF_HDROP) {
        return None;
    }
    // SAFETY: the clipboard is open; the handle stays the clipboard's.
    let handle = unsafe { GetClipboardData(CF_HDROP) }.ok()?;
    let files = HDROP(handle.0);
    // SAFETY: `files` is the clipboard's drop list; the index 0xFFFFFFFF
    // asks how many files it lists, passing no buffer.
    let count = unsafe { DragQueryFileW(files, u32::MAX, None) } as usize;
    if count == 0 {
        return None;
    }
    if count > MAX_FILES {
        return Some(Content::TooLarge);
    }
    let mut paths = Vec::with_capacity(count);
    for index in 0..count as u32 {
        // SAFETY: as above; asking for a length passes no buffer.
        let length = unsafe { DragQueryFileW(files, index, None) } as usize;
        if length == 0 {
            continue;
        }
        let mut name = vec![0u16; length + 1];
        // SAFETY: `name` is writable for its length, NUL included.
        let copied = unsafe { DragQueryFileW(files, index, Some(&mut name)) } as usize;
        paths.push(PathBuf::from(OsString::from_wide(
            &name[..copied.min(length)],
        )));
    }
    (!paths.is_empty()).then_some(Content::Files(paths))
}

/// The image on the open clipboard, if it holds one: the application's own
/// PNG (`png_format`, the registered `PNG`) as it is, else its bitmap
/// (`CF_DIBV5`, else `CF_DIB`) made into a PNG. A PNG larger than Pane
/// keeps, or a bitmap of more than [`MAX_IMAGE_PIXELS`], is too large, and
/// a bitmap's pixels are then not read; a bitmap Pane cannot read
/// (compressed, or of fewer than 24 bits a pixel) is no image it keeps.
fn copied_image(png_format: u32) -> Option<Content> {
    if let Some(png) = bytes(png_format, MAX_IMAGE_BYTES + 1) {
        if png.len() > MAX_IMAGE_BYTES {
            return Some(Content::TooLarge);
        }
        if let Some(image) = CopiedImage::from_png(png) {
            return Some(Content::Image(image));
        }
    }
    let format = [CF_DIBV5, CF_DIB]
        .into_iter()
        .find(|format| available(*format))?;
    let header = bytes(format, DIB_HEADER_BYTES)?;
    let layout = DibLayout::of(&header)?;
    if u64::from(layout.width) * u64::from(layout.height) > MAX_IMAGE_PIXELS {
        return Some(Content::TooLarge);
    }
    let dib = bytes(format, layout.size())?;
    let (width, height, rgba) = dib_pixels(&dib)?;
    let png = crate::icons::encode_png(width, height, &rgba)?;
    if png.len() > MAX_IMAGE_BYTES {
        return Some(Content::TooLarge);
    }
    CopiedImage::from_png(png).map(Content::Image)
}

/// How a bitmap (`CF_DIB`, `CF_DIBV5`) is laid out, as its header says:
/// what Pane reads of it is 24 or 32 bits a pixel, uncompressed or with
/// bit fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DibLayout {
    width: u32,
    height: u32,
    /// Whether its first row is the top one (a negative height).
    top_down: bool,
    bits: u16,
    /// The red, green, blue and alpha masks of a 32-bit bitmap with bit
    /// fields (alpha 0 where none is given); `None` for `BI_RGB`, whose
    /// pixels are blue, green, red and a byte that may be alpha.
    masks: Option<[u32; 4]>,
    /// Where its pixels begin.
    offset: usize,
}

impl DibLayout {
    /// The layout `dib`'s header (at least) states, if Pane reads it.
    fn of(dib: &[u8]) -> Option<DibLayout> {
        let u32_at = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?))
        };
        let header = u32_at(0)? as usize;
        // A `BITMAPCOREHEADER` (12 bytes) is not read.
        if header < 40 {
            return None;
        }
        // LONGs: a negative height says the rows run top down.
        let width = u32_at(4)? as i32;
        let height = u32_at(8)? as i32;
        let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
        let compression = u32_at(16)?;
        let colors = u32_at(32)? as usize;
        if width <= 0 || height == 0 || !matches!(bits, 24 | 32) {
            return None;
        }
        // BI_RGB 0, BI_BITFIELDS 3, BI_ALPHABITFIELDS 6.
        let (masks, after) = match compression {
            0 => (None, header),
            3 | 6 if bits == 32 => {
                let alpha = if header >= 56 { u32_at(52)? } else { 0 };
                let masks = [u32_at(40)?, u32_at(44)?, u32_at(48)?, alpha];
                // An older header's masks follow it.
                let after = if header >= 52 { header } else { header + 12 };
                (Some(masks), after)
            }
            _ => return None,
        };
        Some(DibLayout {
            width: width as u32,
            height: height.unsigned_abs(),
            top_down: height < 0,
            bits,
            masks,
            // A colour table, if one is given, comes before the pixels.
            offset: after + colors * 4,
        })
    }

    /// How many bytes one row of pixels takes, padded to 4.
    fn stride(&self) -> usize {
        (self.width as usize * usize::from(self.bits)).div_ceil(32) * 4
    }

    /// How many bytes the whole bitmap takes.
    fn size(&self) -> usize {
        self.offset + self.stride() * self.height as usize
    }
}

/// The pixels of the bitmap `dib` as straight RGBA, top row first, with its
/// width and height; `None` if Pane does not read it or it is cut short. A
/// 32-bit bitmap whose every alpha is 0 (the byte `BI_RGB` leaves unused)
/// is opaque.
fn dib_pixels(dib: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let layout = DibLayout::of(dib)?;
    if dib.len() < layout.size() {
        return None;
    }
    let (width, height) = (layout.width as usize, layout.height as usize);
    let stride = layout.stride();
    let mut rgba = Vec::with_capacity(width * height * 4);
    let mut any_alpha = false;
    for row in 0..height {
        let from = if layout.top_down {
            row
        } else {
            height - 1 - row
        };
        let line = &dib[layout.offset + from * stride..][..stride];
        for x in 0..width {
            let pixel = match layout.bits {
                24 => {
                    let p = &line[x * 3..x * 3 + 3];
                    [p[2], p[1], p[0], 255]
                }
                _ => {
                    let value = u32::from_le_bytes(line[x * 4..x * 4 + 4].try_into().ok()?);
                    let [red, green, blue, alpha] = match layout.masks {
                        None => [16, 8, 0, 24].map(|shift| (value >> shift) as u8),
                        Some(masks) => masks.map(|mask| channel(value, mask)),
                    };
                    any_alpha |= alpha != 0;
                    [red, green, blue, alpha]
                }
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    if layout.bits == 32 && !any_alpha {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
    Some((layout.width, layout.height, rgba))
}

/// The 8-bit value of the channel `mask` selects in `value`: its bits,
/// scaled to 0–255; 0 for no mask.
fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    let raw = u64::from((value & mask) >> shift);
    if bits >= 8 {
        (raw >> (bits - 8)) as u8
    } else {
        ((raw * 255) / ((1u64 << bits) - 1)) as u8
    }
}

/// The PNG `png` as a bitmap for `CF_DIB`: a `BITMAPINFOHEADER` (32 bits a
/// pixel, `BI_RGB`, bottom row first), then its pixels as blue, green, red
/// and alpha; `None` if it cannot be decoded.
fn png_to_dib(png: &[u8]) -> Option<Vec<u8>> {
    let (width, height, rgba) = crate::icons::decode_png(png)?;
    let row = width as usize * 4;
    let mut dib = Vec::with_capacity(40 + rgba.len());
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&i32::try_from(width).ok()?.to_le_bytes());
    dib.extend_from_slice(&i32::try_from(height).ok()?.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&u32::try_from(rgba.len()).ok()?.to_le_bytes());
    // Resolution and colour table: none.
    dib.extend_from_slice(&[0; 16]);
    for line in rgba.chunks_exact(row).rev() {
        for pixel in line.as_chunks::<4>().0 {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Some(dib)
}

/// The full path of the program of the process whose window owns the
/// clipboard, if the system says: its file name decides an exclusion, and
/// the path lets the history show the program's icon.
fn owner_program() -> Option<String> {
    // SAFETY: no arguments.
    let owner = unsafe { GetClipboardOwner() }.ok()?;
    if owner.is_invalid() {
        return None;
    }
    let mut process = 0u32;
    // SAFETY: `owner` is a window handle; `process` is writable.
    unsafe { GetWindowThreadProcessId(owner, Some(&raw mut process)) };
    if process == 0 {
        return None;
    }
    // SAFETY: plain values; the handle is closed below.
    let handle: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut name = [0u16; 1024];
    let mut length = name.len() as u32;
    // SAFETY: `name` is writable for `length` units.
    let queried = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()),
            &mut length,
        )
    };
    // SAFETY: opened above.
    let _ = unsafe { CloseHandle(handle) };
    queried.ok()?;
    let path = String::from_utf16_lossy(&name[..length as usize]);
    (!path.trim().is_empty()).then_some(path)
}

/// Puts `text` on the clipboard, replacing what was there, with each of
/// `markers` (a registered format's name and its DWORD value), owned by
/// `owner`, a window of this thread.
fn write(owner: &Window, text: &str, markers: &[(&str, u32)]) -> Result<(), String> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    let text: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    write_formats(owner, &[(CF_UNICODETEXT, text)], markers)
}

/// Puts each of `formats` (a format and its bytes) on the clipboard,
/// replacing what was there, with each of `markers` as [`write`] does,
/// owned by `owner`, a window of this thread.
fn write_formats(
    owner: &Window,
    formats: &[(u32, Vec<u8>)],
    markers: &[(&str, u32)],
) -> Result<(), String> {
    let _open = OpenedClipboard::by(owner.handle())?;
    // SAFETY: the clipboard is open by this thread; emptying it makes
    // `owner` its owner.
    unsafe { EmptyClipboard() }.map_err(|error| error.message())?;
    for (format, bytes) in formats {
        put(*format, bytes)?;
    }
    for (name, value) in markers {
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        wide.push(0);
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let format = unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) };
        put(format, &value.to_le_bytes())?;
    }
    Ok(())
}

/// Puts `bytes` on the open, emptied clipboard as `format`.
fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    // SAFETY: a new moveable block of at least one byte, written within its
    // size while locked; the clipboard owns it once set, and it is freed
    // here otherwise.
    unsafe {
        let memory =
            GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|error| error.message())?;
        let data = GlobalLock(memory);
        if data.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err("could not lock the clipboard's memory".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(error.message());
        }
    }
    Ok(())
}

/// Puts `clip` on the clipboard for a command (`crate::system`): text, or a
/// file as File Explorer copies one, with the markers of a concealed copy
/// when `concealed`. Owned by a window of the calling thread, destroyed
/// once the clipboard is closed: what was put stays there.
pub(crate) fn put_clip(clip: &Clip, concealed: bool) -> Result<(), String> {
    let owner = WRITER_CLASS.message_window()?;
    let mut markers: Vec<(&str, u32)> = if concealed {
        CONCEALED_MARKERS.to_vec()
    } else {
        Vec::new()
    };
    match clip {
        Clip::Text(text) => write(&owner, text, &markers),
        Clip::File(path) => {
            markers.push(PREFERRED_DROP_EFFECT);
            write_formats(&owner, &[(CF_HDROP, drop_files(path))], &markers)
        }
    }
}

/// What the clipboard holds, saved to be put back after a paste into the
/// application that was in front before Pane (#253): every format it held
/// in ordinary memory — text, rich text, HTML, a bitmap, a list of files
/// — each as its bytes. The markers of a concealed copy are not kept:
/// the restore adds them itself ([`restore_clip`]).
pub(crate) struct Held {
    /// Each format and its bytes, in the order the clipboard listed them.
    formats: Vec<(u32, Vec<u8>)>,
}

/// How much of what the clipboard holds a paste keeps to put back: what
/// it held beyond this is lost, as a format it could not read is.
const MAX_HELD_BYTES: usize = 64 * 1024 * 1024;

/// The formats whose data is a graphics handle, not memory Pane can copy
/// as bytes: `CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`, `CF_PENDATA`
/// and `CF_ENHMETAFILE`. A private format of another kind that is a
/// handle is caught by its size, which no clipboard memory has.
const HANDLE_FORMATS: [u32; 5] = [2, 3, 9, 10, 14];

/// Keeps what the clipboard holds, to put back with [`restore_clip`]
/// after a paste. Owned by a window of the calling thread, which is
/// destroyed once read; the clipboard is left as it is. Reading a format
/// the application that copied renders only when asked has it render —
/// and keeps what it answers; a format held as a graphics handle cannot
/// be copied as bytes and is lost, and so is anything past
/// [`MAX_HELD_BYTES`].
pub(crate) fn hold_clip() -> Result<Held, String> {
    let owner = WRITER_CLASS.message_window()?;
    let _open = OpenedClipboard::by(owner.handle())?;
    let skips = marked_formats();
    let mut listed = Vec::new();
    // SAFETY: the clipboard is open; 0 asks for the first format, and each
    // answer for the next.
    let mut format = unsafe { EnumClipboardFormats(0) };
    while format != 0 {
        // SAFETY: as above, with the format the enumeration answered.
        let next = unsafe { EnumClipboardFormats(format) };
        listed.push(format);
        format = next;
    }
    let mut formats = Vec::new();
    let mut held = 0;
    for format in listed {
        if HANDLE_FORMATS.contains(&format) || skips.contains(&format) {
            continue;
        }
        // SAFETY: the clipboard is open; the handle stays the clipboard's.
        if let Ok(handle) = unsafe { GetClipboardData(format) } {
            let memory = HGLOBAL(handle.0);
            // SAFETY: a clipboard format's memory, read within its size
            // while locked; a handle that is not memory has no size.
            let size = unsafe { GlobalSize(memory) };
            if size > 0 && held + size <= MAX_HELD_BYTES {
                // SAFETY: as above, locked while copied from.
                let data = unsafe { GlobalLock(memory) };
                if !data.is_null() {
                    // SAFETY: `size` bytes of writable memory, locked above
                    // while read within it.
                    let bytes =
                        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec();
                    // SAFETY: as above.
                    let _ = unsafe { GlobalUnlock(memory) };
                    if !bytes.is_empty() {
                        held += size;
                        formats.push((format, bytes));
                    }
                }
            }
        }
    }
    Ok(Held { formats })
}

/// Puts back what [`hold_clip`] kept, replacing what the paste put
/// there, with the markers of a concealed copy: neither Pane's history
/// nor Windows' own keeps the restore. A clipboard that held nothing is
/// left as the paste left it, not emptied. Owned by a window of the
/// calling thread, destroyed once the clipboard is closed: what was put
/// stays there.
pub(crate) fn restore_clip(held: &Held) -> Result<(), String> {
    if held.formats.is_empty() {
        return Ok(());
    }
    let owner = WRITER_CLASS.message_window()?;
    write_formats(&owner, &held.formats, &CONCEALED_MARKERS)
}

/// The registered formats that say a copy must not be kept, which a hold
/// skips: the restore carries them itself.
fn marked_formats() -> [u32; 4] {
    // SAFETY: valid NUL-terminated names.
    unsafe {
        [
            RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
            RegisterClipboardFormatW(w!("Clipboard Viewer Ignore")),
            RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
            RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
        ]
    }
}

/// `path` as `CF_HDROP` carries it (see [`drop_list`]).
fn drop_files(path: &Path) -> Vec<u8> {
    drop_list(&[path])
}

/// `paths` as `CF_HDROP` carries them: a `DROPFILES` header (the offset of
/// the list, a point, whether it is in the non-client area, and that the
/// paths are UTF-16), then each path with its NUL, and the list's closing
/// NUL.
fn drop_list(paths: &[impl AsRef<Path>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    // pFiles: the list follows the 20-byte header.
    bytes.extend_from_slice(&20u32.to_le_bytes());
    // pt.x, pt.y, fNC.
    for _ in 0..3 {
        bytes.extend_from_slice(&0i32.to_le_bytes());
    }
    // fWide.
    bytes.extend_from_slice(&1i32.to_le_bytes());
    for path in paths {
        for unit in path.as_ref().as_os_str().encode_wide() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// What the clipboard holds for a command (`crate::system`): the first file
/// copied in File Explorer, else its text, else nothing. Text longer than
/// `limit` bytes of UTF-8 is refused rather than read whole.
pub(crate) fn read_clip(limit: usize) -> Result<Option<Clip>, String> {
    let owner = WRITER_CLASS.message_window()?;
    let _open = OpenedClipboard::by(owner.handle())?;
    if available(CF_HDROP)
        // SAFETY: the clipboard is open; the handle stays the clipboard's.
        && let Ok(handle) = unsafe { GetClipboardData(CF_HDROP) }
    {
        let files = HDROP(handle.0);
        // SAFETY: `files` is the clipboard's drop list; asking for a
        // length passes no buffer.
        let length = unsafe { DragQueryFileW(files, 0, None) } as usize;
        if length > 0 {
            let mut name = vec![0u16; length + 1];
            // SAFETY: `name` is writable for its length, NUL included.
            let copied = unsafe { DragQueryFileW(files, 0, Some(&mut name)) } as usize;
            let path = OsString::from_wide(&name[..copied.min(length)]);
            return Ok(Some(Clip::File(PathBuf::from(path))));
        }
    }
    let Some(bytes) = bytes(CF_UNICODETEXT, (limit + 1) * 2) else {
        return Ok(None);
    };
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    let text = String::from_utf16_lossy(&units);
    if text.len() > limit {
        return Err(format!(
            "The clipboard holds more text than Pane reads for a command ({} MiB)",
            limit / (1024 * 1024)
        ));
    }
    Ok(Some(Clip::Text(text)))
}

/// For the Windows adapter's test: putting text with markers on the
/// clipboard, as a password manager does, owned by a window of this
/// process. It replaces what was on the clipboard, which is not saved.
#[doc(hidden)]
pub mod testing {
    use super::{CF_DIB, WRITER_CLASS, Window, write, write_formats};

    /// The window of this process that owns what the test put on the
    /// clipboard, until dropped (by the thread that put it).
    pub struct Owner(#[allow(dead_code)] Window);

    /// Puts `text` on the clipboard with each of `markers`, a registered
    /// format's name and its DWORD value, owned by a window of this process
    /// while the returned owner is kept.
    pub fn set_text(text: &str, markers: &[(&str, u32)]) -> Result<Owner, String> {
        let owner = WRITER_CLASS.message_window()?;
        write(&owner, text, markers)?;
        Ok(Owner(owner))
    }

    /// Puts the bitmap `dib` (a `BITMAPINFOHEADER` and its pixels) on the
    /// clipboard as `CF_DIB` alone, as Paint copies a picture, owned by a
    /// window of this process while the returned owner is kept.
    pub fn set_bitmap(dib: &[u8]) -> Result<Owner, String> {
        let owner = WRITER_CLASS.message_window()?;
        write_formats(&owner, &[(CF_DIB, dib.to_vec())], &[])?;
        Ok(Owner(owner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_file_is_a_drop_list_of_one_wide_path() {
        let bytes = drop_files(Path::new(r"C:\Notes\a.txt"));
        assert_eq!(
            &bytes[..4],
            &20u32.to_le_bytes(),
            "the list after the header"
        );
        assert_eq!(&bytes[16..20], &1i32.to_le_bytes(), "wide paths");
        let units: Vec<u16> = bytes[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let path: Vec<u16> = r"C:\Notes\a.txt".encode_utf16().collect();
        assert_eq!(&units[..path.len()], path.as_slice());
        assert_eq!(
            &units[path.len()..],
            &[0, 0],
            "the path's NUL, then the list's"
        );
    }

    #[test]
    fn copied_files_are_a_drop_list_of_every_path() {
        let bytes = drop_list(&[Path::new(r"C:\a"), Path::new(r"C:\bc")]);
        let units: Vec<u16> = bytes[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let expected: Vec<u16> = "C:\\a\0C:\\bc\0\0".encode_utf16().collect();
        assert_eq!(units, expected);
    }

    /// A 2×2 bitmap of 24 bits a pixel, bottom row first, each row padded
    /// to 4 bytes: red, green over blue, white.
    fn bitmap_24() -> Vec<u8> {
        let mut dib = Vec::new();
        for value in [40u32, 2, 2] {
            dib.extend_from_slice(&value.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        // Bottom row: blue, white (BGR), then 2 bytes of padding.
        dib.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]);
        // Top row: red, green.
        dib.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
        dib
    }

    #[test]
    fn a_bitmap_is_read_top_row_first_as_straight_rgba() {
        let dib = bitmap_24();
        let layout = DibLayout::of(&dib).unwrap();
        assert_eq!(
            (layout.width, layout.height, layout.top_down),
            (2, 2, false)
        );
        assert_eq!(layout.stride(), 8);
        assert_eq!(layout.size(), dib.len());
        let (width, height, rgba) = dib_pixels(&dib).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(
            rgba,
            [
                255, 0, 0, 255, 0, 255, 0, 255, // red, green
                0, 0, 255, 255, 255, 255, 255, 255, // blue, white
            ]
        );
        // Cut short, or compressed: not read.
        assert_eq!(dib_pixels(&dib[..dib.len() - 1]), None);
        let mut compressed = dib.clone();
        compressed[16] = 1;
        assert_eq!(DibLayout::of(&compressed), None);
    }

    #[test]
    fn a_32_bit_bitmap_keeps_its_alpha_or_is_opaque_without_one() {
        // 1×1, top down, BI_RGB: blue-green-red-alpha.
        let header = |alpha: u8| {
            let mut dib = Vec::new();
            dib.extend_from_slice(&40u32.to_le_bytes());
            dib.extend_from_slice(&1i32.to_le_bytes());
            dib.extend_from_slice(&(-1i32).to_le_bytes());
            dib.extend_from_slice(&1u16.to_le_bytes());
            dib.extend_from_slice(&32u16.to_le_bytes());
            dib.extend_from_slice(&[0; 24]);
            dib.extend_from_slice(&[10, 20, 30, alpha]);
            dib
        };
        assert_eq!(dib_pixels(&header(128)).unwrap().2, [30, 20, 10, 128]);
        assert_eq!(dib_pixels(&header(0)).unwrap().2, [30, 20, 10, 255]);
        // Bit fields: the masks say where each channel is.
        assert_eq!(channel(0x00ff_0000, 0x00ff_0000), 255);
        assert_eq!(channel(0b11111 << 10, 0b11111 << 10), 255);
        assert_eq!(channel(0, 0), 0);
    }

    #[test]
    fn a_png_is_put_back_as_a_bitmap_too() {
        let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
        let png = crate::icons::encode_png(2, 1, &rgba).unwrap();
        let dib = png_to_dib(&png).unwrap();
        let (width, height, pixels) = dib_pixels(&dib).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, rgba);
        assert_eq!(png_to_dib(b"not a png"), None);
    }
}
