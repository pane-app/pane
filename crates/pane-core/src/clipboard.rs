//! Clipboard history: what the user copies, which Pane keeps for a package
//! that asked for it through `pane:extension/clipboard-history`, while its
//! recording is on, and never while it is paused or the package does not
//! run (disabled, paused after a failure, uninstalled). Pane's own
//! Clipboard History default extension records from the first start; any
//! other package's recording is off until the package turns it on (see
//! `history::records_by_default`).
//!
//! What is kept, and what is not, is decided here, the same on every system
//! ([`accept`], [`accept_any`]): plain text ([`Content::Text`]) of at most
//! [`MAX_TEXT_BYTES`], not blank; and, for Pane's own Clipboard History
//! only (#167), a copied image ([`Content::Image`], kept as a PNG of at
//! most [`MAX_IMAGE_BYTES`]) and copied files ([`Content::Files`], kept as
//! their paths, at most [`MAX_FILES`] of them) — every other package keeps
//! plain text only, as `wit/clipboard.wit` says. Nothing is kept that the
//! application that copied it marked as something a clipboard monitor or
//! clipboard history must not keep ([`Markers`]), or that was copied from a
//! program the user excluded ([`ProgramName`]). Each package's history is
//! typed and kept in its own file ([`history`]), newest first, one item per
//! text, image or list of files and at most [`MAX_ITEMS`] of them, an
//! image's PNG in a folder beside it; it is the package's extension data
//! (the kind `extension_data::DataKind::ClipboardHistory`, whose storage
//! hooks dispatch here).
//!
//! The system is reached through one small trait, [`ClipboardSystem`], with
//! one adapter per system, chosen by [`native`]:
//!
//! - Windows: a clipboard format listener (`AddClipboardFormatListener`,
//!   `WM_CLIPBOARDUPDATE`) on a thread of Pane's own ([`windows`]);
//! - Linux: the X11 `CLIPBOARD` selection, watched through XFIXES on a
//!   thread of Pane's own ([`linux`]); a Wayland session or a display that
//!   does not answer says so instead;
//! - macOS: the pasteboard's change count, polled on a thread of Pane's
//!   own ([`macos`]);
//! - any other system: clipboard history is unavailable there and says why.
//!
//! An adapter watches the clipboard only while Pane holds the [`Watch`] it
//! returned, which Pane does exactly while some package keeps clipboard
//! history ([`Capture`]). Pane fences what it reports: once Pane stops
//! watching, or items are deleted, a change the adapter was still reading
//! is dropped, so a slow or stuck read never delays stopping and never
//! brings back what was deleted.
//!
//! Kept items expire: each is kept for its package's retention (by default
//! [`DEFAULT_RETENTION_SECONDS`]) after it was copied, told by a [`Clock`].
//! The store removes expired items before anything reads them and, while
//! Pane runs, when they expire, whether the package runs or not, so
//! neither a disabled package nor a stopped Pane keeps them longer
//! ([`history`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::extension_data::{ExtensionData, PackageData};

pub(crate) mod history;
#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

pub use history::Item;
#[doc(hidden)]
pub use history::revealed as revealed_history;
#[cfg(target_os = "linux")]
pub use linux::LinuxClipboard;
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use linux::testing;
#[cfg(target_os = "macos")]
pub use macos::MacosClipboard;
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use macos::testing;
#[cfg(target_os = "windows")]
pub use windows::WindowsClipboard;
#[cfg(target_os = "windows")]
#[doc(hidden)]
pub use windows::testing;

/// The most items Pane keeps for a package: copying more drops the oldest.
pub const MAX_ITEMS: usize = 100;

/// The longest text Pane keeps, in bytes of UTF-8; longer text is not kept
/// at all (not cut short).
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

/// The largest image Pane keeps, as the PNG it stores: 10 MiB (#167,
/// proposed default). A larger copy is not kept at all.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// The most pixels of a copied image an adapter reads to make its PNG
/// (8K UHD, 7680×4320): a larger image is reported as
/// [`Content::TooLarge`] without its pixels being read.
pub const MAX_IMAGE_PIXELS: u64 = 7680 * 4320;

/// The most files one copy names for it to be kept: a larger selection is
/// reported as [`Content::TooLarge`].
pub const MAX_FILES: usize = 1000;

/// How long an item is kept after it was copied unless the user chose
/// otherwise: 7 days. Provisional (#36), pending the user's decision.
pub const DEFAULT_RETENTION_SECONDS: u64 = 7 * 86_400;

/// The shortest retention a package can choose: 1 minute.
pub const MIN_RETENTION_SECONDS: u64 = 60;

