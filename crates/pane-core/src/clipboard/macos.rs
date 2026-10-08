//! The clipboard on macOS: the general pasteboard (`NSPasteboard`), whose
//! `changeCount` a thread of Pane's own watches by looking at it every
//! [`POLL`] and reading the pasteboard only when the count moved. That is
//! the established mechanism for a program that owns no run loop: the
//! pasteboard posts `NSPasteboardDidChangeNotification`, but it is
//! delivered through a run loop, which Pane's own threads do not run (the
//! Windows listener has a message queue of its own and the Linux watcher
//! an X11 connection to wait on; polling the count needs neither). Each
//! look costs one integer read, and a copy is reported within [`POLL`] of
//! being made. No permission is needed.
//!
//! On each change the thread, and only it, reads the pasteboard: first the
//! markers, then, only if they allow it, what was copied: the files Finder
//! copied (every item's `public.file-url`), else the text
//! (`NSPasteboardTypeString`), else an image (`public.png` as it is, else
//! `public.tiff` made into a PNG; #167). The de-facto marker of
//! [nspasteboard.org]'s convention, `org.nspasteboard.ConcealedType`,
//! which password managers such as
//! 1Password and Strongbox set when they copy a secret, is honored: its
//! presence alone withholds the text. macOS has no formats answering
//! Windows' history and cloud questions, so those stay unset. The
//! pasteboard never names the program that copied — the server knows the
//! process, but no API of Pane's asks for it — so the source is always
//! unknown and an excluded program, matched by name, never keeps anything
//! out (as the contract says of an unknown owner). What is kept is
//! decided by `clipboard::accept`, the same on every system.
//!
//! [`ClipboardSystem::write_text`] puts the text in the pasteboard
//! server, which holds it for whoever pastes, as any program's copy does,
//! beyond the watch and Pane's own life: no window and no answering
//! thread of Pane's is needed, unlike X11's selection, and a write is a
//! change like any other, so the watcher reports it too.
//!
//! Dropping the watch tells the thread to stop and waits at most
//! [`STOP_WAIT`] for it. Nothing the thread does waits on the program
//! that copied (the pasteboard server holds what it serves), so a stop is
//! noticed within one poll and the thread ends with it; a read still
//! going when the wait passes is left to end on its own, as on Windows
//! and Linux, with Pane's fence already closed so what it reports then is
//! dropped. A panic while reading one change is caught and that report is
//! lost, and the thread goes on listening.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use std::path::PathBuf;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString, NSURL};

use super::{
    ClipboardSystem, Content, CopiedImage, MAX_FILES, MAX_IMAGE_BYTES, MAX_IMAGE_PIXELS, Markers,
    Observation, Sink, Watch,
};
use crate::system::Clip;
use crate::threads::Joinable;

// Links AppKit, whose constants the adapter reads: the bindings crate
// links nothing (objc2 links only Foundation), and without this the
// pasteboard's extern constants would not resolve in Pane's own tests,
// which link no GPUI that links AppKit for them.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// The type the nspasteboard.org convention names for a copy a password
/// manager made: `org.nspasteboard.ConcealedType`, whose presence alone
/// says the copy must not be kept. 1Password, Strongbox and others set it.
const CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";

/// The pasteboard type of a file URL, which Finder puts there when it
/// copies a file: `public.file-url` (`NSPasteboardTypeFileURL`).
const FILE_URL_TYPE: &str = "public.file-url";

/// The pasteboard type of a PNG image (`NSPasteboardTypePNG`).
const PNG_TYPE: &str = "public.png";

/// The pasteboard type of a TIFF image (`NSPasteboardTypeTIFF`), which
/// most of macOS' own applications copy an image as.
const TIFF_TYPE: &str = "public.tiff";

/// How often the watcher looks at the pasteboard's change count, and so
/// the usual delay before a copy is reported: one integer read per look,
/// with the pasteboard read only when the count moved. Provisional (#37),
/// pending the user's decision.
const POLL: Duration = Duration::from_millis(250);

