//! The extension runtime: a Wasmtime engine that registers only WASI 0.3.
//!
//! The runtime owns every engine, store and guest instance on one dedicated
//! thread. Callers hold a cheap [`Runtime`] handle and await replies, so a slow
//! or failing guest never blocks the caller's thread.
//!
//! The thread serves many calls at once (#136): while a call waits on
//! something outside its guest (the user, a system program, a native
//! helper, a paste, a web request, a clock, another extension's operation),
//! the thread runs other packages' and other commands' calls. Each
//! instance's own calls stay one after another: a call waits for its
//! instance's turn, in the order the calls were asked for, so a guest never
//! sees two of its calls interleaved. A wait is never charged as the
//! guest's computing. Every await point is the same: a host import's future
//! that the guest awaits, so a new kind of wait (a confirmation, a program
//! run) needs no change here. A call into an installed package belongs to
//! the package's generation (see `generation`): when it ends, a pending call
//! of it stops where the guest waits, and one queued behind is never
//! started. Component checks run on a checker thread of their own, so a
//! reload's check never waits behind the call it is about to stop.
//!
//! A crash of the runtime thread itself (a panic, not a guest trap) stops
//! every call it held without sending any again; Pane restarts the thread,
//! unless it crashed shortly before (see `supervisor`).
//!
//! A guest that stops cooperating is bounded too (see `deadlines`): every
//! guest yields to the runtime thread at each epoch tick, a call computing
//! for too long without finishing is stopped as unresponsive (its package's
//! own failure), and a runtime thread that stops responding altogether is
//! given up on and replaced, as a crashed one is.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{self, Poll};

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::packages::paused_reason;
use wasmtime::component::{Component, Linker, ResourceAny, ResourceTable};
use wasmtime::{Cache, CacheConfig, Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use crate::http;

pub(crate) mod deadlines;
mod faults;
mod memory;
mod supervisor;
mod tree;

use deadlines::Doing;
#[doc(hidden)]
pub use deadlines::Limits;
pub use deadlines::{COMPUTE_LIMIT, UNRESPONSIVE_LIMIT, WARN_AFTER};
pub(crate) use deadlines::{HostCall, Hosted, Watch};
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub use faults::Fault;
#[cfg(any(test, debug_assertions))]
use faults::Fault as InjectedFault;
use faults::Faults;
pub use memory::GUEST_MEMORY;
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub use memory::peaks::memory_peak;
pub(crate) use supervisor::CRASH_WINDOW;
use supervisor::{NotSent, Shared};
pub use supervisor::{RuntimeFailure, RuntimeStatus};
pub use tree::{
    Action, ActionKind, ActionStyle, ActionSubmenu, Answer, Item, SubmenuEntries, TREE_VERSION,
    View,
};

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "extension-with-clipboard",
        imports: {
            "pane:extension/operations": store,
            "pane:extension/helpers": store,
            // Saving waits for the file to be written, off the runtime
            // thread, which awaits it.
            "pane:extension/settings.set": async,
            "pane:extension/content.set": async,
            "pane:extension/cache.set": async,
            "pane:extension/credentials.set": async,
        },
        exports: { default: async | store },
    });
}

/// The `root-results` export of a command that computes root results.
mod root_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "root-results-provider",
        exports: { default: async | store },
    });
}

/// The `indexed-results` export of a command that supplies root results
/// ahead of the query.
mod indexed_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "indexed-results-provider",
        exports: { default: async | store },
    });
}

/// The `command-search` export of a command that searches as the user types
/// into its own search field.
mod search_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "command-search-provider",
        exports: { default: async | store },
    });
}

/// The `service` export of a command that runs a continuing service.
mod service_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "service-provider",
        exports: { default: async | store },
    });
}

/// The `published-operations` export of a component serving operations.
mod operations_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "operations-provider",
        exports: { default: async | store },
    });
}

use bindings::exports::pane::extension::command;
use bindings::pane::extension::commands as launching;
use bindings::pane::extension::{
    applications, cache, clipboard_history, content, credentials, settings,
};
use indexed_bindings::exports::pane::extension::indexed_results;
use root_bindings::exports::pane::extension::root_results;

use crate::applications::Applications;
use crate::clipboard::{self, Capture, CaptureState};
use crate::extension_data::{DataKind, PackageData};
use crate::files::{FileAccess, Folders};
use crate::generation::{End, Fence, Generation, Registration};
use crate::helpers;
use crate::helpers::runner::{self, HelperError, HelperErrorKind, Helpers, Running, Spec};
use crate::launch::{LaunchRecord, LaunchRequest, LaunchSource, LaunchType, Launches};
use crate::operations::{self, Directory, OperationCall, OperationError, Target};
use crate::packages::EXTENSION_API;

/// Interface-version prefix every imported WASI interface must carry.
const WASI_VERSION: &str = "@0.3.";

/// The interface an extension command exports.
const COMMAND_INTERFACE: &str = "pane:extension/command@0.1.0";

/// The interface a command that computes root results also exports.
const ROOT_RESULTS_INTERFACE: &str = "pane:extension/root-results@0.1.0";

/// The interface a command that supplies root results ahead of the query
/// also exports.
const INDEXED_RESULTS_INTERFACE: &str = "pane:extension/indexed-results@0.1.0";

/// The interface a component serving published operations also exports.
const OPERATIONS_INTERFACE: &str = "pane:extension/published-operations@0.1.0";

/// The interface a command that searches as the user types also exports.
const COMMAND_SEARCH_INTERFACE: &str = "pane:extension/command-search@0.1.0";

/// The interface a command that runs a continuing service also exports.
const SERVICE_INTERFACE: &str = "pane:extension/service@0.1.0";

/// What a result a command answers with shows as a row: the fields its
/// computed root results, indexed results and search results share (each
/// interface's WIT declares its own record, as a WIT record cannot extend
/// another).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResultListing {
    /// Identifies the result among the command's results; a search result's
    /// is the callback id passed to the command's `handle-event` when its
    /// row is activated.
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
}

/// One thing a command's search found, listed as a row of the command.
pub(crate) type SearchResult = ResultListing;

/// Stops a search that is no longer needed: when it is stopped or dropped,
/// the search is not started if it has not been, and stopped where its guest
/// waits if it has (see [`Runtime::search_with`]).
pub(crate) struct StopSearch(#[allow(dead_code)] oneshot::Sender<()>);

/// Tells the runtime that a search was stopped.
struct SearchStopped(oneshot::Receiver<()>);

impl SearchStopped {
    /// Whether the search has been stopped.
    fn stopped(&mut self) -> bool {
        !matches!(self.0.try_recv(), Err(oneshot::error::TryRecvError::Empty))
    }

    /// Waits until the search is stopped or `wait` ends, whichever comes
    /// first; whether it was stopped (also when both are).
    async fn stopped_before(&mut self, wait: impl Future<Output = ()>) -> bool {
        let mut wait = std::pin::pin!(wait);
        std::future::poll_fn(|context| {
            if std::pin::Pin::new(&mut self.0).poll(context).is_ready() {
                return std::task::Poll::Ready(true);
            }
            wait.as_mut().poll(context).map(|()| false)
        })
        .await
    }
}

/// What a search waits on before it starts, given [`SEARCH_DEBOUNCE`]: the
/// clock's sleep, unless a test sets another
/// (`Runtime::set_search_timer`, in debug builds).
#[cfg(any(test, debug_assertions))]
type SearchTimer = Arc<
    dyn Fn(std::time::Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// How long the runtime waits before it starts a search: one the user
/// replaces by typing on within it is stopped before its command is asked
/// (and before its instance could be dropped for it), so fast typing asks
/// only for the text the user stops at. The runtime serves other calls
/// meanwhile.
pub(crate) const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

/// A way to stop a search, and what the runtime watches for it.
fn stoppable() -> (StopSearch, SearchStopped) {
    let (stop, stopped) = oneshot::channel();
    (StopSearch(stop), SearchStopped(stopped))
}

/// A result a command computed from root search's query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootResult {
    pub listing: ResultListing,
    pub action: RootAction,
}

/// What invoking a computed root result does; Pane performs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RootAction {
    /// Copy this text to the clipboard.
    Copy(String),
    /// Open this web address with the system's link handler.
    OpenUrl(String),
    /// Open this file with the system's handler for its type.
    OpenFile(String),
}

/// A root result a command supplies ahead of the query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexedResult {
    pub listing: ResultListing,
    pub action: IndexedAction,
}

/// What invoking an indexed root result does; Pane performs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexedAction {
    /// Open the installed application with this id.
    OpenApplication(String),
}

/// What a component exports besides `command`, as its package manifest
/// says, for [`Runtime::check_with`] to confirm.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Exports {
    /// `root-results`: it computes root results from the query.
    pub root_results: bool,
    /// `indexed-results`: it supplies root results ahead of the query.
    pub indexed_results: bool,
    /// `published-operations`: it serves published operations.
    pub operations: bool,
    /// `command-search`: it searches as the user types into its own search
    /// field.
    pub search: bool,
    /// `service`: it runs a continuing service while the package's code
    /// may run.
    pub service: bool,
}

/// The system's applications as the runtime's guests and the launcher see
/// them; replaceable, for tests.
type SharedApplications = Arc<Mutex<Arc<dyn Applications>>>;

/// Where the runtime's guests keep clipboard history, once the launcher
/// said (see [`Runtime::set_clipboard`]); held weakly, since the launcher
/// owns it and Pane stops watching the clipboard once it is dropped.
type SharedClipboard = Arc<Mutex<Option<std::sync::Weak<Capture>>>>;

/// The installed packages as the launcher has them, once it has said, for
/// resolving operation calls and finding a guest's helpers.
type SharedDirectory = Arc<Mutex<Option<Directory>>>;

/// What starts the commands guests launch (`pane:extension/commands`),
/// once the launcher has said: the launcher's own.
type SharedLaunches = Arc<Mutex<Option<Launches>>>;

/// What Pane shows of an item's custom view besides the view's drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomViewInfo {
    /// The screen's title.
    pub title: String,
    /// Names the view to assistive technology.
    pub label: String,
    pub role: CustomViewRole,
}

/// What kind of control a custom view is to assistive technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomViewRole {
    /// A color chooser; its frame's value names the chosen color.
    ColorWell,
}

/// What a custom view shows: shapes painted in order over a
/// `width` x `height` area of logical pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub shapes: Vec<Shape>,
    /// The view's current value for assistive technology.
    pub value: String,
}

/// The most shapes a frame may have.
pub const MAX_FRAME_SHAPES: usize = 4096;
/// The most characters a text shape may have.
pub const MAX_TEXT_CHARS: usize = 256;
/// The largest width and height of a frame, in logical pixels.
pub const MAX_FRAME_SIZE: u32 = 4096;

impl Frame {
    /// Why the frame is over one of Pane's limits, if it is. The window
    /// draws each shape as an element, so a frame from an extension is
    /// bounded before it reaches the window.
    fn over_limits(&self) -> Option<String> {
        if self.shapes.len() > MAX_FRAME_SHAPES {
            return Some(format!(
                "the frame has {} shapes; at most {MAX_FRAME_SHAPES} are drawn",
                self.shapes.len()
            ));
        }
        if self.width > MAX_FRAME_SIZE || self.height > MAX_FRAME_SIZE {
            return Some(format!(
                "the frame is {} x {} pixels; at most {MAX_FRAME_SIZE} x {MAX_FRAME_SIZE} are drawn",
                self.width, self.height
            ));
        }
        self.shapes.iter().find_map(|shape| match shape {
            Shape::Text { content, .. } if content.chars().count() > MAX_TEXT_CHARS => {
                Some(format!(
                    "a text of the frame has {} characters; at most {MAX_TEXT_CHARS} are drawn",
                    content.chars().count()
                ))
            }
            _ => None,
        })
    }
}

/// One thing a custom view draws. Coordinates are logical pixels from the
/// view's top-left corner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        fill: Rgb,
    },
    /// One line of text, its top-left corner at `x`, `y`.
    Text {
        x: i32,
        y: i32,
        content: String,
        color: Rgb,
    },
}

/// An opaque color as 0xRRGGBB, the WIT `rgb`; the top byte is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u32);

/// A position in a custom view, in logical pixels from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// The keys a focused custom view receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// The user's input to a custom view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewEvent {
    Key(Key),
    /// The primary pointer button was pressed over the view.
    PointerDown(Point),
    /// The pointer moved while that button is held.
    PointerMove(Point),
    /// That button was released.
    PointerUp(Point),
}

/// Identifies a custom view open in a [`Runtime`]. Ids are never reused,
/// even by a runtime thread that replaced a crashed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewId {
    /// The number of the runtime thread holding the view.
    thread: u64,
    id: u64,
}

impl ViewId {
    /// The number of the runtime thread that holds the view: a crash of
    /// that thread (see [`supervisor::CrashReport`]) closes it.
    pub(crate) fn thread(&self) -> u64 {
        self.thread
    }
}

/// A form an item opens, as produced by the guest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub submit_label: String,
}

/// One field of a [`Form`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
}

/// What a field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A single-line text field, which starts empty.
    Text { placeholder: Option<String> },
    /// Exactly one of these options; the first starts chosen.
    Choice(Vec<Choice>),
}

/// An option of a choice field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// A field's submitted value: its text, or the chosen option's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldValue {
    pub id: String,
    pub value: String,
}

/// Why the guest did not accept a submitted form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormError {
    /// The field the message is about; `None` for the form as a whole.
    pub field: Option<String>,
    pub message: String,
}

/// What one cycle of a continuing service answers: the status to show on
/// the command's screen and how long Pane waits before the next cycle
/// (`pane:extension/service`, which the launcher's services thread runs
/// while the package's code may run).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cycle {
    /// The status to show, as an action's answer is shown.
    pub status: String,
    /// How long to wait before the next cycle, in seconds.
    pub next_seconds: u64,
}

/// Why a call into an extension did not produce a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The runtime could not start, or has stopped.
    RuntimeUnavailable(String),
    /// The component could not be read or compiled.
    Load(String),
    /// The component needs interfaces Pane does not provide, such as WASI 0.2.
    Incompatible(Vec<String>),
    /// The component does not implement Pane's extension interface.
    Interface(String),
    /// The component exports Pane's extension interface, but with functions
    /// or types of another shape of the same API version: it was built
    /// against an older contract, and must be rebuilt.
    OlderApiShape(String),
    /// The guest ran and reported an error.
    Guest(String),
    /// The guest did not accept a submitted form.
    Form(FormError),
    /// The guest answered something Pane cannot read: a tree that is not
    /// JSON, or lacks a field Pane needs (see `tree`). The command's
    /// failure, as an error it answered with is, never a crash.
    Unreadable(String),
    /// The guest trapped or otherwise failed while running.
    Trap(String),
    /// The guest computed for too long without finishing (see
    /// [`COMPUTE_LIMIT`]), so Pane stopped the call and dropped its
    /// instance.
    Unresponsive(String),
    /// The command's package is disabled, so none of its code runs. A call
    /// pending when it was disabled is stopped with this.
    Disabled,
    /// The command's code was replaced by a reload or an update while the
    /// call was pending, so the call was stopped and its answer discarded.
    Replaced,
    /// The command's package was uninstalled while the call was pending, so
    /// the call was stopped and its answer discarded.
    Uninstalled,
    /// Pane paused the command's package after it failed (it could not
    /// start, or crashed too often), so none of its code runs
    /// until the user retries it. A call pending when it was paused is
    /// stopped with this.
    Paused,
    /// The custom view was closed, or its guest instance has stopped, so it
    /// cannot handle events any more.
    ViewClosed,
    /// The caller no longer wanted the answer (root search's query changed,
    /// or root search was left), so the call was not started, or was stopped
    /// where the guest waited, with its instance.
    Cancelled,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Disabled => write!(f, "The extension is disabled"),
            CallError::Replaced => write!(
                f,
                "The extension was reloaded or updated while this was running; try again"
            ),
            CallError::Uninstalled => write!(f, "The extension was uninstalled"),
            CallError::Paused => f.write_str(&paused_reason("The extension")),
            CallError::RuntimeUnavailable(reason) => {
                write!(f, "Extension runtime unavailable: {reason}")
            }
            CallError::Load(reason) => write!(f, "Could not load the extension: {reason}"),
            CallError::Incompatible(imports) => write!(
                f,
                "Incompatible extension: Pane supports only WASI 0.3, but it imports {}",
                imports.join(", ")
            ),
            CallError::Interface(reason) => write!(
                f,
                "Incompatible extension: it does not implement Pane's extension interface: {reason}"
            ),
            CallError::OlderApiShape(reason) => write!(
                f,
                "Incompatible extension: it was built for an older extension API shape: \
                 rebuild it against Pane's current extension API {}.{} ({reason})",
                EXTENSION_API.0, EXTENSION_API.1
            ),
            CallError::Guest(message) => write!(f, "The extension reported an error: {message}"),
            CallError::Form(error) => f.write_str(&error.message),
            CallError::Unreadable(reason) => {
                write!(
                    f,
                    "Pane could not read what the extension answered: {reason}"
                )
            }
            CallError::Trap(reason) => write!(f, "The extension crashed: {reason}"),
            CallError::Unresponsive(reason) => {
                write!(f, "The extension stopped responding: {reason}")
            }
            CallError::ViewClosed => f.write_str("The extension's view is no longer open"),
            CallError::Cancelled => f.write_str("The search was cancelled"),
        }
    }
}

impl std::error::Error for CallError {}

/// A handle to the runtime thread. Cloning shares the same runtime, also
/// once it was restarted after its thread crashed.
#[derive(Clone)]
pub struct Runtime {
    /// The thread serving calls now, and the helper processes guests
    /// started, which end once every handle is dropped.
    shared: Arc<Shared>,
    /// Component checks, served one at a time by the checker thread, apart
    /// from the runtime thread: a reload's check must not wait behind the
    /// guest call the reload is about to stop.
    checks: std::sync::mpsc::Sender<Check>,
}

/// A handle to the runtime thread that does not keep it running: the
/// thread stops once every [`Runtime`] is dropped, even while something it
/// holds (such as its health report) holds one of these.
#[derive(Clone)]
pub(crate) struct WeakRuntime {
    shared: std::sync::Weak<Shared>,
    checks: std::sync::mpsc::Sender<Check>,
}

