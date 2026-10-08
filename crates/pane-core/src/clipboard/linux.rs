//! The clipboard on Linux: the X11 `CLIPBOARD` selection, watched through
//! the XFIXES extension (`XfixesSelectSelectionInput`), which tells Pane's
//! own thread whenever any program takes the selection, without Pane owning
//! it and without a permission. A Wayland session has no X11 clipboard:
//! [`native`] says clipboard history is unavailable there.
//!
//! On each change the thread, and only it, asks the new owner what it
//! offers (`TARGETS`), then for what was copied (`ConvertSelection`, into a
//! property of a window of Pane's own): the files a file manager copied
//! (`text/uri-list`, every `file://` URI), else the text — first as
//! `UTF8_STRING`, then, if the owner refuses that, as `STRING` (Latin-1) —
//! else an image (`image/png`; #167). An owner that does not answer
//! `TARGETS` is asked for the text alone, as before. What the owner sends
//! in pieces (`INCR`) is read piece by piece, and abandoned once it is
//! larger than Pane keeps ([`MAX_TEXT_BYTES`] for text, [`MAX_IMAGE_BYTES`]
//! for an image). What is kept is decided by `clipboard::accept`, the same
//! on every system. X11
//! has no formats that mark a copy as not to be kept: the markers are
//! reported as [`Markers::default`] and only a program the user excluded is
//! skipped, matched by the owner window's process, which `_NET_WM_PID` and
//! `/proc` name (or its `WM_CLASS`, which usually is the program's name);
//! a copy made by a window that says neither is never excluded.
//!
//! Writing ([`ClipboardSystem::write_text`], and an image or files with
//! [`ClipboardSystem::write_image`] and [`ClipboardSystem::write_files`])
//! takes the selection with a window of Pane's own, which serves what was
//! written to whoever pastes, as a
//! program that copied does, until another program copies; a write is a
//! change like any other, so the watcher reports it too. When Pane stops,
//! the server offers the selection to the clipboard manager, as the ICCCM
//! says an owner should, so what Pane put stays pasteable where a manager
//! runs (without one, every program's copy goes with it).
//!
//! Dropping the watch tells the thread to stop and waits at most
//! [`STOP_WAIT`] for it. A read that is still waiting on the program that
//! copied is left to end on its own, as on Windows; unlike there it always
//! does, because every wait while reading is bounded, and Pane dropped the
//! sink's fence before, so what a late read returns is dropped.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEvent, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, Timestamp,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::{
    ClipboardSystem, Content, CopiedImage, MAX_FILES, MAX_IMAGE_BYTES, MAX_TEXT_BYTES, Markers,
    Observation, Sink, Watch,
};
use crate::threads::Joinable;

x11rb::atom_manager! {
    /// The atoms Pane's clipboard adapter uses. `PANE_SELECTION` is the
    /// property the watcher reads what it asked for into; `NET_WM_PID` is
    /// the EWMH property that names a window's process; `text/uri-list`,
    /// `x-special/gnome-copied-files` and `image/png` are the targets of
    /// copied files and images (#167).
    Atoms: AtomCookies {
        CLIPBOARD,
        TARGETS,
        UTF8_STRING,
        INCR,
        CLIPBOARD_MANAGER,
        SAVE_TARGETS,
        PANE_SELECTION: b"PANE_SELECTION",
        NET_WM_PID: b"_NET_WM_PID",
        URI_LIST: b"text/uri-list",
        GNOME_COPIED_FILES: b"x-special/gnome-copied-files",
        IMAGE_PNG: b"image/png",
    }
}

/// The most bytes of the targets an owner offers that Pane reads: far more
/// than any owner lists.
const TARGETS_BYTES: usize = 4096;

/// The most bytes of a list of copied files Pane reads: room for
/// [`MAX_FILES`] long paths; a longer list is too large to keep.
const URI_LIST_BYTES: usize = MAX_FILES * 1024;

/// How long dropping the watch waits for the watcher to end.
const STOP_WAIT: Duration = Duration::from_secs(1);

/// How long the watcher waits for the program that copied to answer a
/// request for the text. Some programs take seconds to render what they
/// copied, so this is generous, and while it waits nothing else is read.
const ANSWER_WAIT: Duration = Duration::from_secs(4);

/// How long the watcher waits for each piece of a text sent in pieces.
const PIECE_WAIT: Duration = Duration::from_millis(500);

/// How long a thread sleeps between looks for an event while it waits for
/// one in particular, so a stop is noticed soon.
const PARK: Duration = Duration::from_millis(20);

/// How long the server offers the selection to the clipboard manager when
/// Pane stops, serving its requests meanwhile.
const HANDOVER_WAIT: Duration = Duration::from_millis(300);

/// How much of a property Pane reads at once to read at most `limit`
/// bytes, in 4-byte units: one unit more than `limit`, so what is too
/// large to keep is seen as too large rather than cut short.
fn read_units(limit: usize) -> u32 {
    u32::try_from((limit + 8) / 4).unwrap_or(u32::MAX)
}