/// Ownership can change before the owner supplies its declared file URL.
/// Give it a bounded time to finish, while still noticing a newer copy or
/// a stopped watch at every poll.
const FILE_READY_WAIT: Duration = Duration::from_secs(2);

/// How long dropping the watch waits for the watcher to end.
const STOP_WAIT: Duration = Duration::from_secs(1);

/// Writes `message` to standard error, if there is one; never what was
/// copied. Unlike `eprintln!`, it cannot panic.
fn log(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// The general pasteboard on macOS.
pub struct MacosClipboard;

impl ClipboardSystem for MacosClipboard {
    fn unavailable(&self) -> Option<String> {
        // No permission and no session kind is refused: reading and
        // writing the pasteboard needs none on the baseline this was
        // written on, macOS 15. Newer systems' pasteboard privacy prompts
        // are untested (docs/clipboard-history.md, Per platform).
        None
    }

    fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String> {
        Watcher::start(sink)
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        put_text(text, false)
    }

    fn write_image(&self, png: &[u8]) -> Result<(), String> {
        put_image(png)
    }

    fn write_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        put_files(paths)
    }
}

/// Puts the image `png` on the pasteboard, replacing what was there: as the
/// PNG itself, and as a TIFF beside it for the applications that read only
/// that.
fn put_image(png: &[u8]) -> Result<(), String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let data = NSData::with_bytes(png);
        if !pasteboard.setData_forType(Some(&data), &NSString::from_str(PNG_TYPE)) {
            return Err("the pasteboard would not take the image".into());
        }
        if let Some(tiff) =
            NSBitmapImageRep::imageRepWithData(&data).and_then(|image| image.TIFFRepresentation())
        {
            let _ = pasteboard.setData_forType(Some(&tiff), &NSString::from_str(TIFF_TYPE));
        }
        Ok(())
    })
}

/// Puts the files `paths` on the pasteboard, replacing what was there, as
/// Finder copies them: one item per file, each its file URL.
fn put_files(paths: &[PathBuf]) -> Result<(), String> {
    autoreleasepool(|_| {
        let kind = NSString::from_str(FILE_URL_TYPE);
        let mut items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = Vec::new();
        for path in paths {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let Some(address) = url.absoluteString() else {
                return Err(format!("{} has no file URL", path.display()));
            };
            let item = NSPasteboardItem::new();
            if !item.setString_forType(&address, &kind) {
                return Err(format!(
                    "{} could not be put on the pasteboard",
                    path.display()
                ));
            }
            items.push(ProtocolObject::from_retained(item));
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if !pasteboard.writeObjects(&NSArray::from_retained_slice(&items)) {
            return Err("the pasteboard would not take the files".into());
        }
        Ok(())
    })
}

/// Puts `text` on the pasteboard, replacing what was there, with the
/// concealed type beside it if `conceal`, as a password manager does. The
/// pasteboard server keeps what is put, as it keeps any program's copy.
fn put_text(text: &str, conceal: bool) -> Result<(), String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let string = NSString::from_str(text);
        // SAFETY: the type is AppKit's static constant for text.
        if !pasteboard.setString_forType(&string, unsafe { NSPasteboardTypeString }) {
            return Err("the pasteboard would not take the text".into());
        }
        if conceal {
            let empty = NSData::with_bytes(&[]);
            let concealed = NSString::from_str(CONCEALED_TYPE);
            if !pasteboard.setData_forType(Some(&empty), &concealed) {
                return Err("the pasteboard would not take the concealed type".into());
            }
        }
        Ok(())
    })
}

/// Puts `clip` on the pasteboard for a command (`crate::system`): text, or a
/// file as Finder copies one (its file URL), with the concealed type
/// beside it if `concealed`.
pub(crate) fn put_clip(clip: &Clip, concealed: bool) -> Result<(), String> {
    match clip {
        Clip::Text(text) => put_text(text, concealed),
        Clip::File(path) => autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let Some(address) = url.absoluteString() else {
                return Err(format!("{} has no file URL", path.display()));
            };
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            if !pasteboard.setString_forType(&address, &NSString::from_str(FILE_URL_TYPE)) {
                return Err("the pasteboard would not take the file".into());
            }
            if concealed {
                let empty = NSData::with_bytes(&[]);
                let concealed = NSString::from_str(CONCEALED_TYPE);
                if !pasteboard.setData_forType(Some(&empty), &concealed) {
                    return Err("the pasteboard would not take the concealed type".into());
                }
            }
            Ok(())
        }),
    }
}

