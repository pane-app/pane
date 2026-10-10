//! Guest-side bindings for Pane's `pane:extension` contract.
//!
//! An extension implements [`Command`] and calls [`export!`]: its screen is
//! a [`List`] of [`Item`]s whose actions are closures, which the SDK hands
//! Pane as the versioned JSON tree of ADR 0036's envelope (`render` and
//! `handle-event`) and runs when the user chooses them; its items may have
//! icons, accessories and tooltips ([`icon`]). Or, for a no-view
//! command, [`Command::run`] runs each time it is launched. Every command
//! receives its launch record and may launch another command with
//! [`commands`]. It tells the user what happened with a toast or a HUD
//! ([`feedback`]), and may close Pane's window or pop back to root search
//! ([`window`]); Pane shows nothing of an action's answer. It may keep
//! values between runs with [`settings`], and its own records, disposable
//! values and secrets with [`content`], [`cache`] and [`credentials`], and
//! read the preferences its package declares with [`preferences`]. It
//! may compute results from root search's query with [`root`], run a continuing
//! service while its package's code may run with [`service`], call
//! operations other packages publish with [`operations::call`], serve those
//! its own package publishes with [`publish`], find and open installed
//! applications with [`applications`], supply root results ahead of the
//! query with [`indexed`], run its package's native helpers with
//! [`helpers`] and the system's own programs with [`programs`], list the
//! files of a folder with [`files`] and the entries of a folder the user
//! typed with [`typed_folder`], search as the
//! user types into its own search field with [`search`], make web
//! requests with [`http`] and keep clipboard history with
//! [`clipboard_history`]. It prints and logs to its package's extension log
//! with [`info!`], [`warn!`], [`println!`] and the like ([`log`]). The crate
//! is `no_std` so the component imports only WASI 0.3 interfaces; it supplies
//! the allocator and a panic handler that logs the panic and traps, which the
//! host reports as a runtime error.
//!
//! Without `std` no libc is linked, so the crate also supplies what the
//! compiler and the component runtime call into libc or `std` for:
//! `memcmp` and `bcmp`, which the compiler emits for byte and string
//! comparisons (`==` on `str`, `starts_with`, ...) as soon as a guest compares
//! strings, and the canonical-ABI `cabi_realloc`.
#![no_std]

pub extern crate alloc;

use core::ffi::c_void;

wit_bindgen::generate!({
    path: "wit",
    world: "extension-with-data",
    pub_export_macro: true,
    default_bindings_module: "pane_extension",
    // No function names the form and custom-view records any more: the tree
    // carries them as JSON. Authors still build them as these types.
    generate_unused_types: true,
});

pub use exports::pane::extension::command::{
    Choice, CustomView, CustomViewInfo, CustomViewRole, Field, FieldKind, FieldValue, Form,
    FormError, Frame, GuestCustomView, Key, Platform, Point, Rect, Shape, Text, TextField,
    ViewEvent,
};
pub use list::{Action, Command, Item, List, Modifier, Shortcut, Submenu};
pub use pane::extension::commands::{LaunchRecord, LaunchSource, LaunchType};

pub mod actions;
pub mod feedback;
pub mod icon;
mod list;
pub mod system;
pub use icon::{Accessory, Color, Icon, Mask, Tint, Tone};
pub use pane::extension::{cache, content, credentials, operations, settings};

/// Pane's launcher window, as the command that runs in it sees it
/// (`pane:extension/window`): [`window::close`] hides it, choosing what its
/// next showing shows ([`window::PopToRootType`]) and whether root search's
/// query is emptied; [`window::pop_to_root`] returns to root search with
/// the window open; [`window::clear_search`] empties the search field on
/// screen. Each answers whether a window was shown for the call: in a
/// background launch, a schedule or a service, it does nothing and answers
/// false.
///
/// ```ignore
/// use pane_extension::window::{PopToRootType, close};
///
/// close(true, PopToRootType::Immediate);
/// ```
pub mod window {
    pub use crate::pane::extension::window::{PopToRootType, clear_search, close, pop_to_root};
}

