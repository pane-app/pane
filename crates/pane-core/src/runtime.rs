//! The extension runtime: a Wasmtime engine that registers only WASI 0.3.
//!
//! The runtime owns every engine, store and guest instance on one dedicated
//! thread. Callers hold a cheap [`Runtime`] handle and await replies, so a slow
//! or failing guest never blocks the caller's thread.
//!
//! Calls are served one at a time. A call into an installed package belongs
//! to the package's generation (see `generation`): when it ends, a pending
//! call of it stops where the guest waits, and one queued behind is never
//! started. Component checks run on a checker thread of their own, so a
//! reload's check never waits behind the call it is about to stop.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{Component, Linker, ResourceAny, ResourceTable};
use wasmtime::{Cache, CacheConfig, Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::platform::Platform;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "extension-with-applications",
        imports: { "pane:extension/operations": store },
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

/// The `published-operations` export of a component serving operations.
mod operations_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "operations-provider",
        exports: { default: async | store },
    });
}

use bindings::exports::pane::extension::command;
use bindings::pane::extension::{applications, cache, content, credentials, settings};
use indexed_bindings::exports::pane::extension::indexed_results;
use root_bindings::exports::pane::extension::root_results;

use crate::applications::Applications;
use crate::extension_data::{DataKind, PackageData};
use crate::generation::{End, Generation};
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

/// A result a command computed from root search's query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootResult {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub action: RootAction,
}

/// What invoking a computed root result does; Pane performs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RootAction {
    /// Copy this text to the clipboard.
    Copy(String),
    /// Open this web address with the system's link handler.
    OpenUrl(String),
}

/// A root result a command supplies ahead of the query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexedResult {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
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
}

/// The system's applications as the runtime's guests and the launcher see
/// them; replaceable, for tests.
type SharedApplications = Arc<Mutex<Arc<dyn Applications>>>;

/// One entry in a command's list view, as produced by the guest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// When set, activating the item opens this form instead of running its
    /// action.
    pub form: Option<Form>,
    /// The operating systems the item's action works on; `None` for every
    /// system.
    pub platforms: Option<Vec<Platform>>,
    /// When set (and `form` is not), activating the item opens this custom
    /// view instead of running its action.
    pub custom_view: Option<CustomViewInfo>,
}

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

/// Identifies a custom view open in a [`Runtime`]. Ids are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewId(u64);

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

/// A command's list view, as produced by the guest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    pub items: Vec<Item>,
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
    /// The guest trapped or otherwise failed while running.
    Trap(String),
    /// The command's package is disabled, so none of its code runs. A call
    /// pending when it was disabled is stopped with this.
    Disabled,
    /// The command's code was replaced by a reload or an update while the
    /// call was pending, so the call was stopped and its answer discarded.
    Replaced,
    /// The command's package was uninstalled while the call was pending, so
    /// the call was stopped and its answer discarded.
    Uninstalled,
    /// The custom view was closed, or its guest instance has stopped, so it
    /// cannot handle events any more.
    ViewClosed,
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
            CallError::Trap(reason) => write!(f, "The extension crashed: {reason}"),
            CallError::ViewClosed => f.write_str("The extension's view is no longer open"),
        }
    }
}

impl std::error::Error for CallError {}

/// A handle to the runtime thread. Cloning shares the same runtime.
#[derive(Clone)]
pub struct Runtime {
    requests: mpsc::UnboundedSender<Request>,
    applications: SharedApplications,
    /// Component checks, served one at a time by the checker thread, apart
    /// from the runtime thread: a reload's check must not wait behind the
    /// guest call the reload is about to stop.
    checks: std::sync::mpsc::Sender<Check>,
}

/// A component check for the checker thread.
struct Check {
    component: PathBuf,
    exports: Exports,
    reply: oneshot::Sender<Result<(), CallError>>,
}

enum Request {
    GetView {
        component: PathBuf,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<View, CallError>>,
    },
    RunAction {
        component: PathBuf,
        item_id: String,
        data: Option<PackageData>,
        reply: oneshot::Sender<Result<String, CallError>>,
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
        reply: oneshot::Sender<usize>,
    },
    Running {
        reply: oneshot::Sender<Vec<PathBuf>>,
    },
    SetDirectory {
        directory: Directory,
    },
}

