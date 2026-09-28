//! The launcher model: the public host interface driven by the native window
//! and by tests alike.
//!
//! Every user action is a method on [`Launcher`]; [`Launcher::view`] returns a
//! snapshot of what the window should show. Actions that call into an
//! extension update the snapshot immediately (for example to "running") and
//! return a future that applies the extension's reply when awaited.
//!
//! Every reply is checked against the screen it was requested from: once the
//! user has left that screen, the reply is discarded, and a custom view that
//! opened after the user left is closed again.
//!
//! A call into an installed package also belongs to the package's
//! generation current when the user asked for it (see `generation`):
//! disabling, reloading or updating the package ends it, which stops the call
//! in the runtime, and its answer is never shown, even on a screen that is
//! still current. Leaving a screen only discards its replies; it does not
//! stop the call.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

mod hotkeys;
mod indexed;

use crate::extension_data::{ExtensionData, PackageData};
use crate::generation::End;
use crate::hotkeys::{self as system_hotkeys, Hotkeys};
use crate::links::{self, LinkOpener, NoOpener};
use crate::operations::{self, Installed};
use crate::packages::{
    InstalledPackage, PackageError, PackageIdentity, SavedData, SourcePackage, Store, folder_name,
};
use crate::platform::{self, Platform};
use crate::runtime::{
    CallError, CustomViewInfo, CustomViewRole, FieldKind, FieldValue, Form, Frame, Point,
    RootAction, RootResult as ComputedResult, Runtime, ViewEvent, ViewId,
};
use crate::search::{self, Keys, Query};

mod reload;
mod uninstall;

use hotkeys::Bindings;
use reload::StartFailures;

/// The id of the root row that installs a package from a local folder.
const INSTALL_FROM_FOLDER: &str = "pane.install-from-folder";

/// The id of the root row that lists installed packages to enable or
/// disable them.
const MANAGE_EXTENSIONS: &str = "pane.manage-extensions";

/// A command offered in root search, backed by one extension component.
#[derive(Clone, Debug)]
pub struct CommandRegistration {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub component: PathBuf,
}

/// Which screen the launcher shows, with what only that screen has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Root search: the root results matching `query`, the text typed into
    /// it, best match first, or every root result when it is empty.
    Root { query: String },
    /// An opened command's list view.
    Command,
    /// A package folder's identity and compatibility, before installing it,
    /// as lines of information under the title.
    Package { details: Vec<String> },
    /// A form opened from an item of the command's list view. It has no
    /// rows.
    Form(FormView),
    /// The installed packages, each enabled or disabled, with lines of
    /// information under the title.
    Extensions { details: Vec<String> },
    /// A custom view opened from an item of the command's list view. It has
    /// no rows.
    CustomView(CustomViewSnapshot),
    /// `question` about an installed package before Pane acts on it, with
    /// lines of information under the title, answered by choosing a row.
    Confirm {
        question: Question,
        details: Vec<String>,
    },
    /// Asks for the keys of a global hotkey that opens the installed command
    /// with id `command` from any application, with lines of information
    /// under the title. The window sends the keys pressed to
    /// [`Launcher::record_hotkey`]; the rows offer to remove its hotkey.
    Hotkey {
        command: String,
        details: Vec<String>,
    },
}

/// What a confirmation screen asks before Pane acts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// Whether to clear the cache of the installed package with this
    /// identity.
    ClearCache(PackageIdentity),
    /// Whether to uninstall the installed package with this identity, and
    /// whether to keep its saved data.
    Uninstall(PackageIdentity),
}

/// A selectable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// Why the row's action cannot be used on this system; `None` when it
    /// can. An unavailable row stays listed and selectable, and activating
    /// it shows this reason instead of calling the extension.
    pub unavailable: Option<String>,
}

/// Feedback about the most recent action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    /// An extension call or package operation is in progress.
    Running,
    /// The outcome of the most recent action, such as the extension's answer.
    Result(String),
    /// Why the most recent action failed. For a rejected form field this is
    /// "<field label>: <message>".
    Error(String),
}

/// An open form, as the user is filling it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormView {
    pub fields: Vec<FormField>,
    pub submit_label: String,
}

/// One field of an open form with its current value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
    /// The text of a text field, or the id of the chosen option.
    pub value: String,
    /// Why the extension rejected this field on the last submission.
    pub error: Option<String>,
}

/// An open custom view as the extension last drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomViewSnapshot {
    /// Which opened view this is: a view opened again, even of the same
    /// item, has another id.
    pub id: ViewId,
    /// Names the view to assistive technology.
    pub label: String,
    pub role: CustomViewRole,
    /// The latest drawing: the answer to the most recent event whose answer
    /// has arrived, or the first drawing.
    pub frame: Frame,
}

/// A snapshot of what the launcher shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherView {
    pub screen: Screen,
    pub title: String,
    /// Empty on the form and custom view screens.
    pub rows: Vec<Row>,
    /// Index into `rows`; `None` when there are no rows.
    pub selected: Option<usize>,
    pub status: Status,
}

impl LauncherView {
    /// `screen` titled `title`, with no rows and an idle status.
    fn new(screen: Screen, title: impl Into<String>) -> LauncherView {
        LauncherView {
            screen,
            title: title.into(),
            rows: Vec::new(),
            selected: None,
            status: Status::Idle,
        }
    }

    /// This view listing `rows`, with the first selected.
    fn with_rows(self, rows: Vec<Row>) -> LauncherView {
        LauncherView {
            selected: first_index(&rows),
            rows,
            ..self
        }
    }

    /// The text typed into root search; `None` on other screens.
    pub fn query(&self) -> Option<&str> {
        match &self.screen {
            Screen::Root { query } => Some(query),
            _ => None,
        }
    }

    /// Lines of information under the title, such as a package's source and
    /// compatibility; empty on screens without any.
    pub fn details(&self) -> &[String] {
        match &self.screen {
            Screen::Package { details }
            | Screen::Extensions { details }
            | Screen::Confirm { details, .. }
            | Screen::Hotkey { details, .. } => details,
            _ => &[],
        }
    }

    /// The open form, on the form screen.
    pub fn form(&self) -> Option<&FormView> {
        match &self.screen {
            Screen::Form(form) => Some(form),
            _ => None,
        }
    }

    /// The open custom view, on the custom view screen.
    pub fn custom_view(&self) -> Option<&CustomViewSnapshot> {
        match &self.screen {
            Screen::CustomView(view) => Some(view),
            _ => None,
        }
    }
}

/// The launcher. Cloning shares the same state.
#[derive(Clone)]
pub struct Launcher {
    runtime: Result<Runtime, CallError>,
    /// Commands supplied by this build rather than by installed packages.
    commands: Arc<[CommandRegistration]>,
    /// Where installed packages are kept, when installing packages is on.
    installation: Option<Installation>,
    /// Opens the web links of computed results.
    links: Arc<dyn LinkOpener>,
    /// Registers the global hotkeys the user assigns with the system.
    hotkeys: Arc<dyn Hotkeys>,
    state: Arc<Mutex<State>>,
}

/// Pane's managed package location and the installed packages' extension
/// data,
/// kept beside it.
#[derive(Clone)]
struct Installation {
    store: Arc<Mutex<Store>>,
    data: ExtensionData,
    /// Where they are kept, with Pane's other records such as the hotkeys.
    dir: PathBuf,
}

struct State {
    view: LauncherView,
    /// What activating each row of the current screen does.
    entries: Vec<Entry>,
    /// Every root result in root search order, built when root search is
    /// shown or refreshed, so that searching only ranks them.
    root: Vec<RootResult>,
    /// The root results commands computed from the current query, listed
    /// first; each command's results are added when it answers.
    computed: Vec<Computed>,
    /// The root results commands supplied ahead of the query, such as the
    /// installed applications, listed for a query that is not blank.
    indexes: indexed::Indexes,
    /// Incremented on every search, so that an answer arriving for an
    /// earlier search, even of the same query, is discarded.
    search_epoch: u64,
    /// The component of the command whose view is open.
    open: Option<PathBuf>,
    /// The form on screen, if one is open.
    form: Option<OpenForm>,
    /// The custom view on screen, if one is open.
    custom_view: Option<OpenCustomView>,
    /// Incremented on every navigation, so a reply that arrives after the
    /// user has left the screen it was requested from is discarded.
    screen_epoch: u64,
    /// The installed packages. Whether each is enabled here is the user's
    /// latest choice, which applies at once, even while it is still being
    /// recorded.
    packages: Vec<InstalledPackage>,
    /// Packages being enabled or disabled, reloaded or updated, with which;
    /// another change to one of them is refused meanwhile (see
    /// [`State::claim`]).
    changing: HashMap<PackageIdentity, Changing>,
    /// Why the installed packages could not be read, if they could not.
    store_problem: Option<String>,
    /// Packages whose reloaded code failed to start, each with why.
    failed: StartFailures,
    /// The global hotkeys the user assigned to commands.
    bindings: Bindings,
}

/// What is happening to a package, which stops another change to it
/// meanwhile.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Changing {
    /// Its enabling or disabling has taken effect and is being recorded.
    Recording,
    /// It is being reloaded, or started again after it failed to start.
    Reloading,
    /// Its managed copy is being replaced from a package folder.
    Updating,
    /// It is being uninstalled.
    Uninstalling,
}

impl State {
    /// The installed package with `identity`.
    fn package(&self, identity: &PackageIdentity) -> Option<&InstalledPackage> {
        self.packages
            .iter()
            .find(|package| package.identity == *identity)
    }