/// The preferences the command's package declares in `pane.json` under
/// `preferences`, for the whole extension or for one command, as the user
/// set them in Pane (`pane:extension/preferences`): on the Setup screen
/// before the command's first run, and on the extension's card in
/// Settings. Pane stores them; a command only reads them, as a type of its
/// own that serde deserializes. A checkbox's value is a `bool`, every other
/// kind's a `String`; a preference with no value and no default is absent,
/// so declare an optional one as an `Option`:
///
/// ```ignore
/// #[derive(serde::Deserialize)]
/// #[serde(rename_all = "camelCase")]
/// struct Preferences {
///     api_key: String,
///     units: String,
///     greeting: Option<String>,
///     verbose: bool,
/// }
///
/// let preferences: Preferences = pane_extension::preferences::values()?;
/// ```
pub mod preferences {
    use alloc::string::String;
    use serde::de::DeserializeOwned;

    /// The effective preference values of the command Pane is running:
    /// its package's preferences, then its own, each the value the user set
    /// or else its declared default. An error says why Pane refused, or
    /// why they do not deserialize into `T`.
    pub fn values<T: DeserializeOwned>() -> Result<T, String> {
        read(None)
    }

    /// Like [`values`], for the command with id `command` (in `pane.json`)
    /// of the same package: for a component serving several commands, in a
    /// call Pane makes for no command in particular (its root results).
    pub fn values_of<T: DeserializeOwned>(command: &str) -> Result<T, String> {
        read(Some(command))
    }

    /// The effective values as Pane sends them, a JSON object's text.
    pub fn json(command: Option<&str>) -> Result<String, String> {
        crate::pane::extension::preferences::values(command)
    }

    fn read<T: DeserializeOwned>(command: Option<&str>) -> Result<T, String> {
        let text = json(command)?;
        serde_json::from_str(&text)
            .map_err(|error| alloc::format!("the preferences do not fit their type: {error}"))
    }
}

/// How the command was launched, and launching another command
/// (`pane:extension/commands`). A no-view command's [`Command::run`]
/// receives its [`LaunchRecord`]; a view command's [`Command::render`]
/// reads it with [`commands::current`]. [`commands::launch`] opens or runs
/// another command of the package (by its id in `pane.json`) or of another
/// installed package (by its package identity), passing JSON context:
///
/// ```ignore
/// use pane_extension::commands::{CommandRef, LaunchType, launch};
///
/// let own = CommandRef { source: None, command: "report".into() };
/// launch(&own, LaunchType::Background, &[], Some(r#"{"from":"launch"}"#))?;
/// ```
///
/// [`commands::set_subtitle`] replaces the subtitle the command's own row
/// shows in root search (`Some("3 unread")`), until it is set again; `None`
/// gives back the one its `pane.json` entry declares.
pub mod commands {
    use core::cell::RefCell;

    pub use crate::pane::extension::commands::{
        ArgumentValue, CommandRef, LaunchRecord, LaunchSource, LaunchType, launch, set_subtitle,
    };

    /// The launch record of the call in progress.
    struct Current(RefCell<Option<LaunchRecord>>);

    // SAFETY: a component's code runs on one thread, and no borrow is held
    // across an `await`.
    unsafe impl Sync for Current {}

    static CURRENT: Current = Current(RefCell::new(None));

    /// The launch record of the command's screen being drawn (in
    /// [`Command::render`](crate::Command::render), and in the actions of
    /// the list it drew), or of the run in progress: how the command was
    /// launched, and with what. Its `command` is the id in `pane.json` of
    /// the command launched, so that a component serving several view
    /// commands draws the screen of the one opened. A launch by the user
    /// from root search with nothing more (and no command) before Pane has
    /// said.
    pub fn current() -> LaunchRecord {
        CURRENT.0.borrow().clone().unwrap_or(LaunchRecord {
            launch_type: LaunchType::UserInitiated,
            source: LaunchSource::RootSearch,
            arguments: alloc::vec::Vec::new(),
            fallback_text: None,
            context: None,
            command: alloc::string::String::new(),
        })
    }