impl WeakRuntime {
    /// The runtime, unless every handle to it was dropped.
    pub(crate) fn upgrade(&self) -> Option<Runtime> {
        Some(Runtime {
            shared: self.shared.upgrade()?,
            checks: self.checks.clone(),
        })
    }
}

/// A component check for the checker thread.
struct Check {
    component: PathBuf,
    exports: Exports,
    reply: oneshot::Sender<Result<Checked, CallError>>,
}

/// What checking a component found out about it besides that Pane can run
/// it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Checked {
    /// It imports `wasi:http`: its code can make web requests.
    pub network: bool,
}

enum Request {
    Render {
        component: PathBuf,
        launch: LaunchRecord,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<View, CallError>>,
    },
    HandleEvent {
        component: PathBuf,
        callback: String,
        details: String,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Answer, CallError>>,
    },
    RunItem {
        component: PathBuf,
        item_id: String,
        launch: LaunchRecord,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Answer, CallError>>,
    },
    Run {
        component: PathBuf,
        command: String,
        launch: LaunchRecord,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Answer, CallError>>,
    },
    RunCycle {
        component: PathBuf,
        command: String,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Cycle, CallError>>,
    },
    IndexedResults {
        component: PathBuf,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Vec<IndexedResult>, CallError>>,
    },
    RootResults {
        component: PathBuf,
        query: String,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<Vec<RootResult>, CallError>>,
    },
    Search {
        component: PathBuf,
        command: String,
        query: String,
        data: Option<PackageData>,
        stopped: SearchStopped,
        reply: oneshot::Sender<Result<Vec<SearchResult>, CallError>>,
    },
    Forget {
        components: Vec<PathBuf>,
    },
    SubmitForm {
        component: PathBuf,
        item_id: String,
        values: Vec<FieldValue>,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<String, CallError>>,
    },
    OpenView {
        component: PathBuf,
        item_id: String,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<(ViewId, Frame), CallError>>,
    },
    ViewEvent {
        view: ViewId,
        event: ViewEvent,
        reply: oneshot::Sender<Result<Frame, CallError>>,
    },
    CloseView {
        view: ViewId,
    },
    ViewCount {
        reply: oneshot::Sender<Result<usize, CallError>>,
    },
    Running {
        reply: oneshot::Sender<Result<Vec<PathBuf>, CallError>>,
    },
    /// Drops the idle instances whose generation ended, as an instance's
    /// entry on its generation's undo list asks.
    DropStopped,
    /// Pane is quitting: the thread stops, dropping every call in progress
    /// with its instance, which ends what it waits on.
    Quit,
}

impl Request {
    /// The component this request's call runs in, when it is a call the
    /// user asked the package for — opening (or drawing again) a command,
    /// handling an event of its list, running an item, running a no-view
    /// command the user launched, a command's search, a form submission or
    /// opening a custom view. Not the background and ambient ones (a
    /// scheduled run, a background launch, a service cycle, a root search's
    /// ask), which a replacement of the package's code ends and its new code
    /// restarts or re-asks, nor a view event, whose screen is what the
    /// launcher checks.
    fn user_component(&self) -> Option<&Path> {
        match self {
            Request::Run {
                component, launch, ..
            } if !launch.is_background() => Some(component),
            Request::Render { component, .. }
            | Request::HandleEvent { component, .. }
            | Request::RunItem { component, .. }
            | Request::Search { component, .. }
            | Request::SubmitForm { component, .. }
            | Request::OpenView { component, .. } => Some(component),
            _ => None,
        }
    }
}

/// How a call into an installed package's code failed, for deciding
/// whether to pause the package (see the launcher's `pausing`). Only the
/// package's own failures are reported: an error the guest answers with is
/// not one, nor is a call stopped because its generation, or that of a
/// caller in its chain, ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Health {
    /// The guest trapped while running: a crash.
    Crashed(CallError),
    /// The guest computed for too long without finishing, and was stopped
    /// ([`CallError::Unresponsive`]). Wasmtime was running its code, so the
    /// failure is its own.
    Unresponsive(CallError),
    /// The component could not be loaded or instantiated.
    FailedToStart(CallError),
}

/// Told of each failure of a call into an installed package's code: its
/// component, the extension data (and so the generation) it ran with, and
/// how it failed. Called on the runtime thread before the call's answer is
/// sent.
pub(crate) type HealthReport = Arc<dyn Fn(&Path, &PackageData, Health) + Send + Sync>;

impl Runtime {
    /// A handle that does not keep the runtime thread running.
    pub(crate) fn downgrade(&self) -> WeakRuntime {
        WeakRuntime {
            shared: Arc::downgrade(&self.shared),
            checks: self.checks.clone(),
        }
    }

    /// Starts the runtime thread. Extensions are compiled on every start.
    pub fn start() -> Result<Runtime, CallError> {
        Runtime::start_with(None)
    }

    /// Starts the runtime thread, keeping compiled extension code in
    /// `cache_dir` so later runtimes load it instead of recompiling. The
    /// directory holds only disposable data.
    pub fn start_with_cache(cache_dir: PathBuf) -> Result<Runtime, CallError> {
        Runtime::start_with(Some(cache_dir))
    }

