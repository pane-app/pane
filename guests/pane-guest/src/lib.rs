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
//! [`helpers`], list the files of a folder with [`files`], search as the
//! user types into its own search field with [`search`], make web
//! requests with [`http`] and keep clipboard history with
//! [`clipboard_history`]. The crate is
//! `no_std` so the component imports only WASI 0.3 interfaces; it supplies the
//! allocator and a panic handler that traps, which the host reports as a
//! runtime error.
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
    path: "../../wit",
    world: "extension-with-data",
    pub_export_macro: true,
    default_bindings_module: "pane_guest",
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
/// use pane_guest::window::{PopToRootType, close};
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
/// let preferences: Preferences = pane_guest::preferences::values()?;
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
/// use pane_guest::commands::{CommandRef, LaunchType, launch};
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
/// pane_guest::export!(Greeter);
/// pane_guest::publish::export!(Greeter);
/// ```
pub mod publish {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "operations-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::publish",
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
/// pane_guest::export!(Calculator);
/// pane_guest::root::export!(Calculator);
/// ```
pub mod root {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "root-results-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::root",
    });

    pub use exports::pane::extension::root_results::{Guest, RootAction, RootResult};
}

/// The applications installed on the system (`pane:extension/applications`),
/// which Pane finds and opens for the extension: [`applications::installed`] and
/// [`applications::open`].
pub mod applications {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "applications-user",
        default_bindings_module: "pane_guest::applications",
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
        path: "../../wit",
        world: "clipboard-history-user",
        default_bindings_module: "pane_guest::clipboard_history",
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
        path: "../../wit",
        world: "helpers-user",
        default_bindings_module: "pane_guest::helpers",
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
        path: "../../wit",
        world: "files-user",
        default_bindings_module: "pane_guest::files",
    });

    pub use pane::extension::files::{
        FolderListing, FolderState, FoundFile, ScanLimits, limits, list_folder,
    };
}

/// Root results a command supplies ahead of the query
/// (`pane:extension/indexed-results`), such as the installed applications,
/// which root search matches by title like commands. A command whose
/// `pane.json` entry sets `"indexedResults": true` implements
/// [`indexed::Guest`] too and calls [`indexed::export!`](crate::indexed::export)
/// beside [`export!`]:
///
/// ```ignore
/// pane_guest::export!(Applications);
/// pane_guest::indexed::export!(Applications);
/// ```
pub mod indexed {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "indexed-results-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::indexed",
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
/// pane_guest::export!(Packages);
/// pane_guest::search::export!(Packages);
/// ```
pub mod search {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "command-search-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::search",
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
/// pane_guest::export!(Watching);
/// pane_guest::service::export!(Watching);
/// ```
pub mod service {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "service-provider",
        pub_export_macro: true,
        default_bindings_module: "pane_guest::service",
    });

    pub use exports::pane::extension::service::{Cycle, Guest};
}

pub mod http;

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

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
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