    /// Notes the record Pane passed to the call in progress.
    pub(crate) fn set_current(launch: LaunchRecord) {
        *CURRENT.0.borrow_mut() = Some(launch);
    }

    /// `launch`'s type in words, such as "by the user", for reporting it.
    pub fn launch_type_name(launch_type: LaunchType) -> &'static str {
        match launch_type {
            LaunchType::UserInitiated => "user-initiated",
            LaunchType::Background => "background",
        }
    }

    /// `source` as `wit/commands.wit` names it, such as "root-search".
    pub fn source_name(source: LaunchSource) -> &'static str {
        match source {
            LaunchSource::RootSearch => "root-search",
            LaunchSource::Alias => "alias",
            LaunchSource::Fallback => "fallback",
            LaunchSource::Hotkey => "hotkey",
            LaunchSource::QuickSlot => "quick-slot",
            LaunchSource::Command => "command",
            LaunchSource::Schedule => "schedule",
        }
    }
}

impl LaunchRecord {
    /// The value of the command's argument `name` (`"arguments"` in its
    /// `pane.json` entry), if it has one: an optional argument left empty
    /// is absent.
    pub fn argument(&self, name: &str) -> Option<&str> {
        self.arguments
            .iter()
            .find(|argument| argument.name == name)
            .map(|argument| argument.value.as_str())
    }
}

impl operations::CallErrorKind {
    /// The kind's WIT name, such as `not-found`, as JavaScript sees it too.
    pub fn name(&self) -> &'static str {
        use operations::CallErrorKind::*;
        match self {
            NotFound => "not-found",
            Disabled => "disabled",
            Incompatible => "incompatible",
            Unavailable => "unavailable",
            Failed => "failed",
            Crashed => "crashed",
            Refused => "refused",
        }
    }
}

impl operations::CallError {
    /// `<kind>: <message>`, such as "failed: a name is needed", to show
    /// people.
    pub fn explain(&self) -> alloc::string::String {
        alloc::format!("{}: {}", self.kind.name(), self.message)
    }
}

/// Serving the operations a package publishes
/// (`pane:extension/published-operations`). The component its `pane.json`
/// names under `operations` implements [`publish::Guest`] too and calls
/// [`publish::export!`](crate::publish::export) beside [`export!`]:
///
/// ```ignore
/// pane_extension::export!(Greeter);
/// pane_extension::publish::export!(Greeter);
/// ```
pub mod publish {
    wit_bindgen::generate!({
        path: "wit",
        world: "operations-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_extension::publish",
    });

    pub use exports::pane::extension::published_operations::Guest;
}

/// Results a command computes from root search's query
/// (`pane:extension/root-results`), such as a calculator's answer. A command
/// whose `pane.json` entry sets `"rootResults": true` implements
/// [`root::Guest`] too and calls [`root::export!`](crate::root::export)
/// beside [`export!`]:
///
/// ```ignore
/// pane_extension::export!(Calculator);
/// pane_extension::root::export!(Calculator);
/// ```
///
/// Pane asks with the query and when it was typed ([`root::WallTime`]), so a
/// command can answer about the current date or time. A result that is an
/// answer card can say more than its title and action in
/// [`root::AnswerDetail`]: the section it sits under, a colour swatch, and
/// further ways to copy it.
///
/// A command whose only job is this, as the calculator's, also says
/// `"mode": "provider"` (a root provider): it has no row of its own and
/// Pane never opens or runs it, so its [`Command`](crate::Command) keeps
/// the defaults (`type CustomView = NoCustomView;` and nothing else).
pub mod root {
    wit_bindgen::generate!({
        path: "wit",
        world: "root-results-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_extension::root",
    });