/// What a server serves, in the target it is served in: the target and its
/// bytes.
type Served = Vec<(Atom, Vec<u8>)>;

/// Writes `message` to standard error, if there is one; never what was
/// copied. Unlike `eprintln!`, it cannot panic.
fn log(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// Connects to the X11 display `display`, saying why as Pane's own words
/// when it cannot.
fn connect(display: &str) -> Result<(RustConnection, usize), String> {
    RustConnection::connect(Some(display))
        .map_err(|problem| format!("Pane could not reach the X11 display {display}: {problem}"))
}

/// One end of Pane's X11 clipboard: its connection, the window the
/// clipboard is talked about through (the watcher's requestor, or the
/// server's owner), a window that wakes the thread waiting for events, and
/// the atoms the clipboard uses.
struct Endpoint {
    connection: Arc<RustConnection>,
    /// The window the clipboard is talked about through.
    window: Window,
    /// The window that wakes the thread waiting for events.
    waker: Window,
    atoms: Atoms,
}

/// Connects to `display` with an endpoint whose talking window selects
/// `events`; its waker selects none.
fn endpoint(display: &str, events: EventMask) -> Result<Endpoint, String> {
    let (connection, screen) = connect(display)?;
    let screen = &connection.setup().roots[screen];
    let (root, root_visual) = (screen.root, screen.root_visual);
    let window = make_window(&connection, root, root_visual, false, events)?;
    let waker = make_window(&connection, root, root_visual, true, EventMask::NO_EVENT)?;
    let atoms = Atoms::new(&connection).map_err(why)?.reply().map_err(why)?;
    Ok(Endpoint {
        connection: Arc::new(connection),
        window,
        waker,
        atoms,
    })
}

/// A 1×1 window of Pane's own on `root` that no one sees, with `events`
/// selected, for talking about the clipboard through.
fn make_window(
    connection: &RustConnection,
    root: Window,
    root_visual: u32,
    input_only: bool,
    events: EventMask,
) -> Result<Window, String> {
    let window = connection.generate_id().map_err(why)?;
    let (class, visual) = if input_only {
        (WindowClass::INPUT_ONLY, 0)
    } else {
        (WindowClass::INPUT_OUTPUT, root_visual)
    };
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            class,
            visual,
            &CreateWindowAux::new().event_mask(events),
        )
        .map_err(why)?
        .check()
        .map_err(why)?;
    Ok(window)
}

/// Wakes a thread waiting on `connection` for events, through its `waker`
/// window.
fn wake(connection: &RustConnection, waker: Window) {
    let message = ClientMessageEvent::new(32, waker, 0u32, [0u32; 5]);
    if let Ok(cookie) = connection.send_event(false, waker, EventMask::NO_EVENT, message) {
        cookie.ignore_error();
    }
    let _ = connection.flush();
}

/// Why an X11 request failed, as Pane's own words.
fn why<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

/// The X11 `CLIPBOARD` selection, on one display.
pub struct LinuxClipboard {
    display: String,
    /// Serves what [`ClipboardSystem::write_text`] puts on the clipboard,
    /// made the first time Pane writes.
    writer: Mutex<Option<Arc<Server>>>,
}

impl LinuxClipboard {
    /// The clipboard of the X11 display `display` (such as `:0`), which
    /// must be reachable; nothing is registered with the server until a
    /// watch starts or something is written.
    pub fn new(display: &str) -> LinuxClipboard {
        LinuxClipboard {
            display: display.into(),
            writer: Mutex::new(None),
        }
    }

    fn writer(&self) -> Result<Arc<Server>, String> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(server) = writer.as_ref() {
            return Ok(server.clone());
        }
        let server = Arc::new(Server::start(&self.display, true)?);
        *writer = Some(server.clone());
        Ok(server)
    }
}

impl ClipboardSystem for LinuxClipboard {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn watch(&self, sink: Arc<dyn Sink>) -> Result<Watch, String> {
        Watcher::start(&self.display, sink)
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.writer()?.put_text(text)
    }

    fn write_image(&self, png: &[u8]) -> Result<(), String> {
        self.writer()?.put_image(png)
    }

    fn write_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.writer()?.put_files(paths)
    }
}

/// This session's X11 clipboard, or why Linux cannot watch the clipboard
/// here. A Wayland session is refused even where XWayland runs: what
/// Wayland applications copy reaches X11 only through XWayland's bridge,
/// which Pane does not rely on.
pub fn native() -> Result<LinuxClipboard, String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty());
    let display = std::env::var("DISPLAY")
        .ok()
        .filter(|display| !display.is_empty());
    session(display.as_deref(), wayland)
}