    fn start_with(cache_dir: Option<PathBuf>) -> Result<Runtime, CallError> {
        let engine = engine(cache_dir.clone())?;
        let applications: SharedApplications = Arc::new(Mutex::new(crate::applications::native()));
        let code = Arc::new(Code::new(engine));
        let (checks, pending_checks) = std::sync::mpsc::channel::<Check>();
        let checker = code.clone();
        std::thread::Builder::new()
            .name("pane-extension-check".into())
            .spawn(move || {
                for check in pending_checks {
                    // A check that panics answers so, and the next one is
                    // still checked.
                    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        checker.check(&check.component, check.exports)
                    }))
                    .unwrap_or_else(|_| {
                        Err(CallError::RuntimeUnavailable(
                            "checking the component crashed Pane's checker".into(),
                        ))
                    });
                    let _ = check.reply.send(checked);
                }
            })
            .map_err(unavailable)?;
        let shared = Shared::start(code, applications, Helpers::default(), cache_dir)?;
        Ok(Runtime { shared, checks })
    }

    /// What the runtime is doing after a crash of its thread, if it had one.
    pub fn status(&self) -> RuntimeStatus {
        self.shared.status()
    }

    /// Starts the runtime again after its thread crashed and Pane did not
    /// restart it by itself ([`RuntimeStatus::Stopped`]); nothing that was
    /// running before is run again. A runtime that runs is left as it is.
    pub(crate) fn restart(&self) -> Result<(), CallError> {
        self.shared.restart()
    }

    /// Injects `fault` into the runtime thread serving calls now, to check
    /// that Pane recovers. For tests and the native smokes only; debug
    /// builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn inject(&self, fault: InjectedFault) {
        self.shared.inject(fault);
    }

    /// Sets the limits the runtime applies to guest calls and to its own
    /// thread from now on ([`Limits`]), so tests and smokes reach them
    /// quickly. For tests and the native smokes only; debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn set_limits(&self, limits: Limits) {
        self.shared.set_limits(limits);
    }

    /// The limits the runtime applies ([`Limits`]).
    pub(crate) fn limits(&self) -> Limits {
        self.shared.limits()
    }

    /// Sets the ceilings of guests' web requests started from now on, so
    /// tests can reach them quickly. For tests only; debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn set_http_limits(&self, limits: http::HttpLimits) {
        self.shared.network.set_limits(limits);
    }

    /// Has every search the runtime starts from now on wait for the future
    /// `timer` returns (given [`SEARCH_DEBOUNCE`]) instead of the clock, so
    /// a test decides when the wait ends, and sees each search reach it,
    /// however slow the machine is. For tests only; debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn set_search_timer(
        &self,
        timer: impl Fn(std::time::Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync
        + 'static,
    ) {
        *lock(&self.shared.search_timer) = Some(Arc::new(timer));
    }

    /// `host:port` of every address the package with identity key `owner`
    /// tried to reach this session, sorted.
    pub(crate) fn contacted(&self, owner: &str) -> Vec<String> {
        self.shared.network.contacted(owner)
    }

    /// Injects a fault each time a file appears at `file`, then removes it:
    /// `crash` injects [`Fault::Crash`], `crash-before-answer:<item>`
    /// [`Fault::CrashBeforeAnswer`] for the action `<item>`, `hang`
    /// [`Fault::Hang`] and `release` [`Fault::Release`];
    /// `limits:<compute>,<warn>,<unresponsive>` (whole seconds) sets the
    /// runtime's [`Limits`]. For the native
    /// smokes, which set `PANE_TEST_RUNTIME_FAULTS`; the file is looked for
    /// every 100 ms, by a thread that stops once every handle to the
    /// runtime is dropped. Debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn watch_fault_file(&self, file: PathBuf) {
        let runtime = self.downgrade();
        let _ = std::thread::Builder::new()
            .name("pane-runtime-faults".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let Some(runtime) = runtime.upgrade() else {
                        return;
                    };
                    let Ok(text) = std::fs::read_to_string(&file) else {
                        continue;
                    };
                    let _ = std::fs::remove_file(&file);
                    let text = text.trim();
                    match text.strip_prefix("crash-before-answer:") {
                        Some(item) => runtime.inject(InjectedFault::CrashBeforeAnswer {
                            item: item.to_owned(),
                        }),
                        None if text == "crash" => runtime.inject(InjectedFault::Crash),
                        None if text == "hang" => runtime.inject(InjectedFault::Hang),
                        None if text == "release" => runtime.inject(InjectedFault::Release),
                        None => match parse_limits(text) {
                            Some(limits) => runtime.set_limits(limits),
                            None => {
                                eprintln!("PANE_TEST_RUNTIME_FAULTS: unknown fault {text:?}")
                            }
                        },
                    }
                }
            });
    }

    /// Tells `report` of each crash of the runtime thread, after Pane
    /// restarted it or chose not to.
    pub(crate) fn set_crash_report(&self, report: supervisor::CrashReport) {
        self.shared.set_crash_report(report);
    }

    /// Tells `report` when the runtime thread is not responding yet, and
    /// when it carries on (see [`supervisor::SlowReport`]).
    pub(crate) fn set_slow_report(&self, report: supervisor::SlowReport) {
        self.shared.set_slow_report(report);
    }

    /// Asks the command in `component` to draw its screen (`render`), and
    /// reads the list its tree describes. The command has no extension
    /// data, and is launched by the user from root search.
    pub async fn render(&self, component: &Path) -> Result<View, CallError> {
        self.render_with(component, None).await
    }

    /// Like [`Runtime::render`], with the launch record `launch`.
    pub async fn render_launched(
        &self,
        component: &Path,
        launch: &LaunchRecord,
    ) -> Result<View, CallError> {
        self.render_launched_with(component, launch, None).await
    }

    /// Runs the no-view command with manifest id `command` in `component`
    /// (`run`), launched as `launch` says, and reads its answer. The
    /// command has no extension data.
    pub async fn run_command(
        &self,
        component: &Path,
        command: &str,
        launch: &LaunchRecord,
    ) -> Result<Answer, CallError> {
        self.run_command_with(component, command, launch, None)
            .await
    }

    /// Like [`Runtime::run_command`]; the command reads and saves `data`.
    /// Starts its instance if it has none.
    pub(crate) async fn run_command_with(
        &self,
        component: &Path,
        command: &str,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::Run {
                component: component.to_path_buf(),
                command: command.to_owned(),
                launch: launch.clone(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Has the command in `component` handle the user's choice of
    /// `callback`, a callback id its tree named (or a search result's id),
    /// with `details`, a JSON object (`handle-event`). The caller asks for
    /// the tree again afterwards. The command has no extension data.
    pub async fn handle_event(
        &self,
        component: &Path,
        callback: &str,
        details: &str,
    ) -> Result<Answer, CallError> {
        self.handle_event_with(component, callback, details, None)
            .await
    }

    /// Runs the action of the item `item_id` of the command in `component`,
    /// as choosing it would: asks for the command's tree, then has it handle
    /// the item's action's callback, in one request, so no other call comes
    /// between. An item the list does not have, or one without an action,
    /// is an error. The command has no extension data.
    pub async fn run_item(&self, component: &Path, item_id: &str) -> Result<Answer, CallError> {
        self.run_item_with(component, item_id, None).await
    }

    /// Like [`Runtime::render`]; the command reads and saves `data`.
    /// An instance keeps the data it was started with.
    pub(crate) async fn render_with(
        &self,
        component: &Path,
        data: Option<PackageData>,
    ) -> Result<View, CallError> {
        self.render_launched_with(component, &LaunchRecord::default(), data)
            .await
    }

    /// Like [`Runtime::render_launched`]; the command reads and saves
    /// `data`.
    pub(crate) async fn render_launched_with(
        &self,
        component: &Path,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<View, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::Render {
                component: component.to_path_buf(),
                launch: launch.clone(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Like [`Runtime::handle_event`]; the command reads and saves `data`.
    pub(crate) async fn handle_event_with(
        &self,
        component: &Path,
        callback: &str,
        details: &str,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::HandleEvent {
                component: component.to_path_buf(),
                callback: callback.to_owned(),
                details: details.to_owned(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Like [`Runtime::run_item`]; the command reads and saves `data`.
    pub(crate) async fn run_item_with(
        &self,
        component: &Path,
        item_id: &str,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        self.run_item_launched_with(component, item_id, &LaunchRecord::default(), data)
            .await
    }

    /// Like [`Runtime::run_item_with`], drawing the list with the launch
    /// record `launch`: a scheduled item's run draws it with its
    /// schedule's.
    pub(crate) async fn run_item_launched_with(
        &self,
        component: &Path,
        item_id: &str,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::RunItem {
                component: component.to_path_buf(),
                item_id: item_id.to_owned(),
                launch: launch.clone(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Checks, without running any guest code, that `component` is a
    /// component Pane can run: it compiles, imports only WASI 0.3 and exports
    /// the extension interface, with the function types of the current
    /// contract ([`CallError::OlderApiShape`] otherwise). The check keeps
    /// nothing loaded, and runs on Pane's checker thread, one check at a
    /// time: it does not wait for guest calls in progress, such as one a
    /// reload is about to stop.
    pub async fn check(&self, component: &Path) -> Result<(), CallError> {
        self.check_with(component, Exports::default())
            .await
            .map(|_| ())
    }

    /// Like [`Runtime::check`]; the component must also export each
    /// interface `exports` names, with the current function types.
    pub(crate) async fn check_with(
        &self,
        component: &Path,
        exports: Exports,
    ) -> Result<Checked, CallError> {
        let (reply, response) = oneshot::channel();
        self.checks
            .send(Check {
                component: component.to_path_buf(),
                exports,
                reply,
            })
            .map_err(|_| checker_stopped())?;
        response.await.unwrap_or_else(|_| Err(checker_stopped()))
    }

    /// Asks the command in `component`, which supplies root results ahead
    /// of the query, for all of them; the command uses
    /// its `data`. Starts its instance if it has none.
    pub(crate) async fn indexed_results_with(
        &self,
        component: &Path,
        data: Option<PackageData>,
    ) -> Result<Vec<IndexedResult>, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::IndexedResults {
                component: component.to_path_buf(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Has the runtime's guests, and the launcher opening their results,
    /// find and open applications through `applications` from now on,
    /// instead of this system's own ([`crate::applications::native`]).
    pub fn set_applications(&self, applications: Arc<dyn Applications>) {
        *lock(&self.shared.applications) = applications;
    }

    /// Has the runtime list granted folders through `folders` from now on,
    /// instead of this system's own ([`crate::files::native`]).
    pub fn set_folders(&self, folders: Arc<dyn Folders>) {
        self.shared.files.set_folders(folders);
    }

    /// The granted folders and their listings, which the launcher shares.
    pub(crate) fn file_access(&self) -> FileAccess {
        self.shared.files.clone()
    }

    /// Finds and opens the system's applications.
    pub(crate) fn applications(&self) -> Arc<dyn Applications> {
        lock(&self.shared.applications).clone()
    }

    /// Has the runtime's guests keep clipboard history through `capture`
    /// from now on; until then they are told that this Pane does not watch
    /// the clipboard.
    pub(crate) fn set_clipboard(&self, capture: &Arc<Capture>) {
        *lock(&self.shared.clipboard) = Some(Arc::downgrade(capture));
    }

    /// Asks the command in `component`, which computes root results, for
    /// its results for `query`; the command reads and saves `data`.
    /// Starts its instance if it has none.
    pub(crate) async fn root_results_with(
        &self,
        component: &Path,
        query: &str,
        data: Option<PackageData>,
    ) -> Result<Vec<RootResult>, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::RootResults {
                component: component.to_path_buf(),
                query: query.to_owned(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Runs one cycle of the continuing service of the command with
    /// manifest id `command` in `component`, which reads and saves `data`.
    /// The launcher's services thread asks for each cycle while the
    /// package's code may run, taking the generation current when it does:
    /// a disable, reload, update, uninstall or pause that happens while the
    /// cycle runs stops it (see `Runtime::handle_event_with`, whose stopping
    /// is the same), and its late answer is discarded. An error the service
    /// answers with is an expected error; a trap, or a cycle that computes
    /// without finishing, is a crash of the package like any call's.
    pub(crate) async fn run_cycle_with(
        &self,
        component: &Path,
        command: &str,
        data: Option<PackageData>,
    ) -> Result<Cycle, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::RunCycle {
                component: component.to_path_buf(),
                command: command.to_owned(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Searches for `query` with the command with manifest id `command` in
    /// `component`, which searches as the user types; the command reads and
    /// saves `data`. Starts its instance if it has none.
    ///
    /// The search is sent at once; the returned future waits for its
    /// answer. Stopping or dropping the returned [`StopSearch`] stops it:
    /// a search queued behind other calls is then never started, and one
    /// waiting inside the guest (on a web request, say) is dropped with its
    /// instance, as when a generation ends; either answers
    /// [`CallError::Cancelled`], and so does a search that completes
    /// once it was stopped, whose results are discarded. A stopped search is
    /// not a failure of the extension.
    pub(crate) fn search_with(
        &self,
        component: &Path,
        command: &str,
        query: &str,
        data: Option<PackageData>,
    ) -> (
        StopSearch,
        impl Future<Output = Result<Vec<SearchResult>, CallError>> + Send + 'static,
    ) {
        let (stop, watched) = stoppable();
        let (reply, response) = oneshot::channel();
        let answer = self.call(
            Request::Search {
                component: component.to_path_buf(),
                command: command.to_owned(),
                query: query.to_owned(),
                data,
                stopped: watched,
                reply,
            },
            response,
        );
        (stop, answer)
    }

    /// Submits the form of `item_id` in the command in `component`. A
    /// rejection by the guest is [`CallError::Form`]. The command has no
    /// extension data.
    pub async fn submit_form(
        &self,
        component: &Path,
        item_id: &str,
        values: Vec<FieldValue>,
    ) -> Result<String, CallError> {
        self.submit_form_with(component, item_id, values, None)
            .await
    }

    /// Like [`Runtime::submit_form`]; the command reads and saves `data`.
    pub(crate) async fn submit_form_with(
        &self,
        component: &Path,
        item_id: &str,
        values: Vec<FieldValue>,
        data: Option<PackageData>,
    ) -> Result<String, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::SubmitForm {
                component: component.to_path_buf(),
                item_id: item_id.to_owned(),
                values,
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Opens the custom view of `item_id` in the command in `component` and
    /// draws it. The view stays open, holding its state in the guest, until
    /// [`Runtime::close_view`] or until its instance stops.
    /// The command has no extension data.
    pub async fn open_view(
        &self,
        component: &Path,
        item_id: &str,
    ) -> Result<(ViewId, Frame), CallError> {
        self.open_view_with(component, item_id, None).await
    }

    /// Like [`Runtime::open_view`]; the command reads and saves `data`.
    pub(crate) async fn open_view_with(
        &self,
        component: &Path,
        item_id: &str,
        data: Option<PackageData>,
    ) -> Result<(ViewId, Frame), CallError> {
        let (reply, response) = oneshot::channel();
        self.call(
            Request::OpenView {
                component: component.to_path_buf(),
                item_id: item_id.to_owned(),
                data,
                reply,
            },
            response,
        )
        .await
    }

    /// Has the open custom view `view` handle `event`, then draws it again.
    /// A view that has closed answers [`CallError::ViewClosed`].
    ///
    /// The event is sent when this is called, not when the returned future
    /// is first polled: events are handled one at a time, in the order of
    /// these calls.
    pub fn view_event(
        &self,
        view: ViewId,
        event: ViewEvent,
    ) -> impl Future<Output = Result<Frame, CallError>> + Send + 'static {
        let (reply, response) = oneshot::channel();
        self.call(Request::ViewEvent { view, event, reply }, response)
    }

    /// Closes the custom view `view`: the guest's view is dropped, after any
    /// event already sent to it, and later events are refused.
    pub fn close_view(&self, view: ViewId) {
        // A stopped runtime holds no views.
        let _ = self.send(Request::CloseView { view });
    }

    /// How many custom views are open in guest instances, counting the
    /// requests sent before this call. A diagnostic for tests and logs, not
    /// part of how the launcher decides anything: it tracks its own open
    /// view.
    pub async fn view_count(&self) -> usize {
        let (reply, response) = oneshot::channel();
        // A stopped runtime, or a thread Pane gave up on, holds no views.
        self.call(Request::ViewCount { reply }, response)
            .await
            .unwrap_or(0)
    }

    /// The components that have a live guest instance, counting the
    /// requests sent before this call, in no particular order. A diagnostic
    /// for tests and logs, like [`Runtime::view_count`]: listing or
    /// searching commands starts none; invoking a command starts its own.
    pub async fn running(&self) -> Vec<PathBuf> {
        self.try_running().await.unwrap_or_default()
    }

    /// The components with calls the user asked for that have not answered
    /// yet, counting the calls asked for before this one: sent, queued
    /// behind another call or running in the guest. The replacement of a
    /// package's code (an update) waits for these, so that no command the
    /// user is waiting on is interrupted mid-run. Background and ambient
    /// calls (a scheduled run, a service cycle, a root search's ask) are
    /// not counted: a replacement ends them and the new code restarts or
    /// re-asks them. Nor is a custom view event, whose open screen is what
    /// the launcher waits for instead. In no particular order.
    pub(crate) fn busy(&self) -> Vec<PathBuf> {
        lock(&self.shared.busy)
            .iter()
            .filter(|(_, busy)| busy.calls > 0)
            .map(|(component, _)| component.clone())
            .collect()
    }

    /// Like [`Runtime::running`], saying why there is no answer: the
    /// runtime is stopped, or its thread failed (a crash, or Pane gave up
    /// on it) before answering. Answered once every request sent before it
    /// was handled, so it tells when those are done.
    pub(crate) async fn try_running(&self) -> Result<Vec<PathBuf>, CallError> {
        let (reply, response) = oneshot::channel();
        self.call(Request::Running { reply }, response).await
    }

    /// The process ids of the native helpers guests started that are still
    /// running (not yet ended and reaped), in no particular order. A
    /// diagnostic for tests and logs, like [`Runtime::running`].
    pub fn helper_processes(&self) -> Vec<u32> {
        self.shared.helpers.running()
    }

    /// Ends every native helper process guests started, waiting until each
    /// is reaped, for Pane quitting: its threads stop with it, and nothing
    /// would end them otherwise. The calls that ran them answer that they
    /// were stopped.
    pub fn stop_helpers(&self) {
        self.shared.helpers.stop_all();
    }

    /// Pane is quitting: ends every native helper ([`Runtime::stop_helpers`])
    /// and stops the runtime thread, dropping every call in progress with
    /// its instance, so whatever a call waits on (a helper, a web request, a
    /// clock) ends now rather than with the process. Calls asked for
    /// afterwards answer that the runtime stopped.
    pub fn quit(&self) {
        self.shared.helpers.stop_all();
        let _ = self.send(Request::Quit);
    }

    /// Ends the native helper processes running a file inside `folder`,
    /// waiting (briefly) until each is reaped: before a replaced managed
    /// copy is removed, so no running program keeps it in use.
    pub(crate) fn stop_helpers_in(&self, folder: &Path) {
        self.shared.helpers.stop_in(folder);
    }

    /// Drops the compiled code and live instances of `components`, for
    /// example after their files were replaced or removed; a later call
    /// loads the file again. Calls made afterwards see the effect; a call
    /// already in progress finishes first, unless its generation ends (as
    /// the launcher does before forgetting a disabled or replaced package),
    /// which stops it. Nothing coordinates this with a command the user has
    /// open: its instance's state is lost. Their calls the user asked for
    /// are being stopped, so they no longer count as busy
    /// ([`Runtime::busy`]): the count's generation moves on, so a call of
    /// the ended generation dropped later cannot touch a newer one's.
    pub fn forget(&self, components: impl IntoIterator<Item = PathBuf>) {
        let components: Vec<PathBuf> = components.into_iter().collect();
        {
            let mut busy = lock(&self.shared.busy);
            for component in &components {
                // Ends the generation the counts so far are of, zeroing
                // them: a newer generation starts its count afresh.
                let busy = busy.entry(component.clone()).or_default();
                busy.calls = 0;
                busy.generation += 1;
            }
        }
        // A stopped runtime holds nothing to forget.
        let _ = self.send(Request::Forget { components });
    }

    /// Resolves the operation calls guests make against `directory` from
    /// now on. Without one, every call is answered that nothing is
    /// installed. It applies at once, not in the order of the requests:
    /// a call already queued resolves against it too. It is shared by
    /// every runtime thread, so a restarted one keeps it. The launcher sets
    /// it once, as it is created, before it asks for any call.
    pub(crate) fn set_directory(&self, directory: Directory) {
        *lock(&self.shared.directory) = Some(directory);
    }

    /// Has `launches` start the commands guests launch
    /// (`pane:extension/commands.launch`) from now on, also for calls
    /// already queued and on a restarted runtime thread. Until then a
    /// launch is refused.
    pub(crate) fn set_launches(&self, launches: Launches) {
        *lock(&self.shared.launches) = Some(launches);
    }

    /// Tells `health` of each later failure of a call into an installed
    /// package's code (see [`Health`]). Like [`Runtime::set_directory`], it
    /// applies at once, to calls already queued too, and holds for a
    /// restarted runtime thread.
    pub(crate) fn set_health(&self, health: HealthReport) {
        *lock(&self.shared.health) = Some(health);
    }

    /// How many runtime threads Pane gave up on, because they stopped
    /// responding, are still stuck, holding what they held. A diagnostic
    /// for tests and logs, like [`Runtime::running`].
    pub fn abandoned_threads(&self) -> usize {
        self.shared.abandoned()
    }

    fn send(&self, request: Request) -> Result<(), CallError> {
        self.shared
            .send(request)
            .map(|_| ())
            .map_err(NotSent::error)
    }

    /// Sends `request`, when this is called, and returns its answer from
    /// `response`. An answer lost because the runtime thread crashed or
    /// stopped responding is known once Pane has restarted it or chosen
    /// not to, and says which; the request is never sent again.
    ///
    /// A call the user asked a package for (see
    /// [`Request::user_component`]) is counted as busy in its component
    /// until its answer is waited for or nobody waits for it any more, so
    /// that replacing the package's code can wait for every such call to
    /// finish ([`Runtime::busy`]).
    fn call<T: Send + 'static>(
        &self,
        request: Request,
        response: oneshot::Receiver<Result<T, CallError>>,
    ) -> impl Future<Output = Result<T, CallError>> + Send + 'static + use<T> {
        let component = request.user_component().map(Path::to_path_buf);
        let handled = self.shared.handled();
        let sent = self.shared.send(request);
        let shared = Arc::downgrade(&self.shared);
        let answer = async move {
            let thread = match sent {
                Err(NotSent::Stopped) => return Err(supervisor::stopped()),
                Err(NotSent::Lost(thread)) => thread,
                Ok(thread) => {
                    // A thread Pane gave up on never answers: its failure
                    // being handled stands for the answer.
                    let mut response = response;
                    let mut given_up =
                        std::pin::pin!(supervisor::handled_after(handled.clone(), thread));
                    let answer = std::future::poll_fn(|cx| {
                        if let std::task::Poll::Ready(answer) =
                            std::pin::Pin::new(&mut response).poll(cx)
                        {
                            return std::task::Poll::Ready(answer.ok());
                        }
                        given_up.as_mut().poll(cx).map(|()| None)
                    })
                    .await;
                    if let Some(answer) = answer {
                        return answer;
                    }
                    thread
                }
            };
            Err(supervisor::lost(shared, handled, thread).await)
        };
        match component {
            None => CountedCall {
                answer,
                counting: None,
            },
            Some(component) => {
                // The generation the count is of, captured here and checked
                // again when it ends: `forget` moves the generation on, so
                // this call cannot take a call of a newer generation's
                // count with it.
                let generation = {
                    let mut busy = lock(&self.shared.busy);
                    let busy = busy.entry(component.clone()).or_default();
                    busy.calls += 1;
                    busy.generation
                };
                CountedCall {
                    answer,
                    counting: Some((self.shared.clone(), component, generation)),
                }
            }
        }
    }
}

/// The answer of one call, counted as busy in its component for as long
/// as it is waited for when the user asked the package for it (see
/// [`Runtime::call`]).
struct CountedCall<F> {
    answer: F,
    /// What to tell when this stops counting, with the component to tell
    /// it for and the generation the count was of; `None` for a call that
    /// is not counted.
    counting: Option<(Arc<Shared>, PathBuf, u64)>,
}

impl<F: Future> Future for CountedCall<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<F::Output> {
        // SAFETY: `answer` is never moved out of this, and is pinned here
        // for as long as `self` is, as structural pinning requires.
        let answer = unsafe {
            self.as_mut()
                .map_unchecked_mut(|counted| &mut counted.answer)
        };
        answer.poll(cx)
    }
}

impl<F> Drop for CountedCall<F> {
    fn drop(&mut self) {
        if let Some((shared, component, generation)) = self.counting.take() {
            let mut busy = lock(&shared.busy);
            if let Some(count) = busy.get_mut(&component)
                && count.generation == generation
                && count.calls > 0
            {
                count.calls -= 1;
            }
        }
    }
}

/// The limits `limits:<compute>,<warn>,<unresponsive>` sets, in whole
/// seconds (see [`Runtime::watch_fault_file`]).
#[cfg(any(test, debug_assertions))]
fn parse_limits(text: &str) -> Option<Limits> {
    let seconds: Vec<u64> = text
        .strip_prefix("limits:")?
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [compute, warn, unresponsive] = seconds[..] else {
        return None;
    };
    let secs = std::time::Duration::from_secs;
    Some(Limits {
        compute: secs(compute),
        warn: secs(warn),
        unresponsive: secs(unresponsive),
    })
}

/// Locks `mutex`, taking it over if a thread panicked while holding it:
/// the runtime thread may crash while a lock is held (see `supervisor`),
/// and Pane carries on with what the lock guarded.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The runtime could not do something because of `error`.
pub(crate) fn unavailable(error: impl fmt::Display) -> CallError {
    CallError::RuntimeUnavailable(error.to_string())
}

/// The engine every runtime thread runs guests with: WASI 0.3 and
/// component-model async, keeping compiled code in `cache_dir`, if given.
fn engine(cache_dir: Option<PathBuf>) -> Result<Engine, CallError> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .wasm_component_model_async(true)
        // Every guest yields to the runtime thread at each tick (see
        // `deadlines`), so none holds it by computing.
        .epoch_interruption(true);
    if let Some(dir) = cache_dir {
        let mut cache = CacheConfig::new();
        cache.with_directory(dir);
        let cache = Cache::new(cache).map_err(unavailable)?;
        config.cache(Some(cache));
    }
    Engine::new(&config).map_err(unavailable)
}

/// How a call whose guest trapped with `trap` answers: as running out of
/// memory if the guest was refused memory before (see `memory`), since its
/// trap then says only where it gave up.
fn crashed(trap: &wasmtime::Error, out_of_memory: bool) -> CallError {
    CallError::Trap(if out_of_memory {
        memory::out_of_memory()
    } else {
        format!("{trap:#}")
    })
}

/// How a check answers once the checker thread has stopped.
fn checker_stopped() -> CallError {
    CallError::RuntimeUnavailable("Pane's component checker has stopped".into())
}

/// How a call of a generation that ended for `end` answers.
fn ended(end: End) -> CallError {
    match end {
        End::Disabled => CallError::Disabled,
        End::Replaced => CallError::Replaced,
        End::Uninstalled => CallError::Uninstalled,
        End::Paused => CallError::Paused,
        End::Abandoned => given_up(),
    }
}

/// Why stopped code (see [`GuestState::stopped`]) may not start host work.
pub(crate) fn stopped_code(end: End) -> String {
    match end {
        End::Abandoned => "Pane's extension runtime stopped responding and was replaced while \
                           this code ran; it no longer changes anything"
            .into(),
        _ => "this code of the extension was stopped (disabled, reloaded or updated)".into(),
    }
}

/// Why code is stopped, if it is: the one check every host interface goes
/// through ([`GuestState::stopped`], and the web requests' sender, which
/// holds no `GuestState`). Code of an installed package is stopped once its
/// generation ended (which says why first); any code, once `fence`, its
/// runtime thread's, closed, whether or not its data was fenced with it
/// ([`PackageData::stopped`] checks that fence too).
pub(crate) fn code_stopped(data: Option<&PackageData>, fence: &Fence) -> Option<End> {
    data.and_then(PackageData::stopped)
        .or_else(|| fence.closed().then_some(End::Abandoned))
}

/// How a call answers that ran on a runtime thread Pane gave up on.
fn given_up() -> CallError {
    CallError::RuntimeUnavailable("it stopped responding and was replaced".into())
}

pub(crate) struct GuestState {
    wasi: WasiCtx,
    table: ResourceTable,
    /// The extension data of the package the command belongs to; `None` for a
    /// command built into Pane.
    data: Option<PackageData>,
    /// The guest's component, which identifies it as a caller.
    pub(crate) component: PathBuf,
    /// Where the guest's operation calls go, to be served while it waits.
    pub(crate) calls: mpsc::UnboundedSender<OperationCall>,
    /// Whether Pane is running a call of this guest, whose frame serves the
    /// guest's operation calls. A call made at any other time, such as while
    /// the component starts, is refused.
    pub(crate) serving: bool,
    /// Finds and opens the system's applications for the guest.
    applications: SharedApplications,
    /// `wasi:http`'s settings for the guest's web requests.
    http: WasiHttpCtx,
    /// Sends the guest's web requests.
    sender: http::Sender,
    /// Whether the guest asked for more memory than it may have
    /// ([`GUEST_MEMORY`]) and was refused: a trap that follows is told as
    /// running out of memory (see `memory`).
    out_of_memory: bool,
    /// The granted folders and their listings.
    files: FileAccess,
    /// Keeps clipboard history for the guest's package.
    clipboard: SharedClipboard,
    /// The installed packages, for finding the guest's helpers.
    directory: SharedDirectory,
    /// Starts the commands the guest launches.
    launches: SharedLaunches,
    /// The runtime's helper processes; those of this instance are ended
    /// with it.
    helpers: Helpers,
    /// Identifies this instance as the owner of the helpers it starts.
    owner: u64,
    /// The runtime thread running the instance: its host calls are marked
    /// there, and once Pane gave up on it (a runtime hang), its fence is
    /// closed and the instance is stopped ([`GuestState::stopped`]).
    watch: Arc<Watch>,
}

impl Drop for GuestState {
    /// The instance is going (its generation ended, it crashed, it was
    /// forgotten or the runtime stopped): so do the helpers it started.
    fn drop(&mut self) {
        self.helpers.stop_owned_by(self.owner);
    }
}

impl GuestState {
    /// Starts the helper `name` of the guest's own package with `args` and
    /// `input`. Its process belongs to this instance and to the generation
    /// of its code.
    pub(crate) fn start_helper(
        &mut self,
        name: String,
        args: Vec<String>,
        input: String,
    ) -> impl Future<Output = Result<Running, HelperError>> + Send + 'static + use<> {
        let checked = (|| {
            // Stopped code starts no more work.
            if let Some(end) = self.stopped() {
                return Err(runner::stopped_code(end));
            }
            if self.data.is_none() {
                return Err(HelperError::new(
                    HelperErrorKind::Refused,
                    "only installed packages ship helpers; this command is built into Pane",
                ));
            }
            runner::check_limits(&args, &input)?;
            Ok(lock(&self.directory).clone())
        })();
        let (component, helpers) = (self.component.clone(), self.helpers.clone());
        let (generation, owner) = (self.generation().cloned(), self.owner);
        let fence = self.watch.fence().clone();
        async move {
            let directory = checked?;
            // Finding and checking its file and starting its process may
            // block, so they run off the runtime thread, which only awaits.
            helpers
                .start_off_thread(move || {
                    let installed = directory.map(|directory| directory()).unwrap_or_default();
                    let program = helpers::find(&installed, &component, &name)?;
                    Ok(Spec {
                        name,
                        program,
                        args,
                        input,
                        generation,
                        fence: Some(fence),
                        owner,
                    })
                })
                .await
        }
    }

    /// The generation the instance belongs to; `None` for a command built
    /// into Pane, which runs as long as Pane.
    pub(crate) fn generation(&self) -> Option<&Generation> {
        self.data.as_ref().map(PackageData::generation)
    }

    /// Why the instance's code is stopped, if it is: its generation ended,
    /// or Pane gave up on the runtime thread running it
    /// ([`End::Abandoned`]). Stopped code may no longer run, save data,
    /// call operations or start any other host work: every host interface
    /// checks this one fence.
    pub(crate) fn stopped(&self) -> Option<End> {
        // The data is fenced with this thread's fence
        // ([`PackageData::fenced`]).
        code_stopped(self.data.as_ref(), self.watch.fence())
    }

    /// Marks a host call on the runtime thread until the guard is dropped
    /// (see `deadlines`): its time is not the guest's, and the thread is
    /// not given up on meanwhile, so what it does must not block for long.
    pub(crate) fn host(&self) -> HostCall<'_> {
        self.watch.host()
    }

    /// The runtime thread running the instance, for marking host work.
    pub(crate) fn watch(&self) -> Arc<Watch> {
        self.watch.clone()
    }

    /// Marks each poll of `future`, a host call's work, as
    /// [`GuestState::host`] does.
    pub(crate) fn hosted<F: Future>(&self, future: F) -> Hosted<F> {
        deadlines::hosted(self.watch.clone(), future)
    }

    fn data(&self) -> Result<&PackageData, String> {
        self.data.as_ref().ok_or_else(|| {
            "only installed packages keep settings or data; this command is built into Pane".into()
        })
    }
}

/// Implements one kind of data's interface over the package's extension
/// data. Reading is a short host call; saving waits for the file to be
/// written, off the runtime thread.
macro_rules! data_host {
    ($interface:ident, $kind:expr) => {
        impl $interface::Host for GuestState {
            fn get(&mut self, key: String) -> Result<Option<String>, String> {
                let _host = self.host();
                self.data()?.get($kind, &key)
            }

            async fn set(&mut self, key: String, value: String) -> Result<(), String> {
                let data = self.data()?.clone();
                self.hosted(async move { data.set($kind, &key, &value).await })
                    .await
            }
        }
    };
}

data_host!(settings, DataKind::Settings);
data_host!(content, DataKind::Content);
data_host!(cache, DataKind::Cache);
data_host!(credentials, DataKind::LocalCredentials);

impl launching::Host for GuestState {
    /// Hands the launch to the launcher, which starts it or says why it
    /// will not, and never waits for the target to run (see
    /// [`Launches`]).
    fn launch(
        &mut self,
        target: launching::CommandRef,
        launch_type: launching::LaunchType,
        arguments: Vec<launching::ArgumentValue>,
        context: Option<String>,
    ) -> Result<(), String> {
        // Stopped code launches nothing: checked once the host call is
        // marked, as applications' are.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let launches = lock(&self.launches)
            .clone()
            .ok_or("this Pane does not launch commands for extensions")?;
        launches(LaunchRequest {
            caller: self.component.clone(),
            source: target.source,
            command: target.command,
            launch_type: match launch_type {
                launching::LaunchType::UserInitiated => LaunchType::UserInitiated,
                launching::LaunchType::Background => LaunchType::Background,
            },
            arguments: arguments
                .into_iter()
                .map(|argument| (argument.name, argument.value))
                .collect(),
            context,
        })
    }
}

/// `launch` as the guest's bindings carry it.
fn launch_record(launch: &LaunchRecord) -> launching::LaunchRecord {
    launching::LaunchRecord {
        launch_type: match launch.launch_type {
            LaunchType::UserInitiated => launching::LaunchType::UserInitiated,
            LaunchType::Background => launching::LaunchType::Background,
        },
        source: match launch.source {
            LaunchSource::RootSearch => launching::LaunchSource::RootSearch,
            LaunchSource::Alias => launching::LaunchSource::Alias,
            LaunchSource::Fallback => launching::LaunchSource::Fallback,
            LaunchSource::Hotkey => launching::LaunchSource::Hotkey,
            LaunchSource::QuickSlot => launching::LaunchSource::QuickSlot,
            LaunchSource::Command => launching::LaunchSource::Command,
            LaunchSource::Schedule => launching::LaunchSource::Schedule,
        },
        arguments: launch
            .arguments
            .iter()
            .map(|(name, value)| launching::ArgumentValue {
                name: name.clone(),
                value: value.clone(),
            })
            .collect(),
        fallback_text: launch.fallback_text.clone(),
        context: launch.context.clone(),
    }
}

impl GuestState {
    fn applications(&self) -> Arc<dyn Applications> {
        lock(&self.applications).clone()
    }

    /// The granted folders and their listings, for the guest.
    pub(crate) fn file_access(&self) -> FileAccess {
        self.files.clone()
    }

    /// The identity key of the guest's package; `None` for a command
    /// built into Pane.
    pub(crate) fn owner(&self) -> Option<String> {
        self.data.as_ref().map(|data| data.owner().to_owned())
    }
}

impl applications::Host for GuestState {
    fn installed(&mut self) -> Result<Vec<applications::Application>, String> {
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the thread.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        Ok(self
            .applications()
            .installed()?
            .into_iter()
            .map(|application| applications::Application {
                id: application.id,
                name: application.name,
                location: application.location,
            })
            .collect())
    }

    fn open(&mut self, id: String) -> Result<(), String> {
        // Stopped code opens nothing: checked once the host call is
        // marked, from when Pane no longer decides to give up on the thread.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        self.applications().open(&id)
    }
}

impl GuestState {
    /// Runs `call` with what the guest's package does with its clipboard
    /// history, as one host call. Stopped code ([`GuestState::stopped`],
    /// the one check every host interface goes through) reads and changes
    /// nothing more, and the call is marked ([`GuestState::host`]), so its
    /// time (the history's lock and file, the system's clipboard) is
    /// Pane's, never the guest's.
    fn clipboard<R>(
        &self,
        call: impl FnOnce(clipboard::Commands<'_>) -> Result<R, String>,
    ) -> Result<R, String> {
        let _host = self.host();
        // Checked once the call is marked, as applications' are.
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let data = self.data.as_ref().ok_or(
            "only installed packages keep clipboard history; this command is built into Pane",
        )?;
        let capture = lock(&self.clipboard)
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        call(clipboard::Commands { data, capture })
    }
}

impl From<CaptureState> for clipboard_history::Capture {
    fn from(state: CaptureState) -> Self {
        match state {
            CaptureState::Off => clipboard_history::Capture::Off,
            CaptureState::On => clipboard_history::Capture::On,
            CaptureState::Paused => clipboard_history::Capture::Paused,
        }
    }
}

impl From<clipboard_history::Capture> for CaptureState {
    fn from(capture: clipboard_history::Capture) -> Self {
        match capture {
            clipboard_history::Capture::Off => CaptureState::Off,
            clipboard_history::Capture::On => CaptureState::On,
            clipboard_history::Capture::Paused => CaptureState::Paused,
        }
    }
}

/// A count for a guest, which cannot exceed `u32` in practice.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

impl clipboard_history::Host for GuestState {
    fn status(&mut self) -> Result<clipboard_history::HistoryStatus, String> {
        let status = self.clipboard(|history| history.status())?;
        Ok(clipboard_history::HistoryStatus {
            capture: status.capture.into(),
            problem: status.problem,
            excluded: status.excluded.into_iter().map(String::from).collect(),
            items: count(status.items),
            retention_seconds: status.retention_seconds,
        })
    }

    fn set_capture(&mut self, wanted: clipboard_history::Capture) -> Result<(), String> {
        self.clipboard(|history| history.set_capture(wanted.into()))
    }

    fn set_excluded(&mut self, programs: Vec<String>) -> Result<(), String> {
        self.clipboard(|history| history.set_excluded(&programs))
    }

    fn set_retention(&mut self, seconds: u64) -> Result<(), String> {
        self.clipboard(|history| history.set_retention(seconds))
    }

    fn entries(&mut self) -> Result<Vec<clipboard_history::Entry>, String> {
        let (items, now) = self.clipboard(|history| history.items())?;
        Ok(items
            .into_iter()
            .map(|item| clipboard_history::Entry {
                id: item.id.to_string(),
                text: item.text,
                copied_at: item.copied_at,
                age_seconds: now.saturating_sub(item.copied_at) / 1000,
                source: item.source,
            })
            .collect())
    }

    fn copy(&mut self, id: String) -> Result<(), String> {
        self.clipboard(|history| history.copy(&id))
    }

    fn clear(&mut self) -> Result<u32, String> {
        Ok(count(self.clipboard(|history| history.clear())?))
    }

    fn delete_items(&mut self, ids: Vec<String>) -> Result<u32, String> {
        Ok(count(self.clipboard(|history| history.delete(&ids))?))
    }

    fn turn_off_and_clear(&mut self) -> Result<u32, String> {
        Ok(count(
            self.clipboard(|history| history.turn_off_and_clear())?,
        ))
    }
}

impl WasiView for GuestState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for GuestState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.sender,
        }
    }
}

/// A running guest instance of one component.
struct Instance {
    store: Store<GuestState>,
    bindings: bindings::ExtensionWithClipboard,
    /// Its root results export, if it has one.
    root_results: Option<root_bindings::RootResultsProvider>,
    /// Its indexed results export, if it has one.
    indexed_results: Option<indexed_bindings::IndexedResultsProvider>,
    /// Its published operations export, if it has one.
    operations: Option<operations_bindings::OperationsProvider>,
    /// Its search export, if it searches as the user types.
    command_search: Option<search_bindings::CommandSearchProvider>,
    /// Its continuing-service export, if it runs one.
    service: Option<service_bindings::ServiceProvider>,
    /// The operation calls its guest makes, which the call it runs serves
    /// ([`Host::run_guest`]); taken out while it runs one.
    calls: Option<mpsc::UnboundedReceiver<OperationCall>>,
    /// Tells this instance apart from a later one of the same component, so
    /// a view of one is never used with the other's store.
    serial: u64,
    /// Its entry on its generation's undo list: the generation's end has the
    /// runtime thread drop it, even while no call asks for it.
    _undo: Option<Registration>,
}

/// A custom view open in a guest instance.
#[derive(Clone)]
struct LiveView {
    /// The component whose instance holds the view.
    component: PathBuf,
    /// The guest's `custom-view` resource.
    resource: ResourceAny,
    /// The instance holding it ([`Instance::serial`]).
    serial: u64,
}

/// The engine and the host interfaces guests link against, shared by the
/// runtime thread and the checker thread.
struct Code {
    engine: Engine,
    linker: Linker<GuestState>,
}

/// One request and the operation calls served inside it: the guest calls
/// running for it, outermost first. Each package in it is busy until its
/// call returns.
#[derive(Clone, Default)]
struct Chain {
    /// Tells it apart from the other chains the thread serves meanwhile.
    id: u64,
    /// The components running a guest call in the chain, outermost first.
    components: Vec<PathBuf>,
    /// The generations of the calls in the chain that have one, outermost
    /// first: when any ends, the calls from it inward stop.
    owners: Vec<Generation>,
}

/// One instance's turn to run calls: one call into an instance at a time,
/// the others queued in the order they were asked for (see [`Host::turn`]).
#[derive(Default)]
struct Lane {
    /// Held by the call whose turn it is.
    turn: Arc<tokio::sync::Mutex<()>>,
    /// The chain holding the turn, if one does.
    holder: Option<u64>,
    /// Counts [`Host::forget`]s of the component: an instance taken out for
    /// a call before one is dropped when the call ends, not put back.
    forgotten: u64,
    /// While the instance is out for a call: its generation, and
    /// `forgotten` as it was when it was taken out.
    out: Option<(Option<Generation>, u64)>,
}

/// A call's turn on its instance's [`Lane`]; the next call queued takes it
/// once this is dropped.
struct Turn<'a> {
    host: &'a Host,
    component: PathBuf,
    _held: tokio::sync::OwnedMutexGuard<()>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        // Never panics, even while the thread unwinds from a crash.
        if let Ok(mut lanes) = self.host.lanes.try_borrow_mut()
            && let Some(lane) = lanes.get_mut(&self.component)
        {
            lane.holder = None;
        }
    }
}

/// Notes, until dropped, which lane a chain waits for (see
/// [`Host::would_deadlock`]).
struct Waiting<'a> {
    host: &'a Host,
    chain: u64,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if let Ok(mut waiting) = self.host.waiting.try_borrow_mut() {
            waiting.remove(&self.chain);
        }
    }
}

/// The requests a runtime thread serves at once, each a task polled on the
/// thread itself, so that a panic in any unwinds the thread (see
/// `supervisor`).
type Tasks<'a> = FuturesUnordered<Task<'a>>;

/// One request being served (see [`Tasks`]).
type Task<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Runtime-thread state: compiled components, their live instances and the
/// custom views open in them, shared by the requests the thread serves at
/// once (see [`Host::serve`]). Only the thread uses it, one task at a time,
/// and nothing borrowed from it is held across an await.
struct Host {
    code: Arc<Code>,
    components: RefCell<HashMap<PathBuf, Component>>,
    /// The live instances not running a call; one running a call is taken
    /// out for it (see [`Lane::out`]).
    instances: RefCell<HashMap<PathBuf, Instance>>,
    /// Each component's turn, which orders the calls into its instance.
    lanes: RefCell<HashMap<PathBuf, Lane>>,
    /// The lane each chain waits for, while it waits.
    waiting: RefCell<HashMap<u64, PathBuf>>,
    views: RefCell<HashMap<ViewId, LiveView>>,
    /// The next view id, shared with the threads that replace this one.
    next_view: Arc<AtomicU64>,
    /// The next chain's id.
    next_chain: Cell<u64>,
    /// The next instance's serial.
    next_serial: Cell<u64>,
    /// Woken whenever an instance taken out for a call comes back or goes.
    returned: tokio::sync::Notify,
    /// Where an instance's undo asks this thread to drop the instances of
    /// ended generations; weak, so that the thread still stops once every
    /// runtime handle is gone.
    nudge: mpsc::WeakUnboundedSender<Request>,
    /// The installed packages operation calls are resolved against, and
    /// guests' helpers found in.
    directory: SharedDirectory,
    /// Starts the commands guests launch.
    launches: SharedLaunches,
    /// The helper processes guests started.
    helpers: Helpers,
    /// Told of each failure of a call into an installed package's code.
    health: Arc<Mutex<Option<HealthReport>>>,
    /// Faults injected into this thread, to check recovery.
    faults: Arc<Faults>,
    /// What the watchdog knows of this thread; whether Pane gave up on it.
    watch: Arc<Watch>,
    /// The limits guest calls run within, shared with the watchdog.
    limits: Arc<Mutex<Limits>>,
    /// This thread's number among those the runtime started.
    number: u64,
    /// Finds and opens the system's applications for guests.
    applications: SharedApplications,
    /// Guests' web requests, shared with the threads that replace this one.
    network: Arc<http::Network>,
    /// The granted folders and their listings.
    files: FileAccess,
    /// What a search waits on before it starts, if a test replaced the
    /// clock. A release build has none.
    #[cfg(any(test, debug_assertions))]
    search_timer: Arc<Mutex<Option<SearchTimer>>>,
    /// Keeps clipboard history for guests' packages.
    clipboard: SharedClipboard,
}

impl Code {
    fn new(engine: Engine) -> Code {
        let mut linker = Linker::new(&engine);
        // Only WASI 0.3 is registered: no P2 linker and no stubs for unknown
        // imports, so a mixed P2/P3 component cannot instantiate.
        wasmtime_wasi::p3::add_to_linker(&mut linker)
            .expect("registering WASI 0.3 in a fresh linker cannot conflict");
        // Web requests (`wasi:http@0.3.0`'s client), sent by `http`.
        wasmtime_wasi_http::p3::add_to_linker(&mut linker)
            .expect("registering wasi:http in a fresh linker cannot conflict");
        settings::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .expect("registering settings in a fresh linker cannot conflict");
        content::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .expect("registering content in a fresh linker cannot conflict");
        cache::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .expect("registering the cache in a fresh linker cannot conflict");
        credentials::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| {
            state
        })
        .expect("registering credentials in a fresh linker cannot conflict");
        bindings::pane::extension::operations::add_to_linker::<_, operations::Calls>(
            &mut linker,
            |state| state,
        )
        .expect("registering operations in a fresh linker cannot conflict");
        applications::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| {
            state
        })
        .expect("registering applications in a fresh linker cannot conflict");
        clipboard_history::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
            &mut linker,
            |state| state,
        )
        .expect("registering clipboard history in a fresh linker cannot conflict");
        bindings::pane::extension::helpers::add_to_linker::<_, helpers::Runs>(
            &mut linker,
            |state| state,
        )
        .expect("registering helpers in a fresh linker cannot conflict");
        bindings::pane::extension::files::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
            &mut linker,
            |state| state,
        )
        .expect("registering files in a fresh linker cannot conflict");
        launching::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .expect("registering launching commands in a fresh linker cannot conflict");
        Code { engine, linker }
    }

    /// Compiles `path` and rejects components that import non-0.3 WASI.
    fn compile(&self, path: &Path) -> Result<Component, CallError> {
        let component = Component::from_file(&self.engine, path)
            .map_err(|error| CallError::Load(format!("{}: {error:#}", path.display())))?;
        let unsupported: Vec<String> = component
            .component_type()
            .imports(&self.engine)
            .map(|(name, _)| name.to_owned())
            .filter(|name| name.starts_with("wasi:") && !name.contains(WASI_VERSION))
            .collect();
        if !unsupported.is_empty() {
            return Err(CallError::Incompatible(unsupported));
        }
        self.check_exports(&component)?;
        Ok(component)
    }

    /// Type-checks the functions of the component's command interface
    /// against those Pane calls, from the component's type alone, so no
    /// guest code runs. Instantiating checks the same, but only when a
    /// command opens; a component built for an older shape of the same API
    /// version is refused here instead. A component without the interface
    /// is left to [`Host::check`], which says so.
    fn check_exports(&self, component: &Component) -> Result<(), CallError> {
        use wasmtime::component::types::{ComponentFunc, ComponentItem};
        use wasmtime::component::{ComponentNamedList, Lift, Lower, ResourceAny};

        let ty = component.component_type();
        let Some(ComponentItem::ComponentInstance(interface)) = ty
            .get_export(&self.engine, COMMAND_INTERFACE)
            .map(|export| export.ty)
        else {
            return Ok(());
        };
        let cx = ty.instance_type();
        let older = |problem: String| CallError::OlderApiShape(problem);
        let func = |name: &str| match interface.get_export(&self.engine, name).map(|e| e.ty) {
            Some(ComponentItem::ComponentFunc(func)) => Ok(func),
            _ => Err(older(format!("it has no function `{name}`"))),
        };
        fn check<P: ComponentNamedList + Lower, R: ComponentNamedList + Lift>(
            name: &str,
            func: ComponentFunc,
            cx: &wasmtime::component::__internal::InstanceType<'_>,
        ) -> Result<(), CallError> {
            func.typecheck::<P, R>(cx)
                .map_err(|error| CallError::OlderApiShape(format!("`{name}`: {error:#}")))
        }
        check::<(launching::LaunchRecord,), (Result<String, String>,)>(
            "render",
            func("render")?,
            &cx,
        )?;
        check::<(String, launching::LaunchRecord), (Result<String, String>,)>(
            "run",
            func("run")?,
            &cx,
        )?;
        check::<(String, String), (Result<String, String>,)>(
            "handle-event",
            func("handle-event")?,
            &cx,
        )?;
        check::<(String, Vec<command::FieldValue>), (Result<String, command::FormError>,)>(
            "submit-form",
            func("submit-form")?,
            &cx,
        )?;
        check::<(String,), (Result<ResourceAny, String>,)>("open-view", func("open-view")?, &cx)?;
        let render = "[method]custom-view.render";
        check::<(ResourceAny,), (command::Frame,)>(render, func(render)?, &cx)?;
        let handle_event = "[method]custom-view.handle-event";
        check::<(ResourceAny, command::ViewEvent), (Result<(), String>,)>(
            handle_event,
            func(handle_event)?,
            &cx,
        )
    }

    /// Type-checks `path` against the linker and the extension world, and
    /// against each interface `exports` names too, without instantiating it,
    /// so no guest code runs.
    fn check(&self, path: &Path, exports: Exports) -> Result<Checked, CallError> {
        let component = self.compile(path)?;
        let network = component
            .component_type()
            .imports(&self.engine)
            .any(|(name, _)| name.starts_with("wasi:http/"));
        let interface = |error: wasmtime::Error| CallError::Interface(format!("{error:#}"));
        let pre = self.linker.instantiate_pre(&component).map_err(interface)?;
        if exports.root_results {
            root_bindings::RootResultsProviderPre::new(pre.clone()).map_err(|error| {
                CallError::Interface(format!(
                    "its manifest says it computes root results, but it does not export \
                     {ROOT_RESULTS_INTERFACE} with the functions Pane calls: {error:#}"
                ))
            })?;
        }
        if exports.indexed_results {
            indexed_bindings::IndexedResultsProviderPre::new(pre.clone()).map_err(|error| {
                CallError::Interface(format!(
                    "its manifest says it supplies indexed results, but it does not export \
                     {INDEXED_RESULTS_INTERFACE} with the functions Pane calls: {error:#}"
                ))
            })?;
        }
        if exports.operations {
            operations_bindings::OperationsProviderPre::new(pre.clone()).map_err(|error| {
                CallError::Interface(format!(
                    "its manifest publishes operations it serves, but it does not export \
                     {OPERATIONS_INTERFACE} with the functions Pane calls: {error:#}"
                ))
            })?;
        }
        if exports.search {
            search_bindings::CommandSearchProviderPre::new(pre.clone()).map_err(|error| {
                CallError::Interface(format!(
                    "its manifest says it searches as the user types, but it does not export \
                     {COMMAND_SEARCH_INTERFACE} with the functions Pane calls: {error:#}"
                ))
            })?;
        }
        if exports.service {
            service_bindings::ServiceProviderPre::new(pre.clone()).map_err(|error| {
                CallError::Interface(format!(
                    "its manifest says it runs a continuing service, but it does not export \
                     {SERVICE_INTERFACE} with the functions Pane calls: {error:#}"
                ))
            })?;
        }
        bindings::ExtensionWithClipboardPre::new(pre).map_err(interface)?;
        Ok(Checked { network })
    }
}

impl Host {
    fn new(
        code: Arc<Code>,
        shared: &Shared,
        number: u64,
        faults: Arc<Faults>,
        watch: Arc<Watch>,
        nudge: mpsc::WeakUnboundedSender<Request>,
    ) -> Host {
        Host {
            code,
            components: RefCell::default(),
            instances: RefCell::default(),
            lanes: RefCell::default(),
            waiting: RefCell::default(),
            views: RefCell::default(),
            next_view: shared.next_view.clone(),
            next_chain: Cell::new(0),
            next_serial: Cell::new(0),
            returned: tokio::sync::Notify::new(),
            nudge,
            directory: shared.directory.clone(),
            launches: shared.launches.clone(),
            helpers: shared.helpers.clone(),
            health: shared.health.clone(),
            faults,
            watch,
            limits: shared.limits.clone(),
            number,
            applications: shared.applications.clone(),
            network: shared.network.clone(),
            files: shared.files.clone(),
            #[cfg(any(test, debug_assertions))]
            search_timer: shared.search_timer.clone(),
            clipboard: shared.clipboard.clone(),
        }
    }

    /// Serves requests until every handle is gone, Pane quits or Pane
    /// gives up on this thread, each request a task of its own (see
    /// [`Tasks`]): while one call waits on something outside its guest (the
    /// user, a program, a helper, the network, a clock), the others run.
    /// Calls into one instance still run one after another ([`Host::turn`]).
    /// An injected [`Fault::Crash`] panics here, wherever the thread waits.
    async fn serve(self, mut requests: mpsc::UnboundedReceiver<Request>) {
        let host = &self;
        let mut tasks: Tasks<'_> = FuturesUnordered::new();
        let faults = self.faults.clone();
        let mut waiting = std::pin::pin!(faults.waiting());
        loop {
            let next = std::future::poll_fn(|cx| {
                // Woken by an injection, it waits for the next one.
                if waiting.as_mut().poll(cx).is_ready() {
                    waiting.set(faults.waiting());
                }
                faults.check(waiting.as_mut(), cx);
                // Given up on while it was stuck: it serves nothing more.
                if host.watch.given_up() {
                    return Poll::Ready(None);
                }
                // The work in progress first, then what is asked next.
                while let Poll::Ready(Some(())) = tasks.poll_next_unpin(cx) {}
                requests.poll_recv(cx).map(Some)
            })
            .await;
            match next {
                // Given up on, every handle gone, or Pane quitting: the work
                // in progress is dropped with its instances, ending its
                // waits (helpers, web requests) where they are.
                None | Some(None) | Some(Some(Request::Quit)) => return,
                Some(Some(request)) => host.dispatch(request, &mut tasks),
            }
        }
    }

    /// Starts serving `request`: what it changes at once (forgetting
    /// components, closing a view) is done here, in the order the requests
    /// were sent, and its calls become a task in `tasks`.
    fn dispatch<'a>(&'a self, request: Request, tasks: &mut Tasks<'a>) {
        let watch = self.watch.clone();
        let _handling = watch.doing(Doing::Handling);
        self.drop_stopped();
        let task: Task<'a> = match request {
            Request::Render {
                component,
                launch,
                data,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(self.render_tree(&component, &launch, data).await);
            }),
            Request::HandleEvent {
                component,
                callback,
                details,
                data,
                reply,
            } => Box::pin(async move {
                let result = self
                    .handle_event(&component, callback.clone(), details, data)
                    .await;
                // An injected fault may lose this answer, after the action
                // ran.
                self.faults.before_answer(&callback);
                let _ = reply.send(result);
            }),
            Request::RunItem {
                component,
                item_id,
                launch,
                data,
                reply,
            } => Box::pin(async move {
                let result = self.run_item(&component, &item_id, &launch, data).await;
                self.faults.before_answer(&item_id);
                let _ = reply.send(result);
            }),
            Request::Run {
                component,
                command,
                launch,
                data,
                reply,
            } => Box::pin(async move {
                let result = self.run(&component, command.clone(), &launch, data).await;
                // An injected fault may lose this answer, after the command
                // ran.
                self.faults.before_answer(&command);
                let _ = reply.send(result);
            }),
            Request::RunCycle {
                component,
                command,
                data,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(self.run_cycle(&component, command, data).await);
            }),
            Request::IndexedResults {
                component,
                data,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(self.indexed_results(&component, data).await);
            }),
            Request::RootResults {
                component,
                query,
                data,
                mut reply,
            } => Box::pin(async move {
                let result = self.root_results(&component, query, data, &mut reply).await;
                let _ = reply.send(result);
            }),

            Request::Search {
                component,
                command,
                query,
                data,
                stopped,
                reply,
            } => Box::pin(async move {
                let result = self.search(&component, command, query, data, stopped).await;
                let _ = reply.send(result);
            }),
            Request::Forget { components } => {
                self.forget(&components);
                return;
            }
            Request::DropStopped | Request::Quit => return,
            Request::SubmitForm {
                component,
                item_id,
                values,
                data,
                reply,
            } => Box::pin(async move {
                let result = self.submit_form(&component, item_id, values, data).await;
                let _ = reply.send(result);
            }),
            Request::OpenView {
                component,
                item_id,
                data,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(self.open_view(&component, item_id, data).await);
            }),
            Request::ViewEvent { view, event, reply } => {
                // The view as it is now: an event sent before the view was
                // closed is still handled, before the view is dropped.
                let open = self.views.borrow().get(&view).cloned();
                let Some(open) = open else {
                    let _ = reply.send(Err(CallError::ViewClosed));
                    return;
                };
                Box::pin(async move {
                    let _ = reply.send(self.view_event(open, event).await);
                })
            }
            Request::CloseView { view } => {
                // Closed at once, for the requests sent after this; its
                // destructor runs in its instance's turn.
                let open = self.views.borrow_mut().remove(&view);
                let Some(open) = open else {
                    return;
                };
                Box::pin(self.close_view(open))
            }
            Request::ViewCount { reply } => Box::pin(async move {
                self.settled().await;
                let count = self.views.borrow().len();
                let _ = reply.send(Ok(count));
            }),
            Request::Running { reply } => Box::pin(async move {
                self.settled().await;
                let _ = reply.send(Ok(self.running()));
            }),
        };
        tasks.push(task);
    }

    /// A new chain, for a request.
    fn chain(&self) -> Chain {
        let id = self.next_chain.get();
        self.next_chain.set(id + 1);
        Chain {
            id,
            ..Chain::default()
        }
    }

    /// Waits for `path`'s turn for `chain`: one call into an instance at a
    /// time, the others in the order they asked. `None` when waiting could
    /// never end, because the turn's holder waits, through other chains, for
    /// a turn `chain` holds: a call that would wait on itself is refused
    /// rather than waited for.
    async fn turn(&self, path: &Path, chain: u64) -> Option<Turn<'_>> {
        let turn = self
            .lanes
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_default()
            .turn
            .clone();
        if self.would_deadlock(path, chain) {
            return None;
        }
        let held = {
            self.waiting.borrow_mut().insert(chain, path.to_path_buf());
            let _waiting = Waiting { host: self, chain };
            turn.lock_owned().await
        };
        if let Some(lane) = self.lanes.borrow_mut().get_mut(path) {
            lane.holder = Some(chain);
        }
        Some(Turn {
            host: self,
            component: path.to_path_buf(),
            _held: held,
        })
    }

    /// [`Host::turn`] for a request's own call, which holds no turn yet and
    /// so never waits on itself.
    async fn turn_for(&self, path: &Path, chain: &Chain) -> Result<Turn<'_>, CallError> {
        self.turn(path, chain.id)
            .await
            .ok_or_else(|| CallError::RuntimeUnavailable("its call would wait on itself".into()))
    }

    /// Whether `chain` waiting for `path`'s turn would wait for ever: the
    /// turn's holder waits for a turn whose holder waits ... for one
    /// `chain` holds.
    fn would_deadlock(&self, path: &Path, chain: u64) -> bool {
        let lanes = self.lanes.borrow();
        let waiting = self.waiting.borrow();
        let mut lane = path;
        // Each chain waits for one turn at most: the walk ends.
        for _ in 0..=lanes.len() {
            let Some(holder) = lanes.get(lane).and_then(|lane| lane.holder) else {
                return false;
            };
            if holder == chain {
                return true;
            }
            let Some(next) = waiting.get(&holder) else {
                return false;
            };
            lane = next.as_path();
        }
        false
    }

    /// Takes the idle instance of `path` out for a call, noting it as out
    /// (see [`Lane::out`]); with what to give [`Host::bring_back`].
    fn take_out(&self, path: &Path) -> Option<(Instance, u64)> {
        let instance = self.instances.borrow_mut().remove(path)?;
        let generation = instance.store.data().generation().cloned();
        let mut lanes = self.lanes.borrow_mut();
        let lane = lanes.entry(path.to_path_buf()).or_default();
        lane.out = Some((generation, lane.forgotten));
        Some((instance, lane.forgotten))
    }

    /// Puts `instance`, taken out of `path` when its forget count was
    /// `taken`, back for the next call, unless `path` was forgotten
    /// meanwhile: then it goes, with its views.
    fn bring_back(&self, path: &Path, taken: u64, instance: Instance) {
        let keep = {
            let mut lanes = self.lanes.borrow_mut();
            let lane = lanes.entry(path.to_path_buf()).or_default();
            lane.out = None;
            lane.forgotten == taken
        };
        if keep {
            self.instances
                .borrow_mut()
                .insert(path.to_path_buf(), instance);
        } else {
            drop(instance);
            self.views
                .borrow_mut()
                .retain(|_, view| view.component != path);
        }
        self.returned.notify_waiters();
    }

    /// Notes that the instance taken out of `path` went (its call stopped,
    /// or it crashed), with its views.
    fn gone(&self, path: &Path) {
        if let Some(lane) = self.lanes.borrow_mut().get_mut(path) {
            lane.out = None;
        }
        self.views
            .borrow_mut()
            .retain(|_, view| view.component != path);
        self.returned.notify_waiters();
    }

    /// Waits until no instance out for a call is going: one whose
    /// generation ended, or that was forgotten, which its call drops as it
    /// stops. Then what a diagnostic answers counts every request sent
    /// before it that ends or forgets something.
    async fn settled(&self) {
        loop {
            let notified = self.returned.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            let going = self.lanes.borrow().values().any(|lane| {
                lane.out.as_ref().is_some_and(|(generation, taken)| {
                    *taken != lane.forgotten
                        || generation.as_ref().is_some_and(|g| g.ended().is_some())
                })
            });
            if !going {
                return;
            }
            notified.await;
        }
    }

    /// The components with a live instance, idle or running a call.
    fn running(&self) -> Vec<PathBuf> {
        let mut running: Vec<PathBuf> = self.instances.borrow().keys().cloned().collect();
        running.extend(
            self.lanes
                .borrow()
                .iter()
                .filter(|(_, lane)| lane.out.is_some())
                .map(|(path, _)| path.clone()),
        );
        running
    }

    /// Drops the compiled code and idle instances of `components`; an
    /// instance running a call finishes it and then goes (see
    /// [`Runtime::forget`]).
    fn forget(&self, components: &[PathBuf]) {
        for component in components {
            self.components.borrow_mut().remove(component);
            self.drop_instance(component);
            if let Some(lane) = self.lanes.borrow_mut().get_mut(component) {
                lane.forgotten += 1;
            }
        }
    }

    /// Asks the command in `path`, launched as `launch` says, for its
    /// tree, and reads it: a tree Pane cannot read is
    /// [`CallError::Unreadable`], and the instance stays.
    async fn render_tree(
        &self,
        path: &Path,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<View, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.render_in_turn(path, launch, data, &chain).await
    }

    /// [`Host::render_tree`] in `chain`, which holds `path`'s turn.
    async fn render_in_turn(
        &self,
        path: &Path,
        launch: &LaunchRecord,
        data: Option<PackageData>,
        chain: &Chain,
    ) -> Result<View, CallError> {
        self.instance(path, data).await?;
        let launch = launch_record(launch);
        let result = self
            .run_guest(path, chain, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| command.call_render(store, launch).await)
                    .await
            })
            .await?;
        let tree = self.settle(path, result, CallError::Guest)?;
        tree::read_view(&tree).map_err(CallError::Unreadable)
    }

    /// Runs the no-view command with manifest id `command` in `path`,
    /// launched as `launch` says (`run`), and reads its answer: one Pane
    /// cannot read is [`CallError::Unreadable`]. An error it answers with
    /// is [`CallError::Guest`], never a failure of the package.
    async fn run(
        &self,
        path: &Path,
        command: String,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.instance(path, data).await?;
        let launch = launch_record(launch);
        let result = self
            .run_guest(path, &chain, async |instance| {
                let exported = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| exported.call_run(store, command, launch).await)
                    .await
            })
            .await?;
        let answer = self.settle(path, result, CallError::Guest)?;
        tree::read_answer(&answer).map_err(CallError::Unreadable)
    }

    /// Has the command in `path` handle `callback` with `details`, and reads
    /// its answer: one Pane cannot read is [`CallError::Unreadable`].
    async fn handle_event(
        &self,
        path: &Path,
        callback: String,
        details: String,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.handle_event_in_turn(path, callback, details, data, &chain)
            .await
    }

    /// [`Host::handle_event`] in `chain`, which holds `path`'s turn.
    async fn handle_event_in_turn(
        &self,
        path: &Path,
        callback: String,
        details: String,
        data: Option<PackageData>,
        chain: &Chain,
    ) -> Result<Answer, CallError> {
        self.instance(path, data).await?;
        let result = self
            .run_guest(path, chain, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| {
                        command.call_handle_event(store, callback, details).await
                    })
                    .await
            })
            .await?;
        let answer = self.settle(path, result, CallError::Guest)?;
        tree::read_answer(&answer).map_err(CallError::Unreadable)
    }

    /// Runs the action of the item `item_id` of the command in `path`: its
    /// tree, then the item's action's callback (see [`Runtime::run_item`]).
    /// Both calls run in one turn, so no other call into the instance comes
    /// between them.
    async fn run_item(
        &self,
        path: &Path,
        item_id: &str,
        launch: &LaunchRecord,
        data: Option<PackageData>,
    ) -> Result<Answer, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        let view = self
            .render_in_turn(path, launch, data.clone(), &chain)
            .await?;
        let Some(item) = view.items.iter().find(|item| item.id == item_id) else {
            return Err(CallError::Guest(format!("unknown item: {item_id}")));
        };
        let Some(action) = item.action() else {
            return Err(CallError::Guest(format!(
                "the item {item_id} has no action"
            )));
        };
        let Some(callback) = action.callback() else {
            return Err(CallError::Guest(format!(
                "the item {item_id}'s first action opens a submenu, which only the Actions \
                 panel shows"
            )));
        };
        self.handle_event_in_turn(path, callback.to_owned(), "{}".into(), data, &chain)
            .await
    }

    async fn submit_form(
        &self,
        path: &Path,
        item_id: String,
        values: Vec<FieldValue>,
        data: Option<PackageData>,
    ) -> Result<String, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.instance(path, data).await?;
        let values = values
            .into_iter()
            .map(|FieldValue { id, value }| command::FieldValue { id, value })
            .collect();
        let result = self
            .run_guest(path, &chain, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| {
                        command.call_submit_form(store, item_id, values).await
                    })
                    .await
            })
            .await?;
        self.settle(path, result, |error: command::FormError| {
            CallError::Form(FormError {
                field: error.field,
                message: error.message,
            })
        })
    }

    async fn open_view(
        &self,
        path: &Path,
        item_id: String,
        data: Option<PackageData>,
    ) -> Result<(ViewId, Frame), CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.instance(path, data).await?;
        let result = self
            .run_guest(path, &chain, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| command.call_open_view(store, item_id).await)
                    .await
            })
            .await?;
        let resource = self.settle(path, result, CallError::Guest)?;
        // Its instance was forgotten while it opened: the view went with it.
        let serial = self.serial(path).ok_or(CallError::ViewClosed)?;
        let view = ViewId {
            thread: self.number,
            id: self.next_view.fetch_add(1, Ordering::Relaxed),
        };
        let open = LiveView {
            component: path.to_path_buf(),
            resource,
            serial,
        };
        self.views.borrow_mut().insert(view, open.clone());
        match self.render(&open, &chain).await {
            Ok(frame) => Ok((view, frame)),
            Err(error) => {
                let closed = self.views.borrow_mut().remove(&view);
                if let Some(open) = closed {
                    self.drop_view(&open).await;
                }
                Err(error)
            }
        }
    }

    async fn view_event(&self, open: LiveView, event: ViewEvent) -> Result<Frame, CallError> {
        let chain = self.chain();
        let path = open.component.clone();
        let _turn = self.turn_for(&path, &chain).await?;
        self.live(&open)?;
        let event = command::ViewEvent::from(event);
        let resource = open.resource;
        let result = self
            .run_guest(&path, &chain, async |instance| {
                let custom_view = instance.bindings.pane_extension_command().custom_view();
                instance
                    .store
                    .run_concurrent(async |store| {
                        custom_view.call_handle_event(store, resource, event).await
                    })
                    .await
            })
            .await?;
        self.settle(&path, result, CallError::Guest)?;
        self.render(&open, &chain).await
    }

    /// Asks the guest to draw the open view `open`, in its instance's turn.
    async fn render(&self, open: &LiveView, chain: &Chain) -> Result<Frame, CallError> {
        self.live(open)?;
        let (path, resource) = (&open.component, open.resource);
        let result = self
            .run_guest(path, chain, async |instance| {
                let custom_view = instance.bindings.pane_extension_command().custom_view();
                instance
                    .store
                    .run_concurrent(async |store| custom_view.call_render(store, resource).await)
                    .await
            })
            .await?;
        let frame = self.settle(path, result.map(|frame| frame.map(Ok)), |never| never)?;
        let frame = Frame::from(frame);
        match frame.over_limits() {
            // The view stays open; its next drawing may be within them.
            Some(problem) => Err(CallError::Guest(problem)),
            None => Ok(frame),
        }
    }

    /// The serial of the idle instance of `path`, if it has one.
    fn serial(&self, path: &Path) -> Option<u64> {
        self.instances
            .borrow()
            .get(path)
            .map(|instance| instance.serial)
    }

    /// Whether the instance holding `open` still runs, and its code may:
    /// otherwise the view went with it.
    fn live(&self, open: &LiveView) -> Result<(), CallError> {
        let live = self
            .instances
            .borrow()
            .get(&open.component)
            .is_some_and(|instance| {
                instance.serial == open.serial && instance.store.data().stopped().is_none()
            });
        live.then_some(()).ok_or(CallError::ViewClosed)
    }

    /// Drops the guest's view `open`, running its destructor, in its
    /// instance's turn.
    async fn close_view(&self, open: LiveView) {
        let chain = self.chain();
        let Some(_turn) = self.turn(&open.component, chain.id).await else {
            return;
        };
        self.drop_view(&open).await;
    }

    /// Runs the destructor of the view `open`; the caller holds its
    /// instance's turn.
    async fn drop_view(&self, open: &LiveView) {
        if self.live(open).is_err() {
            return;
        }
        let path = &open.component;
        let Some((mut instance, taken)) = self.take_out(path) else {
            return;
        };
        let data = instance.store.data().data.clone();
        let dropped = deadlines::metered(
            self.watch.clone(),
            self.limits.clone(),
            open.resource.resource_drop_async(&mut instance.store),
        )
        .await;
        let health = match dropped {
            Ok(Ok(())) => {
                self.bring_back(path, taken, instance);
                return;
            }
            // The destructor trapped: the instance cannot be re-entered. It
            // is a crash of the package, reported as any other.
            Ok(Err(trap)) => Health::Crashed(crashed(&trap, instance.store.data().out_of_memory)),
            // It computed for too long: it is stopped where it yielded.
            Err(reason) => Health::Unresponsive(CallError::Unresponsive(reason)),
        };
        drop(instance);
        self.gone(path);
        if data.as_ref().is_some_and(|data| data.stopped().is_none()) {
            self.report(path, data.as_ref(), health);
        }
    }

    /// Drops the idle instance of `path` and forgets the views open in it,
    /// which went with it.
    fn drop_instance(&self, path: &Path) {
        let dropped = self.instances.borrow_mut().remove(path);
        drop(dropped);
        self.views
            .borrow_mut()
            .retain(|_, view| view.component != path);
    }

    /// Drops the idle instances whose generation has ended, with everything
    /// their stores hold.
    fn drop_stopped(&self) {
        let stopped: Vec<PathBuf> = self
            .instances
            .borrow()
            .iter()
            .filter(|(_, instance)| instance.store.data().stopped().is_some())
            .map(|(path, _)| path.clone())
            .collect();
        for path in stopped {
            self.drop_instance(&path);
        }
    }

    /// Asks the command in `path` for its root results for `query`, unless
    /// its caller gives up on the answer (drops the receiver of `reply`)
    /// first: then the call is not started, or is stopped where the guest
    /// waits, and the instance goes with it (see [`Host::run_guest_until`]).
    async fn root_results(
        &self,
        path: &Path,
        query: String,
        data: Option<PackageData>,
        reply: &mut oneshot::Sender<Result<Vec<RootResult>, CallError>>,
    ) -> Result<Vec<RootResult>, CallError> {
        // Its search was replaced or left before the call started.
        if reply.is_closed() {
            return Err(CallError::Cancelled);
        }
        let chain = self.chain();
        let _turn = match unless(self.turn_for(path, &chain), reply.closed()).await {
            Ok(turn) => turn?,
            // Replaced or left while it waited for its turn.
            Err(()) => return Err(CallError::Cancelled),
        };
        self.instance(path, data).await?;
        let provider = self
            .instances
            .borrow()
            .get(path)
            .and_then(|instance| instance.root_results.as_ref())
            .map(|provider| provider.pane_extension_root_results().clone())
            .ok_or_else(|| {
                CallError::Interface(format!("it does not export {ROOT_RESULTS_INTERFACE}"))
            })?;
        let result = self
            .run_guest_until(
                path,
                &chain,
                async |instance| {
                    instance
                        .store
                        .run_concurrent(async |store| provider.call_results_for(store, query).await)
                        .await
                },
                reply.closed(),
            )
            .await?;
        let results = self.settle(path, result, CallError::Guest)?;
        Ok(results
            .into_iter()
            .map(|result| RootResult {
                listing: ResultListing {
                    id: result.id,
                    title: result.title,
                    subtitle: result.subtitle,
                },
                action: match result.action {
                    root_results::RootAction::Copy(text) => RootAction::Copy(text),
                    root_results::RootAction::OpenUrl(url) => RootAction::OpenUrl(url),
                    root_results::RootAction::OpenFile(path) => RootAction::OpenFile(path),
                },
            })
            .collect())
    }

    /// Runs one cycle of the continuing service of the command with
    /// manifest id `command` in `component` (see `Runtime::run_cycle_with`).
    async fn run_cycle(
        &self,
        path: &Path,
        command: String,
        data: Option<PackageData>,
    ) -> Result<Cycle, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.instance(path, data).await?;
        let service = self
            .instances
            .borrow()
            .get(path)
            .and_then(|instance| instance.service.as_ref())
            .map(|provider| provider.pane_extension_service().clone())
            .ok_or_else(|| {
                CallError::Interface(format!("it does not export {SERVICE_INTERFACE}"))
            })?;
        let result = self
            .run_guest(path, &chain, async |instance| {
                instance
                    .store
                    .run_concurrent(async |store| service.call_run_cycle(store, command).await)
                    .await
            })
            .await?;
        self.settle(path, result, CallError::Guest)
            .map(|cycle| Cycle {
                status: cycle.status,
                next_seconds: cycle.next_seconds,
            })
    }

    async fn search(
        &self,
        path: &Path,
        id: String,
        query: String,
        data: Option<PackageData>,
        mut stopped: SearchStopped,
    ) -> Result<Vec<SearchResult>, CallError> {
        // Replaced while it waited in the queue, or soon after: it is not
        // started.
        #[cfg(any(test, debug_assertions))]
        let wait = match lock(&self.search_timer).clone() {
            Some(timer) => timer(SEARCH_DEBOUNCE),
            None => Box::pin(tokio::time::sleep(SEARCH_DEBOUNCE)),
        };
        #[cfg(not(any(test, debug_assertions)))]
        let wait = tokio::time::sleep(SEARCH_DEBOUNCE);
        if stopped.stopped() || stopped.stopped_before(wait).await {
            return Err(CallError::Cancelled);
        }
        let chain = self.chain();
        let waited = unless(self.turn_for(path, &chain), async {
            let _ = (&mut stopped.0).await;
        })
        .await;
        let _turn = match waited {
            Ok(turn) => turn?,
            // Replaced while it waited for its turn.
            Err(()) => return Err(CallError::Cancelled),
        };
        self.instance(path, data).await?;
        let search = self
            .instances
            .borrow()
            .get(path)
            .and_then(|instance| instance.command_search.as_ref())
            .map(|provider| provider.pane_extension_command_search().clone())
            .ok_or_else(|| {
                CallError::Interface(format!("it does not export {COMMAND_SEARCH_INTERFACE}"))
            })?;
        let result = self
            .run_guest_until(
                path,
                &chain,
                async |instance| {
                    instance
                        .store
                        .run_concurrent(async |store| search.call_search(store, id, query).await)
                        .await
                },
                // Resolves when the search is stopped: its sender sent or
                // was dropped.
                async move {
                    let _ = stopped.0.await;
                },
            )
            .await?;
        let results = self.settle(path, result, CallError::Guest)?;
        Ok(results
            .into_iter()
            .map(|result| ResultListing {
                id: result.id,
                title: result.title,
                subtitle: result.subtitle,
            })
            .collect())
    }

    async fn indexed_results(
        &self,
        path: &Path,
        data: Option<PackageData>,
    ) -> Result<Vec<IndexedResult>, CallError> {
        let chain = self.chain();
        let _turn = self.turn_for(path, &chain).await?;
        self.instance(path, data).await?;
        let exported = self
            .instances
            .borrow()
            .get(path)
            .is_some_and(|instance| instance.indexed_results.is_some());
        if !exported {
            return Err(CallError::Interface(format!(
                "it does not export {INDEXED_RESULTS_INTERFACE}"
            )));
        }
        let result = self
            .run_guest(path, &chain, async |instance| {
                let provider = instance
                    .indexed_results
                    .as_ref()
                    .expect("checked above")
                    .pane_extension_indexed_results();
                instance
                    .store
                    .run_concurrent(async |store| provider.call_results(store).await)
                    .await
            })
            .await?;
        let results = self.settle(path, result, CallError::Guest)?;
        Ok(results
            .into_iter()
            .map(|result| IndexedResult {
                listing: ResultListing {
                    id: result.id,
                    title: result.title,
                    subtitle: result.subtitle,
                },
                action: match result.action {
                    indexed_results::IndexedAction::OpenApplication(id) => {
                        IndexedAction::OpenApplication(id)
                    }
                },
            })
            .collect())
    }

    /// Runs `call` on the live instance of `path`, in `chain`, serving the
    /// operation calls its guest makes while it runs. Without a live
    /// instance (a view's instance has stopped) it is
    /// [`CallError::ViewClosed`]. The caller holds the instance's turn.
    ///
    /// The instance is taken out of the host for the call, so the host can
    /// serve an operation call its guest makes while the guest waits for
    /// the answer: the guest's call is not polled until the operation's
    /// answer is sent, and then resumes. The component is on the call chain
    /// meanwhile, so a call back into its package is refused rather than
    /// waiting on itself.
    ///
    /// While the guest waits on anything outside itself (the user, a
    /// program, a helper, a web request, a clock, another extension's
    /// operation), this call only awaits, and the thread serves the other
    /// requests: a wait is never the guest's computing.
    ///
    /// The call stops as soon as the instance's generation, or that of any
    /// call further out in the chain, ends: the guest's call is dropped where
    /// it waits, and so is the instance, since Wasmtime keeps a dropped call's
    /// task in the store, where it would resume on the next call. A result
    /// that completes after its generation ended is discarded the same way.
    /// The generation is checked each time the guest yields, which it does
    /// at every epoch tick even while it computes (see `deadlines`).
    ///
    /// A call whose guest computed for [`COMPUTE_LIMIT`] without finishing
    /// is stopped the same way, as unresponsive: a failure of the guest's
    /// own package, reported as such ([`Health::Unresponsive`]). Time spent
    /// serving its operation calls counts for their targets, not for it.
    async fn run_guest<R>(
        &self,
        path: &Path,
        chain: &Chain,
        call: impl AsyncFnOnce(&mut Instance) -> R,
    ) -> Result<R, CallError> {
        self.run_guest_until(path, chain, call, std::future::pending())
            .await
    }

    /// Like [`Host::run_guest`], and the call also stops, as when its
    /// generation ends, once `cancelled` resolves: its caller no longer
    /// wants the answer. It is then [`CallError::Cancelled`]; the instance is
    /// dropped all the same (Wasmtime would resume the dropped call's task),
    /// and it is not a failure of the package.
    async fn run_guest_until<R>(
        &self,
        path: &Path,
        chain: &Chain,
        call: impl AsyncFnOnce(&mut Instance) -> R,
        cancelled: impl Future<Output = ()>,
    ) -> Result<R, CallError> {
        /// What happened next while the guest's call ran.
        enum Next<R> {
            Returned(R),
            Stopped(End),
            Cancelled,
            Called(OperationCall),
            /// It computed for too long, as this says.
            Unresponsive(String),
            /// Pane gave up on this thread while it was stuck.
            GivenUp,
        }

        /// Why the call ended without its result.
        enum Halt {
            Stopped(End),
            Cancelled,
            Unresponsive(String),
            GivenUp,
        }

        let (mut instance, taken) = self.take_out(path).ok_or(CallError::ViewClosed)?;
        let own = instance.store.data().generation().cloned();
        let mut inner = chain.clone();
        inner.components.push(path.to_path_buf());
        inner.owners.extend(own.clone());
        let mut ends = std::pin::pin!(first_end(&inner.owners));
        // The guest's operation calls, served by this frame only.
        let mut calls = instance
            .calls
            .take()
            .unwrap_or_else(|| operations::channel().1);
        instance.store.data_mut().serving = true;
        let mut cancelled = std::pin::pin!(cancelled);
        let faults = self.faults.clone();
        let watch = self.watch.clone();
        let limits = self.limits.clone();
        let result = {
            // Only the guest's own computing counts (see `deadlines`).
            let mut running = std::pin::pin!(deadlines::metered(
                watch.clone(),
                limits,
                call(&mut instance)
            ));
            let mut waiting = std::pin::pin!(faults.waiting());
            loop {
                let next = std::future::poll_fn(|cx| {
                    faults.check(waiting.as_mut(), cx);
                    // Given up on while it was stuck: the guest runs no more.
                    if watch.given_up() {
                        return Poll::Ready(Next::GivenUp);
                    }
                    if let Poll::Ready(end) = ends.as_mut().poll(cx) {
                        return Poll::Ready(Next::Stopped(end));
                    }
                    if cancelled.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(Next::Cancelled);
                    }
                    {
                        let _running = watch.doing(Doing::Running);
                        match running.as_mut().poll(cx) {
                            Poll::Ready(Ok(result)) => return Poll::Ready(Next::Returned(result)),
                            Poll::Ready(Err(why)) => return Poll::Ready(Next::Unresponsive(why)),
                            Poll::Pending => {}
                        }
                    }
                    match calls.poll_recv(cx) {
                        Poll::Ready(Some(call)) => Poll::Ready(Next::Called(call)),
                        // The instance holds the sender: never closed here.
                        _ => Poll::Pending,
                    }
                })
                .await;
                match next {
                    Next::Returned(result) => match own.as_ref().and_then(Generation::ended) {
                        Some(end) => break Err(Halt::Stopped(end)),
                        None => break Ok(result),
                    },
                    Next::Stopped(end) => break Err(Halt::Stopped(end)),
                    Next::Cancelled => break Err(Halt::Cancelled),
                    Next::Unresponsive(why) => break Err(Halt::Unresponsive(why)),
                    Next::GivenUp => break Err(Halt::GivenUp),
                    Next::Called(operation_call) => {
                        Box::pin(self.serve_operation(operation_call, &inner)).await;
                    }
                }
            }
        };
        instance.store.data_mut().serving = false;
        // A helper runs no longer than the call that started it: one the
        // guest left running when its call ended is ended too.
        let state = instance.store.data();
        state.helpers.stop_owned_by(state.owner);
        // A call the guest sent but did not wait for before its call ended
        // has no frame to serve it.
        while let Ok(stranded) = calls.try_recv() {
            let _ = stranded.reply.send(Err(operations::outside_a_call()));
        }
        instance.calls = Some(calls);
        match result {
            Ok(result) => {
                self.bring_back(path, taken, instance);
                Ok(result)
            }
            Err(halt) => {
                let data = instance.store.data().data.clone();
                // The instance is dropped with its store: the abandoned
                // task, its host tasks (web requests too), streams, futures
                // and views.
                drop(instance);
                self.gone(path);
                match halt {
                    Halt::Stopped(end) => Err(ended(end)),
                    Halt::Cancelled => Err(CallError::Cancelled),
                    Halt::Unresponsive(why) => {
                        let error = CallError::Unresponsive(why);
                        eprintln!("pane: {} stopped responding: {error}", path.display());
                        self.report(path, data.as_ref(), Health::Unresponsive(error.clone()));
                        Err(error)
                    }
                    Halt::GivenUp => Err(given_up()),
                }
            }
        }
    }

    /// Serves one operation call a guest in `chain` made, answering it.
    async fn serve_operation(&self, mut call: OperationCall, chain: &Chain) {
        // Its caller gave up on it before it started: it is not started.
        if call.reply.is_closed() {
            return;
        }
        let result = self.operation(&mut call, chain).await;
        let _ = call.reply.send(result);
    }

    /// Serves `call`: checks it, resolves its target, runs the operation
    /// (starting the target if it is not running) and checks the answer.
    async fn operation(
        &self,
        call: &mut OperationCall,
        chain: &Chain,
    ) -> Result<String, OperationError> {
        self.check_call(call, chain)?;
        let target = self.resolve_target(call, chain)?;
        // Disabled or replaced while it was serving the call, it was
        // stopped, and its answer is not passed on.
        let answer = self.run_operation(&target, call, chain).await?;
        operations::check_json(&answer, &format!("result of {}", target.title))?;
        Ok(answer)
    }

    /// Refuses a call whose input is not JSON within the limit, or that would
    /// make the chain too deep.
    fn check_call(&self, call: &OperationCall, chain: &Chain) -> Result<(), OperationError> {
        operations::check_json(&call.input, "input")?;
        if chain.components.len() >= operations::MAX_CALL_DEPTH {
            return Err(OperationError::refused(format!(
                "the chain of calls is {} deep; Pane allows at most {}",
                chain.components.len(),
                operations::MAX_CALL_DEPTH
            )));
        }
        Ok(())
    }

    /// The installed package and component serving `call`, unless its
    /// package already serves a call in the chain, through whichever of its
    /// components.
    fn resolve_target(
        &self,
        call: &OperationCall,
        chain: &Chain,
    ) -> Result<Target, OperationError> {
        let directory = lock(&self.directory).clone();
        let installed = match directory {
            Some(directory) => directory(),
            None => operations::Installed::default(),
        };
        let target =
            installed.resolve(&call.caller, &call.source, &call.operation, call.version)?;
        let in_chain = chain.components.iter().any(|component| {
            *component == target.component
                || installed.package_of(component) == Some(&target.identity)
        });
        if in_chain {
            return Err(OperationError::refused(format!(
                "{} is already serving a call in this chain; an extension cannot be \
                 called back while its own call waits",
                target.title
            )));
        }
        Ok(target)
    }

    /// Runs the operation of `call` in `target`'s component, in its turn,
    /// starting it if it is not running, and returns its answer.
    async fn run_operation(
        &self,
        target: &Target,
        call: &mut OperationCall,
        chain: &Chain,
    ) -> Result<String, OperationError> {
        let failed = |error| match error {
            // Only a caller that no longer waits cancels a call.
            CallError::Cancelled => {
                OperationError::refused("the caller no longer waits for this call")
            }
            error => OperationError::from_call(&target.title, error),
        };
        // Its caller gave up on it: the check the call was sent with can
        // miss a give-up that lands while the call waits to be served, so
        // the check is made again just before the operation runs, and the
        // instance the call started for it is dropped again.
        if call.reply.is_closed() {
            return Err(failed(CallError::Cancelled));
        }
        // One call into the target's instance at a time: this one waits for
        // its turn, unless the chain's code stops meanwhile. A turn that
        // would come only once this chain's own calls return is refused.
        let _turn = match unless(
            self.turn(&target.component, chain.id),
            first_end(&chain.owners),
        )
        .await
        {
            Ok(Some(turn)) => turn,
            Ok(None) => {
                return Err(OperationError::refused(format!(
                    "{} is serving another call that waits for this one; call it again once \
                     that call is done",
                    target.title
                )));
            }
            Err(end) => return Err(failed(ended(end))),
        };
        self.instance(&target.component, target.data.clone())
            .await
            .map_err(failed)?;
        let provider = self
            .instances
            .borrow()
            .get(&target.component)
            .and_then(|instance| instance.operations.as_ref())
            .map(|provider| provider.pane_extension_published_operations().clone())
            // The install check requires the export, so only a component
            // replaced behind Pane's back lacks it.
            .ok_or_else(|| {
                failed(CallError::Interface(format!(
                    "it does not export {OPERATIONS_INTERFACE}"
                )))
            })?;
        if call.reply.is_closed() {
            self.drop_instance(&target.component);
            return Err(failed(CallError::Cancelled));
        }
        let (name, input) = (call.operation.clone(), call.input.clone());
        // A call whose caller gives up on it while it runs is stopped, as
        // a search's call is (the caller is gone; the answer has nowhere
        // to go).
        let result = self
            .run_guest_until(
                &target.component,
                chain,
                async |instance| {
                    instance
                        .store
                        .run_concurrent(async |store| {
                            provider.call_run_operation(store, name, input).await
                        })
                        .await
                },
                call.reply.closed(),
            )
            .await
            .map_err(failed)?;
        self.settle(&target.component, result, CallError::Guest)
            .map_err(failed)
    }

    /// Maps a call outcome to the caller's result, turning the guest's own
    /// error with `guest_error`. A trapped instance cannot be re-entered, so
    /// it is dropped and the next call starts a fresh one.
    fn settle<T, E>(
        &self,
        path: &Path,
        outcome: wasmtime::Result<wasmtime::Result<Result<T, E>>>,
        guest_error: impl FnOnce(E) -> CallError,
    ) -> Result<T, CallError> {
        let (data, out_of_memory) = self
            .instances
            .borrow()
            .get(path)
            .map(|instance| {
                let state = instance.store.data();
                (state.data.clone(), state.out_of_memory)
            })
            .unwrap_or_default();
        match outcome.and_then(|inner| inner) {
            Ok(result) => result.map_err(guest_error),
            Err(trap) => {
                self.drop_instance(path);
                let error = crashed(&trap, out_of_memory);
                self.report(path, data.as_ref(), Health::Crashed(error.clone()));
                Err(error)
            }
        }
    }

    /// Tells the health report how a call into `path`, of an installed
    /// package whose extension data is `data`, failed; nothing for a command
    /// built into Pane.
    fn report(&self, path: &Path, data: Option<&PackageData>, health: Health) {
        // Code of a thread Pane gave up on is stopped where it runs (a
        // guest traps at its next tick): no failure of its package.
        if self.watch.given_up() {
            return;
        }
        let report = lock(&self.health).clone();
        if let (Some(report), Some(data)) = (report, data) {
            report(path, data, health);
        }
    }

    /// Makes sure `path` has a live instance in the generation of `data`,
    /// instantiating it on first use. A call whose generation has ended (its
    /// package was disabled, reloaded or updated since it was asked for) gets
    /// none and is not started: it cannot bring its instance back. The
    /// caller holds the component's turn.
    async fn instance(&self, path: &Path, data: Option<PackageData>) -> Result<(), CallError> {
        // An instance of an ended generation goes; one of the package's
        // current generation stays, even for a stale call, which is refused.
        let stopped = self
            .instances
            .borrow()
            .get(path)
            .is_some_and(|instance| instance.store.data().stopped().is_some());
        if stopped {
            self.drop_instance(path);
        }
        if let Some(end) = data.as_ref().and_then(PackageData::stopped) {
            return Err(ended(end));
        }
        let live = self.instances.borrow().contains_key(path);
        if !live {
            let started = self.start_instance(path, data.clone()).await;
            // One stopped while it started (disabled, say) did not fail.
            let stopped = data.as_ref().and_then(PackageData::stopped).is_some();
            if let Err(error) = &started
                && !stopped
                && !self.watch.given_up()
            {
                self.report(path, data.as_ref(), Health::FailedToStart(error.clone()));
            }
            started?;
        }
        Ok(())
    }

    /// Loads and instantiates `path` as a live instance with `data`.
    async fn start_instance(
        &self,
        path: &Path,
        data: Option<PackageData>,
    ) -> Result<(), CallError> {
        let component = self.component(path)?;
        let watch = self.watch.clone();
        // Its code is stopped once Pane gives up on this thread.
        let data = data.map(|data| data.fenced(watch.fence().clone()));
        let generation = data.as_ref().map(|data| data.generation().clone());
        let mut end = std::pin::pin!(first_end(generation.as_slice()));
        let (calls, calls_received) = operations::channel();
        let mut store = Store::new(
            &self.code.engine,
            GuestState {
                wasi: WasiCtx::builder().build(),
                table: ResourceTable::new(),
                http: WasiHttpCtx::new(),
                sender: http::Sender::new(data.clone(), self.network.clone(), watch.clone()),
                out_of_memory: false,
                data,
                component: path.to_path_buf(),
                calls,
                serving: false,
                applications: self.applications.clone(),
                files: self.files.clone(),
                clipboard: self.clipboard.clone(),
                directory: self.directory.clone(),
                launches: self.launches.clone(),
                owner: self.helpers.new_owner(),
                helpers: self.helpers.clone(),
                watch: self.watch.clone(),
            },
        );
        store.limiter(|state| state);
        // The guest yields to this thread at every epoch tick, however long
        // it computes, which is progress for the watchdog (see `deadlines`);
        // once Pane gave up on this thread, it traps there instead.
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(|store| {
            let watch = &store.data().watch;
            if watch.given_up() {
                return Ok(wasmtime::UpdateDeadline::Interrupt);
            }
            watch.beat();
            Ok(wasmtime::UpdateDeadline::Yield(1))
        });
        let load = |error: wasmtime::Error| CallError::Load(format!("{error:#}"));
        // Starting is not metered: a slow start is not a failure of the
        // package. It stops once its generation ends (it was disabled, say):
        // one that never finishes holds its component's turn until then.
        let instance = {
            let mut instantiating =
                std::pin::pin!(self.code.linker.instantiate_async(&mut store, &component));
            std::future::poll_fn(|cx| {
                if watch.given_up() {
                    return Poll::Ready(Err(given_up()));
                }
                if let Poll::Ready(end) = end.as_mut().poll(cx) {
                    return Poll::Ready(Err(ended(end)));
                }
                let _starting = watch.doing(Doing::Starting);
                instantiating
                    .as_mut()
                    .poll(cx)
                    .map(|started| started.map_err(load))
            })
            .await
        };
        // One that asked for more memory than it may have while it started
        // says so, not where it gave up.
        let instance = match instance {
            Err(CallError::Load(_)) if store.data().out_of_memory => {
                return Err(CallError::Load(memory::out_of_memory()));
            }
            started => started?,
        };
        let bindings =
            bindings::ExtensionWithClipboard::new(&mut store, &instance).map_err(load)?;
        // Only a command that computes root results exports them.
        let root_results = root_bindings::RootResultsProvider::new(&mut store, &instance).ok();
        // Only a command that supplies results ahead of the query exports
        // them.
        let indexed_results =
            indexed_bindings::IndexedResultsProvider::new(&mut store, &instance).ok();

        // Only a component serving published operations exports them.
        let operations = operations_bindings::OperationsProvider::new(&mut store, &instance).ok();
        // Only a command that searches as the user types exports it.
        let command_search =
            search_bindings::CommandSearchProvider::new(&mut store, &instance).ok();
        // Only a command that runs a continuing service exports it.
        let service = service_bindings::ServiceProvider::new(&mut store, &instance).ok();
        // On its generation's undo list: the generation's end has this
        // thread drop it at once, as only this thread may, even while no
        // call asks for it.
        let undo = generation.map(|generation| {
            let nudge = self.nudge.clone();
            generation.on_end("extension instance", move || {
                if let Some(requests) = nudge.upgrade() {
                    let _ = requests.send(Request::DropStopped);
                }
                Ok(())
            })
        });
        let serial = self.next_serial.get();
        self.next_serial.set(serial + 1);
        self.instances.borrow_mut().insert(
            path.to_path_buf(),
            Instance {
                store,
                bindings,
                root_results,
                indexed_results,
                operations,
                command_search,
                service,
                calls: Some(calls_received),
                serial,
                _undo: undo,
            },
        );
        Ok(())
    }

    /// Compiles `path` once (see [`Code::compile`]).
    fn component(&self, path: &Path) -> Result<Component, CallError> {
        let compiled = self.components.borrow().get(path).cloned();
        if let Some(component) = compiled {
            return Ok(component);
        }
        // Compiling may take long without anything being stuck.
        let _compiling = self.watch.exempt();
        let component = self.code.compile(path)?;
        self.components
            .borrow_mut()
            .insert(path.to_path_buf(), component.clone());
        Ok(component)
    }
}