/// What the pasteboard holds for a command (`crate::system`): the file
/// Finder copied (the first, when several were), else its text, else
/// nothing. Text longer than `limit` bytes of UTF-8 is refused.
pub(crate) fn read_clip(limit: usize) -> Result<Option<Clip>, String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        if type_names().iter().any(|kind| kind == FILE_URL_TYPE)
            && let Some(address) = pasteboard.stringForType(&NSString::from_str(FILE_URL_TYPE))
            && let Some(url) = NSURL::URLWithString(&address)
            && url.isFileURL()
            && let Some(path) = url.path()
        {
            return Ok(Some(Clip::File(PathBuf::from(path.to_string()))));
        }
        match text() {
            Some(text) if text.len() > limit => Err(format!(
                "The clipboard holds more text than Pane reads for a command ({} MiB)",
                limit / (1024 * 1024)
            )),
            Some(text) => Ok(Some(Clip::Text(text))),
            None => Ok(None),
        }
    })
}

/// Watching the pasteboard, until dropped: tells the watcher thread to
/// stop and waits at most [`STOP_WAIT`] for it. No waker is needed: the
/// thread sleeps at most [`POLL`] between looks and asks nothing of the
/// program that copied, so it ends on its own within the wait; a read
/// still going past it is left to end, and logged.
struct Listening {
    stopping: Arc<AtomicBool>,
    thread: Option<Joinable>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take()
            && !thread.join_within(STOP_WAIT)
        {
            log(
                "Pane stopped watching the clipboard while a read was still going; it ends on its own",
            );
        }
    }
}

/// The watcher thread, reporting each change of the pasteboard to its
/// sink.
struct Watcher {
    sink: Arc<dyn Sink>,
    stopping: Arc<AtomicBool>,
    /// The pasteboard's change count when it was last read (or skipped):
    /// a change is read once, and what is on the pasteboard when the
    /// watch starts is not read at all.
    read_through: isize,
}

impl Watcher {
    /// Watches the pasteboard, reporting each later change of it to
    /// `sink` until the returned watch is dropped.
    fn start(sink: Arc<dyn Sink>) -> Result<Watch, String> {
        let stopping = Arc::new(AtomicBool::new(false));
        let watcher = autoreleasepool(|_| Watcher {
            sink,
            stopping: stopping.clone(),
            read_through: NSPasteboard::generalPasteboard().changeCount(),
        });
        let thread = Joinable::spawn("pane-clipboard", move || watcher.run())
            .map_err(|problem| format!("Pane could not watch the clipboard: {problem}"))?;
        Ok(Watch::new(Listening {
            stopping,
            thread: Some(thread),
        }))
    }

    /// Reports each change of the pasteboard until the watch is dropped.
    /// A change made while one was being read (the count moved again) is
    /// reported on the next look, with whatever the pasteboard holds then.
    fn run(mut self) {
        while !self.stopping.load(Ordering::SeqCst) {
            std::thread::sleep(POLL);
            let count = autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount());
            if count != self.read_through {
                // A panic while reading one change must not end the thread:
                // it goes on listening, and that report is lost.
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| self.observe(count)));
            }
        }
    }

    /// Reads the change that `count` names and reports it, taking the
    /// ticket only once the read begins.
    fn observe(&mut self, count: isize) {
        self.read_through = count;
        let ticket = self.sink.reading();
        let deadline = Instant::now() + FILE_READY_WAIT;
        loop {
            if self.stopping.load(Ordering::SeqCst)
                || autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount()) != count
            {
                return;
            }
            let (observation, waiting_for_file) = autoreleasepool(|_| observation());
            // A newer owner invalidates the whole reading, including its
            // markers. Its contents are read on the next poll instead.
            if autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount()) != count {
                return;
            }
            if !waiting_for_file || Instant::now() >= deadline {
                self.sink.observed(ticket, observation);
                return;
            }
            // Keep the first ticket: a Clear History while data is being
            // supplied must still fence this copy out of the history.
            std::thread::sleep(POLL);
        }
    }
}