/// Decides for a session whose X11 display is `display` (if it has one)
/// and which is Wayland or not, as [`native`] does for this one; separate
/// so the tests can ask for sessions this computer is not in.
fn session(display: Option<&str>, wayland: bool) -> Result<LinuxClipboard, String> {
    if wayland {
        return Err(
            "Not available on Linux with Wayland: Pane watches the clipboard through X11, and \
             what Wayland applications copy reaches it only through XWayland's bridge, which Pane \
             does not rely on. Run Pane in an X11 session"
                .into(),
        );
    }
    let Some(display) = display else {
        return Err("Not available: Pane is not running on an X11 or Wayland display".into());
    };
    // Connected to and dropped again: only that the display answers is
    // checked here, so nothing is registered with the server before a
    // watch starts.
    connect(display)
        .map(|_| ())
        .map_err(|problem| format!("Not available: {problem}"))?;
    Ok(LinuxClipboard::new(display))
}

/// Watching the X11 clipboard, until dropped: stops the watcher thread
/// within [`STOP_WAIT`], or leaves it to end on its own.
struct Listening {
    connection: Arc<RustConnection>,
    waker: Window,
    stopping: Arc<AtomicBool>,
    thread: Option<Joinable>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        wake(&self.connection, self.waker);
        if let Some(thread) = self.thread.take()
            && !thread.join_within(STOP_WAIT)
        {
            log(
                "Pane stopped watching the clipboard while a read was still waiting on the \
                 program that copied; it ends on its own",
            );
        }
    }
}

/// The watcher thread: its own connection, its window that receives the
/// changes and the answers, and the sink it reports to.
struct Watcher {
    connection: Arc<RustConnection>,
    window: Window,
    atoms: Atoms,
    sink: Arc<dyn Sink>,
    stopping: Arc<AtomicBool>,
    /// Events seen while waiting for one in particular, kept for the loop.
    pending: VecDeque<Event>,
}

/// What asking the owner for the clipboard as one target came to.
enum Transfer {
    /// The bytes it served (at most a little over the limit asked for).
    Served(Vec<u8>),
    /// The owner cannot serve this target.
    Refused,
    /// The owner did not answer in time, or the read failed: this change
    /// is skipped.
    Failed(String),
}

impl Watcher {
    /// Watches the X11 display `display`, reporting each later change of
    /// the `CLIPBOARD` selection to `sink` until the returned watch is
    /// dropped.
    fn start(display: &str, sink: Arc<dyn Sink>) -> Result<Watch, String> {
        let Endpoint {
            connection,
            window,
            waker,
            atoms,
        } = endpoint(display, EventMask::PROPERTY_CHANGE)?;
        connection
            .xfixes_query_version(5, 0)
            .map_err(why)?
            .reply()
            .map_err(why)?;
        connection
            .xfixes_select_selection_input(
                window,
                atoms.CLIPBOARD,
                SelectionEventMask::SET_SELECTION_OWNER
                    | SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )
            .map_err(why)?
            .check()
            .map_err(why)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let watcher = Watcher {
            connection: connection.clone(),
            window,
            atoms,
            sink,
            stopping: stopping.clone(),
            pending: VecDeque::new(),
        };
        let thread = Joinable::spawn("pane-clipboard", move || watcher.run())
            .map_err(|failure| format!("Pane could not watch the clipboard: {failure}"))?;
        Ok(Watch::new(Listening {
            connection,
            waker,
            stopping,
            thread: Some(thread),
        }))
    }