/// Resolves once any of `owners` ends, with why; never without any.
fn first_end(owners: &[Generation]) -> impl Future<Output = End> + use<> {
    let mut ends: Vec<_> = owners
        .iter()
        .map(|owner| Box::pin(owner.wait_end()))
        .collect();
    std::future::poll_fn(move |cx| {
        for end in &mut ends {
            if let Poll::Ready(end) = end.as_mut().poll(cx) {
                return Poll::Ready(end);
            }
        }
        Poll::Pending
    })
}

/// Awaits `work`, unless `stop` resolves first: then its output, and `work`
/// is dropped where it waits.
async fn unless<T, S>(
    work: impl Future<Output = T>,
    stop: impl Future<Output = S>,
) -> Result<T, S> {
    let (mut work, mut stop) = (std::pin::pin!(work), std::pin::pin!(stop));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(stopped) = stop.as_mut().poll(cx) {
            return Poll::Ready(Err(stopped));
        }
        work.as_mut().poll(cx).map(Ok)
    })
    .await
}

impl From<command::Frame> for Frame {
    fn from(frame: command::Frame) -> Frame {
        let shapes = frame
            .shapes
            .into_iter()
            .map(|shape| match shape {
                command::Shape::Rect(rect) => Shape::Rect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    fill: Rgb(rect.fill),
                },
                command::Shape::Text(text) => Shape::Text {
                    x: text.x,
                    y: text.y,
                    content: text.content,
                    color: Rgb(text.color),
                },
            })
            .collect();
        Frame {
            width: frame.width,
            height: frame.height,
            shapes,
            value: frame.value,
        }
    }
}