    pub use exports::pane::extension::root_results::{
        AnswerCopy, AnswerDetail, Guest, RootAction, RootResult, WallTime,
    };
}

/// The applications installed on the system (`pane:extension/applications`),
/// which Pane finds and opens for the extension: [`applications::installed`] and
/// [`applications::open`]. An application's `id` is opaque and stable across
/// its updates and Pane's restarts, so a command may keep it in its data.
/// Its `name` is the one the system shows in the user's language; its
/// `alternate_titles` (its untranslated and program names) and `keywords`
/// find it too, so give them to an [`indexed::IndexedResult`]; its
/// `distinction`, when another application has its name, tells them apart
/// as a subtitle.
pub mod applications {
    wit_bindgen::generate!({
        path: "wit",
        world: "applications-user",
        default_bindings_module: "pane_extension::applications",
    });

    pub use pane::extension::applications::{Application, installed, open};
}

/// Clipboard history (`pane:extension/clipboard-history`), which Pane keeps
/// for the command's package once the user turned it on: plain text the
/// user copies while the package runs and the history is not paused, except
/// what the copying application marked as not to be kept or what came from
/// a program the user excluded. [`clipboard_history::set_capture`] turns it
/// on, off or pauses it; [`clipboard_history::entries`] lists what is kept.
pub mod clipboard_history {
    wit_bindgen::generate!({
        path: "wit",
        world: "clipboard-history-user",
        default_bindings_module: "pane_extension::clipboard_history",
    });

    pub use pane::extension::clipboard_history::{
        Capture, Entry, HistoryStatus, clear, copy, delete_items, entries, set_capture,
        set_excluded, set_retention, status, turn_off_and_clear,
    };
}

/// Native helpers (`pane:extension/helpers`): prebuilt programs the
/// command's own package ships, one per system, which Pane runs for it with
/// [`helpers::run`]. Declare them under `helpers` in `pane.json`. Dropping
/// the future of a run before it resolves (for example when a timer wins a
/// race with it) cancels it: Pane ends the helper's process.
pub mod helpers {
    wit_bindgen::generate!({
        path: "wit",
        world: "helpers-user",
        default_bindings_module: "pane_extension::helpers",
    });

    pub use pane::extension::helpers::{HelperError, HelperErrorKind, run};

    impl HelperErrorKind {
        /// The kind's WIT name, such as `not-found`.
        pub fn name(&self) -> &'static str {
            match self {
                HelperErrorKind::NotFound => "not-found",
                HelperErrorKind::Unavailable => "unavailable",
                HelperErrorKind::Failed => "failed",
                HelperErrorKind::Refused => "refused",
            }
        }
    }
}

/// The files of the folder the user granted the command's package
/// (`pane:extension/files`), which Pane lists for it under its scan limits
/// ([`files::limits`]): [`files::list_folder`] answers at once, with the
/// listing Pane keeps for this visit of root search, or that it is still
/// listing (Pane asks the command again when it is done), or that no folder
/// is granted. The package's `pane.json` sets `"folderAccess": true`; the
/// user chooses the folder in Pane's own row, and the extension never sees
/// its path. A command answers `open-file` results
/// ([`root::RootAction::OpenFile`]) with the files' ids.
pub mod files {
    wit_bindgen::generate!({
        path: "wit",
        world: "files-user",
        default_bindings_module: "pane_extension::files",
    });

    pub use pane::extension::files::{
        FolderListing, FolderState, FoundFile, ScanLimits, limits, list_folder,
    };
}

/// Pane's file index (`pane:extension/file-index`): the names of the files
/// and folders under the user's home folder (and the folders the user
/// adds), which Pane keeps current from the system's change records.
/// [`file_index::search`] answers at once from what is indexed, each entry
/// with the id Pane gave it and its path, name, folder, kind, size and
/// modified time; [`file_index::status`] says whether the index is being
/// built, is current or stopped. The package's `pane.json` sets
/// `"fileIndex": true`, so that Pane keeps the index current while it is
/// enabled. A command answers `open-file` results
/// ([`root::RootAction::OpenFile`]) or search results whose `file` is an
/// entry's id; Pane checks the entry again before acting on it, and its
/// Enter never runs a program.
pub mod file_index {
    wit_bindgen::generate!({
        path: "wit",
        world: "file-index-user",
        default_bindings_module: "pane_extension::file_index",
    });