    /// Reports each change of the clipboard until the connection closes or
    /// the watch is dropped. The selection being emptied or its owner
    /// going away is not a change to read.
    fn run(mut self) {
        loop {
            let event = match self.next_event() {
                Some(event) => event,
                None => {
                    log("Pane stopped watching the clipboard: its X11 connection closed");
                    return;
                }
            };
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            if let Event::XfixesSelectionNotify(event) = event
                && event.selection == self.atoms.CLIPBOARD
                && event.subtype == SelectionEvent::SET_SELECTION_OWNER
                && event.owner != 0
            {
                // A panic while reading one change must not end the thread:
                // it goes on listening, and that report is lost.
                let owner = event.owner;
                let time = event.timestamp;
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| self.observe(owner, time)));
            }
        }
    }

    /// The next event, from those kept while a read waited or from the
    /// server; `None` once the connection is closed.
    fn next_event(&mut self) -> Option<Event> {
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        self.connection.wait_for_event().ok()
    }

    /// The next event that `wanted` answers for within `limit`, keeping the
    /// others for the loop; `None` when none came, the connection closed or
    /// the watch was dropped. Sleeps briefly between looks, so a stop is
    /// noticed within [`PARK`].
    fn wait_for(&mut self, limit: Duration, wanted: impl Fn(&Event) -> bool) -> Option<Event> {
        let deadline = Instant::now() + limit;
        loop {
            match self.connection.poll_for_event() {
                Ok(Some(event)) => {
                    if wanted(&event) {
                        return Some(event);
                    }
                    self.pending.push_back(event);
                }
                Ok(None) => {}
                Err(_) => return None,
            }
            if self.stopping.load(Ordering::SeqCst) || Instant::now() >= deadline {
                return None;
            }
            std::thread::park_timeout(PARK);
        }
    }

    /// Reads the change that the owner `owner` made at `time` and reports
    /// it, taking the ticket only once the read begins.
    fn observe(&mut self, owner: Window, time: Timestamp) {
        let ticket = self.sink.reading();
        let source = self.owner_program(owner);
        let content = self.content(time);
        self.sink.observed(
            ticket,
            Observation {
                content,
                markers: Markers::default(),
                source,
            },
        );
    }

    /// What is on the clipboard, as its owner serves it: the files it
    /// offers (`text/uri-list`), else its text — first UTF-8, then, if the
    /// owner refuses that, Latin-1 — else its image (`image/png`). The
    /// owner is asked what it offers first; one that does not say is asked
    /// for the text alone. An owner that answers none of them serves
    /// nothing Pane keeps.
    fn content(&mut self, time: Timestamp) -> Content {
        let failed = |failure: String| {
            log(&format!(
                "Pane could not read the clipboard, and skips this change: {failure}"
            ));
            Content::Other
        };
        let atom = Atom::from(AtomEnum::ATOM);
        let offered = match self.transfer(time, self.atoms.TARGETS, atom, TARGETS_BYTES) {
            Transfer::Served(bytes) => Some(atoms_of(&bytes)),
            Transfer::Refused => None,
            Transfer::Failed(failure) => return failed(failure),
        };
        let offers = |target: Atom| offered.as_ref().is_some_and(|all| all.contains(&target));

        let uri_list = self.atoms.URI_LIST;
        if offers(uri_list) {
            match self.transfer(time, uri_list, uri_list, URI_LIST_BYTES) {
                Transfer::Served(bytes) if bytes.len() > URI_LIST_BYTES => {
                    return Content::TooLarge;
                }
                Transfer::Served(bytes) => {
                    let files = files_of(&bytes);
                    if files.len() > MAX_FILES {
                        return Content::TooLarge;
                    }
                    if !files.is_empty() {
                        return Content::Files(files);
                    }
                }
                Transfer::Refused => {}
                Transfer::Failed(failure) => return failed(failure),
            }
        }

        let string = Atom::from(AtomEnum::STRING);
        let text_offered = offered.is_none() || offers(self.atoms.UTF8_STRING) || offers(string);
        if text_offered {
            for latin1 in [false, true] {
                let target = if latin1 {
                    string
                } else {
                    self.atoms.UTF8_STRING
                };
                match self.transfer(time, target, target, MAX_TEXT_BYTES) {
                    Transfer::Served(bytes) => return Content::Text(text_of(&bytes, latin1)),
                    Transfer::Refused => {}
                    Transfer::Failed(failure) => return failed(failure),
                }
            }
        }

        let png = self.atoms.IMAGE_PNG;
        if offers(png) {
            match self.transfer(time, png, png, MAX_IMAGE_BYTES) {
                Transfer::Served(bytes) if bytes.len() > MAX_IMAGE_BYTES => {
                    return Content::TooLarge;
                }
                Transfer::Served(bytes) => {
                    if let Some(image) = CopiedImage::from_png(bytes) {
                        return Content::Image(image);
                    }
                }
                Transfer::Refused => {}
                Transfer::Failed(failure) => return failed(failure),
            }
        }
        Content::Other
    }

    /// Asks the owner of the clipboard for it as `target`, reading its
    /// answer, of the type `kind`, into the watcher's own property. What
    /// is sent in pieces (`INCR`) is read piece by piece, and abandoned
    /// once it is larger than `limit`, which Pane keeps no more of.
    fn transfer(&mut self, time: Timestamp, target: Atom, kind: Atom, limit: usize) -> Transfer {
        let asked = self
            .connection
            .convert_selection(
                self.window,
                self.atoms.CLIPBOARD,
                target,
                self.atoms.PANE_SELECTION,
                time,
            )
            .map_err(why)
            .and_then(|cookie| cookie.check().map_err(why));
        if let Err(failure) = asked {
            return Transfer::Failed(failure);
        }
        let window = self.window;
        let property = self.atoms.PANE_SELECTION;
        let clipboard = self.atoms.CLIPBOARD;
        let answer = self.wait_for(ANSWER_WAIT, |event| {
            matches!(
                event,
                Event::SelectionNotify(answer)
                    if answer.selection == clipboard
                        && answer.requestor == window
                        && answer.target == target
            )
        });
        let Some(Event::SelectionNotify(answer)) = answer else {
            return Transfer::Failed(format!(
                "the program that copied did not answer within {} seconds",
                ANSWER_WAIT.as_secs()
            ));
        };
        // No property: the owner cannot serve this target.
        if answer.property == 0 {
            return Transfer::Refused;
        }
        let Ok(reply) = self
            .connection
            .get_property(false, window, property, AtomEnum::ANY, 0, read_units(limit))
            .map_err(why)
            .and_then(|cookie| cookie.reply().map_err(why))
        else {
            return Transfer::Failed("the answer could not be read".into());
        };
        if reply.type_ == self.atoms.INCR {
            // Deleting the property is what tells the owner to start
            // sending the pieces.
            let _ = self
                .connection
                .delete_property(window, property)
                .map_err(why)
                .and_then(|cookie| cookie.check().map_err(why));
            return self.pieces(kind, limit);
        }
        // Reading the property is the whole transfer; deleting it tells the
        // owner it is done with.
        let _ = self
            .connection
            .delete_property(window, property)
            .map_err(why)
            .and_then(|cookie| cookie.check().map_err(why));
        if reply.type_ != kind {
            return Transfer::Refused;
        }
        Transfer::Served(reply.value)
    }

    /// The pieces of what the owner sends in pieces, of the type `kind`,
    /// each read by deleting the property, until the empty last piece;
    /// abandoned (with the property deleted, which the owner sees) once it
    /// is larger than `limit`.
    fn pieces(&mut self, kind: Atom, limit: usize) -> Transfer {
        let window = self.window;
        let property = self.atoms.PANE_SELECTION;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let piece = self.wait_for(PIECE_WAIT, |event| {
                matches!(
                    event,
                    Event::PropertyNotify(piece)
                        if piece.window == window
                            && piece.atom == property
                            && piece.state == Property::NEW_VALUE
                )
            });
            if piece.is_none() {
                return Transfer::Failed("the program that copied stopped sending its copy".into());
            }
            let Ok(reply) = self
                .connection
                .get_property(false, window, property, AtomEnum::ANY, 0, read_units(limit))
                .map_err(why)
                .and_then(|cookie| cookie.reply().map_err(why))
            else {
                return Transfer::Failed("a piece of the copy could not be read".into());
            };
            // Deleting the property asks for the next piece.
            let _ = self
                .connection
                .delete_property(window, property)
                .map_err(why)
                .and_then(|cookie| cookie.check().map_err(why));
            if reply.type_ != kind {
                return Transfer::Refused;
            }
            if reply.value.is_empty() {
                return Transfer::Served(bytes);
            }
            bytes.extend_from_slice(&reply.value);
            if bytes.len() > limit {
                log("Pane stopped reading a copy larger than it keeps");
                return Transfer::Served(bytes);
            }
        }
    }

    /// The program that owns the clipboard, if its window says which it is:
    /// the process `_NET_WM_PID` names, or the window's class (which
    /// usually is the program's name). A window that says neither has an
    /// unknown owner, which is never excluded.
    fn owner_program(&self, owner: Window) -> Option<String> {
        let pid = self.number(owner, self.atoms.NET_WM_PID)?;
        program_of(pid).or_else(|| self.window_class(owner))
    }

    /// The first number of a `CARDINAL` property of `window`, if it has
    /// one.
    fn number(&self, window: Window, property: Atom) -> Option<u32> {
        let reply = self
            .connection
            .get_property(false, window, property, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        reply.value32().and_then(|mut values| values.next())
    }

    /// The class of `window` (the second name its `WM_CLASS` holds).
    fn window_class(&self, window: Window) -> Option<String> {
        let reply = self
            .connection
            .get_property(
                false,
                window,
                Atom::from(AtomEnum::WM_CLASS),
                AtomEnum::STRING,
                0,
                256,
            )
            .ok()?
            .reply()
            .ok()?;
        class_of(&reply.value)
    }
}