impl Runtime {
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
        let unavailable =
            |error: &dyn fmt::Display| CallError::RuntimeUnavailable(error.to_string());
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .wasm_component_model_async(true);
        if let Some(dir) = cache_dir {
            let mut cache = CacheConfig::new();
            cache.with_directory(dir);
            let cache = Cache::new(cache).map_err(|error| unavailable(&error))?;
            config.cache(Some(cache));
        }
        let engine = Engine::new(&config).map_err(|error| unavailable(&error))?;
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| unavailable(&error))?;
        let (requests, receiver) = mpsc::unbounded_channel();
        let applications: SharedApplications = Arc::new(Mutex::new(crate::applications::native()));
        let code = Arc::new(Code::new(engine));
        let host = Host::new(code.clone(), applications.clone());
        let (checks, pending_checks) = std::sync::mpsc::channel::<Check>();
        std::thread::Builder::new()
            .name("pane-extension-check".into())
            .spawn(move || {
                for check in pending_checks {
                    let _ = check
                        .reply
                        .send(code.check(&check.component, check.exports));
                }
            })
            .map_err(|error| unavailable(&error))?;
        std::thread::Builder::new()
            .name("pane-extension-runtime".into())
            .spawn(move || executor.block_on(host.serve(receiver)))
            .map_err(|error| unavailable(&error))?;
        Ok(Runtime {
            requests,
            applications,
            checks,
        })
    }

    /// Asks the command in `component` for its list view. The command has
    /// no extension data.
    pub async fn get_view(&self, component: &Path) -> Result<View, CallError> {
        self.get_view_with(component, None).await
    }

    /// Runs the action of `item_id` in the command in `component`. The
    /// command has no extension data.
    pub async fn run_action(&self, component: &Path, item_id: &str) -> Result<String, CallError> {
        self.run_action_with(component, item_id, None).await
    }

    /// Like [`Runtime::get_view`]; the command reads and saves `data`.
    /// An instance keeps the data it was started with.
    pub(crate) async fn get_view_with(
        &self,
        component: &Path,
        data: Option<PackageData>,
    ) -> Result<View, CallError> {
        let (reply, response) = oneshot::channel();
        self.send(Request::GetView {
            component: component.to_path_buf(),
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
    }

    /// Like [`Runtime::run_action`]; the command reads and saves `data`.
    pub(crate) async fn run_action_with(
        &self,
        component: &Path,
        item_id: &str,
        data: Option<PackageData>,
    ) -> Result<String, CallError> {
        let (reply, response) = oneshot::channel();
        self.send(Request::RunAction {
            component: component.to_path_buf(),
            item_id: item_id.to_owned(),
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
    }

    /// Checks, without running any guest code, that `component` is a
    /// component Pane can run: it compiles, imports only WASI 0.3 and exports
    /// the extension interface, with the function types of the current
    /// contract ([`CallError::OlderApiShape`] otherwise). The check keeps
    /// nothing loaded, and runs on Pane's checker thread, one check at a
    /// time: it does not wait for guest calls in progress, such as one a
    /// reload is about to stop.
    pub async fn check(&self, component: &Path) -> Result<(), CallError> {
        self.check_with(component, Exports::default()).await
    }

    /// Like [`Runtime::check`]; the component must also export each
    /// interface `exports` names, with the current function types.
    pub(crate) async fn check_with(
        &self,
        component: &Path,
        exports: Exports,
    ) -> Result<(), CallError> {
        let (reply, response) = oneshot::channel();
        self.checks
            .send(Check {
                component: component.to_path_buf(),
                exports,
                reply,
            })
            .map_err(|_| stopped())?;
        response.await.unwrap_or_else(|_| Err(stopped()))
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
        self.send(Request::IndexedResults {
            component: component.to_path_buf(),
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
    }

    /// Has the runtime's guests, and the launcher opening their results,
    /// find and open applications through `applications` from now on,
    /// instead of this system's own ([`crate::applications::native`]).
    pub fn set_applications(&self, applications: Arc<dyn Applications>) {
        *self
            .applications
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = applications;
    }

    /// Finds and opens the system's applications.
    pub(crate) fn applications(&self) -> Arc<dyn Applications> {
        self.applications
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
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
        self.send(Request::RootResults {
            component: component.to_path_buf(),
            query: query.to_owned(),
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
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
        self.send(Request::SubmitForm {
            component: component.to_path_buf(),
            item_id: item_id.to_owned(),
            values,
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
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
        self.send(Request::OpenView {
            component: component.to_path_buf(),
            item_id: item_id.to_owned(),
            data,
            reply,
        })?;
        response.await.unwrap_or_else(|_| Err(stopped()))
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
        let sent = self.send(Request::ViewEvent { view, event, reply });
        async move {
            sent?;
            response.await.unwrap_or_else(|_| Err(stopped()))
        }
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
        if self.send(Request::ViewCount { reply }).is_err() {
            return 0;
        }
        response.await.unwrap_or(0)
    }

    /// The components that have a live guest instance, counting the
    /// requests sent before this call, in no particular order. A diagnostic
    /// for tests and logs, like [`Runtime::view_count`]: listing or
    /// searching commands starts none; invoking a command starts its own.
    pub async fn running(&self) -> Vec<PathBuf> {
        let (reply, response) = oneshot::channel();
        if self.send(Request::Running { reply }).is_err() {
            return Vec::new();
        }
        response.await.unwrap_or_default()
    }

    /// Drops the compiled code and live instances of `components`, for
    /// example after their files were replaced or removed; a later call
    /// loads the file again. Calls made afterwards see the effect; a call
    /// already in progress finishes first, unless its generation ends (as
    /// the launcher does before forgetting a disabled or replaced package),
    /// which stops it. Nothing coordinates this with a command the user has
    /// open: its instance's state is lost.
    pub fn forget(&self, components: impl IntoIterator<Item = PathBuf>) {
        let components = components.into_iter().collect();
        // A stopped runtime holds nothing to forget.
        let _ = self.send(Request::Forget { components });
    }

    /// Resolves the operation calls guests make against `directory` from
    /// now on. Without one, every call is answered that nothing is
    /// installed.
    pub(crate) fn set_directory(&self, directory: Directory) {
        // A stopped runtime serves no calls.
        let _ = self.send(Request::SetDirectory { directory });
    }

    fn send(&self, request: Request) -> Result<(), CallError> {
        self.requests.send(request).map_err(|_| stopped())
    }
}

fn stopped() -> CallError {
    CallError::RuntimeUnavailable("the runtime has stopped".into())
}

/// How a call of a generation that ended for `end` answers.
fn ended(end: End) -> CallError {
    match end {
        End::Disabled => CallError::Disabled,
        End::Replaced => CallError::Replaced,
        End::Uninstalled => CallError::Uninstalled,
    }
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
}

impl GuestState {
    /// The generation the instance belongs to; `None` for a command built
    /// into Pane, which runs as long as Pane.
    pub(crate) fn generation(&self) -> Option<&Generation> {
        self.data.as_ref().map(PackageData::generation)
    }

    /// Why the instance's generation ended, if it has: its code may no
    /// longer run, save data or call operations.
    pub(crate) fn stopped(&self) -> Option<End> {
        self.data.as_ref().and_then(PackageData::stopped)
    }

    fn data(&self) -> Result<&PackageData, String> {
        self.data.as_ref().ok_or_else(|| {
            "only installed packages keep settings or data; this command is built into Pane".into()
        })
    }
}

/// Implements one kind of data's interface over the package's extension data.
macro_rules! data_host {
    ($interface:ident, $kind:expr) => {
        impl $interface::Host for GuestState {
            fn get(&mut self, key: String) -> Result<Option<String>, String> {
                self.data()?.get($kind, &key)
            }

            fn set(&mut self, key: String, value: String) -> Result<(), String> {
                self.data()?.set($kind, &key, &value)
            }
        }
    };
}

data_host!(settings, DataKind::Settings);
data_host!(content, DataKind::Content);
data_host!(cache, DataKind::Cache);
data_host!(credentials, DataKind::LocalCredentials);

impl GuestState {
    fn applications(&self) -> Arc<dyn Applications> {
        self.applications
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl applications::Host for GuestState {
    fn installed(&mut self) -> Result<Vec<applications::Application>, String> {
        // Code whose generation ended starts no more work.
        if self.stopped().is_some() {
            return Err(
                "this code of the extension was stopped (disabled, reloaded or updated)".into(),
            );
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
        // Code whose generation ended starts no more work.
        if self.stopped().is_some() {
            return Err(
                "this code of the extension was stopped (disabled, reloaded or updated)".into(),
            );
        }
        self.applications().open(&id)
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

/// A running guest instance of one component.
struct Instance {
    store: Store<GuestState>,
    bindings: bindings::ExtensionWithApplications,
    /// Its root results export, if it has one.
    root_results: Option<root_bindings::RootResultsProvider>,
    /// Its indexed results export, if it has one.
    indexed_results: Option<indexed_bindings::IndexedResultsProvider>,
    /// Its published operations export, if it has one.
    operations: Option<operations_bindings::OperationsProvider>,
}

/// A custom view open in a guest instance.
struct LiveView {
    /// The component whose instance holds the view.
    component: PathBuf,
    /// The guest's `custom-view` resource.
    resource: ResourceAny,
}

/// The engine and the host interfaces guests link against, shared by the
/// runtime thread and the checker thread.
struct Code {
    engine: Engine,
    linker: Linker<GuestState>,
}

/// Runtime-thread state: compiled components, their live instances and the
/// custom views open in them.
struct Host {
    code: Arc<Code>,
    components: HashMap<PathBuf, Component>,
    instances: HashMap<PathBuf, Instance>,
    views: HashMap<ViewId, LiveView>,
    next_view: u64,
    /// The installed packages operation calls are resolved against.
    directory: Option<Directory>,
    /// Handed to every guest, for its operation calls.
    calls: mpsc::UnboundedSender<OperationCall>,
    /// Operation calls guests made, served while their callers wait.
    calls_sent: mpsc::UnboundedReceiver<OperationCall>,
    /// Calls taken from `calls_sent` whose caller's frame has not served
    /// them yet (see [`Host::run_guest`]).
    waiting_calls: VecDeque<OperationCall>,
    /// The components running a guest call, outermost first: a chain of
    /// operation calls. Each is busy until its call returns.
    chain: Vec<PathBuf>,
    /// The generations of the calls in the chain that have one, outermost
    /// first: when any ends, the calls from it inward stop.
    owners: Vec<Generation>,
    /// Finds and opens the system's applications for guests.
    applications: SharedApplications,
}

impl Code {
    fn new(engine: Engine) -> Code {
        let mut linker = Linker::new(&engine);
        // Only WASI 0.3 is registered: no P2 linker and no stubs for unknown
        // imports, so a mixed P2/P3 component cannot instantiate.
        wasmtime_wasi::p3::add_to_linker(&mut linker)
            .expect("registering WASI 0.3 in a fresh linker cannot conflict");
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
        check::<(), (Result<command::View, String>,)>("get-view", func("get-view")?, &cx)?;
        check::<(String,), (Result<String, String>,)>("run-action", func("run-action")?, &cx)?;
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
    fn check(&self, path: &Path, exports: Exports) -> Result<(), CallError> {
        let component = self.compile(path)?;
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
        bindings::ExtensionWithApplicationsPre::new(pre).map_err(interface)?;
        Ok(())
    }
}

impl Host {
    fn new(code: Arc<Code>, applications: SharedApplications) -> Host {
        let (calls, calls_sent) = operations::channel();
        Host {
            code,
            components: HashMap::new(),
            instances: HashMap::new(),
            views: HashMap::new(),
            next_view: 0,
            directory: None,
            calls,
            calls_sent,
            waiting_calls: VecDeque::new(),
            chain: Vec::new(),
            owners: Vec::new(),
            applications,
        }
    }

    async fn serve(mut self, mut requests: mpsc::UnboundedReceiver<Request>) {
        while let Some(request) = requests.recv().await {
            self.drop_stopped();
            match request {
                Request::GetView {
                    component,
                    data,
                    reply,
                } => {
                    let result = self.get_view(&component, data).await;
                    let _ = reply.send(result);
                }
                Request::RunAction {
                    component,
                    item_id,
                    data,
                    reply,
                } => {
                    let result = self.run_action(&component, item_id, data).await;
                    let _ = reply.send(result);
                }
                Request::IndexedResults {
                    component,
                    data,
                    reply,
                } => {
                    let result = self.indexed_results(&component, data).await;
                    let _ = reply.send(result);
                }
                Request::RootResults {
                    component,
                    query,
                    data,
                    reply,
                } => {
                    let result = self.root_results(&component, query, data).await;
                    let _ = reply.send(result);
                }
                Request::Forget { components } => {
                    for component in &components {
                        self.components.remove(component);
                        self.drop_instance(component);
                    }
                }
                Request::SubmitForm {
                    component,
                    item_id,
                    values,
                    data,
                    reply,
                } => {
                    let result = self.submit_form(&component, item_id, values, data).await;
                    let _ = reply.send(result);
                }
                Request::OpenView {
                    component,
                    item_id,
                    data,
                    reply,
                } => {
                    let result = self.open_view(&component, item_id, data).await;
                    let _ = reply.send(result);
                }
                Request::ViewEvent { view, event, reply } => {
                    let result = self.view_event(view, event).await;
                    let _ = reply.send(result);
                }
                Request::CloseView { view } => self.close_view(view).await,
                Request::ViewCount { reply } => {
                    let _ = reply.send(self.views.len());
                }
                Request::Running { reply } => {
                    let _ = reply.send(self.instances.keys().cloned().collect());
                }
                Request::SetDirectory { directory } => self.directory = Some(directory),
            }
        }
    }

    async fn get_view(
        &mut self,
        path: &Path,
        data: Option<PackageData>,
    ) -> Result<View, CallError> {
        self.instance(path, data).await?;
        let result = self
            .run_guest(path, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| command.call_get_view(store).await)
                    .await
            })
            .await?;
        let view = self.settle(path, result, CallError::Guest)?;
        Ok(View {
            title: view.title,
            items: view.items.into_iter().map(Item::from).collect(),
        })
    }

    async fn submit_form(
        &mut self,
        path: &Path,
        item_id: String,
        values: Vec<FieldValue>,
        data: Option<PackageData>,
    ) -> Result<String, CallError> {
        self.instance(path, data).await?;
        let values = values
            .into_iter()
            .map(|FieldValue { id, value }| command::FieldValue { id, value })
            .collect();
        let result = self
            .run_guest(path, async |instance| {
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
        &mut self,
        path: &Path,
        item_id: String,
        data: Option<PackageData>,
    ) -> Result<(ViewId, Frame), CallError> {
        self.instance(path, data).await?;
        let result = self
            .run_guest(path, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| command.call_open_view(store, item_id).await)
                    .await
            })
            .await?;
        let resource = self.settle(path, result, CallError::Guest)?;
        let view = ViewId(self.next_view);
        self.next_view += 1;
        self.views.insert(
            view,
            LiveView {
                component: path.to_path_buf(),
                resource,
            },
        );
        match self.render(view).await {
            Ok(frame) => Ok((view, frame)),
            Err(error) => {
                self.close_view(view).await;
                Err(error)
            }
        }
    }

    async fn view_event(&mut self, view: ViewId, event: ViewEvent) -> Result<Frame, CallError> {
        let (path, resource) = self.view(view)?;
        let event = command::ViewEvent::from(event);
        let result = self
            .run_guest(&path, async |instance| {
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
        self.render(view).await
    }

    /// Asks the guest to draw the open view `view`.
    async fn render(&mut self, view: ViewId) -> Result<Frame, CallError> {
        let (path, resource) = self.view(view)?;
        let result = self
            .run_guest(&path, async |instance| {
                let custom_view = instance.bindings.pane_extension_command().custom_view();
                instance
                    .store
                    .run_concurrent(async |store| custom_view.call_render(store, resource).await)
                    .await
            })
            .await?;
        let frame = self.settle(&path, result.map(|frame| frame.map(Ok)), |never| never)?;
        let frame = Frame::from(frame);
        match frame.over_limits() {
            // The view stays open; its next drawing may be within them.
            Some(problem) => Err(CallError::Guest(problem)),
            None => Ok(frame),
        }
    }

    /// The component and guest resource of the open view `view`.
    fn view(&self, view: ViewId) -> Result<(PathBuf, ResourceAny), CallError> {
        let open = self.views.get(&view).ok_or(CallError::ViewClosed)?;
        Ok((open.component.clone(), open.resource))
    }

    /// Drops the guest's view `view`, running its destructor.
    async fn close_view(&mut self, view: ViewId) {
        let Some(open) = self.views.remove(&view) else {
            return;
        };
        if let Some(instance) = self.instances.get_mut(&open.component)
            && open
                .resource
                .resource_drop_async(&mut instance.store)
                .await
                .is_err()
        {
            // The destructor trapped: the instance cannot be re-entered.
            self.drop_instance(&open.component);
        }
    }

    /// Drops the live instance of `path` and forgets the views open in it,
    /// which went with it.
    fn drop_instance(&mut self, path: &Path) {
        self.instances.remove(path);
        self.views.retain(|_, view| view.component != path);
    }

    /// Drops the instances whose generation has ended, with everything
    /// their stores hold.
    fn drop_stopped(&mut self) {
        let stopped: Vec<PathBuf> = self
            .instances
            .iter()
            .filter(|(_, instance)| instance.store.data().stopped().is_some())
            .map(|(path, _)| path.clone())
            .collect();
        for path in stopped {
            self.drop_instance(&path);
        }
    }

    async fn root_results(
        &mut self,
        path: &Path,
        query: String,
        data: Option<PackageData>,
    ) -> Result<Vec<RootResult>, CallError> {
        let instance = self.instance(path, data).await?;
        let provider = instance
            .root_results
            .as_ref()
            .map(|provider| provider.pane_extension_root_results().clone())
            .ok_or_else(|| {
                CallError::Interface(format!("it does not export {ROOT_RESULTS_INTERFACE}"))
            })?;
        let result = self
            .run_guest(path, async |instance| {
                instance
                    .store
                    .run_concurrent(async |store| provider.call_results_for(store, query).await)
                    .await
            })
            .await?;
        let results = self.settle(path, result, CallError::Guest)?;
        Ok(results
            .into_iter()
            .map(|result| RootResult {
                id: result.id,
                title: result.title,
                subtitle: result.subtitle,
                action: match result.action {
                    root_results::RootAction::Copy(text) => RootAction::Copy(text),
                    root_results::RootAction::OpenUrl(url) => RootAction::OpenUrl(url),
                },
            })
            .collect())
    }

    async fn indexed_results(
        &mut self,
        path: &Path,
        data: Option<PackageData>,
    ) -> Result<Vec<IndexedResult>, CallError> {
        let instance = self.instance(path, data).await?;
        if instance.indexed_results.is_none() {
            return Err(CallError::Interface(format!(
                "it does not export {INDEXED_RESULTS_INTERFACE}"
            )));
        }
        let result = self
            .run_guest(path, async |instance| {
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
                id: result.id,
                title: result.title,
                subtitle: result.subtitle,
                action: match result.action {
                    indexed_results::IndexedAction::OpenApplication(id) => {
                        IndexedAction::OpenApplication(id)
                    }
                },
            })
            .collect())
    }

    async fn run_action(
        &mut self,
        path: &Path,
        item_id: String,
        data: Option<PackageData>,
    ) -> Result<String, CallError> {
        self.instance(path, data).await?;
        let result = self
            .run_guest(path, async |instance| {
                let command = instance.bindings.pane_extension_command();
                instance
                    .store
                    .run_concurrent(async |store| command.call_run_action(store, item_id).await)
                    .await
            })
            .await?;
        self.settle(path, result, CallError::Guest)
    }

    /// Runs `call` on the live instance of `path`, serving the operation
    /// calls its guest makes while it runs. Without a live instance (a view's
    /// instance has stopped) it is [`CallError::ViewClosed`].
    ///
    /// The instance is taken out of the host for the call, so the host can
    /// serve an operation call its guest makes, on this same thread, while
    /// the guest waits for the answer: the guest's call is not polled until
    /// the operation's answer is sent, and then resumes. This frame serves
    /// only its own guest's calls, one after another; a call another guest
    /// sent meanwhile waits for that guest's frame. The component is on the
    /// call chain meanwhile, so a call back into its package is refused
    /// rather than waiting on itself.
    ///
    /// The call stops as soon as the instance's generation, or that of any
    /// call further out in the chain, ends: the guest's call is dropped where
    /// it waits, and so is the instance, since Wasmtime keeps a dropped call's
    /// task in the store, where it would resume on the next call. A result
    /// that completes after its generation ended is discarded the same way.
    /// The generation is checked each time the guest yields; a guest that
    /// computes without yielding holds this thread until it does.
    async fn run_guest<R>(
        &mut self,
        path: &Path,
        call: impl AsyncFnOnce(&mut Instance) -> R,
    ) -> Result<R, CallError> {
        use std::task::Poll;

        /// What happened next while the guest's call ran.
        enum Next<R> {
            Returned(R),
            Stopped(End),
            Called(OperationCall),
        }

        let mut instance = self.instances.remove(path).ok_or(CallError::ViewClosed)?;
        let own = instance.store.data().generation().cloned();
        if let Some(generation) = &own {
            self.owners.push(generation.clone());
        }
        let mut ends: Vec<std::pin::Pin<Box<dyn Future<Output = End>>>> = self
            .owners
            .iter()
            .map(|owner| Box::pin(owner.wait_end()) as _)
            .collect();
        self.chain.push(path.to_path_buf());
        instance.store.data_mut().serving = true;
        let result = {
            let mut running = std::pin::pin!(call(&mut instance));
            loop {
                let next = std::future::poll_fn(|cx| {
                    for end in &mut ends {
                        if let Poll::Ready(end) = end.as_mut().poll(cx) {
                            return Poll::Ready(Next::Stopped(end));
                        }
                    }
                    if let Poll::Ready(result) = running.as_mut().poll(cx) {
                        return Poll::Ready(Next::Returned(result));
                    }
                    while let Poll::Ready(Some(call)) = self.calls_sent.poll_recv(cx) {
                        self.waiting_calls.push_back(call);
                    }
                    match self
                        .waiting_calls
                        .iter()
                        .position(|call| call.caller == path)
                    {
                        Some(index) => Poll::Ready(Next::Called(
                            self.waiting_calls.remove(index).expect("found above"),
                        )),
                        None => Poll::Pending,
                    }
                })
                .await;
                match next {
                    Next::Returned(result) => match own.as_ref().and_then(Generation::ended) {
                        Some(end) => break Err(end),
                        None => break Ok(result),
                    },
                    Next::Stopped(end) => break Err(end),
                    Next::Called(operation_call) => {
                        Box::pin(self.serve_operation(operation_call)).await;
                    }
                }
            }
        };
        instance.store.data_mut().serving = false;
        self.chain.pop();
        if own.is_some() {
            self.owners.pop();
        }
        // A call the guest sent but did not wait for before its call ended
        // has no frame to serve it.
        let (stranded, waiting) = std::mem::take(&mut self.waiting_calls)
            .into_iter()
            .partition(|call| call.caller == path);
        self.waiting_calls = waiting;
        for call in stranded {
            let _ = call.reply.send(Err(operations::outside_a_call()));
        }
        match result {
            Ok(result) => {
                self.instances.insert(path.to_path_buf(), instance);
                Ok(result)
            }
            Err(end) => {
                // The instance is dropped with its store: the abandoned
                // task, its host tasks, streams, futures and views.
                drop(instance);
                self.views.retain(|_, view| view.component != path);
                Err(ended(end))
            }
        }
    }

    /// Serves one operation call a guest made, answering it.
    async fn serve_operation(&mut self, call: OperationCall) {
        // Its caller gave up on it before it started: it is not started.
        if call.reply.is_closed() {
            return;
        }
        let result = self.operation(&call).await;
        let _ = call.reply.send(result);
    }

    /// Serves `call`: checks it, resolves its target, runs the operation
    /// (starting the target if it is not running) and checks the answer.
    async fn operation(&mut self, call: &OperationCall) -> Result<String, OperationError> {
        self.check_call(call)?;
        let target = self.resolve_target(call)?;
        // Disabled or replaced while it was serving the call, it was
        // stopped, and its answer is not passed on.
        let answer = self.run_operation(&target, call).await?;
        operations::check_json(&answer, &format!("result of {}", target.title))?;
        Ok(answer)
    }

    /// Refuses a call whose input is not JSON within the limit, or that would
    /// make the chain too deep.
    fn check_call(&self, call: &OperationCall) -> Result<(), OperationError> {
        operations::check_json(&call.input, "input")?;
        if self.chain.len() >= operations::MAX_CALL_DEPTH {
            return Err(OperationError::refused(format!(
                "the chain of calls is {} deep; Pane allows at most {}",
                self.chain.len(),
                operations::MAX_CALL_DEPTH
            )));
        }
        Ok(())
    }

    /// The installed package and component serving `call`, unless its
    /// package already serves a call in the chain, through whichever of its
    /// components.
    fn resolve_target(&self, call: &OperationCall) -> Result<Target, OperationError> {
        let installed = match &self.directory {
            Some(directory) => directory(),
            None => operations::Installed::default(),
        };
        let target = installed.resolve(&call.source, &call.operation, call.version)?;
        let in_chain = self.chain.iter().any(|component| {
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

    /// Runs the operation of `call` in `target`'s component, starting it if
    /// it is not running, and returns its answer.
    async fn run_operation(
        &mut self,
        target: &Target,
        call: &OperationCall,
    ) -> Result<String, OperationError> {
        let failed = |error| OperationError::from_call(&target.title, error);
        let instance = self
            .instance(&target.component, target.data.clone())
            .await
            .map_err(failed)?;
        // The install check requires the export, so only a component replaced
        // behind Pane's back lacks it.
        let provider = instance
            .operations
            .as_ref()
            .map(|provider| provider.pane_extension_published_operations().clone())
            .ok_or_else(|| {
                failed(CallError::Interface(format!(
                    "it does not export {OPERATIONS_INTERFACE}"
                )))
            })?;
        let (name, input) = (call.operation.clone(), call.input.clone());
        let result = self
            .run_guest(&target.component, async |instance| {
                instance
                    .store
                    .run_concurrent(async |store| {
                        provider.call_run_operation(store, name, input).await
                    })
                    .await
            })
            .await
            .map_err(failed)?;
        self.settle(&target.component, result, CallError::Guest)
            .map_err(failed)
    }

    /// Maps a call outcome to the caller's result, turning the guest's own
    /// error with `guest_error`. A trapped instance cannot be re-entered, so
    /// it is dropped and the next call starts a fresh one.
    fn settle<T, E>(
        &mut self,
        path: &Path,
        outcome: wasmtime::Result<wasmtime::Result<Result<T, E>>>,
        guest_error: impl FnOnce(E) -> CallError,
    ) -> Result<T, CallError> {
        match outcome.and_then(|inner| inner) {
            Ok(result) => result.map_err(guest_error),
            Err(trap) => {
                self.drop_instance(path);
                Err(CallError::Trap(format!("{trap:#}")))
            }
        }
    }

    /// Returns the live instance for `path` in the generation of `data`,
    /// instantiating it on first use. A call whose generation has ended (its
    /// package was disabled, reloaded or updated since it was asked for) gets
    /// none and is not started: it cannot bring its instance back.
    async fn instance(
        &mut self,
        path: &Path,
        data: Option<PackageData>,
    ) -> Result<&mut Instance, CallError> {
        // An instance of an ended generation goes; one of the package's
        // current generation stays, even for a stale call, which is refused.
        let stopped = self
            .instances
            .get(path)
            .is_some_and(|instance| instance.store.data().stopped().is_some());
        if stopped {
            self.drop_instance(path);
        }
        if let Some(end) = data.as_ref().and_then(PackageData::stopped) {
            return Err(ended(end));
        }
        if !self.instances.contains_key(path) {
            let component = self.component(path)?.clone();
            let mut store = Store::new(
                &self.code.engine,
                GuestState {
                    wasi: WasiCtx::builder().build(),
                    table: ResourceTable::new(),
                    data,
                    component: path.to_path_buf(),
                    calls: self.calls.clone(),
                    serving: false,
                    applications: self.applications.clone(),
                },
            );
            let load = |error: wasmtime::Error| CallError::Load(format!("{error:#}"));
            let instance = self
                .code
                .linker
                .instantiate_async(&mut store, &component)
                .await
                .map_err(load)?;
            let bindings =
                bindings::ExtensionWithApplications::new(&mut store, &instance).map_err(load)?;
            // Only a command that computes root results exports them.
            let root_results = root_bindings::RootResultsProvider::new(&mut store, &instance).ok();
            // Only a command that supplies results ahead of the query exports
            // them.
            let indexed_results =
                indexed_bindings::IndexedResultsProvider::new(&mut store, &instance).ok();
            // Only a component serving published operations exports them.
            let operations =
                operations_bindings::OperationsProvider::new(&mut store, &instance).ok();
            self.instances.insert(
                path.to_path_buf(),
                Instance {
                    store,
                    bindings,
                    root_results,
                    indexed_results,
                    operations,
                },
            );
        }
        Ok(self.instances.get_mut(path).expect("inserted above"))
    }

    /// Compiles `path` once (see [`Code::compile`]).
    fn component(&mut self, path: &Path) -> Result<&Component, CallError> {
        if !self.components.contains_key(path) {
            let component = self.code.compile(path)?;
            self.components.insert(path.to_path_buf(), component);
        }
        Ok(&self.components[path])
    }
}

impl From<command::Item> for Item {
    fn from(item: command::Item) -> Item {
        Item {
            id: item.id,
            title: item.title,
            subtitle: item.subtitle,
            form: item.form.map(Form::from),
            platforms: item
                .platforms
                .map(|platforms| platforms.into_iter().map(Platform::from).collect()),
            custom_view: item.custom_view.map(|info| CustomViewInfo {
                title: info.title,
                label: info.label,
                role: match info.role {
                    command::CustomViewRole::ColorWell => CustomViewRole::ColorWell,
                },
            }),
        }
    }
}

impl From<command::Platform> for Platform {
    fn from(platform: command::Platform) -> Platform {
        match platform {
            command::Platform::Windows => Platform::Windows,
            command::Platform::Macos => Platform::Macos,
            command::Platform::Linux => Platform::Linux,
        }
    }
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

impl From<command::Form> for Form {
    fn from(form: command::Form) -> Form {
        let fields = form
            .fields
            .into_iter()
            .map(|field| Field {
                id: field.id,
                label: field.label,
                kind: match field.kind {
                    command::FieldKind::Text(text) => FieldKind::Text {
                        placeholder: text.placeholder,
                    },
                    command::FieldKind::Choice(choices) => FieldKind::Choice(
                        choices
                            .into_iter()
                            .map(|choice| Choice {
                                id: choice.id,
                                label: choice.label,
                            })
                            .collect(),
                    ),
                },
            })
            .collect();
        Form {
            title: form.title,
            fields,
            submit_label: form.submit_label,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_data::ExtensionData;
    use crate::packages::PackageIdentity;
    use futures::executor::block_on;

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
        block_on(runtime.get_view_with(&component, Some(owned.clone()))).unwrap();

        settings.set_enabled(&identity, false);
        runtime.forget([component.clone()]);
        let queued = runtime.get_view_with(&component, Some(owned));

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
                block_on(runtime.run_action_with(&component, "hold", Some(owned)))
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

        let stale = runtime.get_view_with(&component, Some(old));

        assert_eq!(block_on(stale), Err(CallError::Disabled));
        assert_eq!(block_on(runtime.view_count()), 1);
        assert!(block_on(runtime.view_event(view, ViewEvent::Key(Key::Right))).is_ok());
    }
}