/// The longest retention a package can choose: 365 days, so that history
/// is always finite.
pub const MAX_RETENTION_SECONDS: u64 = 365 * 86_400;

/// The most programs a package can exclude.
pub const MAX_EXCLUDED: usize = 64;

/// The longest program name Pane accepts to exclude.
const MAX_PROGRAM_NAME: usize = 260;

/// What the application that copied something said about keeping it, as
/// the system's clipboard formats carry it. On Windows these are the
/// formats `ExcludeClipboardContentFromMonitorProcessing` (and the older
/// `Clipboard Viewer Ignore`), `CanIncludeInClipboardHistory` and
/// `CanUploadToCloudClipboard`, which password managers set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Markers {
    /// A clipboard monitor must not look at it.
    pub exclude_from_monitoring: bool,
    /// Whether it may be kept in clipboard history, if the application said.
    pub include_in_history: Option<bool>,
    /// Whether it may be synced to other devices, if the application said.
    /// Pane never syncs anything, but an application saying no is treated as
    /// saying the text is sensitive, so it is not kept either.
    pub upload_to_cloud: Option<bool>,
}

impl Markers {
    /// Whether these markers let Pane keep what was copied.
    pub fn allow(&self) -> bool {
        !self.exclude_from_monitoring
            && self.include_in_history != Some(false)
            && self.upload_to_cloud != Some(false)
    }
}

/// What was on the clipboard, as far as Pane read it. Where a copy holds
/// several, files come first, then text, then an image: copying files
/// often puts their names beside them as text, and copying text in an
/// office application often puts a picture of it beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Plain text.
    Text(String),
    /// An image, as a PNG (#167).
    Image(CopiedImage),
    /// Files and folders, by their absolute paths, in the order the file
    /// manager copied them (#167).
    Files(Vec<PathBuf>),
    /// An image or a list of files larger than Pane reads
    /// ([`MAX_IMAGE_PIXELS`], [`MAX_FILES`]): not read, and not kept.
    TooLarge,
    /// Nothing Pane keeps: no text, image or files, or empty.
    Other,
    /// Not read, because its markers forbid keeping it.
    Withheld,
}

/// A copied image, as the PNG Pane keeps of it, with the size its header
/// states.
#[derive(Clone, PartialEq, Eq)]
pub struct CopiedImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl CopiedImage {
    /// `png` as a copied image, if it is a PNG stating a size of at least
    /// one pixel.
    pub fn from_png(png: Vec<u8>) -> Option<CopiedImage> {
        let (width, height) = png_dimensions(&png)?;
        Some(CopiedImage { png, width, height })
    }
}

impl std::fmt::Debug for CopiedImage {
    /// Its size and how many bytes its PNG has; never the bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopiedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.png.len())
            .finish()
    }
}

/// The width and height the PNG `png` states in its header, if it is a PNG
/// (its signature, then its `IHDR` chunk) of at least one pixel.
pub fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || png[..8] != *b"\x89PNG\r\n\x1a\n" || png[12..16] != *b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// What a kept copy is, as [`accept_any`] lets it be kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copied<'a> {
    Text(&'a str),
    Image(&'a CopiedImage),
    Files(&'a [PathBuf]),
}

/// One change of the clipboard, as an adapter reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub content: Content,
    pub markers: Markers,
    /// The program that owns the clipboard, if the system says which it
    /// is: its full path where the system gives one (Windows:
    /// `C:\Windows\notepad.exe`), else its file name or process name
    /// (`notepad.exe`). [`program_file_name`] reads the file name of
    /// either.
    pub source: Option<String>,
}

/// Why an observation is not kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// The application marked it as not to be kept.
    Marked,
    /// It is not text.
    NotText,
    /// It is empty or only white space.
    Blank,
    /// It is longer than [`MAX_TEXT_BYTES`].
    TooLong,
    /// It is an image whose PNG is larger than [`MAX_IMAGE_BYTES`], or one
    /// or a list of files larger than Pane reads ([`Content::TooLarge`]).
    TooLarge,
    /// It was copied from this excluded program.
    Excluded(ProgramName),
}

/// The text of `observation` if Pane keeps it for a package that excluded
/// the programs `excluded`, or why not: what a package keeps through
/// `wit/clipboard.wit`, which is plain text only. An image or files are
/// [`Skip::NotText`] here; Pane's own Clipboard History keeps them too
/// ([`accept_any`]).
pub fn accept<'a>(observation: &'a Observation, excluded: &[ProgramName]) -> Result<&'a str, Skip> {
    match accept_any(observation, excluded)? {
        Copied::Text(text) => Ok(text),
        Copied::Image(_) | Copied::Files(_) => Err(Skip::NotText),
    }
}