    /// The title of the installed package with `identity`, or its identity if
    /// it is no longer installed.
    fn title_of(&self, identity: &PackageIdentity) -> String {
        self.package(identity)
            .map(InstalledPackage::title)
            .unwrap_or_else(|| identity.to_string())
    }

    /// Notes that `what` begins on the package with `identity`, unless
    /// something else is happening to it: then it says what, and returns
    /// false. A second enabling or disabling while one is recorded is ignored
    /// without a word, as pressing Enter twice would do.
    fn claim(&mut self, identity: &PackageIdentity, what: Changing) -> bool {
        let busy = match self.changing.get(identity) {
            None => {
                self.changing.insert(identity.clone(), what);
                return true;
            }
            Some(Changing::Recording) => return false,
            Some(Changing::Reloading) => "is reloading",
            Some(Changing::Updating) => "is updating",
            Some(Changing::Uninstalling) => "is being uninstalled",
        };
        self.view.status = Status::Error(format!("{} {busy}", self.title_of(identity)));
        false
    }

    /// Notes that what began with [`State::claim`] on the package with
    /// `identity` has ended.
    fn release(&mut self, identity: &PackageIdentity) {
        self.changing.remove(identity);
    }
}

/// An enabling or disabling that has taken effect and is being recorded.
struct Change {
    identity: PackageIdentity,
    enabled: bool,
}

/// What the launcher keeps about the open form besides its view.
struct OpenForm {
    /// The item of the open command that the form belongs to.
    item_id: String,
    /// The command view that Back returns to.
    return_to: LauncherView,
    /// Whether a submission is waiting for the extension's reply; further
    /// submissions are ignored meanwhile.
    submitting: bool,
}

/// What the launcher keeps about the open custom view besides its snapshot.
struct OpenCustomView {
    /// The view in the runtime; closed when the view leaves the screen.
    id: ViewId,
    /// The command view that Back returns to.
    return_to: LauncherView,
    /// Whether the primary pointer button was pressed over the view and is
    /// still held; pointer moves and the release are sent only meanwhile.
    pressed: bool,
    /// How many events were sent to the view.
    sent: u64,
    /// The number of the event whose answer is on screen, so an older answer
    /// arriving late does not replace a newer one.
    shown: u64,
    /// Pointer moves sent to the view and not answered yet. While there are
    /// any, a further move waits in `waiting_move` instead of being sent.
    moves_in_flight: u32,
    /// The latest move of a drag that has not been sent: sent when the
    /// moves in flight are answered, or before the next other event.
    waiting_move: Option<Point>,
}

/// An event sent to the open view, whose answer is still to be shown.
struct SentEvent {
    /// The event's number among those sent to the view.
    number: u64,
    is_move: bool,
    reply: Pin<Box<dyn Future<Output = Result<Frame, CallError>> + Send>>,
}

impl OpenCustomView {
    fn send(&mut self, runtime: &Runtime, event: ViewEvent) -> SentEvent {
        self.sent += 1;
        let is_move = matches!(event, ViewEvent::PointerMove(_));
        if is_move {
            self.moves_in_flight += 1;
        }
        SentEvent {
            number: self.sent,
            is_move,
            reply: Box::pin(runtime.view_event(self.id, event)),
        }
    }
}

/// A result root search can list: its row, what activating it does, and
/// its text as the query is matched against it.
struct RootResult {
    row: Row,
    entry: Entry,
    keys: Keys,
}

/// A root result a command computed from the current query.
struct Computed {
    /// The component of the command that computed it.
    component: PathBuf,
    row: Row,
    entry: Entry,
}

/// What activating a row does.
#[derive(Clone)]
enum Entry {
    /// Copy this text to the clipboard, which the window does (root).
    Copy(String),
    /// Open this web address with the link opener (root).
    OpenUrl(String),
    /// Open the installed application `id`, named `name` (root).
    OpenApplication { id: String, name: String },
    /// Open the command with this component (root).
    Open(PathBuf),
    /// Explain why this installed package cannot load (root).
    Broken(String),
    /// Explain why this command (root) or this item's action (command view)
    /// is unavailable on this system; the extension is not called.
    Unavailable(String),
    /// Nothing in the launcher: the window asks for a folder (root).
    InstallFromFolder,
    /// Run the open command's item with this id.
    Run(String),
    /// Open this form of the open command's item with this id.
    Form(String, Form),
    /// Open the custom view of the open command's item with this id.
    CustomView(String, CustomViewInfo),
    /// Install the previewed package from this folder, or replace its
    /// installed copy.
    Install(PathBuf, Mode),
    /// Show the installed packages (root).
    Manage,
    /// Enable this installed package if it is disabled, else disable it.
    Toggle(PackageIdentity),
    /// Reload this installed package from its source folder.
    Reload(PackageIdentity),
    /// Start again this package, whose reloaded code failed to start.
    Retry(PackageIdentity),
    /// Ask whether to clear this installed package's cache (extension list).
    AskClearCache(PackageIdentity),
    /// Clear this installed package's cache (confirmation).
    ClearCache(PackageIdentity),
    /// Ask for the keys of the hotkey of the command with this id
    /// (extension list).
    AskHotkey(String),
    /// Remove the hotkey of the command with this id (hotkey screen).
    RemoveHotkey(String),
    /// Ask whether to uninstall this installed package, and whether to keep
    /// its saved data (extension list).
    AskUninstall(PackageIdentity),
    /// Uninstall this installed package, keeping or deleting its saved data
    /// (confirmation).
    Uninstall(PackageIdentity, SavedData),
    /// Return to the extension list without acting (confirmation).
    Cancel,
}

#[derive(Clone)]
enum Mode {
    Install,
    /// Replace the managed copy of the installed package with this identity.
    Update(PackageIdentity),
}

impl Launcher {
    /// Creates a launcher at root search. A runtime that failed to start
    /// leaves navigation usable and explains the failure when a command opens.
    pub fn new(runtime: Result<Runtime, CallError>, commands: Vec<CommandRegistration>) -> Self {
        Launcher::create(runtime, commands, None)
    }

    /// Creates a launcher that also offers the commands of the packages
    /// installed in `packages_dir` and installs packages there. Listing them
    /// reads only their manifests: no guest runs until a command opens.
    pub fn with_packages(
        runtime: Result<Runtime, CallError>,
        commands: Vec<CommandRegistration>,
        packages_dir: PathBuf,
    ) -> Self {
        let installation = Installation {
            data: ExtensionData::open(&packages_dir),
            dir: packages_dir.clone(),
            store: Arc::new(Mutex::new(Store::open(packages_dir))),
        };
        Launcher::create(runtime, commands, Some(installation))
    }

    fn create(
        runtime: Result<Runtime, CallError>,
        commands: Vec<CommandRegistration>,
        installation: Option<Installation>,
    ) -> Self {
        let (packages, store_problem, bindings) = match &installation {
            Some(installation) => {
                let store = installation.store.lock().unwrap_or_else(|p| p.into_inner());
                let bindings = Bindings::open(&installation.dir);
                (store.installed(), store.problem(), bindings)
            }
            None => (Vec::new(), None, Bindings::default()),
        };
        let state = State {
            // Replaced by root search below.
            view: LauncherView::new(Screen::Command, ""),
            entries: Vec::new(),
            root: Vec::new(),
            computed: Vec::new(),
            indexes: indexed::Indexes::default(),
            search_epoch: 0,
            open: None,
            form: None,
            custom_view: None,
            screen_epoch: 0,
            packages,
            changing: HashMap::new(),
            store_problem,
            failed: StartFailures::default(),
            bindings,
        };
        if let Some(installation) = &installation {
            for package in &state.packages {
                installation
                    .data
                    .set_enabled(&package.identity, package.enabled);
            }
        }
        let launcher = Launcher {
            runtime,
            commands: commands.into(),
            installation,
            links: Arc::new(NoOpener),
            hotkeys: system_hotkeys::none(),
            state: Arc::new(Mutex::new(state)),
        };
        if let (Ok(runtime), Some(installation)) = (&launcher.runtime, &launcher.installation) {
            // Operation calls see the packages as the launcher has them.
            let state = Arc::downgrade(&launcher.state);
            let data = installation.data.clone();
            runtime.set_directory(Arc::new(move || Installed {
                packages: state.upgrade().map_or_else(Vec::new, |state| {
                    let state = state.lock().unwrap_or_else(|p| p.into_inner());
                    state.packages.clone()
                }),
                data: Some(data.clone()),
            }));
        }
        launcher.show_root(&mut launcher.lock(), None);
        launcher
    }

    /// This launcher opening web links, such as quicklinks, with `links`,
    /// normally the system's handler. Without one, opening a link explains
    /// that this Pane has no link handler.
    pub fn with_link_opener(self, links: Arc<dyn LinkOpener>) -> Self {
        Launcher { links, ..self }
    }

    /// This launcher registering the global hotkeys the user assigns with
    /// `hotkeys`, normally the system's ([`crate::hotkeys::native`]); the
    /// recorded ones are registered now. Without it, assigning a hotkey
    /// explains that this Pane has none.
    pub fn with_hotkeys(self, hotkeys: Arc<dyn Hotkeys>) -> Self {
        let launcher = Launcher { hotkeys, ..self };
        launcher.sync_hotkeys(&mut launcher.lock());
        launcher
    }

    pub fn view(&self) -> LauncherView {
        self.lock().view.clone()
    }

    /// The installed packages, as read from their managed copies.
    pub fn packages(&self) -> Vec<InstalledPackage> {
        self.lock().packages.clone()
    }

    /// Shows `message` as the outcome of the most recent action; for
    /// failures outside the launcher, such as a folder picker that could not
    /// open.
    pub fn show_error(&self, message: impl Into<String>) {
        self.lock().view.status = Status::Error(message.into());
    }

