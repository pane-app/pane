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

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::packages::{
    InstalledPackage, PackageError, PackageIdentity, SourcePackage, Store, folder_name,
};
use crate::platform::{self, Platform};
use crate::runtime::{
    CallError, CustomViewInfo, CustomViewRole, FieldKind, FieldValue, Form, Frame, Point, Runtime,
    ViewEvent, ViewId,
};
use crate::settings::{PackageSettings, Settings};

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

/// Which screen the launcher shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Root search: the installed commands.
    Root,
    /// An opened command's list view.
    Command,
    /// A package folder's identity and compatibility, before installing it.
    Package,
    /// A form opened from an item of the command's list view.
    Form,
    /// The installed packages, each enabled or disabled.
    Extensions,
    /// A custom view opened from an item of the command's list view.
    CustomView,
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
    /// Lines of information under the title, such as a package's source and
    /// compatibility.
    pub details: Vec<String>,
    /// Empty on the form screen.
    pub rows: Vec<Row>,
    /// Index into `rows`; `None` when there are no rows.
    pub selected: Option<usize>,
    pub status: Status,
    /// The open form; `Some` exactly on the form screen.
    pub form: Option<FormView>,
    /// The open custom view; `Some` exactly on the custom view screen.
    pub custom_view: Option<CustomViewSnapshot>,
}

/// The launcher. Cloning shares the same state.
#[derive(Clone)]
pub struct Launcher {
    runtime: Result<Runtime, CallError>,
    /// Commands supplied by this build rather than by installed packages.
    commands: Arc<[CommandRegistration]>,
    /// Where installed packages are kept, when installing packages is on.
    installation: Option<Installation>,
    state: Arc<Mutex<State>>,
}

/// Pane's managed package location and the installed packages' settings,
/// kept beside it.
#[derive(Clone)]
struct Installation {
    store: Arc<Mutex<Store>>,
    settings: Settings,
}