/// What of `observation` Pane's own Clipboard History keeps, having
/// excluded the programs `excluded`, or why nothing: text as [`accept`]
/// keeps it, an image whose PNG has at most [`MAX_IMAGE_BYTES`], or a list
/// of at least one and at most [`MAX_FILES`] files; under the same markers
/// and exclusions as text.
pub fn accept_any<'a>(
    observation: &'a Observation,
    excluded: &[ProgramName],
) -> Result<Copied<'a>, Skip> {
    if !observation.markers.allow() {
        return Err(Skip::Marked);
    }
    match &observation.content {
        Content::Withheld => return Err(Skip::Marked),
        Content::Other => return Err(Skip::NotText),
        Content::Text(_) | Content::Image(_) | Content::Files(_) | Content::TooLarge => {}
    }
    if let Some(source) = &observation.source
        && let Some(program) = excluded.iter().find(|program| program.names(source))
    {
        return Err(Skip::Excluded(program.clone()));
    }
    match &observation.content {
        Content::Text(text) if text.trim().is_empty() => Err(Skip::Blank),
        Content::Text(text) if text.len() > MAX_TEXT_BYTES => Err(Skip::TooLong),
        Content::Text(text) => Ok(Copied::Text(text)),
        Content::Image(image) if image.png.len() > MAX_IMAGE_BYTES => Err(Skip::TooLarge),
        Content::Image(image) => Ok(Copied::Image(image)),
        Content::Files(files) if files.is_empty() => Err(Skip::Blank),
        Content::Files(files) if files.len() > MAX_FILES => Err(Skip::TooLarge),
        Content::Files(files) => Ok(Copied::Files(files)),
        Content::TooLarge => Err(Skip::TooLarge),
        Content::Other | Content::Withheld => unreachable!("answered above"),
    }
}

/// A program the user excluded, by its file name, trimmed and lowercased,
/// such as `keepass.exe`. A name read from the history file is lowercased
/// too, whatever case it was written in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct ProgramName(String);

impl ProgramName {
    /// `name` as an excluded program, or why it is not a program's file
    /// name.
    pub fn parse(name: &str) -> Result<ProgramName, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Name the program's file, such as KeePass.exe".into());
        }
        if name.contains(['/', '\\', ':']) {
            return Err(format!(
                "“{name}” is a path: name only the program's file, such as KeePass.exe"
            ));
        }
        if name.chars().count() > MAX_PROGRAM_NAME {
            return Err(format!(
                "A program's file name has at most {MAX_PROGRAM_NAME} characters"
            ));
        }
        Ok(ProgramName::from(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the program `source` (a file name, or a path, whose file
    /// name counts) is this one: the same file name, or the same name
    /// without its extension, ignoring case, so `KeePass` excludes
    /// `KeePass.exe` and `C:\Program Files\KeePass\KeePass.exe`.
    pub fn names(&self, source: &str) -> bool {
        let source = program_file_name(source).trim().to_lowercase();
        let stem = source
            .rsplit_once('.')
            .map_or(source.as_str(), |(stem, _)| stem);
        source == self.0 || stem == self.0
    }
}

/// The file name of the program `source` names: the last part of a path
/// (an observation's source is the program's path where the system gives
/// one, as Windows does), or `source` itself.
pub fn program_file_name(source: &str) -> &str {
    source
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(source)
}

impl From<String> for ProgramName {
    fn from(name: String) -> ProgramName {
        ProgramName(name.trim().to_lowercase())
    }
}

impl From<ProgramName> for String {
    fn from(name: ProgramName) -> String {
        name.0
    }
}

/// Whether Pane keeps what is copied for a package.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureState {
    /// Never turned on, or turned off: nothing is kept. Every package
    /// starts so, except Pane's own Clipboard History, which starts
    /// [`CaptureState::On`] (see `history::records_by_default`).
    #[default]
    Off,
    /// Text copied is kept while the package runs.
    On,
    /// Turned on, then paused: nothing is kept until it is resumed.
    Paused,
}

impl CaptureState {
    pub fn is_off(&self) -> bool {
        *self == CaptureState::Off
    }
}

/// Taken by an adapter just before it reads a change of the clipboard, and
/// handed back with what it read ([`Sink::observed`]): if Pane stopped
/// watching or items were deleted meanwhile, what it read is dropped. The
/// default ticket is for a sink of an adapter's own tests, which fences
/// nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ticket(u64);

/// Where an adapter reports the changes of the clipboard, from its own
/// thread.
pub trait Sink: Send + Sync + 'static {
    /// Called before the adapter reads a change.
    fn reading(&self) -> Ticket;

    /// What the adapter read with `ticket`.
    fn observed(&self, ticket: Ticket, observation: Observation);
}