/// The text `bytes` hold, as the target they were asked for says: Latin-1
/// or UTF-8, read lossily, ending at its first NUL as the Windows reader's
/// does.
fn text_of(bytes: &[u8], latin1: bool) -> String {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let text = &bytes[..end];
    if latin1 {
        text.iter().map(|&byte| byte as char).collect()
    } else {
        String::from_utf8_lossy(text).into_owned()
    }
}

/// The atoms of an `ATOM` property's value `bytes`, as the server sends
/// them (32 bits each, in this connection's byte order, which x11rb makes
/// the host's).
fn atoms_of(bytes: &[u8]) -> Vec<Atom> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|atom| u32::from_ne_bytes(*atom))
        .collect()
}

/// The files of a `text/uri-list` (RFC 2483): one URI a line, lines
/// starting with `#` being comments; each `file://` URI of this computer
/// (no host, or `localhost`) as its path, percent-decoded, in order. Other
/// URIs are not files and are left out.
fn files_of(list: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(list)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(path_of_uri)
        .collect()
}

/// The path a `file://` URI of this computer names, percent-decoded.
fn path_of_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let host = &rest[..slash];
    if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let encoded = &rest.as_bytes()[slash..];
    let mut path = Vec::with_capacity(encoded.len());
    let mut at = 0;
    while at < encoded.len() {
        let hex = |byte: u8| (byte as char).to_digit(16);
        match (encoded[at], encoded.get(at + 1), encoded.get(at + 2)) {
            (b'%', Some(&high), Some(&low)) if hex(high).is_some() && hex(low).is_some() => {
                path.push((hex(high)? * 16 + hex(low)?) as u8);
                at += 3;
            }
            (byte, _, _) => {
                path.push(byte);
                at += 1;
            }
        }
    }
    Some(PathBuf::from(OsString::from_vec(path)))
}