    pub use pane::extension::file_index::{
        Category, EntryKind, FileEntry, IndexState, IndexStatus, SearchOptions, Sort, search,
        status,
    };

    impl SearchOptions {
        /// The first `limit` entries by relevance, of any kind.
        pub fn first(limit: u32) -> SearchOptions {
            SearchOptions {
                kind: None,
                category: None,
                sort: Sort::Relevance,
                limit,
                offset: 0,
            }
        }
    }
}

/// The entries of a folder the user typed into root search
/// (`pane:extension/typed-folder`, #204): Pane's host lists the folder for
/// the command, within bounds like the granted folder's scan policy (the
/// direct entries only, folders first and each in name order, at most 500,
/// a partial listing saying so), since a pure WASI guest has no folders to
/// read. No folder is granted: the user named it, so the command passes
/// what the user typed and Pane resolves it (`~` to the home folder,
/// `file://` taken off). [`typed_folder::list`] answers the entries, each
/// with the id Pane gave it, its name, whether it is a folder and whether
/// opening it would run a program; a command answers `open-file` results
/// ([`root::RootAction::OpenFile`]) with the ids, and Pane checks each
/// entry again before acting on it.
pub mod typed_folder {
    wit_bindgen::generate!({
        path: "wit",
        world: "typed-folder-user",
        default_bindings_module: "pane_extension::typed_folder",
    });

    pub use pane::extension::typed_folder::{FolderEntry, FolderListing, list};
}

/// Root results a command supplies ahead of the query
/// (`pane:extension/indexed-results`), such as the installed applications,
/// which root search matches by title like commands, by each of an
/// [`indexed::IndexedResult`]'s `alternate_titles` as by its title, and by
/// its `keywords` as by its subtitle (empty lists for none). A command whose
/// `pane.json` entry sets `"indexedResults": true` implements
/// [`indexed::Guest`] too and calls [`indexed::export!`](crate::indexed::export)
/// beside [`export!`]:
///
/// ```ignore
/// pane_extension::export!(Applications);
/// pane_extension::indexed::export!(Applications);
/// ```
///
/// A command whose only job is this, as Applications', also says `"mode":
/// "provider"` (a root provider): it has no row of its own, each result it
/// supplies is its own root result, and Pane never opens or runs it, so
/// its [`Command`](crate::Command) keeps the defaults.
pub mod indexed {
    wit_bindgen::generate!({
        path: "wit",
        world: "indexed-results-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_extension::indexed",
    });

    pub use exports::pane::extension::indexed_results::{
        Guest, IndexedAction, IndexedResult, OpenTarget,
    };
}

/// A command that searches as the user types into its own search field
/// (`pane:extension/command-search`), such as one searching an online
/// service. Pane asks it only once the user has opened it, never while they
/// type in root search. A command whose `pane.json` entry sets
/// `"search": true` implements [`search::Guest`] too and calls
/// [`search::export!`](crate::search::export) beside [`export!`]. Choosing
/// a result calls [`crate::Command::run_search_result`] with its id, so the id
/// should say which result it is:
///
/// ```ignore
/// pane_extension::export!(Packages);
/// pane_extension::search::export!(Packages);
/// ```
pub mod search {
    wit_bindgen::generate!({
        path: "wit",
        world: "command-search-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_extension::search",
    });

    pub use exports::pane::extension::command_search::{Guest, SearchResult};
}