impl From<ViewEvent> for command::ViewEvent {
    fn from(event: ViewEvent) -> command::ViewEvent {
        let point = |Point { x, y }| command::Point { x, y };
        match event {
            ViewEvent::Key(key) => command::ViewEvent::Key(match key {
                Key::Left => command::Key::Left,
                Key::Right => command::Key::Right,
                Key::Up => command::Key::Up,
                Key::Down => command::Key::Down,
                Key::Home => command::Key::Home,
                Key::End => command::Key::End,
            }),
            ViewEvent::PointerDown(at) => command::ViewEvent::PointerDown(point(at)),
            ViewEvent::PointerMove(at) => command::ViewEvent::PointerMove(point(at)),
            ViewEvent::PointerUp(at) => command::ViewEvent::PointerUp(point(at)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_data::ExtensionData;
    use crate::packages::PackageIdentity;
    use futures::executor::block_on;
    use std::time::Duration;

    fn settings_sample() -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/sample_settings.wasm");
        assert!(
            path.exists(),
            "{} is missing; run `cargo xtask guests`",
            path.display()
        );
        path
    }

    /// Quitting Pane drops the launcher, and with it the last runtime
    /// handle: a helper still running then ends too.
    #[test]
    fn dropping_the_last_runtime_handle_ends_running_helpers() {
        let target = pane_target::Target::current().expect("a known target");
        let program = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/sample-helper/helpers")
            .join(target.id())
            .join(format!("pane-echo{}", target.exe_suffix()));
        assert!(
            program.exists(),
            "{} is missing; run `cargo xtask guests`",
            program.display()
        );
        let runtime = Runtime::start().unwrap();
        let helpers = runtime.shared.helpers.clone();
        let running = helpers
            .start(runner::Spec {
                fence: None,
                name: "echo".into(),
                program,
                args: vec!["--wait".into(), "5".into()],
                input: "hi".into(),
                generation: None,
                owner: helpers.new_owner(),
            })
            .unwrap();
        let clone = runtime.clone();
        drop(runtime);
        assert_eq!(helpers.running().len(), 1, "a clone keeps the runtime");

        let dropped = std::time::Instant::now();
        drop(clone);

        assert_eq!(helpers.running(), Vec::<u32>::new());
        let error = block_on(running.finish()).unwrap_err();
        assert_eq!(error.kind, HelperErrorKind::Refused, "{error:?}");
        assert!(dropped.elapsed() < std::time::Duration::from_secs(4));
        // Nothing starts once it is gone.
        assert!(
            helpers
                .start(runner::Spec {
                    fence: None,
                    name: "echo".into(),
                    program: PathBuf::from("unused"),
                    args: vec![],
                    input: String::new(),
                    generation: None,
                    owner: 0,
                })
                .is_err()
        );
    }

    /// A call for a package that was disabled, served after its instances
    /// were dropped, must not start a new instance of it.
    #[test]
    fn a_disabled_package_command_starts_no_instance() {
        let data = tempfile::tempdir().unwrap();
        let settings = ExtensionData::open(data.path());
        let identity = PackageIdentity::local(data.path()).unwrap();
        let owned = settings.owned_by(&identity);
        let component = settings_sample();
        let runtime = Runtime::start().unwrap();
        block_on(runtime.render_with(&component, Some(owned.clone()))).unwrap();

        settings.set_enabled(&identity, false);
        runtime.forget([component.clone()]);
        let queued = runtime.render_with(&component, Some(owned));

        assert_eq!(block_on(queued), Err(CallError::Disabled));
    }

    fn guest(file: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests")
            .join(file);
        assert!(path.exists(), "{} is missing", path.display());
        path
    }

    /// Stopping a call releases what its instance holds: a stream open to
    /// the host with the future of its write pending, the clock the call
    /// awaits and a custom view open in the same instance.
    #[test]
    fn stopping_a_call_releases_the_stream_future_and_view_its_instance_holds() {
        use std::time::{Duration, Instant};

        let data = tempfile::tempdir().unwrap();
        let packages = ExtensionData::open(data.path());
        let identity = PackageIdentity::local(data.path()).unwrap();
        let owned = packages.owned_by(&identity);
        let component = guest("faulty.wasm");
        let runtime = Runtime::start().unwrap();
        let (view, _) =
            block_on(runtime.open_view_with(&component, "view", Some(owned.clone()))).unwrap();
        let holding = {
            let (runtime, component) = (runtime.clone(), component.clone());
            std::thread::spawn(move || {
                // Not listed: the fixture runs it as a callback no item names.
                block_on(runtime.handle_event_with(&component, "hold", "{}", Some(owned)))
            })
        };
        let started = Instant::now();
        let saved =
            || std::fs::read_to_string(data.path().join("settings.json")).unwrap_or_default();
        while !saved().contains("\"holding\": \"started\"") {
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "it did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        packages.set_enabled(&identity, false);

        assert_eq!(holding.join().unwrap(), Err(CallError::Disabled));
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(!saved().contains("finished"));
        assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());
        assert_eq!(block_on(runtime.view_count()), 0);
        assert_eq!(
            block_on(runtime.view_event(view, ViewEvent::Key(Key::Up))),
            Err(CallError::ViewClosed)
        );
    }

    /// A custom view the window still shows from a crashed runtime thread
    /// names no view of the thread that replaced it: ids are never reused.
    #[test]
    fn a_restarted_runtime_never_reuses_a_view_id_of_the_crashed_one() {
        let component = guest("sample_rust.wasm");
        let runtime = Runtime::start().unwrap();
        let (old, _) = block_on(runtime.open_view(&component, "color")).unwrap();

        runtime.inject(Fault::Crash);
        let started = std::time::Instant::now();
        while runtime.status() == RuntimeStatus::Running {
            assert!(started.elapsed() < std::time::Duration::from_secs(8));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert!(matches!(runtime.status(), RuntimeStatus::Restarted { .. }));
        let (new, _) = block_on(runtime.open_view(&component, "color")).unwrap();
        assert_ne!(old, new);
        assert_eq!(
            block_on(runtime.view_event(old, ViewEvent::Key(Key::Up))),
            Err(CallError::ViewClosed)
        );
        assert!(block_on(runtime.view_event(new, ViewEvent::Key(Key::Up))).is_ok());
        assert_eq!(block_on(runtime.view_count()), 1);
    }

    /// A settings sample instance of a package in `data`, and its data.
    fn settings_package(data: &tempfile::TempDir) -> (ExtensionData, PackageIdentity) {
        let packages = ExtensionData::open(data.path());
        let identity = PackageIdentity::local(data.path()).unwrap();
        (packages, identity)
    }

    /// The one check every host interface goes through stops code whose
    /// runtime thread's fence closed, also through package data that was
    /// not fenced with it; a generation that ended says why first.
    #[test]
    fn code_is_stopped_by_its_thread_s_fence_with_or_without_data() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let unfenced = packages.owned_by(&identity);
        let fence = Fence::default();
        assert_eq!(code_stopped(Some(&unfenced), &fence), None);
        assert_eq!(code_stopped(None, &fence), None);

        fence.close();

        assert_eq!(code_stopped(Some(&unfenced), &fence), Some(End::Abandoned));
        assert_eq!(code_stopped(None, &fence), Some(End::Abandoned));
        packages.set_enabled(&identity, false);
        assert_eq!(code_stopped(Some(&unfenced), &fence), Some(End::Disabled));
    }

    /// What the settings sample saved under `key` in `identity`'s settings.
    fn saved(packages: &ExtensionData, identity: &PackageIdentity, key: &str) -> Option<String> {
        packages
            .owned_by(identity)
            .get(DataKind::Settings, key)
            .unwrap()
    }

    /// Waits, generously, until `done` holds.
    fn until(what: &str, done: impl Fn() -> bool) {
        let started = std::time::Instant::now();
        while !done() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(60),
                "{what} did not happen"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Short limits, so tests reach them quickly (see [`Limits`]).
    fn short_limits() -> Limits {
        Limits {
            compute: Duration::from_secs(1),
            warn: Duration::from_millis(500),
            unresponsive: Duration::from_secs(2),
        }
    }

    /// A runtime with [`short_limits`], and every failure it reports.
    fn watched_runtime() -> (Runtime, Arc<Mutex<Vec<Health>>>) {
        let runtime = Runtime::start().unwrap();
        runtime.set_limits(short_limits());
        let reported = Arc::new(Mutex::new(Vec::new()));
        {
            let reported = reported.clone();
            runtime.set_health(Arc::new(move |_, _, health| {
                lock(&reported).push(health);
            }));
        }
        (runtime, reported)
    }

    /// Runs `item` of the settings sample on another thread.
    fn run_in_background(
        runtime: &Runtime,
        packages: &ExtensionData,
        identity: &PackageIdentity,
        item: &str,
    ) -> std::thread::JoinHandle<Result<Answer, CallError>> {
        let (runtime, owned, item) = (
            runtime.clone(),
            packages.owned_by(identity),
            item.to_owned(),
        );
        std::thread::spawn(move || {
            block_on(runtime.run_item_with(&settings_sample(), &item, Some(owned)))
        })
    }

    /// A guest computing without waiting yields at every tick: after the
    /// compute limit its call is stopped as unresponsive, reported as its
    /// package's failure, and its instance is gone, while the runtime
    /// serves the next call.
    #[test]
    fn a_guest_computing_without_waiting_is_stopped_after_the_compute_limit() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let component = settings_sample();
        let (runtime, reported) = watched_runtime();
        // Another command is active meanwhile, and stays so.
        let other = guest("sample_rust.wasm");
        let (view, _) = block_on(runtime.open_view(&other, "color")).unwrap();

        let busy =
            block_on(runtime.run_item_with(&component, "busy", Some(packages.owned_by(&identity))));

        let Err(CallError::Unresponsive(reason)) = &busy else {
            panic!("expected it stopped as unresponsive, got {busy:?}");
        };
        assert!(reason.contains("computed for 1 second without"), "{reason}");
        assert_eq!(
            saved(&packages, &identity, "busy").as_deref(),
            Some("started")
        );
        assert!(matches!(
            lock(&reported).as_slice(),
            [Health::Unresponsive(CallError::Unresponsive(_))]
        ));
        assert_eq!(block_on(runtime.running()), vec![other.clone()]);
        // The other command's view is still open, and the package runs
        // again from a fresh instance.
        assert!(block_on(runtime.view_event(view, ViewEvent::Key(Key::Up))).is_ok());
        assert!(
            block_on(runtime.render_with(&component, Some(packages.owned_by(&identity)))).is_ok()
        );
        assert_eq!(runtime.status(), RuntimeStatus::Running);
    }

    /// Ending a generation stops a guest that computes without waiting at
    /// its next tick, rather than when it yields by itself or reaches the
    /// compute limit (which it would report as its failure).
    #[test]
    fn disabling_a_package_stops_its_computing_guest_at_once() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let (runtime, reported) = watched_runtime();
        runtime.set_limits(Limits {
            compute: Duration::from_secs(600),
            ..Limits::default()
        });
        let busy = run_in_background(&runtime, &packages, &identity, "busy");
        until("it started computing", || {
            saved(&packages, &identity, "busy").as_deref() == Some("started")
        });

        packages.set_enabled(&identity, false);

        assert_eq!(busy.join().unwrap(), Err(CallError::Disabled));
        assert!(lock(&reported).is_empty());
        packages.set_enabled(&identity, true);
        assert_eq!(
            saved(&packages, &identity, "busy").as_deref(),
            Some("started")
        );
    }

    /// A guest whose host calls are slow (Pane's own work, here computing
    /// for three times the compute limit and past the time the watchdog
    /// gives up after) is never stopped nor blamed, and the runtime thread
    /// inside the host call is never given up on.
    #[test]
    fn a_guest_whose_host_calls_are_slow_is_never_stopped_or_blamed() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let component = settings_sample();
        let (runtime, reported) = watched_runtime();
        let slow = short_limits().compute * 3;
        assert!(slow > short_limits().unresponsive);

        runtime.inject(Fault::SlowHostCall(slow));
        let saved_note =
            block_on(runtime.run_item_with(&component, "note", Some(packages.owned_by(&identity))));

        assert_eq!(
            saved_note,
            Ok(Answer {
                status: Some("Saved a note".into()),
                entries: None,
            })
        );
        assert!(lock(&reported).is_empty(), "{:?}", lock(&reported));
        assert_eq!(runtime.status(), RuntimeStatus::Running);
        assert_eq!(runtime.abandoned_threads(), 0);
        assert_eq!(block_on(runtime.running()), vec![component]);
    }