/// The `file://` URI of the absolute path `path`, as file managers write
/// it: every byte but the unreserved ones and `/` percent-encoded.
fn uri_of(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The class of a `WM_CLASS` value, its second NUL-terminated name.
fn class_of(value: &[u8]) -> Option<String> {
    let mut names = value.splitn(3, |&byte| byte == 0);
    let _instance = names.next()?;
    let class = names.next()?;
    let class = String::from_utf8_lossy(class).into_owned();
    (!class.is_empty()).then_some(class)
}

/// The file name of the process `pid`, as `/proc` names it: the file its
/// executable is, or the process's name when that cannot be read (another
/// user's process, a replaced file); `None` when neither can. The
/// executable's path carries " (deleted)" once its file was replaced,
/// which is not part of the name.
fn program_of(pid: u32) -> Option<String> {
    let executable = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|path| {
            let path = path.to_string_lossy();
            let path = path.strip_suffix(" (deleted)").unwrap_or(&path);
            Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    let name = executable.or_else(|| {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|name| name.lines().next().unwrap_or_default().to_owned())
    });
    name.filter(|name| !name.is_empty())
}

/// Serves what Pane (or its tests) put on the clipboard: the X11 clipboard
/// is only what its owner serves, so a window of Pane's own takes the
/// `CLIPBOARD` selection and answers whoever pastes, until another program
/// copies. One thread of Pane's own does the answering, on a connection of
/// its own, because it must keep answering after the watch is dropped, for
/// as long as Pane runs.
struct Server {
    connection: Arc<RustConnection>,
    window: Window,
    atoms: Atoms,
    /// What to serve, as the target it is served in; empty once another
    /// program took the clipboard.
    value: Arc<Mutex<Served>>,
    stopping: Arc<AtomicBool>,
    waker: Window,
    thread: Option<Joinable>,
}

impl Server {
    /// Starts serving on `display`; with `identify`, the window says Pane's
    /// process (`_NET_WM_PID`), so a watcher names the program that put
    /// what it serves.
    fn start(display: &str, identify: bool) -> Result<Server, String> {
        let Endpoint {
            connection,
            window,
            waker,
            atoms,
        } = endpoint(
            display,
            EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY,
        )?;
        if identify {
            let pid = std::process::id();
            connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    atoms.NET_WM_PID,
                    Atom::from(AtomEnum::CARDINAL),
                    &[pid],
                )
                .map_err(why)?
                .check()
                .map_err(why)?;
        }
        let value = Arc::new(Mutex::new(Served::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let serving = Serving {
            connection: connection.clone(),
            window,
            atoms,
            value: value.clone(),
            stopping: stopping.clone(),
        };
        let thread = Joinable::spawn("pane-clipboard-write", move || serving.run())
            .map_err(|failure| format!("Pane could not put text on the clipboard: {failure}"))?;
        Ok(Server {
            connection,
            window,
            atoms,
            value,
            stopping,
            waker,
            thread: Some(thread),
        })
    }

    /// Puts `text` on the clipboard, replacing what was there, served as
    /// `UTF8_STRING` (and as `TARGETS` for whoever asks what there is).
    fn put_text(&self, text: &str) -> Result<(), String> {
        let targets = vec![(self.atoms.UTF8_STRING, text.as_bytes().to_vec())];
        self.put(targets)
    }

    /// Puts the image `png` on the clipboard, replacing what was there,
    /// served as `image/png` (#167).
    fn put_image(&self, png: &[u8]) -> Result<(), String> {
        self.put(vec![(self.atoms.IMAGE_PNG, png.to_vec())])
    }

    /// Puts the files `paths` on the clipboard, replacing what was there,
    /// as file managers copy them (#167): their `file://` URIs as
    /// `text/uri-list`, and as `x-special/gnome-copied-files` (GNOME
    /// Files' own, saying they are copied, not cut); their paths as
    /// `UTF8_STRING`, for pasting into text.
    fn put_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        let uris: Vec<String> = paths.iter().map(|path| uri_of(path)).collect();
        let mut list = uris.join("\r\n");
        list.push_str("\r\n");
        let gnome = format!("copy\n{}", uris.join("\n"));
        let text = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        self.put(vec![
            (self.atoms.URI_LIST, list.into_bytes()),
            (self.atoms.GNOME_COPIED_FILES, gnome.into_bytes()),
            (self.atoms.UTF8_STRING, text.into_bytes()),
        ])
    }

    /// Takes the `CLIPBOARD` selection with what `targets` serve,
    /// replacing what was on the clipboard. What it serves is in place
    /// before the ownership, so no request is answered with the old text.
    fn put(&self, targets: Served) -> Result<(), String> {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = targets;
        self.connection
            .set_selection_owner(self.window, self.atoms.CLIPBOARD, x11rb::CURRENT_TIME)
            .map_err(why)?
            .check()
            .map_err(why)?;
        let owner = self
            .connection
            .get_selection_owner(self.atoms.CLIPBOARD)
            .map_err(why)?
            .reply()
            .map_err(why)?;
        if owner.owner != self.window {
            return Err("another program took the clipboard at once".into());
        }
        Ok(())
    }

    /// The atom named `name`, for what a test puts on the clipboard.
    fn atom(&self, name: &str) -> Result<Atom, String> {
        self.connection
            .intern_atom(false, name.as_bytes())
            .map_err(why)?
            .reply()
            .map(|reply| reply.atom)
            .map_err(why)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        wake(&self.connection, self.waker);
        if let Some(thread) = self.thread.take() {
            thread.join_within(STOP_WAIT);
        }
    }
}

/// The server's thread: answers whoever pastes until Pane stops, then
/// offers the selection to the clipboard manager.
struct Serving {
    connection: Arc<RustConnection>,
    window: Window,
    atoms: Atoms,
    value: Arc<Mutex<Served>>,
    stopping: Arc<AtomicBool>,
}

impl Serving {
    fn run(self) {
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                self.hand_over();
                return;
            }
            match self.connection.wait_for_event() {
                Ok(event) => self.handle(event),
                Err(_) => return,
            }
        }
    }

    fn handle(&self, event: Event) {
        match event {
            Event::SelectionRequest(request) => self.answer(request),
            // Another program copied: Pane's text is no longer the
            // clipboard's, so nothing is served any more.
            Event::SelectionClear(_) => {
                *self
                    .value
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Vec::new();
            }
            _ => {}
        }
    }

    /// Answers `request` with what is served, or refuses it, and tells the
    /// asking program so. The property to answer into is the one it asked
    /// for, or the target's own name when it named none.
    fn answer(&self, request: SelectionRequestEvent) {
        let property = if request.property == 0 {
            request.target
        } else {
            request.property
        };
        let value = self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let served = if request.target == self.atoms.TARGETS {
            let mut targets = vec![self.atoms.TARGETS];
            targets.extend(value.iter().map(|(target, _)| *target));
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    Atom::from(AtomEnum::ATOM),
                    &targets,
                )
                .map_err(why)
                .and_then(|cookie| cookie.check().map_err(why))
                .is_ok()
        } else if let Some((target, bytes)) =
            value.iter().find(|(target, _)| *target == request.target)
        {
            self.connection
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    *target,
                    bytes,
                )
                .map_err(why)
                .and_then(|cookie| cookie.check().map_err(why))
                .is_ok()
        } else {
            false
        };
        drop(value);
        let answer = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: request.sequence,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if served {
                property
            } else {
                Atom::from(AtomEnum::NONE)
            },
        };
        if let Ok(cookie) =
            self.connection
                .send_event(false, request.requestor, EventMask::NO_EVENT, answer)
        {
            cookie.ignore_error();
        }
        let _ = self.connection.flush();
    }

    /// Offers the selection to the clipboard manager, as the ICCCM says an
    /// owner should before going away, and serves its requests for a
    /// moment, so what Pane put stays on the clipboard where a manager
    /// runs. Without one (bare Xvfb, most light window managers) it goes
    /// with Pane, as any program's copy does.
    fn hand_over(&self) {
        let serving = !self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty();
        if !serving {
            return;
        }
        let manager = self
            .connection
            .get_selection_owner(self.atoms.CLIPBOARD_MANAGER)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map_or(0, |owner| owner.owner);
        if manager == 0 {
            return;
        }
        let offered = self
            .connection
            .convert_selection(
                self.window,
                self.atoms.CLIPBOARD_MANAGER,
                self.atoms.SAVE_TARGETS,
                self.atoms.PANE_SELECTION,
                x11rb::CURRENT_TIME,
            )
            .map_err(why)
            .and_then(|cookie| cookie.check().map_err(why));
        if offered.is_err() {
            return;
        }
        let _ = self.connection.flush();
        let deadline = Instant::now() + HANDOVER_WAIT;
        while Instant::now() < deadline {
            match self.connection.poll_for_event() {
                Ok(Some(Event::SelectionRequest(request))) => self.answer(request),
                Ok(Some(_)) => {}
                Ok(None) => std::thread::park_timeout(PARK),
                Err(_) => return,
            }
        }
    }
}