/// What is on the pasteboard, read once: the markers first, then, only if
/// they allow it, the files, else the text, else an image. The pasteboard
/// does not name the program that copied, so the source is unknown. The
/// second value says a declared file's data (or the new owner's types)
/// is not ready yet. AppKit's change count tracks ownership, not later
/// data writes: https://developer.apple.com/documentation/appkit/nspasteboard/changecount
fn observation() -> (Observation, bool) {
    let types = type_names();
    let markers = markers_of(&types);
    let mut waiting_for_file = false;
    let content = if markers.allow() {
        let files = files(&types);
        waiting_for_file =
            types.is_empty() || (types.iter().any(|kind| kind == FILE_URL_TYPE) && files.is_none());
        files
            .or_else(|| text().map(Content::Text))
            .or_else(|| image(&types))
            .unwrap_or(Content::Other)
    } else {
        Content::Withheld
    };
    (
        Observation {
            content,
            markers,
            source: None,
        },
        waiting_for_file,
    )
}

/// The names of the pasteboard's types, as it lists them for its current
/// contents.
fn type_names() -> Vec<String> {
    autoreleasepool(|_| {
        NSPasteboard::generalPasteboard()
            .types()
            .map_or(Vec::new(), |types| {
                (0..types.count())
                    .map(|index| types.objectAtIndex(index).to_string())
                    .collect()
            })
    })
}

/// The text on the pasteboard, as the first item that has any serves it;
/// `None` when the pasteboard holds no text (an image or files alone).
/// The text is read whole, the pasteboard offering no shorter read, so
/// its length is decided after it, as `clipboard::accept` does.
fn text() -> Option<String> {
    autoreleasepool(|_| {
        NSPasteboard::generalPasteboard()
            // SAFETY: the type is AppKit's static constant for text.
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|string| string.to_string())
    })
}

/// The files on the pasteboard, as Finder copies them (each item's file
/// URL), when `types` says it holds any; more than [`MAX_FILES`] are not
/// read.
fn files(types: &[String]) -> Option<Content> {
    if !types.iter().any(|kind| kind == FILE_URL_TYPE) {
        return None;
    }
    autoreleasepool(|_| {
        let items = NSPasteboard::generalPasteboard().pasteboardItems()?;
        if items.count() > MAX_FILES {
            return Some(Content::TooLarge);
        }
        let kind = NSString::from_str(FILE_URL_TYPE);
        let paths: Vec<PathBuf> = (0..items.count())
            .filter_map(|index| {
                let address = items.objectAtIndex(index).stringForType(&kind)?;
                let url = NSURL::URLWithString(&address)?;
                if !url.isFileURL() {
                    return None;
                }
                url.path().map(|path| PathBuf::from(path.to_string()))
            })
            .collect();
        (!paths.is_empty()).then_some(Content::Files(paths))
    })
}

/// The image on the pasteboard, when `types` says it holds one: its PNG as
/// it is, else its TIFF made into a PNG. A PNG larger than Pane keeps, or
/// a TIFF of more than [`MAX_IMAGE_PIXELS`], is too large.
fn image(types: &[String]) -> Option<Content> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        if types.iter().any(|kind| kind == PNG_TYPE)
            && let Some(data) = pasteboard.dataForType(&NSString::from_str(PNG_TYPE))
        {
            if data.length() > MAX_IMAGE_BYTES {
                return Some(Content::TooLarge);
            }
            if let Some(image) = CopiedImage::from_png(data.to_vec()) {
                return Some(Content::Image(image));
            }
        }
        if !types.iter().any(|kind| kind == TIFF_TYPE) {
            return None;
        }
        let tiff = pasteboard.dataForType(&NSString::from_str(TIFF_TYPE))?;
        let image = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let pixels = (image.pixelsWide().max(0) as u64) * (image.pixelsHigh().max(0) as u64);
        if pixels > MAX_IMAGE_PIXELS {
            return Some(Content::TooLarge);
        }
        let properties = NSDictionary::<NSString, AnyObject>::new();
        // SAFETY: an empty dictionary of properties is of the type asked.
        let png = unsafe {
            image.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }?;
        if png.length() > MAX_IMAGE_BYTES {
            return Some(Content::TooLarge);
        }
        CopiedImage::from_png(png.to_vec()).map(Content::Image)
    })
}