/// Watching the clipboard: the adapter's listener, which stops listening
/// when this is dropped. Dropping it never waits long for a listener stuck
/// in a read; Pane stops using the listener's reports before it drops this.
pub struct Watch(#[allow(dead_code)] Box<dyn Send>);

impl Watch {
    /// A watch that stops when `listener` is dropped.
    pub fn new(listener: impl Send + 'static) -> Watch {
        Watch(Box::new(listener))
    }
}

/// The system's clipboard, as Pane watches and writes it.
pub trait ClipboardSystem: Send + Sync + 'static {
    /// Why Pane cannot watch the clipboard on this system, if it cannot.
    fn unavailable(&self) -> Option<String>;

    /// Starts watching: `sink` is told of each later change of the
    /// clipboard (not of what is on it now) until the returned watch is
    /// dropped.
    fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String>;

    /// Puts `text` on the clipboard, as copying it would.
    fn write_text(&self, text: &str) -> Result<(), String>;

    /// Puts the image `png` (a PNG file's bytes) on the clipboard, as
    /// copying an image would (#167). A system whose adapter cannot says
    /// why.
    fn write_image(&self, png: &[u8]) -> Result<(), String> {
        let _ = png;
        Err(self
            .unavailable()
            .unwrap_or_else(|| "Putting an image on the clipboard is not available here".into()))
    }

    /// Puts the files `paths` on the clipboard, as copying them in the
    /// file manager would (#167). A system whose adapter cannot says why.
    fn write_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        let _ = paths;
        Err(self
            .unavailable()
            .unwrap_or_else(|| "Putting files on the clipboard is not available here".into()))
    }
}

/// This system's adapter: Windows' clipboard format listener, Linux's
/// watcher of the X11 CLIPBOARD selection, macOS's watcher of the
/// pasteboard's change count, or one that explains why clipboard history
/// is unavailable here.
pub fn native() -> Arc<dyn ClipboardSystem> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(WindowsClipboard)
    }
    #[cfg(target_os = "linux")]
    {
        match linux::native() {
            Ok(clipboard) => Arc::new(clipboard),
            Err(reason) => Arc::new(Unavailable(reason)),
        }
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacosClipboard)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let here = crate::platform::Platform::current()
            .map_or_else(|| std::env::consts::OS.to_string(), |p| p.to_string());
        Arc::new(Unavailable(format!(
            "Not available on {here}: Pane watches the clipboard only on Windows, macOS and Linux"
        )))
    }
}

/// A clipboard Pane does not watch, for a launcher given none.
pub fn none() -> Arc<dyn ClipboardSystem> {
    Arc::new(Unavailable(
        "Not available: this Pane does not watch the clipboard".into(),
    ))
}

/// A system where clipboard history is unavailable, saying why.
struct Unavailable(String);

impl ClipboardSystem for Unavailable {
    fn unavailable(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn watch(&self, _sink: Arc<dyn Sink>) -> Result<Watch, String> {
        Err(self.0.clone())
    }

    fn write_text(&self, _text: &str) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn write_image(&self, _png: &[u8]) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn write_files(&self, _paths: &[PathBuf]) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// Tells the time for the work Pane does by its own clock rather than the
/// user's asking: clipboard history (when an item was copied and whether
/// it expired) and scheduled work (when a run is due). Tests and
/// development builds give the launcher another clock
/// ([`crate::Launcher::with_clock`]); release builds keep the system's.
pub trait Clock: Send + Sync + 'static {
    /// Now, in milliseconds since the Unix epoch.
    fn now(&self) -> u64;

    /// How far the local time at `at` (milliseconds since the Unix epoch)
    /// is from UTC, in milliseconds: the system's, by its time zone
    /// settings, so a guest answering a query about the local date or
    /// time can (#196). A clock that says nothing answers 0.
    fn local_offset(&self, at: u64) -> i64 {
        let _ = at;
        0
    }

    /// Has `changed` called whenever this clock is set other than by time
    /// passing (the system's never is), so that expiry and due scheduled
    /// work are looked at again.
    fn on_change(&self, changed: Box<dyn Fn() + Send + Sync>) {
        let _ = changed;
    }
}

/// The system's clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn local_offset(&self, at: u64) -> i64 {
        crate::launcher::clipboard_view::local_offset_ms(at)
    }
}