    /// Clipboard history's host calls (#35) are marked like every other
    /// host call: the slow host call computes inside the view's first one,
    /// so the view takes at least that long, yet the guest is never stopped
    /// or blamed and the thread is never given up on.
    #[test]
    fn a_guest_whose_clipboard_host_calls_are_slow_is_never_stopped_or_blamed() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let component = guest("clipboard_history.wasm");
        let (runtime, reported) = watched_runtime();
        let slow = short_limits().compute * 3;
        assert!(slow > short_limits().unresponsive);

        runtime.inject(Fault::SlowHostCall(slow));
        let started = std::time::Instant::now();
        let view = block_on(runtime.render_with(&component, Some(packages.owned_by(&identity))));

        assert_eq!(view.expect("the view is shown").title, "Clipboard History");
        assert!(
            started.elapsed() >= slow,
            "the slow host call was not one of clipboard history's: {:?}",
            started.elapsed()
        );
        assert!(lock(&reported).is_empty(), "{:?}", lock(&reported));
        assert_eq!(runtime.status(), RuntimeStatus::Running);
        assert_eq!(runtime.abandoned_threads(), 0);
        assert_eq!(block_on(runtime.running()), vec![component]);
    }

    /// Waits until Pane gave up on the runtime thread, as it does once the
    /// thread made no progress for its limit.
    fn until_given_up(runtime: &Runtime) {
        until("Pane gave up on the thread", || {
            runtime.status() != RuntimeStatus::Running
        });
    }

    /// A runtime thread stuck outside any guest (made to hang) is given up
    /// on: the call it held answers that the runtime stopped responding, a
    /// fresh thread serves, no package is reported, and once the stuck
    /// thread returns it runs nothing more.
    #[test]
    fn a_runtime_thread_that_stops_responding_is_replaced_and_runs_nothing_more() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let component = settings_sample();
        let (runtime, reported) = watched_runtime();
        let slow = run_in_background(&runtime, &packages, &identity, "slow");
        until("it started waiting", || {
            saved(&packages, &identity, "slow-save").as_deref() == Some("started")
        });

        runtime.inject(Fault::Hang);

        let Err(CallError::RuntimeUnavailable(reason)) = slow.join().unwrap() else {
            panic!("expected the runtime unavailable");
        };
        assert!(
            reason.contains("stopped responding before answering and was started again"),
            "{reason}"
        );
        let RuntimeStatus::Restarted { failure, why } = runtime.status() else {
            panic!("expected a restart, got {:?}", runtime.status());
        };
        assert_eq!(failure, RuntimeFailure::Unresponsive);
        assert!(why.contains("made no progress for 2 seconds"), "{why}");
        assert!(why.contains("which code held it is not known"), "{why}");
        assert_eq!(runtime.abandoned_threads(), 1);
        assert!(lock(&reported).is_empty(), "no package is named");
        // A fresh thread serves.
        assert!(
            block_on(runtime.render_with(&component, Some(packages.owned_by(&identity)))).is_ok()
        );

        runtime.inject(Fault::Release);
        until("the stuck thread ended", || {
            runtime.abandoned_threads() == 0
        });

        // It ran nothing more: its instance, and the guest's wait, went
        // with it.
        assert_eq!(
            saved(&packages, &identity, "slow-save").as_deref(),
            Some("started")
        );
        assert_eq!(block_on(runtime.running()), vec![component]);
        assert!(lock(&reported).is_empty());
    }

    /// Asking which instances run, or how many views are open, answers
    /// once Pane gave up on a thread that held the request, rather than
    /// waiting for it for ever.
    #[test]
    fn running_and_view_count_answer_when_the_thread_is_given_up_on() {
        let (runtime, _) = watched_runtime();
        let other = guest("sample_rust.wasm");
        block_on(runtime.open_view(&other, "color")).unwrap();

        runtime.inject(Fault::Hang);
        let (running, views) = {
            let (a, b) = (runtime.clone(), runtime.clone());
            (
                std::thread::spawn(move || block_on(a.try_running())),
                std::thread::spawn(move || block_on(b.view_count())),
            )
        };

        assert!(matches!(
            running.join().unwrap(),
            Err(CallError::RuntimeUnavailable(_))
        ));
        assert_eq!(views.join().unwrap(), 0);
        until_given_up(&runtime);
        assert_eq!(block_on(runtime.try_running()), Ok(Vec::new()));
        runtime.inject(Fault::Release);
        until("the stuck thread ended", || {
            runtime.abandoned_threads() == 0
        });
    }

    /// A package reloaded while the runtime thread is stuck runs its new
    /// code on the fresh thread; the obsolete generation's instance, held by
    /// the stuck thread, is never restored, even once that thread returns.
    #[test]
    fn a_reload_during_a_hang_never_restores_the_obsolete_generation() {
        let data = tempfile::tempdir().unwrap();
        let (packages, identity) = settings_package(&data);
        let component = settings_sample();
        let (runtime, reported) = watched_runtime();
        let obsolete = packages.owned_by(&identity);
        let slow = run_in_background(&runtime, &packages, &identity, "slow");
        until("it started waiting", || {
            saved(&packages, &identity, "slow-save").as_deref() == Some("started")
        });

        runtime.inject(Fault::Hang);
        packages.replace_code(&identity);
        runtime.forget([component.clone()]);

        assert!(matches!(
            slow.join().unwrap(),
            Err(CallError::RuntimeUnavailable(_))
        ));
        until_given_up(&runtime);
        let current = packages.owned_by(&identity);
        assert!(block_on(runtime.render_with(&component, Some(current.clone()))).is_ok());
        runtime.inject(Fault::Release);
        until("the stuck thread ended", || {
            runtime.abandoned_threads() == 0
        });

        // A call of the obsolete generation starts nothing.
        assert_eq!(
            block_on(runtime.run_item_with(&component, "slow", Some(obsolete))),
            Err(CallError::Replaced)
        );
        assert_eq!(block_on(runtime.running()), vec![component.clone()]);
        assert_eq!(
            block_on(runtime.run_item_with(&component, "note", Some(current))),
            Ok(Answer {
                status: Some("Saved a note".into()),
                entries: None,
            })
        );
        assert_eq!(
            saved(&packages, &identity, "slow-save").as_deref(),
            Some("started")
        );
        assert!(lock(&reported).is_empty());
    }

    /// A call of an ended generation served after the package's next
    /// generation started an instance at the same component is refused, and
    /// leaves that instance and its open view alone.
    #[test]
    fn a_stale_call_leaves_the_current_generation_running() {
        let data = tempfile::tempdir().unwrap();
        let packages = ExtensionData::open(data.path());
        let identity = PackageIdentity::local(data.path()).unwrap();
        let old = packages.owned_by(&identity);
        packages.set_enabled(&identity, false);
        packages.set_enabled(&identity, true);
        let current = packages.owned_by(&identity);
        let component = guest("sample_rust.wasm");
        let runtime = Runtime::start().unwrap();
        let (view, _) =
            block_on(runtime.open_view_with(&component, "color", Some(current))).unwrap();

        let stale = runtime.render_with(&component, Some(old));

        assert_eq!(block_on(stale), Err(CallError::Disabled));
        assert_eq!(block_on(runtime.view_count()), 1);
        assert!(block_on(runtime.view_event(view, ViewEvent::Key(Key::Right))).is_ok());
    }
}