    /// Moves the selection by `delta` rows, clamped to the list.
    pub fn move_selection(&self, delta: isize) {
        let mut state = self.lock();
        let view = &mut state.view;
        if let Some(selected) = view.selected {
            let last = view.rows.len() - 1;
            view.selected = Some(selected.saturating_add_signed(delta).min(last));
        }
    }

    /// Searches root search for `query`: the rows become the root results
    /// that match it, best match first, and the best match is selected. An
    /// empty query lists every root result. Ignored on other screens, and
    /// when `query` is already the query.
    ///
    /// Metadata is searched at once, without running any guest. For a query
    /// that is not blank, the enabled commands that compute root results
    /// (such as the calculator) are asked too, one after another: await the
    /// returned future to list each one's results, above the others, as soon
    /// as it answers. Their answers are discarded if the query has changed
    /// meanwhile, and a command that fails is listed as a result explaining
    /// the failure.
    ///
    /// The first query that is not blank since root search was shown also
    /// asks the enabled commands that supply results ahead of the query
    /// (such as the installed applications), after those; the future lists
    /// their results, matched like titles, when they answer, and they are
    /// kept for later queries. Until then the results kept from before are
    /// listed.
    pub fn set_query(&self, query: &str) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let (asked, indexing) = match &state.view.screen {
            Screen::Root { query: current } if current != query => {
                self.search(&mut state, query);
                let indexing = self.ask_for_indexed_results(&mut state, query);
                (self.ask_for_root_results(&state, query), indexing)
            }
            // Searching the same query again changes nothing, not even the
            // selection.
            _ => (Vec::new(), Vec::new()),
        };
        let query = query.to_owned();
        let epoch = state.screen_epoch;
        let search = state.search_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if !asked.is_empty() {
                launcher
                    .show_root_results(epoch, search, query, asked)
                    .await
            }
            launcher.show_indexed_results(indexing).await;
        }
    }

    /// The enabled commands that supply root results ahead of the query and
    /// are to be asked now, each with its settings; none for a blank query.
    fn ask_for_indexed_results(
        &self,
        state: &mut State,
        query: &str,
    ) -> Vec<(CommandRegistration, Option<PackageData>)> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let commands = state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .indexed_result_commands()
                    .into_iter()
                    .map(move |command| (command, data.clone()))
            })
            .collect();
        state.indexes.begin_asking(commands)
    }

    /// Asks each of `commands` in turn for its results ahead of the query
    /// and keeps them, listing them in root search if it is on screen,
    /// whatever the query is by then. A command disabled or replaced
    /// meanwhile contributes nothing.
    async fn show_indexed_results(
        &self,
        commands: Vec<(CommandRegistration, Option<PackageData>)>,
    ) {
        for (command, data) in commands {
            let answer = match self.runtime() {
                Ok(runtime) => {
                    runtime
                        .indexed_results_with(&command.component, data.clone())
                        .await
                }
                Err(error) => Err(error),
            };
            let mut state = self.lock();
            let state = &mut *state;
            if data.as_ref().and_then(PackageData::stopped).is_some() {
                continue;
            }
            state.indexes.answer(&command, answer);
            if let Some(query) = state.view.query().map(str::to_owned) {
                relist_root(state, &query);
            }
        }
    }

    /// Forgets the results supplied ahead of the query by commands that are
    /// no longer enabled, or were replaced.
    fn forget_indexes(state: &mut State) {
        let indexing: Vec<PathBuf> = state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| package.indexed_result_commands())
            .map(|command| command.component)
            .collect();
        state
            .indexes
            .retain(|component| indexing.iter().any(|kept| kept == component));
    }

    /// Opens the installed application `id`, named `name`, off the calling
    /// thread, and reports whether the system opened it.
    async fn open_application(&self, epoch: u64, id: String, name: String) {
        let opened = match self.runtime() {
            Ok(runtime) => {
                let applications = runtime.applications();
                off_thread(move || applications.open(&id)).await
            }
            Err(error) => Err(error.to_string()),
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        state.view.status = match opened {
            Ok(()) => Status::Result(format!("Opened {name}")),
            Err(problem) => Status::Error(format!("Could not open {name}: {problem}")),
        };
    }

    /// Shows the root results matching `query` from metadata alone; results
    /// computed for an earlier query are gone.
    fn search(&self, state: &mut State, query: &str) {
        state.search_epoch += 1;
        state.computed.clear();
        let (rows, entries) = root_rows(state, query);
        state.view.screen = Screen::Root {
            query: query.to_owned(),
        };
        state.view.selected = first_index(&rows);
        state.view.rows = rows;
        state.entries = entries;
    }

    /// The enabled commands that compute root results, each with its
    /// extension data, to be asked for their results for `query`; none for a blank
    /// query.
    fn ask_for_root_results(
        &self,
        state: &State,
        query: &str,
    ) -> Vec<(CommandRegistration, Option<PackageData>)> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .root_result_commands()
                    .into_iter()
                    .map(move |command| (command, data.clone()))
            })
            .collect()
    }

    /// Asks each of `commands` in turn for its root results for `query` and
    /// lists each one's as soon as it answers, unless the query, the search
    /// or the screen has changed meanwhile.
    ///
    /// The runtime serves calls one at a time, so a command that is slow or
    /// hangs still delays the commands asked after it (cancellation and
    /// timeouts are #29 and #18); it no longer hides the answers of those
    /// asked before it.
    async fn show_root_results(
        &self,
        epoch: u64,
        search: u64,
        query: String,
        commands: Vec<(CommandRegistration, Option<PackageData>)>,
    ) {
        for (command, data) in commands {
            let answer = match self.runtime() {
                Ok(runtime) => {
                    runtime
                        .root_results_with(&command.component, &query, data.clone())
                        .await
                }
                Err(error) => Err(error),
            };
            let Some(mut state) = self.lock_if_current(epoch) else {
                return;
            };
            if state.search_epoch != search || state.view.query() != Some(query.as_str()) {
                return;
            }
            let state = &mut *state;
            // A command disabled or replaced meanwhile contributes nothing.
            if data.as_ref().and_then(PackageData::stopped).is_some() {
                continue;
            }
            state
                .computed
                .extend(computed_results(command, &query, answer));
            relist_root(state, &query);
        }
    }

    /// Selects the row at `index`, if there is one.
    pub fn select(&self, index: usize) {
        let mut state = self.lock();
        if index < state.view.rows.len() {
            state.view.selected = Some(index);
        }
    }

    /// The text activating the selected row copies to the clipboard, if it
    /// copies. Activating it only reports the copy: the window writes the
    /// clipboard.
    pub fn selected_copy(&self) -> Option<String> {
        let state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index));
        match entry {
            Some(Entry::Copy(text)) => Some(text.clone()),
            _ => None,
        }
    }

    /// Whether the selected row installs a package from a folder the user
    /// chooses. Activating it does nothing in the launcher: the window asks
    /// for a folder and calls [`Launcher::preview_package`].
    pub fn selected_asks_for_folder(&self) -> bool {
        let state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index));
        matches!(entry, Some(Entry::InstallFromFolder))
    }

    /// Leaves an open form or custom view for its command's list, or an open
    /// command, package preview or the extension list for root search. A
    /// custom view is closed. On root search it clears the query.
    pub fn back(&self) {
        let mut state = self.lock();
        match &state.view.screen {
            Screen::Form(_) => {
                let form = state.form.take().expect("a form is open");
                state.screen_epoch += 1;
                state.view = LauncherView {
                    status: Status::Idle,
                    ..form.return_to
                };
            }
            Screen::CustomView(_) => self.return_from_custom_view(&mut state, Status::Idle),
            Screen::Confirm { .. } => self.leave_confirm(&mut state),
            Screen::Hotkey { command, .. } => {
                let command = command.clone();
                self.show_extensions_at_hotkey(&mut state, &command);
            }
            Screen::Command | Screen::Package { .. } | Screen::Extensions { .. } => {
                self.show_root(&mut state, None)
            }
            Screen::Root { query } => {
                if !query.is_empty() {
                    self.search(&mut state, "");
                }
            }
        }
    }

    /// Opens the selected command (root), opens the selected item's form or
    /// runs its action (command view), or installs or updates the previewed
    /// package. Await the returned future to apply the reply.
    ///
    /// A row that [asks for a folder](Launcher::selected_asks_for_folder)
    /// does nothing here. An [unavailable](Row::unavailable) item shows its
    /// reason as the status error without calling the extension.
    pub fn activate_selected(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index).cloned());
        let mut change = None;
        let mut reload = None;
        let mut hotkey_change = None;
        let mut uninstall = None;
        let entry = match entry {
            Some(Entry::Broken(problem) | Entry::Unavailable(problem)) => {
                state.view.status = Status::Error(problem);
                None
            }
            Some(Entry::Copy(text)) => {
                state.view.status = Status::Result(format!("Copied {text} to the clipboard"));
                None
            }
            Some(Entry::Form(item_id, form)) => {
                open_form(&mut state, item_id, form);
                None
            }
            Some(Entry::OpenUrl(url)) => match links::refusal(&url) {
                Some(reason) => {
                    state.view.status = Status::Error(format!("Could not open {url}: {reason}"));
                    None
                }
                None => {
                    state.view.status = Status::Running;
                    Some(Entry::OpenUrl(url))
                }
            },
            Some(Entry::Manage) => {
                self.show_extensions(&mut state);
                None
            }
            Some(Entry::AskClearCache(identity)) => {
                self.show_clear_cache(&mut state, &identity);
                None
            }
            Some(Entry::AskUninstall(identity)) => {
                self.show_uninstall(&mut state, &identity);
                None
            }
            Some(Entry::Uninstall(identity, saved)) => {
                uninstall = self.begin_uninstall(&mut state, identity, saved);
                None
            }
            Some(Entry::Cancel) => {
                self.leave_confirm(&mut state);
                None
            }
            Some(Entry::AskHotkey(command)) => {
                self.show_hotkey(&mut state, &command);
                None
            }
            Some(Entry::RemoveHotkey(command)) => {
                hotkey_change = self.remove_hotkey(&mut state, &command);
                None
            }
            Some(Entry::Toggle(identity)) => {
                // The package's state when the user pressed, not when the
                // future runs.
                let enable = state.package(&identity).is_some_and(|p| !p.enabled);
                change = self.begin_change(&mut state, identity, enable);
                None
            }
            Some(Entry::Reload(identity)) => {
                reload = self.begin_reload(&mut state, identity, reload::Attempt::Reload);
                None
            }
            Some(Entry::Retry(identity)) => {
                reload = self.begin_reload(&mut state, identity, reload::Attempt::Retry);
                None
            }
            Some(Entry::Install(_, Mode::Update(identity)))
                if !state.claim(&identity, Changing::Updating) =>
            {
                None
            }
            Some(Entry::InstallFromFolder) | None => None,
            Some(entry) => {
                state.view.status = Status::Running;
                Some(entry)
            }
        };
        let epoch = state.screen_epoch;
        let open = state.open.clone();
        // A call into the package belongs to its generation as of now, not
        // as of when the returned future first runs.
        let called = match &entry {
            Some(Entry::Open(component)) => Some(component),
            Some(Entry::Run(_) | Entry::CustomView(..)) => open.as_ref(),
            _ => None,
        };
        let data = called.and_then(|component| self.data_in(&state, component));
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_change(epoch, change).await;
            }
            if let Some(reload) = reload {
                launcher.finish_reload(epoch, reload).await;
            }
            if let Some(hotkey_change) = hotkey_change {
                launcher.finish_hotkey_change(hotkey_change).await;
            }
            if let Some(uninstall) = uninstall {
                launcher.finish_uninstall(epoch, uninstall).await;
            }
            match entry {
                Some(Entry::Open(component)) => launcher.open_command(epoch, component, data).await,
                Some(Entry::OpenApplication { id, name }) => {
                    launcher.open_application(epoch, id, name).await
                }
                Some(Entry::Run(item_id)) => {
                    if let Some(component) = open {
                        launcher.run_action(epoch, component, item_id, data).await
                    }
                }
                Some(Entry::Install(folder, mode)) => launcher.install(epoch, folder, mode).await,
                Some(Entry::OpenUrl(url)) => launcher.open_url(epoch, url).await,
                Some(Entry::ClearCache(identity)) => launcher.clear_cache(epoch, identity).await,
                Some(Entry::CustomView(item_id, info)) => {
                    if let Some(component) = open {
                        launcher
                            .open_custom_view(epoch, component, item_id, info, data)
                            .await
                    }
                }
                Some(
                    Entry::Copy(_)
                    | Entry::Broken(_)
                    | Entry::Unavailable(_)
                    | Entry::InstallFromFolder
                    | Entry::Manage
                    | Entry::Toggle(_)
                    | Entry::Reload(_)
                    | Entry::Retry(_)
                    | Entry::AskClearCache(_)
                    | Entry::AskHotkey(_)
                    | Entry::RemoveHotkey(_)
                    | Entry::AskUninstall(_)
                    | Entry::Uninstall(..)
                    | Entry::Cancel
                    | Entry::Form(..),
                )
                | None => {}
            }
        }
    }

    /// Reads the package in `folder` and shows its identity, version,
    /// commands and compatibility, offering Install, or Update when a
    /// package with the same identity is installed. No guest code runs. An
    /// invalid or incompatible package is explained instead.
    pub fn preview_package(&self, folder: &Path) -> impl Future<Output = ()> + Send + 'static {
        let epoch = self.start_running();
        let launcher = self.clone();
        let folder = folder.to_path_buf();
        async move {
            let checked = launcher.read_and_check(folder.clone()).await;
            let mut state = launcher.lock();
            if state.screen_epoch != epoch {
                return;
            }
            let installed = checked.as_ref().ok().and_then(|package| {
                state
                    .packages
                    .iter()
                    .find(|installed| installed.identity == package.identity)
                    .cloned()
            });
            let (view, entries) = preview_view(&folder, checked, installed);
            launcher.leave_command(&mut state);
            state.view = view;
            state.entries = entries;
        }
    }

    /// Installs the package in `folder` as an explicit install request. A
    /// package whose identity is already installed is rejected: replacing
    /// it is an update.
    pub fn install_package(&self, folder: &Path) -> impl Future<Output = ()> + Send + 'static {
        let epoch = self.start_running();
        let launcher = self.clone();
        let folder = folder.to_path_buf();
        async move { launcher.install(epoch, folder, Mode::Install).await }
    }

    /// Enables or disables the installed package with `identity` and
    /// records the choice, so it holds after a restart. The choice applies at
    /// once: a disabled package's commands leave root search, an open one
    /// closes, its running instances are dropped and it can no longer save
    /// settings, even before the choice is on disk. Its settings are kept for
    /// when it is enabled again. Other installations, even with the same
    /// title, are unaffected. Await the returned future to record the choice;
    /// if it cannot be recorded, the package returns to its previous state.
    ///
    /// While an earlier change to the same package is being recorded, this
    /// does nothing.
    pub fn set_enabled(
        &self,
        identity: &PackageIdentity,
        enabled: bool,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let change = self.begin_change(&mut state, identity.clone(), enabled);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_change(epoch, change).await;
            }
        }
    }

    /// Applies the user's choice to enable or disable a package, to be
    /// recorded by [`Launcher::finish_change`]. Explains why not and returns
    /// `None` if there is no such package or it is being reloaded or updated;
    /// returns `None` without a word while another change to it is being
    /// recorded.
    fn begin_change(
        &self,
        state: &mut State,
        identity: PackageIdentity,
        enabled: bool,
    ) -> Option<Change> {
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        if state.package(&identity).is_none() {
            state.view.status = Status::Error(PackageError::NotInstalled(identity).to_string());
            return None;
        }
        if !state.claim(&identity, Changing::Recording) {
            return None;
        }
        self.apply_enabled(state, &identity, enabled);
        state.view.status = Status::Running;
        Some(Change { identity, enabled })
    }

    /// Records a change begun by [`Launcher::begin_change`], undoing it if
    /// it cannot be recorded.
    async fn finish_change(&self, epoch: u64, change: Change) {
        let Change { identity, enabled } = change;
        let store = self
            .installation
            .as_ref()
            .expect("begin_change checked there is an installation")
            .store
            .clone();
        let recorded = {
            let identity = identity.clone();
            off_thread(move || {
                let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                store.set_enabled(&identity, enabled)
            })
            .await
        };
        let mut state = self.lock();
        state.release(&identity);
        let status = match recorded {
            Ok(()) => {
                let title = state.title_of(&identity);
                if enabled {
                    Status::Result(format!("Enabled {title}"))
                } else {
                    Status::Result(format!("Disabled {title}"))
                }
            }
            Err(error) => {
                self.apply_enabled(&mut state, &identity, !enabled);
                Status::Error(error.to_string())
            }
        };
        if state.screen_epoch == epoch {
            state.view.status = status;
        }
    }

    /// Enables or disables the package with `identity` in this launcher,
    /// without recording it: whether it offers commands and may save
    /// settings, its instances, and the screen showing them.
    fn apply_enabled(&self, state: &mut State, identity: &PackageIdentity, enabled: bool) {
        let Some(package) = state.packages.iter_mut().find(|p| p.identity == *identity) else {
            return;
        };
        package.enabled = enabled;
        if let Some(installation) = &self.installation {
            installation.data.set_enabled(identity, enabled);
        }
        let components: Vec<PathBuf> = package
            .commands()
            .into_iter()
            .map(|command| command.component)
            .collect();
        // Its hotkeys are released while it is disabled.
        self.sync_hotkeys(state);
        if !enabled {
            // Its results kept for root search go, and so does an answer
            // from it being awaited.
            Launcher::forget_indexes(state);
            // Its instances stop; enabling it again starts fresh ones, so a
            // failure to start no longer describes it.
            if let Ok(runtime) = self.runtime() {
                runtime.forget(components.iter().cloned());
            }
            state.failed.forget(identity);
            if state
                .open
                .as_ref()
                .is_some_and(|open| components.contains(open))
            {
                self.show_root(state, None);
                return;
            }
        }
        self.refresh(state);
    }

    fn start_running(&self) -> u64 {
        let mut state = self.lock();
        state.view.status = Status::Running;
        state.screen_epoch
    }

    async fn install(&self, epoch: u64, folder: PathBuf, mode: Mode) {
        let result = match self.installation.as_ref().map(|i| &i.store) {
            None => Err(PackageError::Storage(
                "this launcher does not install packages".into(),
            )),
            Some(store) => match self.read_and_check(folder).await {
                // Not installed again while its data is being removed.
                Ok(package) if self.is_uninstalling(&package.identity) => {
                    Err(PackageError::Storage(format!(
                        "{} is being uninstalled; install it again once that is done",
                        package.manifest.title
                    )))
                }
                Ok(package) => {
                    let store = store.clone();
                    let mode = mode.clone();
                    off_thread(move || {
                        let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                        match mode {
                            Mode::Install => store.install(&package),
                            Mode::Update(_) => store.update(&package),
                        }
                    })
                    .await
                }
                Err(error) => Err(error),
            },
        };
        let mut state = self.lock();
        if let Mode::Update(identity) = &mode {
            state.release(identity);
        }
        let current = state.screen_epoch == epoch;
        match result {
            Ok(installed) => {
                let message = match (mode, installed.version()) {
                    (Mode::Install, _) => format!("Installed {}", installed.title()),
                    (Mode::Update(_), Some(version)) => {
                        format!("Updated {} to {version}", installed.title())
                    }
                    (Mode::Update(_), None) => format!("Updated {}", installed.title()),
                };
                let first = installed.commands().first().map(|c| c.component.clone());
                let replaced_is_open = self.put_installed(&mut state, installed);
                if current || replaced_is_open {
                    self.show_root(&mut state, first);
                    state.view.status = Status::Result(message);
                } else {
                    self.refresh(&mut state);
                }
            }
            Err(error) if current => state.view.status = Status::Error(error.to_string()),
            Err(_) => {}
        }
    }

    /// Records `installed` as the managed copy of its package: added, or
    /// replacing the copy before it, whose instances stop. Returns whether a
    /// command of the replaced copy is open; the caller leaves it, since its
    /// state is not carried over to the new code.
    fn put_installed(&self, state: &mut State, installed: InstalledPackage) -> bool {
        state.failed.forget(&installed.identity);
        let Some(package) = state
            .packages
            .iter_mut()
            .find(|package| package.identity == installed.identity)
        else {
            // An identity installed again after it was uninstalled may save
            // data again.
            if let Some(installation) = &self.installation {
                installation
                    .data
                    .set_enabled(&installed.identity, installed.enabled);
            }
            state.packages.push(installed);
            self.sync_hotkeys(state);
            return false;
        };
        // The replaced copy's code is not run again: its generation ends,
        // which stops its pending calls, and the new code runs in a new one.
        if let Some(installation) = &self.installation {
            installation.data.replace_code(&installed.identity);
        }
        let replaced: Vec<PathBuf> = package
            .commands()
            .into_iter()
            .map(|c| c.component)
            .collect();
        if let Ok(runtime) = self.runtime() {
            runtime.forget(replaced.iter().cloned());
        }
        *package = installed;
        // A command the new copy no longer has releases its hotkey.
        self.sync_hotkeys(state);
        // The replaced copy's results are asked for afresh.
        state
            .indexes
            .retain(|component| !replaced.iter().any(|old| old == component));
        state
            .open
            .as_ref()
            .is_some_and(|open| replaced.contains(open))
    }

    /// Reads the package in `folder` off the calling thread, then has the
    /// runtime check each component without running it.
    async fn read_and_check(&self, folder: PathBuf) -> Result<SourcePackage, PackageError> {
        let package = off_thread(move || SourcePackage::read(&folder)).await?;
        let mut checked_components = Vec::new();
        for (name, component) in package.manifest.components() {
            // A component serving several commands or operations is checked
            // once, for everything it serves.
            if checked_components.contains(&component) {
                continue;
            }
            checked_components.push(component);
            let exports = package.manifest.exports_of(component);
            let source = package.folder.join(component);
            let checked = match self.runtime() {
                Ok(runtime) => runtime.check_with(&source, exports).await,
                Err(error) => Err(error),
            };
            checked.map_err(|error| PackageError::Component {
                command: name.clone(),
                error,
            })?;
        }
        Ok(package)
    }

    /// Shows root search with an empty query: this build's commands, then
    /// the installed packages' commands, then the install row. Selects the
    /// command with component `select` if given, else the first row.
    fn show_root(&self, state: &mut State, select: Option<PathBuf>) {
        state.root = self.root_results(state);
        state.computed.clear();
        state.indexes.stale();
        let (rows, entries) = root_rows(state, "");
        let selected = select
            .and_then(|component| {
                entries
                    .iter()
                    .position(|entry| matches!(entry, Entry::Open(c) if *c == component))
            })
            .or_else(|| first_index(&rows));
        self.leave_command(state);
        state.entries = entries;
        state.view = LauncherView {
            rows,
            selected,
            status: match &state.store_problem {
                Some(problem) => Status::Error(problem.clone()),
                None => Status::Idle,
            },
            ..LauncherView::new(
                Screen::Root {
                    query: String::new(),
                },
                "Pane",
            )
        };
    }

    /// Updates root search or the extension list on screen after a package
    /// changed; other screens show no package state.
    fn refresh(&self, state: &mut State) {
        match &state.view.screen {
            Screen::Root { .. } => self.refresh_root(state),
            Screen::Extensions { .. } => self.refresh_extensions(state),
            Screen::Command
            | Screen::Package { .. }
            | Screen::Form(_)
            | Screen::CustomView(_)
            | Screen::Confirm { .. }
            | Screen::Hotkey { .. } => {}
        }
    }

    /// Updates the rows of the root search on screen after the installed
    /// packages changed in the background. Unlike navigating, it keeps the
    /// screen epoch, so an action the user started from root still
    /// applies, and it keeps the query and the selection on the same row.
    fn refresh_root(&self, state: &mut State) {
        let selected_id = state
            .view
            .selected
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        state.root = self.root_results(state);
        // A command that was disabled or replaced contributes nothing more;
        // one enabled again answers from the next change of the query.
        let computing: Vec<PathBuf> = state
            .packages
            .iter()
            .filter(|package| package.enabled)
            .flat_map(|package| package.root_result_commands())
            .map(|command| command.component)
            .collect();
        state
            .computed
            .retain(|computed| computing.contains(&computed.component));
        Launcher::forget_indexes(state);
        let query = state.view.query().unwrap_or_default();
        let (rows, entries) = root_rows(state, query);
        let selected = selected_id
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .or_else(|| first_index(&rows));
        state.entries = entries;
        state.view.rows = rows;
        state.view.selected = selected;
    }

    /// Every root result, in root search order: this build's commands, the
    /// enabled packages' commands, the packages that cannot load, then
    /// Pane's own rows.
    fn root_results(&self, state: &State) -> Vec<RootResult> {
        let mut results = Vec::new();
        let mut add = |row: Row, entry: Entry, package: Option<&str>| {
            let keys = Keys::new(&row.title, row.subtitle.as_deref(), package);
            results.push(RootResult { row, entry, keys });
        };
        let command = |(command, unavailable): (CommandRegistration, Option<String>)| {
            let entry = match &unavailable {
                Some(reason) => Entry::Unavailable(reason.clone()),
                None => Entry::Open(command.component),
            };
            let row = Row {
                id: command.id,
                title: command.title,
                subtitle: command.subtitle,
                unavailable,
            };
            (row, entry)
        };
        // A disabled package contributes nothing to root search.
        let enabled = || state.packages.iter().filter(|package| package.enabled);
        for built in self.commands.iter().cloned() {
            let (row, entry) = command((built, None));
            add(row, entry, None);
        }
        for package in enabled() {
            // Its commands are found by its title too, even those that show
            // a subtitle of their own.
            let title = package.title();
            for available in package.available_commands() {
                let (row, entry) = command(available);
                add(row, entry, Some(&title));
            }
        }
        for package in enabled() {
            if let Err(error) = &package.manifest {
                let row = Row {
                    id: package.identity.key(),
                    title: package.title(),
                    subtitle: Some("Cannot load this installed extension".into()),
                    unavailable: None,
                };
                let problem = format!(
                    "{} cannot load from {}: {error}",
                    package.title(),
                    package.location.display()
                );
                add(row, Entry::Broken(problem), None);
            }
        }
        if self.installation.is_some() {
            let row = Row {
                id: INSTALL_FROM_FOLDER.into(),
                title: "Install extension from folder…".into(),
                subtitle: Some("Choose a local extension package to install".into()),
                unavailable: None,
            };
            add(row, Entry::InstallFromFolder, None);
        }
        if self.installation.is_some() && !state.packages.is_empty() {
            let row = Row {
                id: MANAGE_EXTENSIONS.into(),
                title: "Manage extensions…".into(),
                subtitle: Some("Enable or disable installed extensions".into()),
                unavailable: None,
            };
            add(row, Entry::Manage, None);
        }
        results
    }

    /// Sets the value of the open form's field `field_id`: a text field's
    /// text, or the id of an option of a choice field. Unknown fields and
    /// options are ignored. Editing a field clears its error.
    pub fn set_field_value(&self, field_id: &str, value: &str) {
        let mut state = self.lock();
        let Screen::Form(form) = &mut state.view.screen else {
            return;
        };
        let Some(field) = form.fields.iter_mut().find(|field| field.id == field_id) else {
            return;
        };
        if let FieldKind::Choice(choices) = &field.kind
            && !choices.iter().any(|choice| choice.id == value)
        {
            return;
        }
        field.value = value.to_owned();
        field.error = None;
    }

    /// Submits the open form to its extension. Await the returned future to
    /// apply the reply: the answer as the result, or the extension's
    /// rejection next to its field. While a submission is waiting for its
    /// reply, submitting again does nothing.
    pub fn submit_form(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let state = &mut *state;
        let submission = match (&state.view.screen, &mut state.form, &state.open) {
            (Screen::Form(form), Some(open), Some(component)) if !open.submitting => {
                open.submitting = true;
                let values: Vec<FieldValue> = form
                    .fields
                    .iter()
                    .map(|field| FieldValue {
                        id: field.id.clone(),
                        value: field.value.clone(),
                    })
                    .collect();
                Some((component.clone(), open.item_id.clone(), values))
            }
            _ => None,
        };
        if submission.is_some() {
            state.view.status = Status::Running;
        }
        let epoch = state.screen_epoch;
        let data = submission
            .as_ref()
            .and_then(|(component, ..)| self.data_in(state, component));
        let launcher = self.clone();
        async move {
            if let Some((component, item_id, values)) = submission {
                launcher
                    .submit(epoch, component, item_id, values, data)
                    .await
            }
        }
    }

    /// Sends `values` and applies the reply as though it had arrived before
    /// any edit made meanwhile: a rejected field that the user has changed
    /// since is not marked, since editing a field clears its error.
    async fn submit(
        &self,
        epoch: u64,
        component: PathBuf,
        item_id: String,
        values: Vec<FieldValue>,
        data: Option<PackageData>,
    ) {
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .submit_form_with(&component, &item_id, values.clone(), data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        let state = &mut *state;
        state.form.as_mut().expect("a form is open").submitting = false;
        if let Some(problem) = stopped(state, &component, &data) {
            // Stopped while it was submitting: its answer is not shown.
            state.view.status = Status::Error(problem);
            return;
        }
        let view = &mut state.view;
        let Screen::Form(form) = &mut view.screen else {
            unreachable!("a form is open");
        };
        let fields = &mut form.fields;
        for field in fields.iter_mut() {
            field.error = None;
        }
        view.status = match result {
            Ok(answer) => Status::Result(answer),
            Err(CallError::Form(error)) => {
                let field = error
                    .field
                    .as_deref()
                    .and_then(|id| fields.iter_mut().find(|field| field.id == id));
                match field {
                    Some(field) => {
                        let status = format!("{}: {}", field.label, error.message);
                        let unchanged = values
                            .iter()
                            .any(|sent| sent.id == field.id && sent.value == field.value);
                        if unchanged {
                            field.error = Some(error.message);
                        }
                        Status::Error(status)
                    }
                    // A rejection of the form as a whole is the extension's
                    // message to the user, shown as it is.
                    None => Status::Error(error.message),
                }
            }
            Err(error) => Status::Error(error.to_string()),
        };
    }

    /// Shows the installed packages, each enabled or disabled.
    fn show_extensions(&self, state: &mut State) {
        let (rows, entries) = self.extension_rows(state);
        self.leave_command(state);
        state.entries = entries;
        let details = vec![
            "A disabled extension adds no commands and runs nothing; it keeps its settings.".into(),
            "Reloading replaces an extension's code with its source folder's current build; it \
             keeps its settings."
                .into(),
            "Clearing an extension's cache keeps its settings, content and credentials.".into(),
            "Uninstalling an extension asks whether to keep its settings and content.".into(),
        ];
        state.view =
            LauncherView::new(Screen::Extensions { details }, "Extensions").with_rows(rows);
    }

    /// The extension list's rows: each package's state, reload and cache
    /// rows, then the hotkey of each command of the enabled packages.
    fn extension_rows(&self, state: &State) -> (Vec<Row>, Vec<Entry>) {
        let (mut rows, mut entries) = extension_rows(&state.packages, &state.failed);
        for (row, entry) in self.hotkey_rows(state) {
            rows.push(row);
            entries.push(entry);
        }
        (rows, entries)
    }

    /// Asks whether to clear the cache of the installed package with
    /// `identity`, saying what is deleted and what is kept.
    fn show_clear_cache(&self, state: &mut State, identity: &PackageIdentity) {
        let title = state.title_of(identity);
        let choice = |title: &str, subtitle: &str| Row {
            id: title.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            unavailable: None,
        };
        state.screen_epoch += 1;
        state.entries = vec![Entry::ClearCache(identity.clone()), Entry::Cancel];
        let details = vec![
            format!("From {identity}"),
            "Pane deletes the data this extension keeps as its cache. Its settings, content and \
             credentials are kept, and the extension does not run."
                .into(),
        ];
        let screen = Screen::Confirm {
            question: Question::ClearCache(identity.clone()),
            details,
        };
        state.view =
            LauncherView::new(screen, format!("Clear the cache of {title}?")).with_rows(vec![
                choice("Clear cache", "Delete the cached data now"),
                choice("Cancel", "Keep the cache"),
            ]);
    }

    /// Clears the cache of the installed package with `identity` without
    /// running it, then shows the extension list with the outcome. An
    /// instance of it that is running keeps what it holds in memory and may
    /// save it to its cache again, which the outcome then says.
    async fn clear_cache(&self, epoch: u64, identity: PackageIdentity) {
        let cleared = match &self.installation {
            Some(installation) => {
                let data = installation.data.clone();
                let identity = identity.clone();
                off_thread(move || data.clear_cache(&identity)).await
            }
            None => Err("this launcher does not install packages".into()),
        };
        let components: Vec<PathBuf> = {
            let state = self.lock();
            let package = state.package(&identity);
            package.map_or_else(Vec::new, |package| {
                package
                    .commands()
                    .into_iter()
                    .map(|c| c.component)
                    .collect()
            })
        };
        let running = match self.runtime() {
            Ok(runtime) => runtime.running().await,
            Err(_) => Vec::new(),
        };
        let still_running = running.iter().any(|path| components.contains(path));
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        let title = state.title_of(&identity);
        self.show_extensions_at_clear_cache(&mut state, &identity);
        state.view.status = match cleared {
            Ok(()) if still_running => Status::Result(format!(
                "Cleared the cache of {title}. A running instance may write it again until it \
                 stops."
            )),
            Ok(()) => Status::Result(format!("Cleared the cache of {title}")),
            Err(reason) => Status::Error(format!("Could not clear the cache of {title}: {reason}")),
        };
    }

    /// Returns from a confirmation to the extension list without acting.
    fn leave_confirm(&self, state: &mut State) {
        let Screen::Confirm { question, .. } = &state.view.screen else {
            return;
        };
        match question.clone() {
            Question::ClearCache(identity) => self.show_extensions_at_clear_cache(state, &identity),
            Question::Uninstall(identity) => self.show_extensions_at(
                state,
                |entry| matches!(entry, Entry::AskUninstall(asked) if *asked == identity),
            ),
        }
    }

    /// Shows the extension list with the row that clears the cache of the
    /// package with `identity` selected, where the user asked.
    fn show_extensions_at_clear_cache(&self, state: &mut State, identity: &PackageIdentity) {
        self.show_extensions_at(
            state,
            |entry| matches!(entry, Entry::AskClearCache(asked) if asked == identity),
        );
    }

    /// Shows the extension list with the first row whose entry is `wanted`
    /// selected, or the first row if there is none.
    fn show_extensions_at(&self, state: &mut State, wanted: impl Fn(&Entry) -> bool) {
        self.show_extensions(state);
        let row = state.entries.iter().position(wanted);
        if row.is_some() {
            state.view.selected = row;
        }
    }

    /// Updates the installed packages on screen after one changed, keeping
    /// the selection on the same row.
    fn refresh_extensions(&self, state: &mut State) {
        let (rows, entries) = self.extension_rows(state);
        // The same row stays selected; if it is gone (a Retry row once the
        // package started), the row before it.
        let selected = state.view.selected.and_then(|index| {
            let id = &state.view.rows.get(index)?.id;
            rows.iter()
                .position(|row| row.id == *id)
                .or_else(|| Some(index.saturating_sub(1).min(rows.len().checked_sub(1)?)))
        });
        state.entries = entries;
        state.view.selected = selected.or_else(|| first_index(&rows));
        state.view.rows = rows;
    }

    /// Opens the custom view of `item_id` and shows its first drawing, or
    /// closes it again if the user has left the command meanwhile.
    async fn open_custom_view(
        &self,
        epoch: u64,
        component: PathBuf,
        item_id: String,
        info: CustomViewInfo,
        data: Option<PackageData>,
    ) {
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .open_view_with(&component, &item_id, data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        let current = self.lock_if_current(epoch);
        let stopped = current
            .as_ref()
            .and_then(|state| stopped(state, &component, &data));
        let Some(mut state) = current.filter(|_| stopped.is_none()) else {
            if let (Ok((id, _)), Ok(runtime)) = (result, self.runtime()) {
                runtime.close_view(id);
            }
            if let Some(problem) = stopped {
                // Stopped while it was opening.
                self.lock().view.status = Status::Error(problem);
            }
            return;
        };
        match result {
            Ok((id, frame)) => {
                let snapshot = CustomViewSnapshot {
                    id,
                    label: info.label,
                    role: info.role,
                    frame,
                };
                let view = LauncherView::new(Screen::CustomView(snapshot), info.title);
                let return_to = LauncherView {
                    status: Status::Idle,
                    ..std::mem::replace(&mut state.view, view)
                };
                state.custom_view = Some(OpenCustomView {
                    id,
                    return_to,
                    pressed: false,
                    sent: 0,
                    shown: 0,
                    moves_in_flight: 0,
                    waiting_move: None,
                });
                state.screen_epoch += 1;
            }
            Err(error) => state.view.status = Status::Error(error.to_string()),
        }
    }

    /// Sends the user's input to the open custom view. Await the returned
    /// future to show the view's new drawing. A pointer move or release is
    /// sent only while the button pressed over the view is held; anything
    /// sent when no view is open is ignored.
    ///
    /// Events are handled in order, and a drawing is shown only if no later
    /// event's drawing is on screen yet. An error the extension reports is
    /// shown while the view stays open; a crash closes the view.
    ///
    /// A drag is coalesced: while a pointer move is being handled, a further
    /// move is not sent; only the latest one waiting is, once the moves in
    /// flight are answered (by the future of the move answered last), or
    /// before the next other event. Await every returned future.
    pub fn send_view_event(&self, event: ViewEvent) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let epoch = state.screen_epoch;
        // Sent now, so the view handles events in the order of these calls
        // whenever the returned futures are awaited.
        let mut sent: VecDeque<SentEvent> = self.send_to_view(&mut state, event).into();
        drop(state);
        let launcher = self.clone();
        async move {
            while let Some(event) = sent.pop_front() {
                let result = event.reply.await;
                launcher.show_view_answer(epoch, event.number, result);
                if event.is_move {
                    sent.extend(launcher.finish_move(epoch));
                }
            }
        }
    }

    /// Whether the primary pointer button was pressed over the open view and
    /// is still held, so the window should forward pointer moves and the
    /// release.
    pub fn pointer_held(&self) -> bool {
        self.lock()
            .custom_view
            .as_ref()
            .is_some_and(|open| open.pressed)
    }

    /// Sends `event` to the open view, after a waiting move, unless it is a
    /// move or release with no press held, or a move to wait (see
    /// [`Launcher::send_view_event`]).
    fn send_to_view(&self, state: &mut State, event: ViewEvent) -> Vec<SentEvent> {
        let (Some(open), Ok(runtime)) = (state.custom_view.as_mut(), self.runtime()) else {
            return Vec::new();
        };
        match event {
            ViewEvent::PointerDown(_) => open.pressed = true,
            ViewEvent::PointerMove(_) | ViewEvent::PointerUp(_) if !open.pressed => {
                return Vec::new();
            }
            ViewEvent::PointerUp(_) => open.pressed = false,
            ViewEvent::PointerMove(_) | ViewEvent::Key(_) => {}
        }
        if let ViewEvent::PointerMove(at) = event
            && open.moves_in_flight > 0
        {
            open.waiting_move = Some(at);
            return Vec::new();
        }
        let mut sent = Vec::new();
        if let Some(at) = open.waiting_move.take() {
            sent.push(open.send(runtime, ViewEvent::PointerMove(at)));
        }
        sent.push(open.send(runtime, event));
        sent
    }

    /// Notes that a move sent to the view of `epoch` was answered, and
    /// sends the waiting move once no other move is in flight.
    fn finish_move(&self, epoch: u64) -> Option<SentEvent> {
        let mut state = self.lock_if_current(epoch)?;
        let runtime = self.runtime().ok()?;
        let open = state.custom_view.as_mut()?;
        open.moves_in_flight = open.moves_in_flight.saturating_sub(1);
        if open.moves_in_flight > 0 {
            return None;
        }
        let at = open.waiting_move.take()?;
        Some(open.send(runtime, ViewEvent::PointerMove(at)))
    }

    /// Shows the open view's answer to its event number `number`.
    fn show_view_answer(&self, epoch: u64, number: u64, result: Result<Frame, CallError>) {
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        let state = &mut *state;
        let open = state.custom_view.as_mut().expect("a view is open");
        match result {
            // An answer to an event older than the one on screen is stale.
            Ok(_) | Err(CallError::Guest(_)) if number <= open.shown => {}
            Ok(frame) => {
                open.shown = number;
                let Screen::CustomView(snapshot) = &mut state.view.screen else {
                    unreachable!("a view is open");
                };
                snapshot.frame = frame;
                state.view.status = Status::Idle;
            }
            // The view refused the event and keeps its drawing.
            Err(error @ CallError::Guest(_)) => {
                open.shown = number;
                state.view.status = Status::Error(error.to_string());
            }
            // The guest instance, and the view with it, is gone.
            Err(error) => self.return_from_custom_view(state, Status::Error(error.to_string())),
        }
    }

    /// Closes the open custom view and shows the command view it was opened
    /// from, with `status`, as a new screen.
    fn return_from_custom_view(&self, state: &mut State, status: Status) {
        let return_to = self.close_custom_view(state).expect("a view is open");
        state.screen_epoch += 1;
        state.view = LauncherView {
            status,
            ..return_to
        };
    }

    /// Leaves the open command, and any form or custom view of it, for
    /// another screen, which the caller then shows: the view is closed in
    /// the runtime, and replies for the old screen are discarded.
    fn leave_command(&self, state: &mut State) {
        self.close_custom_view(state);
        state.open = None;
        state.form = None;
        state.screen_epoch += 1;
    }

    /// Closes the open custom view, if there is one, and returns the command
    /// view it was opened from.
    fn close_custom_view(&self, state: &mut State) -> Option<LauncherView> {
        let open = state.custom_view.take()?;
        if let Ok(runtime) = self.runtime() {
            runtime.close_view(open.id);
        }
        Some(open.return_to)
    }

    /// Opens `url` with the link opener, off the calling thread, and reports
    /// the outcome while the screen is the one it was opened from.
    async fn open_url(&self, epoch: u64, url: String) {
        let links = self.links.clone();
        let opened = {
            let url = url.clone();
            off_thread(move || links.open(&url)).await
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        state.view.status = match opened {
            Ok(()) => Status::Result(format!("Opened {url}")),
            Err(reason) => Status::Error(format!("Could not open {url}: {reason}")),
        };
    }

    async fn run_action(
        &self,
        epoch: u64,
        component: PathBuf,
        item_id: String,
        data: Option<PackageData>,
    ) {
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .run_action_with(&component, &item_id, data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        state.view.status = match (stopped(&state, &component, &data), result) {
            // Stopped while it was running: its answer is not shown.
            (Some(problem), _) => Status::Error(problem),
            (None, Ok(answer)) => Status::Result(answer),
            (None, Err(error)) => Status::Error(error.to_string()),
        };
    }

    async fn open_command(&self, epoch: u64, component: PathBuf, data: Option<PackageData>) {
        let result = match self.runtime() {
            Ok(runtime) => runtime.get_view_with(&component, data.clone()).await,
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        let end = data.as_ref().and_then(PackageData::stopped);
        if end == Some(End::Disabled) {
            // Disabled while it was opening.
            state.view.status = Status::Error(disabled(&state, &component));
            return;
        }
        if end == Some(End::Replaced) {
            // Reloaded or updated while it was opening: the call was stopped,
            // or its answer came from code that no longer runs. Its package's commands
            // are in root search again. Unless the reload or update has
            // reported its outcome meanwhile, this opening is still shown as
            // running, so it ends here.
            if state.view.status == Status::Running {
                state.view.status = Status::Error(
                    "The extension changed while its command was opening; open it again".into(),
                );
            }
            return;
        }
        if end == Some(End::Uninstalled) {
            // Uninstalled while it was opening: the uninstall reports its
            // own outcome.
            return;
        }
        let state = &mut *state;
        if let (Ok(_) | Err(CallError::Guest(_)), Some(package)) =
            (&result, owner(&state.packages, &component))
        {
            // It started, even if it refused to show its view.
            state.failed.forget(&package.identity);
        }
        match result {
            Ok(view) => {
                let (rows, entries): (Vec<Row>, Vec<Entry>) = view
                    .items
                    .into_iter()
                    .map(|item| {
                        let unavailable =
                            platform::unavailable(item.platforms.as_deref(), "this action");
                        let entry = match (&unavailable, item.form, item.custom_view) {
                            (Some(reason), _, _) => Entry::Unavailable(reason.clone()),
                            (None, Some(form), _) => Entry::Form(item.id.clone(), form),
                            (None, None, Some(info)) => Entry::CustomView(item.id.clone(), info),
                            (None, None, None) => Entry::Run(item.id.clone()),
                        };
                        let row = Row {
                            id: item.id,
                            title: item.title,
                            subtitle: item.subtitle,
                            unavailable,
                        };
                        (row, entry)
                    })
                    .unzip();
                state.entries = entries;
                state.open = Some(component);
                state.screen_epoch += 1;
                state.view = LauncherView::new(Screen::Command, view.title).with_rows(rows);
            }
            Err(error) => state.view.status = Status::Error(error.to_string()),
        }
    }

    /// The extension data of the installed package `component` belongs to,
    /// in its current generation; `None` for a command built into Pane.
    fn data_of(&self, component: &Path) -> Option<PackageData> {
        self.data_in(&self.lock(), component)
    }

    /// Like [`Launcher::data_of`], with the state locked.
    fn data_in(&self, state: &State, component: &Path) -> Option<PackageData> {
        let package = owner(&state.packages, component)?;
        Some(self.installation.as_ref()?.data.owned_by(&package.identity))
    }

    fn runtime(&self) -> Result<&Runtime, CallError> {
        self.runtime.as_ref().map_err(Clone::clone)
    }

    /// Locks the state only if the screen is still the one of `epoch`.
    fn lock_if_current(&self, epoch: u64) -> Option<MutexGuard<'_, State>> {
        let state = self.lock();
        (state.screen_epoch == epoch).then_some(state)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The installed package whose managed copy holds `component`.
fn owner<'a>(packages: &'a [InstalledPackage], component: &Path) -> Option<&'a InstalledPackage> {
    packages
        .iter()
        .find(|package| component.starts_with(&package.location))
}

/// Why the answer of a call into `component` made with `data` is not shown:
/// the generation it belonged to has ended, since its package was disabled
/// ("<title> is disabled"), its code replaced or it was uninstalled. `None`
/// while it lasts, and for a command built into Pane.
fn stopped(state: &State, component: &Path, data: &Option<PackageData>) -> Option<String> {
    match data.as_ref()?.stopped()? {
        End::Disabled => Some(disabled(state, component)),
        End::Replaced => Some(CallError::Replaced.to_string()),
        End::Uninstalled => Some(CallError::Uninstalled.to_string()),
    }
}

/// "<title> is disabled", for the package `component` belongs to.
fn disabled(state: &State, component: &Path) -> String {
    match owner(&state.packages, component) {
        Some(package) => format!("{} is disabled", package.title()),
        None => CallError::Disabled.to_string(),
    }
}

/// One row per installed package, saying whether it is enabled and which
/// source it is, so copies with the same title can be told apart; then the
/// rows that reload each enabled package, each followed by a Retry row if
/// its reloaded code failed to start; then one row per package to clear its
/// cache, and one to uninstall it, in the same order.
fn extension_rows(packages: &[InstalledPackage], failed: &StartFailures) -> (Vec<Row>, Vec<Entry>) {
    let failure = |package: &InstalledPackage| failed.of(&package.identity).map(str::to_owned);
    let toggles = packages.iter().map(|package| {
        let state = match (package.enabled, failure(package)) {
            (false, _) => "Disabled",
            (true, None) => "Enabled",
            (true, Some(_)) => "Enabled · Failed to start",
        };
        let row = Row {
            id: package.identity.key(),
            title: package.title(),
            subtitle: Some(format!("{state} · {}", package.identity)),
            unavailable: None,
        };
        (row, Entry::Toggle(package.identity.clone()))
    });
    let reloads = packages
        .iter()
        .filter(|package| package.enabled)
        .flat_map(|package| {
            let title = package.title();
            let source = match package.identity.local_folder() {
                Some(folder) => folder.display().to_string(),
                None => package.identity.to_string(),
            };
            let reload = Row {
                id: format!("reload:{}", package.identity.key()),
                title: format!("Reload {title}"),
                subtitle: Some(format!(
                    "Replace its code with the current build in {source}"
                )),
                unavailable: None,
            };
            let retry = failure(package).map(|problem| {
                let row = Row {
                    id: format!("retry:{}", package.identity.key()),
                    title: format!("Retry starting {title}"),
                    subtitle: Some(problem),
                    unavailable: None,
                };
                (row, Entry::Retry(package.identity.clone()))
            });
            std::iter::once((reload, Entry::Reload(package.identity.clone()))).chain(retry)
        });
    let clear_cache = packages.iter().map(|package| {
        let row = Row {
            id: format!("clear-cache:{}", package.identity.key()),
            title: format!("Clear cache of {}", package.title()),
            subtitle: Some(format!(
                "Keeps its settings, content and credentials · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskClearCache(package.identity.clone()))
    });
    let uninstall = packages.iter().map(|package| {
        let row = Row {
            id: format!("uninstall:{}", package.identity.key()),
            title: format!("Uninstall {}", package.title()),
            subtitle: Some(format!(
                "Remove it and choose whether to keep its saved data · {}",
                package.identity
            )),
            unavailable: None,
        };
        (row, Entry::AskUninstall(package.identity.clone()))
    });
    toggles
        .chain(reloads)
        .chain(clear_cache)
        .chain(uninstall)
        .unzip()
}

/// The package screen for `folder`: what the package is and whether it can
/// be installed, or why it cannot.
fn preview_view(
    folder: &Path,
    checked: Result<SourcePackage, PackageError>,
    installed: Option<InstalledPackage>,
) -> (LauncherView, Vec<Entry>) {
    let package = match checked {
        Ok(package) => package,
        Err(error) => {
            let details = vec![format!("Folder: {}", folder.display())];
            let view = LauncherView {
                status: Status::Error(error.to_string()),
                ..LauncherView::new(
                    Screen::Package { details },
                    format!("Cannot install {}", folder_name(folder)),
                )
            };
            return (view, Vec::new());
        }
    };
    let manifest = &package.manifest;
    let mut details = vec![format!("Source: {}", package.identity)];
    if let Some(version) = &manifest.version {
        details.push(format!("Version: {version}"));
    }
    let titles: Vec<&str> = manifest.commands.iter().map(|c| c.title.as_str()).collect();
    if !titles.is_empty() {
        details.push(format!("Commands: {}", titles.join(", ")));
    }
    if let Some(operations) = operations::describe(&manifest.operations) {
        details.push(operations);
    }
    details.push(format!(
        "Compatible: needs extension API {}, and its components import only WASI 0.3",
        manifest.api_version
    ));
    if let Some(platforms) = &manifest.platforms {
        // A package that does not support this system is explained instead.
        let names: Vec<String> = platforms
            .iter()
            .map(|&platform| {
                if Some(platform) == Platform::current() {
                    format!("{platform} (this system)")
                } else {
                    platform.to_string()
                }
            })
            .collect();
        details.push(format!("Supported systems: {}", platform::join(&names)));
    }
    let (row, entry) = match installed {
        Some(installed) => {
            details.push(match installed.version() {
                Some(version) => format!("Installed: version {version} from this folder"),
                None => "Installed from this folder".into(),
            });
            if !installed.enabled {
                details.push("Disabled: enable it in Manage extensions".into());
            }
            let row = Row {
                id: "update".into(),
                title: "Update".into(),
                subtitle: Some("Replace the installed copy with this folder's contents".into()),
                unavailable: None,
            };
            let mode = Mode::Update(installed.identity.clone());
            (row, Entry::Install(package.folder.clone(), mode))
        }
        None => {
            let row = Row {
                id: "install".into(),
                title: "Install".into(),
                subtitle: Some("Copy the package into Pane and add its commands".into()),
                unavailable: None,
            };
            (row, Entry::Install(package.folder.clone(), Mode::Install))
        }
    };
    let view =
        LauncherView::new(Screen::Package { details }, manifest.title.clone()).with_rows(vec![row]);
    (view, vec![entry])
}

/// Replaces the command view with `form`, which belongs to item `item_id`.
/// The command's row entries stay, for when the form closes.
fn open_form(state: &mut State, item_id: String, form: Form) {
    let fields = form
        .fields
        .into_iter()
        .map(|field| {
            let value = match &field.kind {
                FieldKind::Text { .. } => String::new(),
                FieldKind::Choice(choices) => choices
                    .first()
                    .map(|choice| choice.id.clone())
                    .unwrap_or_default(),
            };
            FormField {
                id: field.id,
                label: field.label,
                kind: field.kind,
                value,
                error: None,
            }
        })
        .collect();
    let form_view = LauncherView::new(
        Screen::Form(FormView {
            fields,
            submit_label: form.submit_label,
        }),
        form.title,
    );
    let return_to = std::mem::replace(&mut state.view, form_view);
    state.form = Some(OpenForm {
        item_id,
        return_to,
        submitting: false,
    });
    state.screen_epoch += 1;
}

/// The rows of root search for `query`, and what activating each does: the
/// results computed from it, then the root results matching it, best match
/// first, with, for a query that is not blank, those supplied ahead of it
/// (after the others of the same rank), then the rows explaining why a
/// command could not supply them.
fn root_rows(state: &State, query: &str) -> (Vec<Row>, Vec<Entry>) {
    let blank = query.trim().is_empty();
    let candidates: Vec<&RootResult> = state
        .root
        .iter()
        .chain(state.indexes.results().filter(|_| !blank))
        .collect();
    let keys: Vec<&Keys> = candidates.iter().map(|result| &result.keys).collect();
    let matches = search::ranked_matches(&Query::new(query), keys.into_iter())
        .into_iter()
        .map(|index| (&candidates[index].row, &candidates[index].entry));
    let failures = state
        .indexes
        .failures()
        .filter(|_| !blank)
        .map(|(row, entry)| (row, entry));
    state
        .computed
        .iter()
        .map(|computed| (&computed.row, &computed.entry))
        .chain(matches)
        .chain(failures)
        .map(|(row, entry)| (row.clone(), entry.clone()))
        .unzip()
}

/// Lists root search's rows for `query` again after results arrived: the
/// best match stays selected, now that better ones may be first, and a row
/// the user moved to stays selected.
fn relist_root(state: &mut State, query: &str) {
    let keep = state
        .view
        .selected
        .filter(|&index| index > 0)
        .and_then(|index| state.view.rows.get(index))
        .map(|row| row.id.clone());
    let (rows, entries) = root_rows(state, query);
    state.view.selected = keep
        .and_then(|id| rows.iter().position(|row| row.id == id))
        .or_else(|| first_index(&rows));
    state.view.rows = rows;
    state.entries = entries;
}

/// The rows for `command`'s `answer` to `query`: its results, or one
/// explaining why it failed.
fn computed_results(
    command: CommandRegistration,
    query: &str,
    answer: Result<Vec<ComputedResult>, CallError>,
) -> Vec<Computed> {
    let computed = |row: Row, entry: Entry| Computed {
        component: command.component.clone(),
        row,
        entry,
    };
    match answer {
        Ok(results) => results
            .into_iter()
            .map(|result| {
                let row = Row {
                    id: format!("{}:{}", command.id, result.id),
                    title: result.title,
                    subtitle: result.subtitle,
                    unavailable: None,
                };
                let entry = match result.action {
                    RootAction::Copy(text) => Entry::Copy(text),
                    RootAction::OpenUrl(url) => Entry::OpenUrl(url),
                };
                computed(row, entry)
            })
            .collect(),
        Err(error) => {
            let row = Row {
                id: format!("{}:failed", command.id),
                title: command.title.clone(),
                subtitle: Some(format!("Could not answer: {error}")),
                unavailable: None,
            };
            let problem = format!("{} could not answer “{query}”: {error}", command.title);
            vec![computed(row, Entry::Broken(problem))]
        }
    }
}

/// Runs blocking file work on its own thread, so the caller's thread (the
/// window's) never waits on the file system.
async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (reply, response) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = reply.send(work());
    });
    response.await.expect("package file work panicked")
}

fn first_index(rows: &[Row]) -> Option<usize> {
    (!rows.is_empty()).then_some(0)
}