struct State {
    view: LauncherView,
    /// What activating each row of the current screen does.
    entries: Vec<Entry>,
    /// The component of the command whose view is open.
    open: Option<PathBuf>,
    /// The form on screen, if one is open.
    form: Option<OpenForm>,
    /// The custom view on screen, if one is open.
    custom_view: Option<OpenCustomView>,
    /// Incremented on every navigation, so a reply that arrives after the
    /// user has left the screen it was requested from is discarded.
    screen_generation: u64,
    /// The installed packages. Whether each is enabled here is the user's
    /// latest choice, which applies at once, even while it is still being
    /// recorded.
    packages: Vec<InstalledPackage>,
    /// Packages whose enabling or disabling is still being recorded; another
    /// change to one of them is ignored meanwhile.
    changing: Vec<PackageIdentity>,
    /// Why the installed packages could not be read, if they could not.
    store_problem: Option<String>,
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

/// What activating a row does.
#[derive(Clone)]
enum Entry {
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
}

#[derive(Clone, Copy)]
enum Mode {
    Install,
    Update,
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
            settings: Settings::open(&packages_dir),
            store: Arc::new(Mutex::new(Store::open(packages_dir))),
        };
        Launcher::create(runtime, commands, Some(installation))
    }

    fn create(
        runtime: Result<Runtime, CallError>,
        commands: Vec<CommandRegistration>,
        installation: Option<Installation>,
    ) -> Self {
        let (packages, store_problem) = match &installation {
            Some(installation) => {
                let store = installation.store.lock().unwrap_or_else(|p| p.into_inner());
                (store.installed(), store.problem())
            }
            None => (Vec::new(), None),
        };
        let state = State {
            view: LauncherView {
                screen: Screen::Root,
                title: String::new(),
                details: Vec::new(),
                rows: Vec::new(),
                selected: None,
                status: Status::Idle,
                form: None,
                custom_view: None,
            },
            entries: Vec::new(),
            open: None,
            form: None,
            custom_view: None,
            screen_generation: 0,
            packages,
            changing: Vec::new(),
            store_problem,
        };
        if let Some(installation) = &installation {
            for package in &state.packages {
                installation
                    .settings
                    .set_enabled(&package.identity, package.enabled);
            }
        }
        let launcher = Launcher {
            runtime,
            commands: commands.into(),
            installation,
            state: Arc::new(Mutex::new(state)),
        };
        launcher.show_root(&mut launcher.lock(), None);
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

    /// Selects the row at `index`, if there is one.
    pub fn select(&self, index: usize) {
        let mut state = self.lock();
        if index < state.view.rows.len() {
            state.view.selected = Some(index);
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
    /// custom view is closed.
    pub fn back(&self) {
        let mut state = self.lock();
        match state.view.screen {
            Screen::Form => {
                let form = state.form.take().expect("a form is open");
                state.screen_generation += 1;
                state.view = LauncherView {
                    status: Status::Idle,
                    ..form.return_to
                };
            }
            Screen::CustomView => self.return_from_custom_view(&mut state, Status::Idle),
            Screen::Command | Screen::Package | Screen::Extensions => {
                self.show_root(&mut state, None)
            }
            Screen::Root => {}
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
        let entry = match entry {
            Some(Entry::Broken(problem) | Entry::Unavailable(problem)) => {
                state.view.status = Status::Error(problem);
                None
            }
            Some(Entry::Form(item_id, form)) => {
                open_form(&mut state, item_id, form);
                None
            }
            Some(Entry::Manage) => {
                self.show_extensions(&mut state);
                None
            }
            Some(Entry::Toggle(identity)) => {
                // The package's state when the user pressed, not when the
                // future runs.
                let enable = state
                    .packages
                    .iter()
                    .find(|package| package.identity == identity)
                    .is_some_and(|package| !package.enabled);
                change = self.begin_change(&mut state, identity, enable);
                None
            }
            Some(Entry::InstallFromFolder) | None => None,
            Some(entry) => {
                state.view.status = Status::Running;
                Some(entry)
            }
        };
        let generation = state.screen_generation;
        let open = state.open.clone();
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_change(generation, change).await;
            }
            match entry {
                Some(Entry::Open(component)) => launcher.open_command(generation, component).await,
                Some(Entry::Run(item_id)) => {
                    if let Some(component) = open {
                        launcher.run_action(generation, component, item_id).await
                    }
                }
                Some(Entry::Install(folder, mode)) => {
                    launcher.install(generation, folder, mode).await
                }
                Some(Entry::CustomView(item_id, info)) => {
                    if let Some(component) = open {
                        launcher
                            .open_custom_view(generation, component, item_id, info)
                            .await
                    }
                }
                Some(
                    Entry::Broken(_)
                    | Entry::Unavailable(_)
                    | Entry::InstallFromFolder
                    | Entry::Manage
                    | Entry::Toggle(_)
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
        let generation = self.start_running();
        let launcher = self.clone();
        let folder = folder.to_path_buf();
        async move {
            let checked = launcher.read_and_check(folder.clone()).await;
            let mut state = launcher.lock();
            if state.screen_generation != generation {
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
        let generation = self.start_running();
        let launcher = self.clone();
        let folder = folder.to_path_buf();
        async move { launcher.install(generation, folder, Mode::Install).await }
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
        let generation = state.screen_generation;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_change(generation, change).await;
            }
        }
    }

    /// Applies the user's choice to enable or disable a package, to be
    /// recorded by [`Launcher::finish_change`]. Explains why not and returns
    /// `None` if there is no such package; returns `None` without a word
    /// while another change to it is being recorded.
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
        if !state
            .packages
            .iter()
            .any(|package| package.identity == identity)
        {
            state.view.status = Status::Error(PackageError::NotInstalled(identity).to_string());
            return None;
        }
        if state.changing.contains(&identity) {
            return None;
        }
        state.changing.push(identity.clone());
        self.apply_enabled(state, &identity, enabled);
        state.view.status = Status::Running;
        Some(Change { identity, enabled })
    }

    /// Records a change begun by [`Launcher::begin_change`], undoing it if
    /// it cannot be recorded.
    async fn finish_change(&self, generation: u64, change: Change) {
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
        state.changing.retain(|changing| *changing != identity);
        let status = match recorded {
            Ok(()) => {
                let title = state
                    .packages
                    .iter()
                    .find(|package| package.identity == identity)
                    .map(InstalledPackage::title)
                    .unwrap_or_default();
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
        if state.screen_generation == generation {
            state.view.status = status;
        }
    }

    /// Enables or disables the package with `identity` in this launcher,
    /// without recording it: whether it offers commands and may save
    /// settings, its instances, and the screen showing them.
    fn apply_enabled(&self, state: &mut State, identity: &PackageIdentity, enabled: bool) {
        let Some(package) = state
            .packages
            .iter_mut()
            .find(|package| package.identity == *identity)
        else {
            return;
        };
        package.enabled = enabled;
        if let Some(installation) = &self.installation {
            installation.settings.set_enabled(identity, enabled);
        }
        if !enabled {
            let components: Vec<PathBuf> = package
                .commands()
                .into_iter()
                .map(|command| command.component)
                .collect();
            // Its instances stop; enabling it again starts fresh ones.
            if let Ok(runtime) = self.runtime() {
                runtime.forget(components.iter().cloned());
            }
            if state
                .open
                .as_ref()
                .is_some_and(|open| components.contains(open))
            {
                self.show_root(state, None);
                return;
            }
        }
        match state.view.screen {
            Screen::Root => self.refresh_root(state),
            Screen::Extensions => self.refresh_extensions(state),
            // Other screens show no package state.
            Screen::Command | Screen::Package | Screen::Form | Screen::CustomView => {}
        }
    }

    fn start_running(&self) -> u64 {
        let mut state = self.lock();
        state.view.status = Status::Running;
        state.screen_generation
    }

    async fn install(&self, generation: u64, folder: PathBuf, mode: Mode) {
        let result = match self.installation.as_ref().map(|i| &i.store) {
            None => Err(PackageError::Storage(
                "this launcher does not install packages".into(),
            )),
            Some(store) => match self.read_and_check(folder).await {
                Ok(package) => {
                    let store = store.clone();
                    off_thread(move || {
                        let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                        match mode {
                            Mode::Install => store.install(&package),
                            Mode::Update => store.update(&package),
                        }
                    })
                    .await
                }
                Err(error) => Err(error),
            },
        };
        let mut state = self.lock();
        let current = state.screen_generation == generation;
        match result {
            Ok(installed) => {
                let message = match (mode, installed.version()) {
                    (Mode::Install, _) => format!("Installed {}", installed.title()),
                    (Mode::Update, Some(version)) => {
                        format!("Updated {} to {version}", installed.title())
                    }
                    (Mode::Update, None) => format!("Updated {}", installed.title()),
                };
                let first = installed.commands().first().map(|c| c.component.clone());
                let mut replaced = Vec::new();
                match state
                    .packages
                    .iter_mut()
                    .find(|package| package.identity == installed.identity)
                {
                    Some(package) => {
                        // The replaced copy's code is not run again, and a
                        // command of it that is open closes below: its state
                        // is not carried over (#11, #14).
                        replaced = package
                            .commands()
                            .into_iter()
                            .map(|c| c.component)
                            .collect();
                        if let Ok(runtime) = self.runtime() {
                            runtime.forget(replaced.iter().cloned());
                        }
                        *package = installed;
                    }
                    None => state.packages.push(installed),
                }
                let replaced_is_open = state
                    .open
                    .as_ref()
                    .is_some_and(|open| replaced.contains(open));
                if current || replaced_is_open {
                    self.show_root(&mut state, first);
                    state.view.status = Status::Result(message);
                } else if state.view.screen == Screen::Root {
                    self.refresh_root(&mut state);
                }
            }
            Err(error) if current => state.view.status = Status::Error(error.to_string()),
            Err(_) => {}
        }
    }

    /// Reads the package in `folder` off the calling thread, then has the
    /// runtime check each component without running it.
    async fn read_and_check(&self, folder: PathBuf) -> Result<SourcePackage, PackageError> {
        let package = off_thread(move || SourcePackage::read(&folder)).await?;
        for (command, component) in package.components() {
            let checked = match self.runtime() {
                Ok(runtime) => runtime.check(&component).await,
                Err(error) => Err(error),
            };
            checked.map_err(|error| PackageError::Component {
                command: command.title.clone(),
                error,
            })?;
        }
        Ok(package)
    }

    /// Shows root search: this build's commands, then the installed
    /// packages' commands, then the install row. Selects the command with
    /// component `select` if given, else the first row.
    fn show_root(&self, state: &mut State, select: Option<PathBuf>) {
        let (rows, entries) = self.root_rows(state);
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
            screen: Screen::Root,
            title: "Pane".into(),
            details: Vec::new(),
            rows,
            selected,
            status: match &state.store_problem {
                Some(problem) => Status::Error(problem.clone()),
                None => Status::Idle,
            },
            form: None,
            custom_view: None,
        };
    }

    /// Updates the rows of the root search on screen after the installed
    /// packages changed in the background. Unlike navigating, it keeps the
    /// screen generation, so an action the user started from root still
    /// applies, and it keeps the selection on the same row.
    fn refresh_root(&self, state: &mut State) {
        let selected_id = state
            .view
            .selected
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        let (rows, entries) = self.root_rows(state);
        let selected = selected_id
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .or_else(|| first_index(&rows));
        state.entries = entries;
        state.view.rows = rows;
        state.view.selected = selected;
    }

    /// The rows of root search and what activating each does.
    fn root_rows(&self, state: &State) -> (Vec<Row>, Vec<Entry>) {
        let mut rows = Vec::new();
        let mut entries = Vec::new();
        let mut add = |row: Row, entry: Entry| {
            rows.push(row);
            entries.push(entry);
        };
        // A disabled package contributes nothing to root search.
        let enabled = || state.packages.iter().filter(|package| package.enabled);
        let built = self.commands.iter().cloned().map(|command| (command, None));
        let installed = enabled().flat_map(InstalledPackage::available_commands);
        for (command, unavailable) in built.chain(installed) {
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
            add(row, entry);
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
                add(row, Entry::Broken(problem));
            }
        }
        if self.installation.is_some() {
            let row = Row {
                id: INSTALL_FROM_FOLDER.into(),
                title: "Install extension from folder…".into(),
                subtitle: Some("Choose a local extension package to install".into()),
                unavailable: None,
            };
            add(row, Entry::InstallFromFolder);
        }
        if self.installation.is_some() && !state.packages.is_empty() {
            let row = Row {
                id: MANAGE_EXTENSIONS.into(),
                title: "Manage extensions…".into(),
                subtitle: Some("Enable or disable installed extensions".into()),
                unavailable: None,
            };
            add(row, Entry::Manage);
        }
        (rows, entries)
    }

    /// Sets the value of the open form's field `field_id`: a text field's
    /// text, or the id of an option of a choice field. Unknown fields and
    /// options are ignored. Editing a field clears its error.
    pub fn set_field_value(&self, field_id: &str, value: &str) {
        let mut state = self.lock();
        let Some(form) = state.view.form.as_mut() else {
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
        let submission = match (&state.view.form, &mut state.form, &state.open) {
            (Some(form), Some(open), Some(component)) if !open.submitting => {
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
        let generation = state.screen_generation;
        let launcher = self.clone();
        async move {
            if let Some((component, item_id, values)) = submission {
                launcher
                    .submit(generation, component, item_id, values)
                    .await
            }
        }
    }

    /// Sends `values` and applies the reply as though it had arrived before
    /// any edit made meanwhile: a rejected field that the user has changed
    /// since is not marked, since editing a field clears its error.
    async fn submit(
        &self,
        generation: u64,
        component: PathBuf,
        item_id: String,
        values: Vec<FieldValue>,
    ) {
        let settings = self.settings_of(&component);
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .submit_form_with(&component, &item_id, values.clone(), settings)
                    .await
            }
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        let state = &mut *state;
        state.form.as_mut().expect("a form is open").submitting = false;
        if let Some(problem) = disabled_owner(state, &component) {
            // Disabled while it was submitting: its answer is not shown.
            state.view.status = Status::Error(problem);
            return;
        }
        let view = &mut state.view;
        let fields = &mut view.form.as_mut().expect("a form is open").fields;
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
        let (rows, entries) = extension_rows(&state.packages);
        self.leave_command(state);
        state.entries = entries;
        state.view = LauncherView {
            screen: Screen::Extensions,
            title: "Extensions".into(),
            details: vec![
                "A disabled extension adds no commands and runs nothing; it keeps its settings."
                    .into(),
            ],
            selected: first_index(&rows),
            rows,
            status: Status::Idle,
            form: None,
            custom_view: None,
        };
    }

    /// Updates the installed packages on screen after one was enabled or
    /// disabled; the rows stay in place, and so does the selection.
    fn refresh_extensions(&self, state: &mut State) {
        let (rows, entries) = extension_rows(&state.packages);
        state.entries = entries;
        state.view.rows = rows;
    }

    /// Opens the custom view of `item_id` and shows its first drawing, or
    /// closes it again if the user has left the command meanwhile.
    async fn open_custom_view(
        &self,
        generation: u64,
        component: PathBuf,
        item_id: String,
        info: CustomViewInfo,
    ) {
        let settings = self.settings_of(&component);
        let result = match self.runtime() {
            Ok(runtime) => runtime.open_view_with(&component, &item_id, settings).await,
            Err(error) => Err(error),
        };
        let current = self.lock_if_current(generation);
        let disabled = current
            .as_ref()
            .and_then(|state| disabled_owner(state, &component));
        let Some(mut state) = current.filter(|_| disabled.is_none()) else {
            if let (Ok((id, _)), Ok(runtime)) = (result, self.runtime()) {
                runtime.close_view(id);
            }
            if let Some(problem) = disabled {
                // Disabled while it was opening.
                self.lock().view.status = Status::Error(problem);
            }
            return;
        };
        match result {
            Ok((id, frame)) => {
                let view = LauncherView {
                    screen: Screen::CustomView,
                    title: info.title,
                    details: Vec::new(),
                    rows: Vec::new(),
                    selected: None,
                    status: Status::Idle,
                    form: None,
                    custom_view: Some(CustomViewSnapshot {
                        id,
                        label: info.label,
                        role: info.role,
                        frame,
                    }),
                };
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
                state.screen_generation += 1;
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
        let generation = state.screen_generation;
        // Sent now, so the view handles events in the order of these calls
        // whenever the returned futures are awaited.
        let mut sent: VecDeque<SentEvent> = self.send_to_view(&mut state, event).into();
        drop(state);
        let launcher = self.clone();
        async move {
            while let Some(event) = sent.pop_front() {
                let result = event.reply.await;
                launcher.show_view_answer(generation, event.number, result);
                if event.is_move {
                    sent.extend(launcher.finish_move(generation));
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

    /// Notes that a move sent to the view of `generation` was answered, and
    /// sends the waiting move once no other move is in flight.
    fn finish_move(&self, generation: u64) -> Option<SentEvent> {
        let mut state = self.lock_if_current(generation)?;
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
    fn show_view_answer(&self, generation: u64, number: u64, result: Result<Frame, CallError>) {
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        let state = &mut *state;
        let open = state.custom_view.as_mut().expect("a view is open");
        match result {
            // An answer to an event older than the one on screen is stale.
            Ok(_) | Err(CallError::Guest(_)) if number <= open.shown => {}
            Ok(frame) => {
                open.shown = number;
                let snapshot = state.view.custom_view.as_mut().expect("a view is open");
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
        state.screen_generation += 1;
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
        state.screen_generation += 1;
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

    async fn run_action(&self, generation: u64, component: PathBuf, item_id: String) {
        let settings = self.settings_of(&component);
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .run_action_with(&component, &item_id, settings)
                    .await
            }
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        state.view.status = match (disabled_owner(&state, &component), result) {
            // Disabled while it was running: its answer is not shown.
            (Some(problem), _) => Status::Error(problem),
            (None, Ok(answer)) => Status::Result(answer),
            (None, Err(error)) => Status::Error(error.to_string()),
        };
    }

    async fn open_command(&self, generation: u64, component: PathBuf) {
        let settings = self.settings_of(&component);
        let result = match self.runtime() {
            Ok(runtime) => runtime.get_view_with(&component, settings).await,
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(generation) else {
            return;
        };
        if let Some(problem) = disabled_owner(&state, &component) {
            // Disabled while it was opening.
            state.view.status = Status::Error(problem);
            return;
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
                state.screen_generation += 1;
                state.view = LauncherView {
                    screen: Screen::Command,
                    title: view.title,
                    details: Vec::new(),
                    selected: first_index(&rows),
                    rows,
                    status: Status::Idle,
                    form: None,
                    custom_view: None,
                };
            }
            Err(error) => state.view.status = Status::Error(error.to_string()),
        }
    }

    /// The settings of the installed package `component` belongs to; `None`
    /// for a command built into Pane.
    fn settings_of(&self, component: &Path) -> Option<PackageSettings> {
        let state = self.lock();
        let package = owner(&state.packages, component)?;
        Some(
            self.installation
                .as_ref()?
                .settings
                .owned_by(&package.identity),
        )
    }

    fn runtime(&self) -> Result<&Runtime, CallError> {
        self.runtime.as_ref().map_err(Clone::clone)
    }

    /// Locks the state only if the screen is still the one of `generation`.
    fn lock_if_current(&self, generation: u64) -> Option<MutexGuard<'_, State>> {
        let state = self.lock();
        (state.screen_generation == generation).then_some(state)
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

/// "<title> is disabled" if `component` belongs to a disabled package.
fn disabled_owner(state: &State, component: &Path) -> Option<String> {
    owner(&state.packages, component)
        .filter(|package| !package.enabled)
        .map(|package| format!("{} is disabled", package.title()))
}

/// One row per installed package, saying whether it is enabled and which
/// source it is, so copies with the same title can be told apart.
fn extension_rows(packages: &[InstalledPackage]) -> (Vec<Row>, Vec<Entry>) {
    packages
        .iter()
        .map(|package| {
            let state = if package.enabled {
                "Enabled"
            } else {
                "Disabled"
            };
            let row = Row {
                id: package.identity.key(),
                title: package.title(),
                subtitle: Some(format!("{state} · {}", package.identity)),
                unavailable: None,
            };
            (row, Entry::Toggle(package.identity.clone()))
        })
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
            let view = LauncherView {
                screen: Screen::Package,
                title: format!("Cannot install {}", folder_name(folder)),
                details: vec![format!("Folder: {}", folder.display())],
                rows: Vec::new(),
                selected: None,
                status: Status::Error(error.to_string()),
                form: None,
                custom_view: None,
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
    details.push(format!("Commands: {}", titles.join(", ")));
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
            (row, Entry::Install(package.folder.clone(), Mode::Update))
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
    let view = LauncherView {
        screen: Screen::Package,
        title: manifest.title.clone(),
        details,
        rows: vec![row],
        selected: Some(0),
        status: Status::Idle,
        form: None,
        custom_view: None,
    };
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
    let form_view = LauncherView {
        screen: Screen::Form,
        title: form.title,
        details: Vec::new(),
        rows: Vec::new(),
        selected: None,
        status: Status::Idle,
        form: Some(FormView {
            fields,
            submit_label: form.submit_label,
        }),
        custom_view: None,
    };
    let return_to = std::mem::replace(&mut state.view, form_view);
    state.form = Some(OpenForm {
        item_id,
        return_to,
        submitting: false,
    });
    state.screen_generation += 1;
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