/// A continuing service a command runs while its package's code may run
/// (`pane:extension/service`), at the cadence the service itself chooses:
/// Pane calls `run-cycle` from when the code may run (the package is
/// installed enabled, enabled again, replaced, or Pane starts) until it
/// may not (disabled, uninstalled, paused, replaced), each cycle answering
/// the status to show and how long to wait before the next. A command whose
/// `pane.json` entry sets `"service": true` implements
/// [`service::Guest`] too and calls
/// [`service::export!`](crate::service::export) beside [`export!`]:
///
/// ```ignore
/// pane_extension::export!(Watching);
/// pane_extension::service::export!(Watching);
/// ```
pub mod service {
    wit_bindgen::generate!({
        path: "wit",
        world: "service-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_extension::service",
    });

    pub use exports::pane::extension::service::{Cycle, Guest};
}

pub mod http;
pub mod log;
pub mod programs;

/// Logs a line at debug level to the package's extension log, formatted as
/// [`alloc::format!`] formats (see [`log`]).
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Debug, ::core::format_args!($($arg)*))
    };
}

/// Logs a line at info level to the package's extension log (see [`log`]).
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Info, ::core::format_args!($($arg)*))
    };
}

/// Logs a warning to the package's extension log (see [`log`]).
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Warn, ::core::format_args!($($arg)*))
    };
}

/// Logs an error to the package's extension log (see [`log`]).
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Error, ::core::format_args!($($arg)*))
    };
}

/// Prints to standard output: a line of the package's extension log at info
/// level, which this call ends (see [`log`]).
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::log::print($crate::log::Output::Stdout, ::core::format_args!($($arg)*))
    };
}

/// Prints a line to standard output (see [`print!`]).
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("")
    };
    ($($arg:tt)*) => {
        $crate::print!($($arg)*)
    };
}

/// Prints to standard error: a line of the package's extension log at error
/// level, which this call ends (see [`log`]).
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::log::print($crate::log::Output::Stderr, ::core::format_args!($($arg)*))
    };
}

/// Prints a line to standard error (see [`eprint!`]).
#[macro_export]
macro_rules! eprintln {
    () => {
        $crate::eprint!("")
    };
    ($($arg:tt)*) => {
        $crate::eprint!($($arg)*)
    };
}

/// The custom view type of a command that has none: `type CustomView =
/// NoCustomView;` in its [`Command`] implementation, with an `open_view`
/// that returns `Err`. It has no values, so no view of it can be opened.
pub enum NoCustomView {}

impl GuestCustomView for NoCustomView {
    async fn render(&self) -> Frame {
        match *self {}
    }

    async fn handle_event(&self, _event: ViewEvent) -> Result<(), alloc::string::String> {
        match *self {}
    }
}

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

/// Logs the panic's message and location to the package's extension log,
/// then traps; a panic while logging it traps at once.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    static PANICKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !PANICKED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        log::panicked(info);
    }
    core::arch::wasm32::unreachable()
}

/// Byte comparison, normally supplied by libc. The compiler emits calls to it
/// for slice and string comparisons (`==` on `str`, `starts_with`, ...).
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    for i in 0..n {
        // Byte by byte through volatile reads, so that the compiler cannot
        // turn this loop back into a call to memcmp.
        let (x, y) = unsafe { (a.add(i).read_volatile(), b.add(i).read_volatile()) };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}

/// Equality-only form of [`memcmp`], which the compiler may call instead.
///
/// # Safety
/// `a` and `b` must be valid for reads of `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

/// Canonical-ABI allocation entry point, normally supplied by `std`.
///
/// # Safety
/// Called only by the component runtime with valid allocation metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, realloc};
    if new_size == 0 {
        return align as *mut u8;
    }
    let result = if old_size == 0 {
        unsafe { alloc(Layout::from_size_align_unchecked(new_size, align)) }
    } else {
        unsafe {
            realloc(
                old,
                Layout::from_size_align_unchecked(old_size, align),
                new_size,
            )
        }
    };
    if result.is_null() {
        core::arch::wasm32::unreachable();
    }
    result
}