/// For the Linux adapter's test: putting text on the clipboard from a
/// window of this process, as another program copying text would, or
/// putting a target no text can be read from. It replaces what is on the
/// clipboard, which is not saved.
#[doc(hidden)]
pub mod testing {
    use super::Server;

    /// A window of this process that owns what the test put on the
    /// clipboard, serving it until dropped.
    pub struct Owner(#[allow(dead_code)] Server);

    /// Puts `text` on the clipboard, owned by a window of this process
    /// that says its process (`_NET_WM_PID`), as a program copying text
    /// does.
    pub fn set_text(text: &str) -> Result<Owner, String> {
        let server = Server::start(&display()?, true)?;
        server.put_text(text)?;
        Ok(Owner(server))
    }

    /// Puts `bytes` on the clipboard as `target` (such as `image/png`),
    /// owned by a window of this process that names no process: a copy no
    /// text can be read from.
    pub fn set_target(target: &str, bytes: &[u8]) -> Result<Owner, String> {
        let server = Server::start(&display()?, false)?;
        let atom = server.atom(target)?;
        server.put(vec![(atom, bytes.to_vec())])?;
        Ok(Owner(server))
    }

    fn display() -> Result<String, String> {
        std::env::var("DISPLAY").map_err(|_| "no DISPLAY is set".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{atoms_of, class_of, files_of, program_of, session, text_of, uri_of};

    /// #167: a file manager's `text/uri-list` is read as its files, in
    /// order, percent-decoded; comments, other hosts and other schemes are
    /// left out; and Pane writes a path as the URI it reads back.
    #[test]
    fn copied_files_are_read_from_and_written_as_a_uri_list() {
        let list = b"# copied by Files\r\nfile:///home/me/a%20b.txt\r\n\
            file://localhost/tmp/caf%C3%A9\r\nfile://other-host/x\r\nhttps://example.com\r\n\
            file:///home/me/100%25\r\n";
        assert_eq!(
            files_of(list),
            [
                PathBuf::from("/home/me/a b.txt"),
                PathBuf::from("/tmp/café"),
                PathBuf::from("/home/me/100%"),
            ]
        );
        assert!(files_of(b"").is_empty());
        let path = Path::new("/home/me/My Notes/été #1.md");
        let uri = uri_of(path);
        assert_eq!(uri, "file:///home/me/My%20Notes/%C3%A9t%C3%A9%20%231.md");
        assert_eq!(files_of(uri.as_bytes()), [path.to_path_buf()]);
    }

    #[test]
    fn the_targets_an_owner_offers_are_read_as_atoms() {
        let mut bytes = Vec::new();
        for atom in [1u32, 300, 70_000] {
            bytes.extend_from_slice(&atom.to_ne_bytes());
        }
        assert_eq!(atoms_of(&bytes), [1, 300, 70_000]);
        assert!(atoms_of(&[1, 2, 3]).is_empty(), "a partial atom is none");
    }

    #[test]
    fn text_is_read_as_latin_1_or_utf_8_and_ends_at_its_first_nul() {
        assert_eq!(text_of("héllo ✓".as_bytes(), false), "héllo ✓");
        assert_eq!(text_of(b"caf\xe9", true), "café");
        // Some programs end their text with a NUL, as the Windows format
        // does; none is kept as part of the text.
        assert_eq!(text_of(b"one\0two", false), "one");
        assert_eq!(text_of(b"\0", true), "");
        // Invalid UTF-8 is read lossily, as the Windows reader does.
        assert_eq!(text_of(&[0xff, 0xfe], false), "\u{fffd}\u{fffd}");
    }

    #[test]
    fn the_class_of_a_window_is_its_second_name() {
        assert_eq!(
            class_of(b"keepassxc\0keepassxc\0").as_deref(),
            Some("keepassxc")
        );
        assert_eq!(
            class_of(b"\0gnome-terminal\0").as_deref(),
            Some("gnome-terminal")
        );
        assert_eq!(
            class_of(b"instance\0class\0more\0").as_deref(),
            Some("class")
        );
        assert_eq!(class_of(b"only-one"), None);
        assert_eq!(class_of(b""), None);
    }

    #[test]
    fn the_program_of_a_process_is_its_executable_or_its_name() {
        let own = std::env::current_exe().unwrap();
        let expected = own.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            program_of(std::process::id()).as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(program_of(u32::MAX), None);
    }

    #[test]
    fn wayland_and_no_display_sessions_cannot_watch() {
        let wayland = refused(session(Some(":0"), true));
        assert!(
            wayland.starts_with("Not available on Linux with Wayland"),
            "{wayland}"
        );
        assert!(wayland.contains("XWayland"), "{wayland}");
        let displayless = refused(session(None, false));
        assert_eq!(
            displayless,
            "Not available: Pane is not running on an X11 or Wayland display"
        );
    }

    #[test]
    fn a_display_that_does_not_answer_is_refused() {
        let unreachable = refused(session(Some(":pane-does-not-exist"), false));
        assert!(unreachable.starts_with("Not available: "), "{unreachable}");
        assert!(unreachable.contains("could not reach"), "{unreachable}");
    }

    /// Why `result` was refused, refusing to accept a clipboard.
    fn refused(result: Result<super::LinuxClipboard, String>) -> String {
        match result {
            Err(reason) => reason,
            Ok(_) => panic!("expected the session to be refused"),
        }
    }
}