/// A clock that stands still until it is set or advanced, for tests and
/// development builds ([`crate::Launcher::with_clock`]).
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub struct ManualClock {
    now: Mutex<u64>,
    listeners: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

#[cfg(any(test, debug_assertions))]
impl ManualClock {
    /// A clock showing `now`, in milliseconds since the Unix epoch.
    pub fn at(now: u64) -> Arc<ManualClock> {
        Arc::new(ManualClock {
            now: Mutex::new(now),
            listeners: Mutex::new(Vec::new()),
        })
    }

    /// Moves the clock `by` forward.
    pub fn advance(&self, by: std::time::Duration) {
        {
            let mut now = self.now.lock().unwrap_or_else(|p| p.into_inner());
            *now = now.saturating_add(u64::try_from(by.as_millis()).unwrap_or(u64::MAX));
        }
        for listener in self
            .listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
        {
            listener();
        }
    }
}

#[cfg(any(test, debug_assertions))]
impl Clock for ManualClock {
    fn now(&self) -> u64 {
        *self.now.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn on_change(&self, changed: Box<dyn Fn() + Send + Sync>) {
        self.listeners
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(changed);
    }
}

/// Watches the clipboard exactly while some package keeps clipboard
/// history: its capture is on and it runs. Every change to either calls
/// [`Capture::reconcile`] (see [`ExtensionData::set_changed`]), so turning
/// history off, pausing it or disabling, pausing or uninstalling the
/// package stops the watch at once, and turning it on or enabling the
/// package starts it again.
pub(crate) struct Capture {
    system: Arc<dyn ClipboardSystem>,
    data: ExtensionData,
    watching: Mutex<Watching>,
}

#[derive(Default)]
struct Watching {
    /// The adapter's watch, with the fence of its reports.
    current: Option<(Watch, Arc<Fence>)>,
    /// Why the adapter could not start watching, until it next can.
    problem: Option<String>,
}

/// Lets a watch's reports through until it is closed. A report is kept
/// while holding it, so once [`Fence::close`] returns, none is kept any
/// more.
struct Fence(Mutex<bool>);

impl Fence {
    fn lock(&self) -> MutexGuard<'_, bool> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn close(&self) {
        *self.lock() = false;
    }
}

/// Where a watch reports: it keeps what the packages capturing now accept.
struct CaptureSink {
    data: ExtensionData,
    fence: Arc<Fence>,
}

impl Sink for CaptureSink {
    fn reading(&self) -> Ticket {
        Ticket(self.data.clipboard_history().deletions())
    }

    fn observed(&self, ticket: Ticket, observation: Observation) {
        let open = self.fence.lock();
        if !*open {
            return;
        }
        let running = self.data.running_owners();
        let store = self.data.clipboard_history();
        // An image's PNG is written before the history is changed, with the
        // history's lock not held, and only for Pane's own Clipboard
        // History, the one package that keeps images (#167).
        let image = match &observation.content {
            Content::Image(image)
                if observation.markers.allow() && image.png.len() <= MAX_IMAGE_BYTES =>
            {
                let owner = history::default_owner();
                let capturing =
                    running.contains(&owner) && store.capturing_owners().contains(&owner);
                capturing
                    .then(|| match store.keep_image(&owner, image) {
                        Ok(kept) => Some(kept),
                        Err(error) => {
                            crate::diagnostic!("Pane could not keep a copied image: {error}");
                            None
                        }
                    })
                    .flatten()
            }
            _ => None,
        };
        let mut image_used = false;
        store.capture(ticket.0, |packages, now| {
            let mut changed = false;
            let source = observation.source.as_deref();
            for (owner, history) in packages.iter_mut() {
                if history.capture != CaptureState::On || !running.contains(owner) {
                    continue;
                }
                let copied = if history::keeps_images_and_files(owner) {
                    accept_any(&observation, &history.excluded)
                } else {
                    accept(&observation, &history.excluded).map(Copied::Text)
                };
                match copied {
                    Ok(Copied::Text(text)) => history.add(text, source, now),
                    Ok(Copied::Files(files)) => history.add_files(files, source, now),
                    Ok(Copied::Image(_)) => match &image {
                        Some(kept) if kept.owner() == owner => {
                            history.add_image(kept.stored().clone(), source, now);
                            image_used = true;
                        }
                        _ => continue,
                    },
                    Err(_) => continue,
                }
                changed = true;
            }
            changed
        });
        if let Some(kept) = image {
            store.release_image(kept, image_used);
        }
        drop(open);
    }
}

impl Capture {
    /// Keeps clipboard history for the packages of `data` through
    /// `system`, starting to watch now if one keeps it.
    pub fn start(system: Arc<dyn ClipboardSystem>, data: ExtensionData) -> Arc<Capture> {
        let capture = Arc::new(Capture {
            system,
            data: data.clone(),
            watching: Mutex::new(Watching::default()),
        });
        let weak: Weak<Capture> = Arc::downgrade(&capture);
        data.set_changed(Arc::new(move || {
            if let Some(capture) = weak.upgrade() {
                capture.reconcile();
            }
        }));
        capture.reconcile();
        capture
    }

    /// The system's clipboard.
    pub fn system(&self) -> &Arc<dyn ClipboardSystem> {
        &self.system
    }

    /// Why Pane does not watch the clipboard now although it should, or
    /// cannot on this system.
    pub fn problem(&self) -> Option<String> {
        self.system
            .unavailable()
            .or_else(|| self.lock().problem.clone())
    }

    /// Whether some package keeps clipboard history now: its capture is on
    /// and its code may run.
    fn capturing(&self) -> bool {
        let running = self.data.running_owners();
        self.data
            .clipboard_history()
            .capturing_owners()
            .iter()
            .any(|owner| running.contains(owner))
    }

    /// Starts or stops watching, as the packages' capture states and
    /// generations now require. Stopping closes the watch's fence, then
    /// drops it with nothing locked, so it never waits for a read in
    /// progress.
    pub fn reconcile(&self) {
        let wanted = self.system.unavailable().is_none() && self.capturing();
        let stopping = {
            let mut watching = self.lock();
            if wanted {
                if watching.current.is_none() {
                    let fence = Arc::new(Fence(Mutex::new(true)));
                    let sink = Arc::new(CaptureSink {
                        data: self.data.clone(),
                        fence: fence.clone(),
                    });
                    match self.system.watch(sink) {
                        Ok(watch) => {
                            watching.current = Some((watch, fence));
                            watching.problem = None;
                        }
                        Err(problem) => watching.problem = Some(problem),
                    }
                }
                None
            } else {
                watching.problem = None;
                watching.current.take()
            }
        };
        if let Some((watch, fence)) = stopping {
            fence.close();
            drop(watch);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Watching> {
        self.watching
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A package's clipboard history as its commands see it.
pub(crate) struct Status {
    pub capture: CaptureState,
    /// Why Pane does not watch the clipboard although it should, or cannot.
    pub problem: Option<String>,
    pub excluded: Vec<ProgramName>,
    pub items: usize,
    /// How long each item is kept after it was copied, in seconds.
    pub retention_seconds: u64,
}

/// What a command of the package with `data` does with its clipboard
/// history through `capture` (none: this Pane does not watch the
/// clipboard). Stopped code (its generation ended, or Pane gave up on the
/// runtime thread running it, see `PackageData::stopped`) reads and changes
/// nothing more.
pub(crate) struct Commands<'a> {
    pub data: &'a PackageData,
    pub capture: Option<Arc<Capture>>,
}

impl Commands<'_> {
    fn unavailable() -> String {
        none().unavailable().unwrap_or_default()
    }

    fn system(&self) -> Result<Arc<dyn ClipboardSystem>, String> {
        self.capture
            .as_ref()
            .map(|capture| capture.system().clone())
            .ok_or_else(Commands::unavailable)
    }

    pub fn status(&self) -> Result<Status, String> {
        let history = self.data.clipboard_history()?.get(self.data.owner())?;
        let problem = match &self.capture {
            Some(capture) => capture.problem(),
            None => Some(Commands::unavailable()),
        };
        Ok(Status {
            capture: history.capture,
            problem,
            retention_seconds: history.retention(),
            excluded: history.excluded,
            items: history.items.len(),
        })
    }

    pub fn set_capture(&self, state: CaptureState) -> Result<(), String> {
        let system = self.system()?;
        if state == CaptureState::On
            && let Some(reason) = system.unavailable()
        {
            return Err(reason);
        }
        self.update(|history| {
            history.capture = state;
            Ok(())
        })
    }

    pub fn set_excluded(&self, programs: &[String]) -> Result<(), String> {
        self.update(|history| history.set_excluded(programs))
    }

    /// Keeps each item `seconds` after it was copied; items already older
    /// are deleted at once.
    pub fn set_retention(&self, seconds: u64) -> Result<(), String> {
        self.update(|history| history.set_retention(seconds))
    }

    /// The kept items, newest first, none expired, with the time now.
    pub fn items(&self) -> Result<(Vec<Item>, u64), String> {
        let store = self.data.clipboard_history()?;
        let items = store.get(self.data.owner())?.items;
        Ok((items, store.now()))
    }

    /// Puts the kept item `id` on the clipboard again, as what it was: text
    /// as text, an image as an image, files as files (#167).
    pub fn copy(&self, id: &str) -> Result<(), String> {
        let system = self.system()?;
        let item = self
            .items()?
            .0
            .into_iter()
            .find(|item| item.id.to_string() == id)
            .ok_or("That item is no longer kept")?;
        // One that cannot be read on this computer (#130) says why.
        if let Some(why) = item.unreadable {
            return Err(why);
        }
        if let Some(image) = &item.image {
            let path = self
                .data
                .clipboard_history()?
                .image_path(self.data.owner(), &image.digest);
            let png = std::fs::read(&path)
                .map_err(|error| format!("The kept image cannot be read: {error}"))?;
            return system.write_image(&png);
        }
        if !item.files.is_empty() {
            return system.write_files(&item.files);
        }
        system.write_text(&item.text)
    }

    /// Deletes every kept item; returns how many there were. A change the
    /// adapter was reading meanwhile is not kept.
    pub fn clear(&self) -> Result<usize, String> {
        self.update(|history| Ok(history.clear()))
    }

    /// Deletes the kept items `ids`; returns how many were kept. An id no
    /// longer kept is passed over. A change the adapter was reading
    /// meanwhile is not kept.
    pub fn delete(&self, ids: &[String]) -> Result<usize, String> {
        let ids: Vec<u64> = ids.iter().filter_map(|id| id.parse().ok()).collect();
        self.update(|history| Ok(history.delete(&ids)))
    }

    /// Turns history off and deletes every kept item at once, so nothing
    /// copied meanwhile is kept; returns how many items there were. The
    /// excluded programs and the retention stay.
    pub fn turn_off_and_clear(&self) -> Result<usize, String> {
        self.update(|history| {
            history.capture = CaptureState::Off;
            Ok(history.clear())
        })
    }

    fn update<R>(
        &self,
        change: impl FnOnce(&mut history::PackageHistory) -> Result<R, String>,
    ) -> Result<R, String> {
        let (answer, capture_changed) = self.data.update_clipboard_history(change)?;
        if capture_changed {
            self.data.changed();
        }
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Observation {
        Observation {
            content: Content::Text(text.into()),
            markers: Markers::default(),
            source: None,
        }
    }

    fn from(text: &str, source: &str) -> Observation {
        Observation {
            source: Some(source.into()),
            ..self::text(text)
        }
    }

    #[test]
    fn plain_text_is_kept() {
        assert_eq!(accept(&text("hello"), &[]), Ok("hello"));
        assert_eq!(accept(&text("  two words \n"), &[]), Ok("  two words \n"));
    }

    #[test]
    fn marked_text_is_not_kept() {
        let marked = |markers: Markers| Observation {
            markers,
            ..text("hunter2")
        };
        let excluded = Markers {
            exclude_from_monitoring: true,
            ..Markers::default()
        };
        let no_history = Markers {
            include_in_history: Some(false),
            ..Markers::default()
        };
        let no_cloud = Markers {
            upload_to_cloud: Some(false),
            ..Markers::default()
        };
        for markers in [excluded, no_history, no_cloud] {
            assert_eq!(accept(&marked(markers), &[]), Err(Skip::Marked));
        }
        // Saying yes changes nothing.
        let allowed = Markers {
            include_in_history: Some(true),
            upload_to_cloud: Some(true),
            ..Markers::default()
        };
        assert_eq!(accept(&marked(allowed), &[]), Ok("hunter2"));
        let withheld = Observation {
            content: Content::Withheld,
            ..text("")
        };
        assert_eq!(accept(&withheld, &[]), Err(Skip::Marked));
    }

    #[test]
    fn other_blank_and_long_content_is_not_kept() {
        let other = Observation {
            content: Content::Other,
            ..text("")
        };
        assert_eq!(accept(&other, &[]), Err(Skip::NotText));
        assert_eq!(accept(&text(""), &[]), Err(Skip::Blank));
        assert_eq!(accept(&text(" \t\r\n"), &[]), Err(Skip::Blank));
        let longest = "a".repeat(MAX_TEXT_BYTES);
        assert_eq!(accept(&text(&longest), &[]), Ok(longest.as_str()));
        let longer = format!("{longest}é");
        assert_eq!(accept(&text(&longer), &[]), Err(Skip::TooLong));
    }

    #[test]
    fn text_from_an_excluded_program_is_not_kept() {
        let keepass = ProgramName::parse(" KeePass.exe ").unwrap();
        let excluded = vec![keepass.clone(), ProgramName::from("1Password".to_string())];
        assert_eq!(
            accept(&from("secret", "KEEPASS.EXE"), &excluded),
            Err(Skip::Excluded(keepass))
        );
        assert_eq!(
            accept(&from("secret", "1Password.exe"), &excluded),
            Err(Skip::Excluded(ProgramName::from("1password".to_string())))
        );
        assert_eq!(accept(&from("note", "notepad.exe"), &excluded), Ok("note"));
        // A program named by its path (Windows names the owner so, #166)
        // is matched by its file name.
        assert_eq!(
            accept(
                &from(
                    "secret",
                    r"C:\Program Files\KeePass Password Safe 2\KeePass.exe"
                ),
                &excluded
            ),
            Err(Skip::Excluded(ProgramName::from("keepass.exe".to_string())))
        );
        assert_eq!(program_file_name("/usr/bin/keepassxc"), "keepassxc");
        assert_eq!(program_file_name("notepad.exe"), "notepad.exe");
        // A program the system does not name is not excluded.
        assert_eq!(accept(&text("note"), &excluded), Ok("note"));
        // Only the whole name counts.
        assert_eq!(
            accept(&from("note", "keepassxc.exe"), &excluded),
            Ok("note")
        );
    }

    #[test]
    fn a_program_is_named_by_its_file_in_lower_case() {
        assert_eq!(
            ProgramName::parse("KeePass.exe").unwrap().as_str(),
            "keepass.exe"
        );
        assert!(ProgramName::parse("  ").is_err());
        assert!(ProgramName::parse(r"C:\Program Files\KeePass.exe").is_err());
        assert!(ProgramName::parse(&"a".repeat(261)).is_err());
        // However it was written in the file.
        let read: Vec<ProgramName> =
            serde_json::from_str(r#"["KeePass.EXE", " Bitwarden "]"#).unwrap();
        assert_eq!(
            read,
            [
                ProgramName::from("keepass.exe".to_string()),
                ProgramName::from("bitwarden".to_string())
            ]
        );
        assert!(read[0].names("KEEPASS.exe") && read[1].names("Bitwarden.exe"));
    }

    /// A PNG of `width` × `height` transparent pixels.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![0u8; (width * height * 4) as usize];
        crate::icons::encode_png(width, height, &pixels).unwrap()
    }

    #[test]
    fn a_png_states_its_size_in_its_header() {
        assert_eq!(png_dimensions(&png(3, 2)), Some((3, 2)));
        let image = CopiedImage::from_png(png(5, 4)).unwrap();
        assert_eq!((image.width, image.height), (5, 4));
        assert!(!format!("{image:?}").contains('['), "no bytes are shown");
        assert_eq!(png_dimensions(b"not a png at all, not at all"), None);
        assert_eq!(CopiedImage::from_png(png(3, 2)[..20].to_vec()), None);
    }

    /// #167: Pane's own Clipboard History keeps an image and files under
    /// the same markers and exclusions as text; a package keeping history
    /// through the contract keeps text only.
    #[test]
    fn images_and_files_are_kept_under_the_rules_of_text() {
        let image = CopiedImage::from_png(png(2, 2)).unwrap();
        let of = |content: Content| Observation {
            content,
            markers: Markers::default(),
            source: Some(r"C:\Windows\explorer.exe".into()),
        };
        let copied = of(Content::Image(image.clone()));
        assert_eq!(accept_any(&copied, &[]), Ok(Copied::Image(&image)));
        assert_eq!(accept(&copied, &[]), Err(Skip::NotText));
        let files = vec![PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b")];
        let listed = of(Content::Files(files.clone()));
        assert_eq!(accept_any(&listed, &[]), Ok(Copied::Files(&files)));
        assert_eq!(accept(&listed, &[]), Err(Skip::NotText));
        assert_eq!(
            accept_any(&of(Content::Files(Vec::new())), &[]),
            Err(Skip::Blank)
        );
        // Excluded programs and markers count for them as for text.
        let explorer = [ProgramName::parse("explorer.exe").unwrap()];
        assert!(matches!(
            accept_any(&listed, &explorer),
            Err(Skip::Excluded(_))
        ));
        let marked = Observation {
            markers: Markers {
                exclude_from_monitoring: true,
                ..Markers::default()
            },
            ..copied.clone()
        };
        assert_eq!(accept_any(&marked, &[]), Err(Skip::Marked));
        // Oversized copies are skipped.
        let huge = CopiedImage {
            png: vec![0; MAX_IMAGE_BYTES + 1],
            width: 1,
            height: 1,
        };
        assert_eq!(
            accept_any(&of(Content::Image(huge)), &[]),
            Err(Skip::TooLarge)
        );
        let many = vec![PathBuf::from("/a"); MAX_FILES + 1];
        assert_eq!(
            accept_any(&of(Content::Files(many)), &[]),
            Err(Skip::TooLarge)
        );
        assert_eq!(accept_any(&of(Content::TooLarge), &[]), Err(Skip::TooLarge));
    }
}