/// The markers a copy's pasteboard types carry: the de-facto
/// `org.nspasteboard.ConcealedType` password-manager marker, whose
/// presence alone says the copy must not be kept. macOS has no formats
/// answering Windows' history and cloud questions, so those stay unset
/// there, and nothing but this marker can keep a copy out: the pasteboard
/// names no program to exclude.
fn markers_of<T: AsRef<str>>(types: &[T]) -> Markers {
    Markers {
        exclude_from_monitoring: types.iter().any(|kind| kind.as_ref() == CONCEALED_TYPE),
        ..Markers::default()
    }
}

/// For the macOS adapter's test: putting text, text marked as a password
/// manager marks it, or a type no text can be read from, on the
/// pasteboard, as another program copying would. The pasteboard server
/// keeps what is put, as any program's copy does, so nothing has to be
/// held while the test reads the report. It replaces what is on the
/// pasteboard, which is not saved.
#[doc(hidden)]
pub mod testing {
    use super::put_text;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSData, NSString};

    /// Puts `text` on the pasteboard, with the concealed type
    /// `org.nspasteboard.ConcealedType` beside it if `conceal`, as a
    /// password manager copying a password does.
    pub fn set_text(text: &str, conceal: bool) -> Result<(), String> {
        put_text(text, conceal)
    }

    /// Puts `bytes` on the pasteboard as `kind` (such as `public.png`)
    /// with no text beside it: a copy no text can be read from.
    pub fn set_target(kind: &str, bytes: &[u8]) -> Result<(), String> {
        autoreleasepool(|_| {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            let data = NSData::with_bytes(bytes);
            let kind = NSString::from_str(kind);
            if !pasteboard.setData_forType(Some(&data), &kind) {
                return Err("the pasteboard would not take the data".into());
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CONCEALED_TYPE, markers_of};
    use crate::clipboard::{Content, Markers, Observation, Skip, accept};

    #[test]
    fn a_copy_with_the_concealed_type_is_marked_and_withheld() {
        // nspasteboard.org's name is what password managers set, and its
        // presence alone marks the copy.
        assert_eq!(markers_of(&["public.utf8-plain-text"]), Markers::default());
        assert_eq!(
            markers_of(&["public.utf8-plain-text", CONCEALED_TYPE]),
            Markers {
                exclude_from_monitoring: true,
                ..Markers::default()
            }
        );
        assert_eq!(
            markers_of(&[CONCEALED_TYPE]),
            Markers {
                exclude_from_monitoring: true,
                ..Markers::default()
            }
        );
        // A marked copy withholds its text, as on Windows.
        assert_eq!(
            accept(
                &Observation {
                    content: Content::Withheld,
                    markers: markers_of(&[CONCEALED_TYPE]),
                    source: None,
                },
                &[],
            ),
            Err(Skip::Marked)
        );
        // Nothing else marks a copy: no other type counts, not even the
        // convention's transient one, which Pane does not read.
        assert_eq!(
            markers_of(&["public.tiff", "org.nspasteboard.TransientType"]),
            Markers::default()
        );
    }

    #[test]
    fn the_concealed_type_is_the_conventions_name() {
        // The name is matched as a whole, so a copy of it that another
        // type's name merely contains does not count.
        assert_eq!(CONCEALED_TYPE, "org.nspasteboard.ConcealedType");
        assert_eq!(
            markers_of(&[
                "x-org.nspasteboard.ConcealedType-y",
                "org.nspasteboard.ConcealedType.org"
            ]),
            Markers::default()
        );
    }
}
