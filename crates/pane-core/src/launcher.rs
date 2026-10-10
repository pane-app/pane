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
//! stop the call. So does pausing a package that keeps failing (see
//! `pausing`). The one exception is root search's own calls for results
//! computed from the query: a search owns them, so a newer query, or
//! leaving root search, cancels those still pending.
//!
//! Scheduled work follows the same model (see `launcher/schedules`): a
//! command whose manifest declares a schedule runs its item's action every
//! interval while the package's code may run, each run a call into the
//! generation current when the scheduler asked for it. Continuing services
//! do too (see `launcher/services`): a command whose manifest declares one
//! runs its cycles while the package's code may run, at the cadence the
//! service itself answers with, each cycle a call into the generation
//! current when the services thread asked for it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

mod acquire;
mod actions;
mod aliases;
mod application_changes;
mod application_icons;
mod application_update;
mod argument_form;
mod choices;
mod clipboard_settings;
pub mod clipboard_view;
mod command_search;
mod confirmations;
mod crash_notice;
mod feedback;
mod hotkeys;
mod icon_loads;
mod indexed;
mod item_actions;
mod launching;
mod network;
mod own_actions;
mod presentation;
mod programs;
mod providers;
mod publishing;
mod quick_slots;
pub mod search_files;
mod submenus;
pub(crate) mod typed_query;

use crate::clipboard::{Capture, ClipboardSystem};
use crate::dependencies;
use crate::extension_data::{ExtensionData, PackageData};
use crate::files::FileAccess;
use crate::generation::End;
use crate::host_settings::SearchSensitivity;
use crate::hotkeys::{self as system_hotkeys, Hotkeys};
use crate::keyboard::PaneKeys;
use crate::launch::{LaunchRecord, LaunchSource};
use crate::links::{LinkOpener, NoOpener};
use crate::operations::Installed;
use crate::packages::{
    CommandMatches, CommandWhen, InstalledPackage, PackageError, PackageIdentity, RetainedData,
    SavedData, SourcePackage, Store, paused_reason,
};
use crate::platform;
use crate::runtime::{
    AnswerDetail as ComputedDetail, CallError, CustomViewInfo, CustomViewRole, FieldKind,
    FieldValue, Form, Frame, Item, Point, ResultListing, RootAction, RootResult as ComputedResult,
    Runtime, ScreenForm, View, ViewEvent, ViewId, WallTime, WeakRuntime,
};
use crate::search::{self, Candidate, Keys, Query};

mod dependents;
mod developing;
mod extensions;
mod file_search;
mod files;
mod install;
mod learned;
mod looks;
mod pausing;
mod recovery;
mod reload;
mod retained;
mod schedules;
mod services;
mod setup;
mod shortcuts;
mod subtitles;
mod system;
mod uninstall;
mod updates;

use acquire::{Acquisitions, Defaults};
use actions::selected_action;
pub use actions::{DISMISS_NOTICE, ResultAction, ResultActionItem, ResultActions};
use aliases::AliasChoices;
pub use aliases::AliasOutcome;
pub use application_update::ApplicationUpdate;
use application_update::{Application, Updates};
use choices::Record;
pub use crash_notice::{LogNotice, UNEXPECTED_QUIT};
use developing::Developing;
pub use developing::{BuildFailure, Development};
pub(crate) use developing::{BuildNow, Remote};
pub use extensions::{ExtensionMark, ExtensionOperation, OperationKind};
pub use hotkeys::HotkeyOutcome;
use hotkeys::{Bindings, OpenPane};
pub(crate) use install::InstallPreview;
pub use item_actions::{ItemAction, ItemActions, UnboundShortcut};
pub use looks::{AccessoryKind, ShownAccessory, absolute_date, relative_date};
use pausing::{Pauses, Recorder};
pub use presentation::{
    ComputedAnswer, ListPresentation, Presentation, RowKind, RowPresentation, Section,
    answer_sections, root_sections,
};
pub use quick_slots::{PinTarget, QuickSlot, SlotChange};
use schedules::Schedules;
use services::Services;
pub use setup::{
    CommandPreferences, PackagePreferences, PreferenceField, PreferencesTarget, SetupHeader,
};
pub use shortcuts::{ShortcutCatalog, ShortcutCommand, ShortcutGroup};
pub use submenus::{OpenSubmenu, SubmenuState};
pub use updates::UpdateHold;

/// The id of the root row that installs a package from a local folder.
const INSTALL_FROM_FOLDER: &str = "pane.install-from-folder";

/// The folder beside the managed copies where packages downloaded from npm
/// or Git are written until they are installed.
const DOWNLOADS_DIR: &str = "downloads";

/// The folder beside the managed copies where the payloads of default
/// extensions are cached, named by version and integrity, so an
/// interrupted first setup can acquire again without downloading what it
/// already holds (see `acquire`).
const ACQUIRED_DIR: &str = "acquired";

/// The id of the root row that installs a package from npm.
const INSTALL_FROM_NPM: &str = "pane.install-from-npm";

/// The id of the npm package field of Pane's own form that asks which npm
/// package to install.
const NPM_PACKAGE_FIELD: &str = "package";

/// The id of the root row that installs a package from a Git repository.
const INSTALL_FROM_GIT: &str = "pane.install-from-git";

/// The id of the repository field of Pane's own form that asks which Git
/// repository to install from.
const GIT_REPOSITORY_FIELD: &str = "repository";

/// The id of the root row of Pane's "Manage Extensions" command, which
/// opens Settings at the extensions (#168): the window draws its tile by
/// it.
pub const MANAGE_EXTENSIONS: &str = "pane.manage-extensions";

/// The id of the root row that opens Pane's Settings window.
const SETTINGS: &str = "pane.settings";

/// A command offered in root search, backed by one extension component.
#[derive(Clone, Debug)]
pub struct CommandRegistration {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub component: PathBuf,
    /// The keywords the command declares in its manifest (`"keywords"`),
    /// which find its row as its subtitle does: an author's search
    /// terms, distinct from the user's aliases (#197). Pane's own
    /// commands declare none.
    pub keywords: Vec<String>,
    /// Whether the command takes a query (`"takesQuery": true`, or a first
    /// argument that is text with every other optional): text typed into
    /// root search, sent to it through its alias or as a fallback.
    pub takes_query: bool,
    /// Whether the command searches as the user types into its own search
    /// field once it is open (`"search": true`); root search never asks it.
    pub search: bool,
    /// When root search lists the command (`"when"` in its manifest, #195;
    /// always when it does not say).
    pub when: CommandWhen,
    /// What root search matches the command's row by (`"matches"` in its
    /// manifest, #195; its title when it does not say). A command declared
    /// for URL-like or path-like queries is listed only for such a query,
    /// without title matching, and the parsed address or resolved path is
    /// sent to it as its launch record's fallback text when invoked (see
    /// `typed_query`).
    pub matches: CommandMatches,
}

impl CommandRegistration {
    /// The command's id in its package manifest: the part of [`Self::id`]
    /// after the package identity's key, for an installed command.
    pub fn manifest_id(&self) -> &str {
        choices::split(&self.id).1
    }
}

/// Which screen the launcher shows, with what only that screen has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Root search: the root results matching `query`, the text typed into
    /// it, best match first, or every root result when it is empty.
    Root { query: String },
    /// An opened command's list view.
    Command,
    /// An opened command that searches as the user types into its own
    /// search field, holding `query`, the text typed there: while it is
    /// blank, the command's list view; otherwise what the command found
    /// for it. Only the opened command is asked, never root search's
    /// providers.
    CommandSearch { query: String },
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
    /// What an installed package that uses the network did on it this
    /// session (the addresses it tried to reach), as lines of information
    /// under the title. It has no rows.
    NetworkDetails {
        identity: PackageIdentity,
        details: Vec<String>,
    },
    /// The system programs an installed package that runs them ran this
    /// session, as lines of information under the title. It has no rows.
    ProgramDetails {
        identity: PackageIdentity,
        details: Vec<String>,
    },
    /// Why Pane paused an installed package, as lines of information under
    /// the title, with a row that retries it.
    PauseDetails {
        identity: PackageIdentity,
        details: Vec<String>,
    },
    /// Why the last development build of an installed package failed, as
    /// lines of information under the title (its command and output), with
    /// a row that builds it again.
    BuildDetails {
        identity: PackageIdentity,
        details: Vec<String>,
    },
    /// The extension log of an installed package, as its title says it
    /// ("Logs for <title>"): what its code wrote and Pane's messages about
    /// it, which the window reads ([`Launcher::extension_log`]) and follows
    /// as they come. It has no rows.
    ExtensionLog { identity: PackageIdentity },
    /// `question` about an installed package before Pane acts on it, with
    /// lines of information under the title, answered by choosing a row.
    Confirm {
        question: Question,
        details: Vec<String>,
    },
    /// Why Pane's extension runtime stopped after its thread crashed, and
    /// what Pane did, as lines of information under the title, with a row
    /// that restarts it when Pane did not.
    RuntimeDetails { details: Vec<String> },
    /// Asks for the keys of a global hotkey that opens the installed command
    /// with id `command` from any application, with lines of information
    /// under the title. The window sends the keys pressed to
    /// [`Launcher::record_hotkey`]; the rows offer to remove its hotkey.
    Hotkey {
        command: String,
        details: Vec<String>,
    },
}

/// Where in Pane's Settings window a root row is handled (see
/// [`Launcher::selected_settings_target`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    /// The Settings window, wherever it is.
    Settings,
    /// Its extensions: "Manage Extensions".
    Extensions,
    /// Its install flow from a folder.
    InstallFromFolder,
    /// Its install flow from npm.
    InstallFromNpm,
    /// Its install flow from Git.
    InstallFromGit,
}

/// What a confirmation screen asks before Pane acts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// Whether to clear the cache of the installed package with this
    /// identity.
    ClearCache(PackageIdentity),
    /// Whether to clear the history of Pane's own Clipboard History, the
    /// installed package with this identity (#166).
    ClearClipboardHistory(PackageIdentity),
    /// Whether to uninstall the installed package with this identity, and
    /// whether to keep its saved data.
    Uninstall(PackageIdentity),
    /// Whether to delete the retained data of this identity, which is not
    /// installed.
    DeleteRetained(PackageIdentity),
    /// Whether to disable the installed package with this identity together
    /// with the enabled packages that require it.
    DisableDependents(PackageIdentity),
    /// Whether to uninstall the installed package with this identity
    /// together with the installed packages that require it, and whether to
    /// keep their saved data.
    UninstallDependents(PackageIdentity),
}

/// A selectable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// Why the row's action cannot be used; `None` when it can. An
    /// unavailable row stays listed and selectable, and activating it shows
    /// the reason instead of calling the extension.
    pub unavailable: Option<Unavailable>,
}

/// Why a row's action cannot be used, with the reason to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// It does not work on this system: the command, action or package does
    /// not support it, or the system lacks what it needs (such as global
    /// hotkeys).
    OnThisSystem(String),
    /// Its package is paused after an error until the user retries it.
    Paused(String),
}

impl Row {
    /// The row of a result a command answered with (computed or indexed
    /// root results, search results), available. Its id is the result's,
    /// under `scope` if given (`<scope>:<id>`), so results of different
    /// commands among root search's rows cannot share one.
    fn listed(listing: ResultListing, scope: Option<&str>) -> Row {
        Row {
            id: match scope {
                Some(scope) => format!("{scope}:{}", listing.id),
                None => listing.id,
            },
            title: listing.title,
            subtitle: listing.subtitle,
            unavailable: None,
        }
    }
}

/// An opened command's own list: its rows and what activating each does.
#[derive(Clone, Default)]
struct CommandList {
    rows: Vec<Row>,
    entries: Vec<Entry>,
}

impl CommandList {
    /// The list of a command whose list view has `items`. Choosing an item
    /// opens its form, else its custom view, else runs its primary action
    /// (the first, by its callback id); an item with none of them cannot be
    /// activated and says so.
    fn of(items: Vec<Item>) -> CommandList {
        let (rows, entries) = items
            .into_iter()
            .map(|item| {
                let unavailable = platform::unavailable(item.platforms.as_deref(), "this action");
                let entry = match (&unavailable, item.form, item.custom_view) {
                    (Some(reason), ..) => Entry::Unavailable(reason.clone()),
                    (None, Some(form), _) => Entry::Form(item.id.clone(), form),
                    (None, None, Some(info)) => Entry::CustomView(item.id.clone(), info),
                    (None, None, None) if item.actions.is_empty() => Entry::NoActions,
                    (None, None, None) => Entry::Actions(item_actions::Listed {
                        id: item.id.clone(),
                        title: item.title.clone(),
                        actions: item.actions,
                    }),
                };
                let row = Row {
                    id: item.id,
                    title: item.title,
                    subtitle: item.subtitle,
                    unavailable: unavailable.map(Unavailable::OnThisSystem),
                };
                (row, entry)
            })
            .unzip();
        CommandList { rows, entries }
    }
}

impl Unavailable {
    /// The reason, as shown to the user.
    pub fn reason(&self) -> &str {
        match self {
            Unavailable::OnThisSystem(reason) | Unavailable::Paused(reason) => reason,
        }
    }
}

/// Feedback about the most recent action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    /// An extension call or package operation is in progress.
    Running,
    /// Work Pane does in the background is in progress, saying what, such
    /// as building a package being developed.
    Progress(String),
    /// The outcome of the most recent action, such as the extension's answer.
    Result(String),
    /// Why the most recent action failed. For a rejected form field this is
    /// "<field label>: <message>".
    Error(String),
}

/// What the launcher's primary action — Enter, or the window's footer
/// button — does with the selected row right now: one definition for the
/// label, the availability and the binding the window shows, so behavior
/// and presentation cannot diverge.
///
/// The action's identity is the screen plus the selected row's entry,
/// never a display title: the label says what activating that row does
/// there — "Open command" for a selected extension command in root search,
/// "Submit" on a form. The labels are the specification's provisional
/// synthesis ([#70](https://github.com/pane-app/pane/issues/70)), named
/// here so behavior and wording move together. Dispatch itself stays where
/// it is: both Enter and the button route through
/// [`Launcher::activate_selected`], or [`Launcher::submit_form`] on a form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedAction {
    /// The action's label, from its identity. Empty where there is no
    /// primary action to show at all: a custom view takes the keys itself,
    /// the network details screen has only Back, and the hotkey screen
    /// with no row to remove has only the keys it records — the window
    /// shows no button there.
    pub label: String,
    /// Whether the action can run now: `false` with no row selected, for a
    /// row whose action is unavailable on this system or paused (its reason
    /// stays visible where the row shows it), and while an action is
    /// already running. Enter keeps the behavior it has today either way;
    /// this keeps the button from dispatching what cannot run.
    pub available: bool,
}

/// An open form, as the user is filling it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormView {
    pub fields: Vec<FormField>,
    pub submit_label: String,
    /// What the Setup screen shows above and beside its fields, when the
    /// form is the Setup screen Pane shows before a command whose required
    /// preferences are unset (see `setup`); `None` for every other form.
    pub setup: Option<SetupHeader>,
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
    /// What the value is for, shown under the field: a preference's
    /// description on the Setup screen; `None` on other forms.
    pub description: Option<String>,
    /// Whether the form needs a value in it: a required argument in Pane's
    /// argument form, whose first empty one the window focuses. An
    /// extension's form and Pane's other forms say `false`.
    pub required: bool,
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

    /// The text of the search field on screen: root search's query, or the
    /// search of an open command that searches as the user types; `None` on
    /// screens without one.
    pub fn search_field(&self) -> Option<&str> {
        self.screen.search_field()
    }

    /// Lines of information under the title, such as a package's source and
    /// compatibility; empty on screens without any.
    pub fn details(&self) -> &[String] {
        match &self.screen {
            Screen::Package { details }
            | Screen::Extensions { details }
            | Screen::Confirm { details, .. }
            | Screen::PauseDetails { details, .. }
            | Screen::NetworkDetails { details, .. }
            | Screen::ProgramDetails { details, .. }
            | Screen::BuildDetails { details, .. }
            | Screen::RuntimeDetails { details }
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

impl Screen {
    /// The text of this screen's search field: root search's query, or the
    /// search of an open command that searches as the user types; `None`
    /// on a screen without one.
    pub fn search_field(&self) -> Option<&str> {
        match self {
            Screen::Root { query } | Screen::CommandSearch { query } => Some(query),
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
    /// The default extensions this build acquires at first setup, and
    /// where their payloads come from; `None` when this launcher
    /// installs none.
    defaults: Option<Defaults>,
    /// This build's own application update: the version of Pane it runs,
    /// where its updates come from and the program an update replaces;
    /// `None` when this build wires no updater.
    application: Option<Application>,
    /// Opens the web links of computed results.
    links: Arc<dyn LinkOpener>,
    /// Registers the global hotkeys the user assigns with the system.
    hotkeys: Arc<dyn Hotkeys>,
    /// Keeps the clipboard history of the packages that keep one, given
    /// with the system's clipboard ([`Launcher::with_clipboard`]).
    clipboard: Option<Arc<Capture>>,
    /// Runs the scheduled work of the installed commands whose manifests
    /// declare a schedule, by the launcher's clock
    /// ([`Launcher::with_clock`]). Only a launcher that installs packages
    /// schedules anything.
    schedules: Option<Arc<Schedules>>,
    /// Runs the continuing services of the installed commands whose
    /// manifests declare one, by the launcher's clock
    /// ([`Launcher::with_clock`]). Only a launcher that installs packages
    /// runs any.
    services: Option<Arc<Services>>,
    /// Checks for newer versions of the installed npm packages and
    /// updates the eligible ones at a safe boundary (see `updates`).
    /// Only a launcher that installs packages checks anything.
    updates: Option<Arc<updates::Updates>>,
    /// Reads packages from folders and downloads them from npm.
    sources: install::Sources,
    /// The packages being developed: built and reloaded on save. The
    /// sender that tells the window the launcher changed in the background
    /// lives in its configuration ([`Launcher::with_development`]), shared
    /// so that the threads the constructor started see it once it is wired.
    developing: Arc<Developing>,
    state: Arc<Mutex<State>>,
}

/// A launcher that does not keep itself or its runtime running, for the
/// runtime to hold.
#[derive(Clone)]
struct WeakLauncher {
    runtime: Result<WeakRuntime, CallError>,
    commands: Arc<[CommandRegistration]>,
    installation: Option<Installation>,
    defaults: Option<Defaults>,
    application: Option<Application>,
    links: Arc<dyn LinkOpener>,
    hotkeys: Arc<dyn Hotkeys>,
    /// Held weakly, so that Pane stops watching the clipboard as soon as
    /// the launcher is dropped.
    clipboard: Option<std::sync::Weak<Capture>>,
    /// Held weakly, so that Pane stops running scheduled work as soon as
    /// the launcher is dropped.
    schedules: Option<std::sync::Weak<Schedules>>,
    /// Held weakly, so that Pane stops running continuing services as soon
    /// as the launcher is dropped.
    services: Option<std::sync::Weak<Services>>,
    /// Held weakly, so that Pane stops checking for updates as soon as the
    /// launcher is dropped.
    updates: Option<std::sync::Weak<updates::Updates>>,
    sources: install::Sources,
    developing: std::sync::Weak<Developing>,
    state: std::sync::Weak<Mutex<State>>,
}

impl WeakLauncher {
    /// The launcher, unless it or its runtime has stopped.
    fn upgrade(&self) -> Option<Launcher> {
        let runtime = match &self.runtime {
            Ok(runtime) => Ok(runtime.upgrade()?),
            Err(error) => Err(error.clone()),
        };
        let clipboard = match &self.clipboard {
            Some(capture) => Some(capture.upgrade()?),
            None => None,
        };
        Some(Launcher {
            runtime,
            commands: self.commands.clone(),
            installation: self.installation.clone(),
            defaults: self.defaults.clone(),
            application: self.application.clone(),
            links: self.links.clone(),
            hotkeys: self.hotkeys.clone(),
            clipboard,
            schedules: self.schedules.as_ref().and_then(std::sync::Weak::upgrade),
            services: self.services.as_ref().and_then(std::sync::Weak::upgrade),
            updates: self.updates.as_ref().and_then(std::sync::Weak::upgrade),
            sources: self.sources.clone(),
            developing: self.developing.upgrade()?,
            state: self.state.upgrade()?,
        })
    }
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
    /// Writes which packages are paused, in the background.
    records: Recorder,
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
    /// The answers that arrived while the current query's list is held
    /// (#201, see `publishing`): listed when the list is published, in
    /// place of every answer of an earlier query.
    staged: Vec<Computed>,
    /// While the current query's list is not yet published (#201, see
    /// `publishing`): what still holds it. `None` once it is, and for a
    /// query that asks no provider.
    holding: Option<publishing::Holding>,
    /// When a late answer's merge into the published list happens, by the
    /// launcher's clock, while one is coalescing (#201): answers arriving
    /// close together become one update.
    merge: Option<u64>,
    /// The query whose list the rows shown are (#201): the field's own
    /// query once its list is published, the previous query's until then.
    published: String,
    /// The root results commands supplied ahead of the query, such as the
    /// installed applications, listed for a query that is not blank.
    indexes: indexed::Indexes,
    /// What the query typed into root search is, beyond the words it
    /// holds: a web address or a path, parsed once per change of the query
    /// (see `typed_query`, #195). `None` for words and for a blank query.
    typed: Option<typed_query::TypedQuery>,
    /// The user's home folder, which `~` in a typed path resolves to: the
    /// one the file index is configured with, else the environment's.
    home: Option<PathBuf>,
    /// Incremented on every search, so that an answer arriving for an
    /// earlier search, even of the same query, is discarded.
    search_epoch: u64,
    /// Kept while the current search's calls for computed results may run:
    /// dropping it, when the query changes or root search is left, cancels
    /// those still pending (see [`State::next_screen`]).
    search_alive: Option<tokio::sync::oneshot::Sender<()>>,
    /// The folders granted to packages and their listings, shared with the
    /// runtime; `None` without a runtime.
    files: Option<FileAccess>,
    /// The component of the command whose view is open.
    open: Option<PathBuf>,
    /// The launch record the open command's screen was opened with: its
    /// `render` receives it again each time the screen is drawn again.
    launch: LaunchRecord,
    /// The open command's search, when it searches as the user types.
    searching: Option<command_search::Searching>,
    /// The form on screen, if one is open.
    form: Option<OpenForm>,
    /// The search an alias or hotkey flow returns to when the Actions
    /// panel opened it; `None` when the flow came from the extension list.
    actions_return: Option<actions::Return>,
    /// The custom view on screen, if one is open.
    custom_view: Option<OpenCustomView>,
    /// The open command's items' icons, tooltips and accessories, by item
    /// id (see `looks`).
    looks: looks::Looks,
    /// The web images and system icons rows show, loaded in the background
    /// (see `icon_loads`).
    icon_loads: icon_loads::IconLoads,
    /// The installed applications' own icons, which their rows draw bare
    /// (see `application_icons`, #172).
    application_icons: application_icons::ApplicationIcons,
    /// The clock dates are shown relative to: the system's, or the one a
    /// test gave the launcher ([`Launcher::with_clock`]).
    clock: Arc<dyn crate::clipboard::Clock>,
    /// Incremented on every navigation, so a reply that arrives after the
    /// user has left the screen it was requested from is discarded.
    screen_epoch: u64,
    /// The installed packages. Whether each is enabled here is the user's
    /// latest choice, which applies at once, even while it is still being
    /// recorded.
    packages: Vec<InstalledPackage>,
    /// The identities that are not installed but whose data Pane keeps, as
    /// last recorded in the store.
    retained: Vec<RetainedData>,
    /// Packages being enabled or disabled, reloaded or updated, with which;
    /// another change to one of them is refused meanwhile (see
    /// [`State::claim`]).
    changing: HashMap<PackageIdentity, Changing>,
    /// Why the installed packages could not be read, if they could not.
    store_problem: Option<String>,
    /// The packages Pane paused after they failed, each with why, and the
    /// crashes counted towards pausing a package (see `pausing`).
    paused: Pauses,
    /// The global hotkeys the user assigned to commands.
    bindings: Bindings,
    /// The Open Pane hotkey: the application-owned binding the host
    /// settings record and the window applies through the same
    /// registration path (see [`crate::hotkeys`]).
    open_pane: OpenPane,
    /// The aliases and fallbacks the user gave commands.
    aliases: Record<AliasChoices>,
    /// The dropdown arguments' values each command was last launched with
    /// (see `argument_form`).
    remembered_arguments: Record<argument_form::ArgumentChoices>,
    /// The quick slots the user pinned results to, and their record (see
    /// `quick_slots`).
    quick_slots: quick_slots::Kept,
    /// Acquiring Pane's default extensions: what the status line says of
    /// the one being acquired, and which failed and can be tried again
    /// (see `acquire`).
    acquisitions: Acquisitions,
    /// Pane's own update: what the last check found, and what is running
    /// now (see `application_update`).
    updates: Updates,
    /// This run's crash record, and whether root search still tells that
    /// Pane quit unexpectedly last time (see `crash_notice`).
    crash: crash_notice::Notice,
    /// The Clipboard History view's records, as last made from the history,
    /// shared until it changes (see `clipboard_view`, #192).
    clipboard_records: clipboard_view::Projected,
    /// The query root search showed when the status line began showing a
    /// no-view command's answer (or its running), launched from it with
    /// that query typed, so that changing the query clears it.
    sent_from: Option<String>,
    /// Whether the extension list was entered as a screen of its own
    /// ([`Launcher::manage_extensions`], test support): only then do the
    /// operations' confirmations and details screens return to it. The
    /// operations Settings runs ([`Launcher::run_extension_operation`])
    /// leave the launcher where the user had it.
    list_entered: bool,
    /// Whether a command a guest launched wants Pane's window shown (see
    /// [`Launcher::take_window_request`]).
    window_wanted: bool,
    /// The launches guests asked for that are still running.
    launches: Arc<launching::InFlight>,
    /// The newest status of a developed package's builds, kept while
    /// another screen is shown (see `developing`).
    development_status: Option<(PackageIdentity, Status)>,
    /// While the runtime thread is not responding yet, the status line it
    /// replaced, put back if the thread carries on (see `recovery`).
    runtime_slow: Option<Status>,
    /// The user's automatic-update choices (see `updates`): whether every
    /// eligible package updates in the background, and which packages the
    /// user turned it off for.
    update_controls: updates::UpdateControls,
    /// Pane's own keys in force, which no action shortcut takes (see
    /// `item_actions`).
    pane_keys: PaneKeys,
    /// How strict root search's matching is, as the window holds the
    /// Launcher page's choice: High until one is pushed, applied on the
    /// next keystroke (see [`Launcher::set_search_sensitivity`]).
    sensitivity: SearchSensitivity,
    /// The open command's unbound shortcuts as last noted, so a developed
    /// package's report is made again only when they change.
    reported_unbound: Vec<UnboundShortcut>,
    /// The manifest id of the command whose view is open, beside its
    /// component ([`State::open`]): the host functions its calls make act
    /// for it.
    open_command: Option<String>,
    /// The window the host functions drive, whether it is shown, and the
    /// toast (see `feedback`).
    feedback: feedback::Feedback,
    /// The subtitles commands gave their root search rows (see
    /// `subtitles`).
    subtitles: Record<subtitles::Subtitles>,
    /// The writes of the subtitles' record still going on.
    subtitle_saves: Arc<launching::InFlight>,
    /// What root search learned from what the user chooses, and its
    /// record (see `learned`, #199).
    learned: Record<learned::LearnedChoices>,
    /// The writes of the learned record still going on.
    learned_saves: Arc<launching::InFlight>,
    /// The submenus open in the Actions panel over the selected item (see
    /// `submenus`).
    submenus: submenus::Submenus,
    /// The system the `system` host functions act on (see `system`).
    system: Arc<dyn crate::system::System>,
    /// The answers the user told Pane to remember for confirmations (see
    /// `confirmations`).
    confirmations: Record<confirmations::Confirmations>,
    /// The writes of the remembered answers' record still going on.
    confirmation_saves: Arc<launching::InFlight>,
    /// The installed commands, by id, whose required preferences are unset
    /// as last noted: their rows in root search say "Needs setup" (see
    /// `setup`).
    setup_needed: HashSet<String>,
    /// What this start forgot because its command is a root provider (see
    /// `providers`), for the toast naming it.
    provider_forgotten: providers::Forgotten,
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
    /// Its managed copy is being replaced by the update Pane applied by
    /// itself (the updater's apply window). As an update it is, but new
    /// calls the user makes into the package are refused until it lands
    /// (see [`Launcher::open_command`]), so that the boundary the updater
    /// waited for holds: no command the user starts is stopped by it.
    BackgroundUpdating,
    /// It is being uninstalled.
    Uninstalling,
    /// It is not installed, and its retained data is being deleted.
    DeletingRetained,
    /// It is being installed with another package, or an install relies on
    /// it as a required dependency staying as it is.
    Installing,
}

impl Changing {
    /// What is happening, after the package's title: "is reloading".
    fn doing(self) -> &'static str {
        match self {
            Changing::Recording => "is being enabled or disabled",
            Changing::Reloading => "is reloading",
            Changing::Updating | Changing::BackgroundUpdating => "is updating",
            Changing::Uninstalling => "is being uninstalled",
            Changing::DeletingRetained => "is having its retained data deleted",
            Changing::Installing => "is part of an install in progress",
        }
    }
}

impl State {
    /// The installed package with `identity`.
    fn package(&self, identity: &PackageIdentity) -> Option<&InstalledPackage> {
        self.packages
            .iter()
            .find(|package| package.identity == *identity)
    }

    /// Whether `package`'s code may run: it is enabled and not paused.
    fn runs(&self, package: &InstalledPackage) -> bool {
        package.enabled && !self.paused.is_paused(&package.identity)
    }

    /// The title of the installed package with `identity`, the title its
    /// retained data was kept under if it is not installed, or else its
    /// identity.
    fn title_of(&self, identity: &PackageIdentity) -> String {
        self.package(identity)
            .map(InstalledPackage::title)
            .or_else(|| {
                self.retained
                    .iter()
                    .find(|retained| retained.identity == *identity)
                    .map(|retained| retained.title.clone())
            })
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
            Some(busy) => busy.doing(),
        };
        self.view.status = Status::Error(format!("{} {busy}", self.title_of(identity)));
        false
    }

    /// Notes that each `(identity, what)` of `claims` begins, all of them or
    /// none: if something is already happening to one of them, claims
    /// nothing and returns that identity with what is happening to it.
    fn claim_all(
        &mut self,
        claims: &[(PackageIdentity, Changing)],
    ) -> Result<(), (PackageIdentity, Changing)> {
        for (identity, _) in claims {
            if let Some(&busy) = self.changing.get(identity) {
                return Err((identity.clone(), busy));
            }
        }
        for (identity, what) in claims {
            self.changing.insert(identity.clone(), *what);
        }
        Ok(())
    }

    /// Notes that what began with [`State::claim`] on the package with
    /// `identity` has ended.
    fn release(&mut self, identity: &PackageIdentity) {
        self.changing.remove(identity);
    }

    /// Notes that the user left the screen on display: replies for it are
    /// discarded from now on, and root search's pending calls for computed
    /// results are cancelled.
    fn next_screen(&mut self) {
        self.screen_epoch += 1;
        self.search_alive = None;
        // The query's list is neither held nor merged any more (#201): its
        // providers' calls are cancelled, and whatever is shown next
        // builds its own list.
        self.holding = None;
        self.merge = None;
        self.staged.clear();
        // A submenu belongs to the screen it opened on; an answer still on
        // its way finds it gone.
        self.submenus.close_all();
        // A granted folder is listed again on the next visit, and a
        // listing being made for this one stops.
        if let Some(files) = &self.files {
            files.new_visit();
        }
    }
}

/// An enabling or disabling that has taken effect and is being recorded:
/// of one package, or of a package and the packages that require it, the
/// package asked about first.
struct Change {
    identities: Vec<PackageIdentity>,
    enabled: bool,
}

/// What the launcher keeps about the open form besides its view.
struct OpenForm {
    /// What submitting the form does.
    purpose: FormPurpose,
    /// The command view that Back returns to.
    return_to: LauncherView,
    /// Whether a submission is waiting for the extension's reply; further
    /// submissions are ignored meanwhile.
    submitting: bool,
}

/// What submitting a form does.
enum FormPurpose {
    /// Sends it to the open command, for its item with this id.
    Item(String),
    /// Sends it to the open command with this id: the form is the
    /// command's whole screen (`"type": "form"`), so Back leaves the
    /// command for root search.
    Screen(String),
    /// Sets the alias of the installed command with this id (Pane's own).
    Alias(String),
    /// Previews the npm package it names (Pane's own).
    Npm,
    /// Previews the Git repository it names (Pane's own).
    Git,
    /// Saves the preferences the Setup screen asks for, then launches the
    /// command it held back (Pane's own; see `setup`).
    Setup(Box<setup::SetupGate>),
    /// Launches the command waiting for its arguments, with the form's
    /// values (Pane's own argument form; see `argument_form`).
    Arguments(Box<argument_form::Asking>),
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
    /// The installed command it opens, for its alias and fallback.
    target: Option<aliases::Target>,
    /// Its identity as a quick slot holds it, if one can: a registered
    /// command, or an indexed result under its command (see
    /// `quick_slots`).
    pin: Option<PinTarget>,
    /// When the command's row is listed (`"when"`, #195): always, only
    /// with a blank query, or only while the user searches.
    when: CommandWhen,
    /// What the command's row is matched by (`"matches"`, #195): its
    /// title, or only URL-like or path-like queries (see `typed_query`).
    matches: CommandMatches,
}

/// A root result a command computed from the current query.
struct Computed {
    /// The component of the command that computed it.
    component: PathBuf,
    /// The query it was computed for, which its card answers (#201): the
    /// query the field shows may have moved on while the list is held.
    query: String,
    /// The title of the command that computed it, which labels its
    /// answers in root search ("Calculator").
    command_title: String,
    /// What the answer's card says beyond its title and action, when the
    /// result is one (see `runtime::AnswerDetail`): its own section, its
    /// swatch and further ways to copy it, as the command answered — the
    /// calculator's colour and date answers (#196).
    answer: Option<ComputedDetail>,
    row: Row,
    entry: Entry,
    /// A file row, or the row searching all files: listed after what is
    /// found by title, under "Files".
    in_files: bool,
}

/// What activating a row does.
#[derive(Clone)]
enum Entry {
    /// Copy this text to the clipboard, which the window does (root).
    Copy(String),
    /// Open this web address with the link opener (root).
    OpenUrl(String),
    /// A file of a package's granted folder (root search's file results,
    /// Search Files' results): Pane performs its actions itself, Enter
    /// opening a document and revealing a program (see `own_actions`).
    File(files::FileRow),
    /// Nothing in the launcher: the window asks for the folder to grant
    /// this package, then calls [`Launcher::grant_folder`] (command view).
    ChooseFolder(PackageIdentity),
    /// Take back the folder granted to this package (command view).
    StopSharingFolder(PackageIdentity),
    /// Open the installed application `id`, named `name` (root).
    OpenApplication { id: String, name: String },
    /// Open `target` (a URL of any scheme, a file, a folder or an
    /// application), named `name`, with the system's handler or with
    /// `application`, as the `system.open` host function does: an indexed
    /// result such as a quicklink (root).
    OpenTarget {
        target: String,
        application: Option<String>,
        name: String,
    },
    /// Launch this command: open its screen, or run it if it is no-view
    /// (root).
    Open(Opening),
    /// Launch a command that takes a query with the text typed as its
    /// fallback text (root: an alias or fallback).
    Send(aliases::Sending),
    /// Explain why this installed package cannot load (root).
    Broken(String),
    /// Explain why this command (root) or this item's action (command view)
    /// is unavailable on this system, or paused; the extension is not called.
    Unavailable(String),
    /// Nothing in the launcher: the window asks for a folder (root).
    InstallFromFolder,
    /// Ask which npm package to install (root).
    AskNpm,
    /// Ask which Git repository to install from (root).
    AskGit,
    /// Acquire this default extension again, after Pane could not (root).
    Acquire(String),
    /// Install the offered Pane application update, which the user chose
    /// (root).
    InstallUpdate,
    /// Check for a Pane application update again, after the check failed
    /// (root).
    CheckUpdate,
    /// Open Pane's log folder with the system's file manager, after Pane
    /// quit unexpectedly last time (root; see `crash_notice`).
    OpenLogFolder,
    /// Have the open command handle this callback, a search result's id
    /// (`handle-event`), then list it again.
    Run(String),
    /// Run the first of this item's actions (Enter), or another of them
    /// (see `item_actions`): the open command handles its callback, then
    /// lists it again.
    Actions(item_actions::Listed),
    /// Say that this item has no actions, so it cannot be activated
    /// (command view).
    NoActions,
    /// Open this form of the open command's item with this id.
    Form(String, Form),
    /// Open the custom view of the open command's item with this id.
    CustomView(String, CustomViewInfo),
    /// Install the previewed package from this folder or npm package, or
    /// replace its installed copy, as the preview's plan assumed things to
    /// be.
    Install(install::Request, Mode, dependencies::Assumptions),
    /// Pane's "Manage Extensions" command (root): the launcher window
    /// opens Settings at the extensions instead (see
    /// [`Launcher::selected_settings_target`]); activated here, it enters
    /// the extension-management flow Settings drives
    /// ([`Launcher::manage_extensions`]).
    Manage,
    /// Nothing in the launcher: the window opens or focuses its Settings
    /// window (root). Which pages Settings offers is the app's, not the
    /// launcher's.
    Settings,
    /// Enable this installed package if it is disabled, else disable it, or
    /// first ask about the enabled packages that require it.
    Toggle(PackageIdentity),
    /// Turn automatic updates of every eligible package (with `None`), or
    /// of this installed package, on or off (extension list).
    ToggleUpdates(Option<PackageIdentity>),
    /// Disable this installed package and the packages that require it,
    /// which the confirmation showed (confirmation).
    DisableAll(PackageIdentity, Vec<PackageIdentity>),
    /// Reload this installed package from its source folder.
    Reload(PackageIdentity),
    /// Start again this package, which Pane paused after it failed.
    Retry(PackageIdentity),
    /// Show why Pane paused this package (extension list).
    PauseDetails(PackageIdentity),
    /// Show what this package did on the network this session (extension
    /// list).
    NetworkDetails(PackageIdentity),
    /// Show the system programs this package ran this session (extension
    /// list).
    ProgramDetails(PackageIdentity),
    /// Show why Pane's extension runtime stopped (extension list).
    RuntimeDetails,
    /// Start Pane's extension runtime again after it crashed and Pane did
    /// not restart it (extension list, runtime details).
    RestartRuntime,
    /// Build and reload this package after each save in its source folder
    /// (extension list).
    Develop(PackageIdentity),
    /// Stop developing this package (extension list).
    StopDeveloping(PackageIdentity),
    /// Show why this developed package's last build failed (extension
    /// list).
    BuildDetails(PackageIdentity),
    /// Build this developed package now (build details).
    BuildAgain(PackageIdentity),
    /// Show the extension log of this developed package (extension list,
    /// build details).
    ExtensionLog(PackageIdentity),
    /// Ask whether to clear this installed package's cache (extension list).
    AskClearCache(PackageIdentity),
    /// Forget the answers remembered for this installed package's
    /// confirmations, so its commands ask again (extension list).
    ResetConfirmations(PackageIdentity),
    /// Clear this installed package's cache (confirmation).
    ClearCache(PackageIdentity),
    /// Ask whether to clear the history of Pane's own Clipboard History,
    /// this installed package (extension list, #166).
    AskClearClipboardHistory(PackageIdentity),
    /// Clear the history of Pane's own Clipboard History, this installed
    /// package (confirmation).
    ClearClipboardHistory(PackageIdentity),
    /// Ask for the keys of the hotkey of the command with this id
    /// (extension list).
    AskHotkey(String),
    /// Remove the hotkey of the command with this id (hotkey screen).
    RemoveHotkey(String),
    /// Show the form setting the alias of the command with this id
    /// (extension list).
    AskAlias(String),
    /// Make the command with this id a fallback, or no longer one
    /// (extension list).
    ToggleFallback(String),
    /// Forget the alias and fallback of the command with this id, which
    /// cannot be listed (extension list).
    ForgetChoices(String),
    /// Ask whether to uninstall this installed package, and whether to keep
    /// its saved data (extension list).
    AskUninstall(PackageIdentity),
    /// Uninstall this installed package, keeping or deleting its saved data
    /// (confirmation).
    Uninstall(PackageIdentity, SavedData),
    /// Uninstall this installed package and the packages that require it,
    /// which the confirmation showed, keeping or deleting their saved data
    /// (confirmation).
    UninstallAll(PackageIdentity, Vec<PackageIdentity>, SavedData),
    /// Ask whether to delete the retained data of this identity, which is
    /// not installed (extension list).
    AskDeleteRetained(PackageIdentity),
    /// Delete the retained data of this identity (confirmation).
    DeleteRetained(PackageIdentity),
    /// Return to the extension list without acting (confirmation).
    Cancel,
}

/// What activating a row leaves to do once the launcher is unlocked, for
/// [`Launcher::activate_selected`]'s future: at most one piece of work,
/// begun while the launcher was locked.
enum Pending {
    /// Nothing: the row did at once all it does, or there was none.
    Nothing,
    Open(Opening),
    Send(aliases::Sending),
    OpenApplication {
        id: String,
        name: String,
    },
    OpenTarget {
        target: String,
        application: Option<String>,
        name: String,
    },
    Run(String),
    CustomView(String, CustomViewInfo),
    OpenUrl(String),
    /// One of Pane's own actions on a row (see `own_actions`).
    Own(own_actions::Work),
    ClearCache(PackageIdentity),
    Develop(PackageIdentity, developing::DevelopStart),
    Change(Change),
    Reload(reload::Reload),
    HotkeyChange(hotkeys::HotkeyChange),
    ChoiceChange(aliases::ChoiceChange),
    UpdateToggle(updates::UpdateToggle),
    Uninstall(uninstall::Uninstall),
    DeleteRetained(RetainedData),
    Install(install::Begun),
    Acquire(String),
    InstallUpdate,
    CheckUpdate,
    OpenLogFolder,
    StopSharing(PackageIdentity),
}

/// A command to launch, and how: what root search, a hotkey, a quick slot
/// or another command launches.
#[derive(Clone)]
struct Opening {
    component: PathBuf,
    /// Its id in its package manifest, sent with each of its searches and
    /// to its run entry point.
    command: String,
    /// Whether it searches as the user types into its own search field
    /// ([`CommandRegistration::search`]).
    search: bool,
    /// Whether it is a no-view command, which runs instead of opening a
    /// screen.
    no_view: bool,
    /// How it is launched.
    launch: LaunchRecord,
    /// The text its own search field opens with, for a command that
    /// searches: root search's "Search Files for “…”" row (#175).
    initial_search: Option<String>,
}

impl Opening {
    /// `command`, launched by the user from `source`; a no-view command if
    /// `no_view`.
    fn of(command: &CommandRegistration, no_view: bool, source: LaunchSource) -> Opening {
        Opening {
            component: command.component.clone(),
            command: command.manifest_id().to_owned(),
            search: command.search,
            no_view,
            launch: LaunchRecord::by_user(source),
            initial_search: None,
        }
    }
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
        let store = Arc::new(Mutex::new(Store::open(packages_dir.clone())));
        // Only an install in progress needs what it downloaded.
        crate::downloads::remove_abandoned(
            &packages_dir.join(DOWNLOADS_DIR),
            std::time::SystemTime::now(),
        );
        let data = ExtensionData::open(&packages_dir);
        // Expired clipboard history goes before anything shows it, whether
        // or not its package runs.
        data.keep_expiring_clipboard_history();
        let installation = Installation {
            data,
            dir: packages_dir,
            records: Recorder::start(store.clone()),
            store,
        };
        Launcher::create(runtime, commands, Some(installation))
    }

    fn create(
        runtime: Result<Runtime, CallError>,
        commands: Vec<CommandRegistration>,
        installation: Option<Installation>,
    ) -> Self {
        let (packages, retained, store_problem, paused, bindings, aliases) = match &installation {
            Some(installation) => {
                let store = installation.store.lock().unwrap_or_else(|p| p.into_inner());
                let bindings = Bindings::open(&installation.dir);
                let aliases = Record::open(&installation.dir);
                (
                    store.installed(),
                    store.retained(),
                    store.problem(),
                    store.paused(),
                    bindings,
                    aliases,
                )
            }
            None => (
                Vec::new(),
                Vec::new(),
                None,
                Vec::new(),
                Bindings::default(),
                Record::default(),
            ),
        };
        let subtitles = installation
            .as_ref()
            .map_or_else(Record::default, |installation| {
                Record::open(&installation.dir)
            });
        let learned = installation
            .as_ref()
            .map_or_else(Record::default, |installation| {
                Record::open(&installation.dir)
            });
        let confirmations = installation
            .as_ref()
            .map_or_else(Record::default, |installation| {
                Record::open(&installation.dir)
            });
        let remembered_arguments = installation
            .as_ref()
            .map_or_else(Record::default, |installation| {
                Record::open(&installation.dir)
            });
        let update_controls = installation
            .as_ref()
            .and_then(|installation| updates::UpdateControls::open(&installation.dir))
            .unwrap_or_default();
        let logs = runtime.as_ref().map(Runtime::logs).unwrap_or_default();
        let developing = Arc::new(Developing::new(None, None, logs));
        // A web image, a system icon or an application's icon that loaded
        // redraws its row: the window is told through development's shared
        // configuration, as the launcher's other background work tells it.
        let told = Arc::downgrade(&developing);
        let told_icons = told.clone();
        let application_icons = application_icons::ApplicationIcons::new(
            runtime.as_ref().ok(),
            Arc::new(move || {
                if let Some(developing) = told_icons.upgrade() {
                    developing.changed();
                }
            }),
        );
        let icon_loads = icon_loads::IconLoads::new(
            installation
                .as_ref()
                .map(|installation| (installation.data.clone(), installation.dir.clone())),
            runtime.as_ref().ok().map(Runtime::network),
            Arc::new(move || {
                if let Some(developing) = told.upgrade() {
                    developing.changed();
                }
            }),
        );
        // An extension's list may name an application's own icon (#172).
        icon_loads.set_application_icons(application_icons.cache.clone());
        let mut state = State {
            // Replaced by root search below.
            view: LauncherView::new(Screen::Command, ""),
            entries: Vec::new(),
            root: Vec::new(),
            computed: Vec::new(),
            staged: Vec::new(),
            holding: None,
            merge: None,
            published: String::new(),
            indexes: indexed::Indexes::default(),
            typed: None,
            home: home_folder(),
            search_epoch: 0,
            search_alive: None,
            files: runtime.as_ref().ok().map(Runtime::file_access),
            open: None,
            launch: LaunchRecord::default(),
            searching: None,
            form: None,
            actions_return: None,
            custom_view: None,
            looks: looks::Looks::default(),
            icon_loads,
            application_icons,
            clock: Arc::new(crate::clipboard::SystemClock),
            screen_epoch: 0,
            packages,
            retained,
            changing: HashMap::new(),
            development_status: None,
            store_problem,
            paused: Pauses::default(),
            bindings,
            open_pane: OpenPane::default(),
            aliases,
            remembered_arguments,
            quick_slots: quick_slots::Kept::default(),
            acquisitions: Acquisitions::default(),
            updates: Updates::default(),
            crash: crash_notice::Notice::default(),
            clipboard_records: clipboard_view::Projected::default(),
            sent_from: None,
            list_entered: false,
            window_wanted: false,
            launches: Arc::default(),
            runtime_slow: None,
            update_controls,
            pane_keys: PaneKeys::default(),
            sensitivity: SearchSensitivity::default(),
            reported_unbound: Vec::new(),
            open_command: None,
            feedback: feedback::Feedback::default(),
            subtitles,
            subtitle_saves: Arc::default(),
            learned,
            learned_saves: Arc::default(),
            submenus: submenus::Submenus::default(),
            system: crate::system::none(),
            confirmations,
            confirmation_saves: Arc::default(),
            setup_needed: HashSet::new(),
            provider_forgotten: providers::Forgotten::default(),
        };
        if let (Some(installation), Some(files)) = (&installation, &state.files) {
            files.open_record(&installation.dir);
        }
        if let Some(installation) = &installation {
            for package in &state.packages {
                installation
                    .data
                    .set_enabled(&package.identity, package.enabled);
            }
            // A package paused before Pane stopped stays paused, if its
            // code is still the one that failed.
            for (identity, pause) in paused {
                let same = state
                    .package(&identity)
                    .is_some_and(|p| p.enabled && p.version() == pause.version);
                if same {
                    installation.data.pause(&identity);
                    state.paused.restore(identity, pause);
                }
            }
        }
        let sources = install::Sources {
            registry: crate::npm::Registry::npmjs(),
            downloads: installation.as_ref().map(|i| i.dir.join(DOWNLOADS_DIR)),
        };
        let mut launcher = Launcher {
            runtime,
            commands: commands.into(),
            installation,
            defaults: None,
            application: None,
            links: Arc::new(NoOpener),
            hotkeys: system_hotkeys::none(),
            clipboard: None,
            schedules: None,
            services: None,
            updates: None,
            sources,
            developing,
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
        // Scheduled work and continuing services follow the system's clock
        // until a test or a development build gives the launcher another
        // one.
        if let Some(installation) = &launcher.installation {
            let schedules =
                Schedules::start(Arc::new(crate::clipboard::SystemClock), &installation.data);
            schedules.run(launcher.downgrade());
            launcher.schedules = Some(schedules);
            let services =
                Services::start(Arc::new(crate::clipboard::SystemClock), &installation.data);
            services.run(launcher.downgrade());
            launcher.services = Some(services);
            // Pane checks for newer versions of the installed npm packages
            // in the background (see `updates`), starting shortly after
            // this, once a development build's registry is in place.
            let updates = updates::Updates::start(launcher.sources.clone());
            updates.run(launcher.downgrade());
            launcher.updates = Some(updates);
        }
        launcher.report_failures();
        launcher.show_root(&mut launcher.lock(), None);
        // Aliases, fallbacks and hotkeys recorded for a command that has
        // become a root provider are forgotten, with a toast saying so
        // (#164); `with_quick_slots` does the same for its pins.
        launcher.forget_provider_choices();
        launcher.sync_file_index(&launcher.lock());
        launcher
    }

    /// This launcher opening web links, such as quicklinks, with `links`,
    /// normally the system's handler. Without one, opening a link explains
    /// that this Pane has no link handler.
    pub fn with_link_opener(self, links: Arc<dyn LinkOpener>) -> Self {
        let launcher = Launcher { links, ..self };
        launcher.report_failures();
        launcher
    }

    /// The link opener this launcher was given (see
    /// [`Launcher::with_link_opener`]), for the app to open Pane's own
    /// links with the same handler — the documentation entry of Settings'
    /// About page — rather than construct a second opener instance. A
    /// launcher given none returns the opener that says so.
    pub fn link_opener(&self) -> Arc<dyn LinkOpener> {
        self.links.clone()
    }

    /// This launcher downloading npm packages from `registry` rather than
    /// from the public npm registry: one on this computer, for tests and
    /// development ([`crate::npm::Registry::local`]). Release builds have
    /// no way to replace the public registry.
    #[cfg(any(test, debug_assertions))]
    pub fn with_npm_registry(self, registry: crate::npm::Registry) -> Self {
        let sources = install::Sources {
            registry,
            ..self.sources.clone()
        };
        // The updater reads from the new registry too, so that its checks
        // download from where installs do.
        if let Some(updates) = &self.updates {
            updates.follow_registry(sources.clone());
        }
        Launcher { sources, ..self }
    }

    /// This launcher telling the time for clipboard history, scheduled work,
    /// continuing services and root search's publishing by `clock` rather
    /// than the system's clock, for tests and development builds: clipboard
    /// items are kept and expire by it, scheduled commands run by it, their
    /// intervals restarting from its now, services cycle by it, their
    /// cadence restarting from its now, and a query's list is published by
    /// its budget (#201). Items that already expired by the system's clock
    /// were removed when the launcher started. Release builds have no way
    /// to replace the system's clock.
    #[cfg(any(test, debug_assertions))]
    pub fn with_clock(self, clock: Arc<dyn crate::clipboard::Clock>) -> Self {
        // Rows' dates are shown relative to it too (see `looks`).
        self.lock().clock = clock.clone();
        if let Some(installation) = &self.installation {
            let history = installation.data.clipboard_history();
            history.set_clock(clock.clone());
            history.sweep();
        }
        if let Some(schedules) = &self.schedules {
            schedules.follow(clock.clone());
        }
        if let Some(services) = &self.services {
            services.follow(clock.clone());
        }
        if let Some(updates) = &self.updates {
            updates.follow(clock.clone());
        }
        // A clock a test advances publishes what is due by it (#201): the
        // system's clock only moves by time passing, which a thread of the
        // launcher's own waits out.
        let publisher = self.downgrade();
        clock.on_change(Box::new(move || {
            if let Some(launcher) = publisher.upgrade() {
                launcher.publish_due();
            }
        }));
        self
    }
    /// Waits until Pane's clipboard history expiry thread swept after every
    /// change of the history and of the clock so far; `false` if it did not
    /// within `limit`. For tests and development builds, which so wait for
    /// expiry without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_clipboard_expiry(&self, limit: std::time::Duration) -> bool {
        self.installation
            .as_ref()
            .is_some_and(|installation| installation.data.clipboard_history().wait_swept(limit))
    }

    /// Waits until nothing of Pane's clipboard history waits to be written
    /// (#192): the batch of copies and expiries was written, its delay
    /// after the first; `false` if it was not within `limit`. For tests and
    /// development builds, which so read the file once Pane wrote it,
    /// without timing the delay.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_clipboard_writes(&self, limit: std::time::Duration) -> bool {
        self.installation
            .as_ref()
            .is_some_and(|installation| installation.data.clipboard_history().wait_written(limit))
    }

    /// How many times Pane wrote its clipboard history file since it
    /// started (#192), for tests: a burst of copies is one write.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn clipboard_history_writes(&self) -> u64 {
        self.installation.as_ref().map_or(0, |installation| {
            installation.data.clipboard_history().writes()
        })
    }

    /// How many clipboard history items Pane encrypted to write them since
    /// it started (#130, #192), for tests: each once, however many writes
    /// follow; 0 where the system protects nothing.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn clipboard_items_protected(&self) -> u64 {
        self.installation.as_ref().map_or(0, |installation| {
            installation.data.clipboard_history().items_protected()
        })
    }

    /// Waits until the scheduler looked at every change of the clock and of
    /// the installed packages so far, and every scheduled run it started
    /// has reported; `false` if it did not within `limit`. For tests and
    /// development builds, which so wait for scheduled work without timing
    /// it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_schedules(&self, limit: std::time::Duration) -> bool {
        self.schedules
            .as_ref()
            .is_some_and(|schedules| schedules.settled(limit))
    }

    /// Waits until the services thread looked at every change of the clock
    /// and of the installed packages so far, and every service cycle it
    /// started has reported; `false` if it did not within `limit`. For tests
    /// and development builds, which so wait for continuing services
    /// without timing it.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_services(&self, limit: std::time::Duration) -> bool {
        self.services
            .as_ref()
            .is_some_and(|services| services.settled(limit))
    }

    /// Waits until the updater completed a check after this was asked —
    /// the one shortly after Pane starts, or the next one after the clock
    /// moved a cadence on or a control changed — and applied or deferred
    /// every update it staged in it; `false` if it did not within `limit`.
    /// For tests and development builds, which so wait for updates
    /// without timing them.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_updates(&self, limit: std::time::Duration) -> bool {
        self.updates
            .as_ref()
            .is_some_and(|updates| updates.checked(limit))
    }

    /// This launcher registering the global hotkeys the user assigns with
    /// `hotkeys`, normally the system's ([`crate::hotkeys::native`]); the
    /// recorded ones are registered now. Without it, assigning a hotkey
    /// explains that this Pane has none.
    pub fn with_hotkeys(self, hotkeys: Arc<dyn Hotkeys>) -> Self {
        let launcher = Launcher { hotkeys, ..self };
        launcher.sync_hotkeys(&mut launcher.lock());
        launcher.report_failures();
        launcher
    }

    /// This launcher keeping clipboard history for the installed packages
    /// that ask for it through `clipboard`, normally the system's
    /// ([`crate::clipboard::native`]): Pane watches the clipboard exactly
    /// while a package keeps history (the user turned it on, and did not
    /// pause it, in the package's command) and runs (it is enabled and not
    /// paused), so a history kept before a restart is kept again now.
    /// Without it, a package is told that this Pane does not watch the
    /// clipboard. Only a launcher that installs packages keeps any.
    pub fn with_clipboard(self, clipboard: Arc<dyn ClipboardSystem>) -> Self {
        let Some(installation) = &self.installation else {
            return self;
        };
        let capture = Capture::start(clipboard, installation.data.clone());
        if let Ok(runtime) = &self.runtime {
            runtime.set_clipboard(&capture);
        }
        let launcher = Launcher {
            clipboard: Some(capture),
            ..self
        };
        launcher.report_failures();
        launcher
    }

    /// Has the runtime tell this launcher of each failure of an installed
    /// package's code, which may pause the package (see `pausing`). The
    /// runtime holds it weakly: it does not keep this launcher, or itself,
    /// running.
    fn report_failures(&self) {
        self.report_runtime_crashes();
        // The window and feedback host functions commands call are the
        // launcher's, for every command it runs (see `feedback`).
        if let Ok(runtime) = &self.runtime {
            runtime.set_host_functions(Arc::new(feedback::Hosted(self.downgrade())));
            // The commands that asked for the installed applications are
            // asked for their results again when the list changes (see
            // `application_changes`).
            let launcher = self.downgrade();
            runtime.on_applications_changed(move |components| {
                if let Some(launcher) = launcher.upgrade() {
                    launcher.applications_changed(components);
                }
            });
        }
        let (Ok(runtime), Some(_)) = (&self.runtime, &self.installation) else {
            return;
        };
        let launcher = self.downgrade();
        runtime.set_health(Arc::new(move |component, data, health| {
            if let Some(launcher) = launcher.upgrade() {
                launcher.note_health(component, data, health);
            }
        }));
        // The commands guests launch start here.
        let launcher = self.downgrade();
        runtime.set_launches(Arc::new(move |request| match launcher.upgrade() {
            Some(launcher) => launcher.launch_from_guest(request),
            None => Err("Pane is stopping".into()),
        }));
    }

    fn downgrade(&self) -> WeakLauncher {
        WeakLauncher {
            runtime: self
                .runtime
                .as_ref()
                .map(Runtime::downgrade)
                .map_err(Clone::clone),
            commands: self.commands.clone(),
            installation: self.installation.clone(),
            defaults: self.defaults.clone(),
            application: self.application.clone(),
            links: self.links.clone(),
            hotkeys: self.hotkeys.clone(),
            clipboard: self.clipboard.as_ref().map(Arc::downgrade),
            schedules: self.schedules.as_ref().map(Arc::downgrade),
            services: self.services.as_ref().map(Arc::downgrade),
            updates: self.updates.as_ref().map(Arc::downgrade),
            sources: self.sources.clone(),
            developing: Arc::downgrade(&self.developing),
            state: Arc::downgrade(&self.state),
        }
    }

    pub fn view(&self) -> LauncherView {
        self.lock().view.clone()
    }

    /// The screen on show: what the window's keys, focus and screen sync
    /// ask on every key press, read without copying the view's rows, which
    /// a long list makes costly (#165).
    pub fn screen(&self) -> Screen {
        self.lock().view.screen.clone()
    }

    /// The status line's state, read without copying the view's rows.
    pub fn status(&self) -> Status {
        self.lock().view.status.clone()
    }

    /// The installed packages, as read from their managed copies.
    pub fn packages(&self) -> Vec<InstalledPackage> {
        self.lock().packages.clone()
    }

    /// What the current generation of the package `identity` still has set
    /// up, oldest first: its undo list, which the generation's end runs
    /// (see `generation`), such as "extension instance", "native helper" or
    /// "web request". A diagnostic for tests; debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn undo_list(&self, identity: &PackageIdentity) -> Vec<&'static str> {
        self.installation
            .as_ref()
            .map(|installation| {
                installation
                    .data
                    .owned_by(identity)
                    .generation()
                    .undo_list()
            })
            .unwrap_or_default()
    }

    /// Whether this launcher installs packages — whether it was made with
    /// a packages folder ([`Launcher::with_packages`]). Only such a launcher
    /// offers the install rows in root search and the extension list's
    /// global update choice, so a window reaching those rows through the
    /// launcher offers them only then, as root search does.
    pub fn installs_packages(&self) -> bool {
        self.installation.is_some()
    }

    /// Test support: whether an update Pane applies by itself is
    /// replacing the installed package whose command's component is
    /// `component` right now — the claim held while the replacement is
    /// written, which makes calls into the package refused. `component` is
    /// a command's component, as [`InstalledPackage::commands`] gives; the
    /// claim is readable from the moment it is taken until the replacement
    /// lands, however fast the copy is written.
    #[doc(hidden)]
    pub fn package_being_updated(&self, component: &std::path::Path) -> bool {
        self.updating(component).is_some()
    }

    /// Test support: holds every update Pane applies by itself from now
    /// on once it has claimed its package, before the replacement is
    /// written, until the returned hold is dropped: the claim
    /// ([`Launcher::package_being_updated`]) then lasts as long as the test
    /// needs to ask things of the package, however fast the copy is
    /// written (a clone on APFS takes no time at all). One hold at a time.
    #[doc(hidden)]
    pub fn hold_update_applies(&self) -> UpdateHold {
        match &self.updates {
            Some(updates) => updates.hold_applies(),
            // Nothing to hold: this launcher installs no packages.
            None => UpdateHold::of_nothing(),
        }
    }

    /// Test support: the identities holding a change claim right now,
    /// with what each is doing, and the identities and locations of the
    /// installed packages — the raw claim map and the paths the owner
    /// lookup uses, so a test can observe a claim without the lookup.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn claims_now(
        &self,
    ) -> (
        Vec<(String, &'static str)>,
        Vec<(String, std::path::PathBuf)>,
    ) {
        let state = self.lock();
        let claims = state
            .changing
            .iter()
            .map(|(identity, changing)| {
                let doing = match changing {
                    Changing::Recording => "recording",
                    Changing::Reloading => "reloading",
                    Changing::Updating | Changing::BackgroundUpdating => "updating",
                    Changing::Uninstalling => "uninstalling",
                    Changing::DeletingRetained => "deleting-retained",
                    Changing::Installing => "installing",
                };
                (identity.key(), doing)
            })
            .collect();
        let packages = state
            .packages
            .iter()
            .map(|package| (package.identity.key(), package.location.clone()))
            .collect();
        (claims, packages)
    }

    /// Shows `message` as the outcome of the most recent action; for
    /// failures outside the launcher, such as a folder picker that could not
    /// open.
    pub fn show_error(&self, message: impl Into<String>) {
        self.lock().view.status = Status::Error(message.into());
    }

    /// Shows `status` as the outcome of what the window did itself, such as
    /// copying a line of a Logs screen, or the status line at rest again
    /// once the user moved on.
    pub fn show_status(&self, status: Status) {
        self.lock().view.status = status;
    }

    /// Moves the selection by `delta` rows, clamped to the list.
    pub fn move_selection(&self, delta: isize) {
        let mut state = self.lock();
        let view = &mut state.view;
        match view.selected {
            Some(selected) => {
                let last = view.rows.len() - 1;
                view.selected = Some(selected.saturating_add_signed(delta).min(last));
            }
            // Nothing is selected only while the list is empty — root
            // search selects its first fallback too (ADR 0031): Down
            // chooses the first, Up the last.
            None if view.rows.is_empty() => {}
            None if delta > 0 => view.selected = Some(0),
            None => view.selected = Some(view.rows.len() - 1),
        }
    }

    /// Searches root search for `query`: the rows become the root results
    /// that match it, best match first, and the best match is selected —
    /// the first fallback, when nothing but fallbacks is listed (ADR 0031).
    /// An empty query lists every root result. Ignored on other screens,
    /// and when `query` is already the query.
    ///
    /// Metadata is searched at once, without running any guest. For a query
    /// that is not blank, the enabled commands that compute root results
    /// (such as the calculator) are asked too, one after another: the new
    /// query's list is published once every one of them has answered, or
    /// 200 ms after the query changed, whichever comes first (#201); until
    /// then the rows shown stay the previous query's, while the field shows
    /// this query at once (see `publishing`). An answer arriving after the
    /// list was published is merged into it, coalesced within 16 ms with
    /// any that arrives close after. The answers are discarded if the query
    /// has changed meanwhile, and a command that fails is listed as a
    /// result explaining the failure.
    ///
    /// The first query that is not blank since root search was shown also
    /// asks the enabled commands that supply results ahead of the query
    /// (such as the installed applications), after those; the future lists
    /// their results, matched like titles, when they answer, and they are
    /// kept for later queries. Until then the results kept from before are
    /// listed.
    ///
    /// On an open command that searches as the user types, `query` is the
    /// text of its own search field instead: see
    /// [`Launcher::search_in_command`]. Root search's providers are not
    /// asked then, and the opened command never is from root search.
    pub fn set_query(&self, query: &str) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let in_command = match &state.view.screen {
            Screen::CommandSearch { query: current } if current != query => {
                self.search_in_command(&mut state, query)
            }
            _ => None,
        };
        let (asked, indexing, cancelled) = match &state.view.screen {
            Screen::Root { query: current } if current != query => {
                let (cancelled, asked) = self.search(&mut state, query);
                let indexing = self.ask_for_indexed_results(&mut state, query);
                (asked, indexing, Some(cancelled))
            }
            // Searching the same query again changes nothing, not even the
            // selection.
            _ => (Vec::new(), Vec::new(), None),
        };
        // When the query is asked about, for a command that answers about
        // the moment ("now", "today", #196): the whole search shares it.
        let at = asked_at(&state);
        let query = query.to_owned();
        let epoch = state.screen_epoch;
        let search = state.search_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(searching) = in_command {
                searching.await;
            }
            let Some(mut cancelled) = cancelled.filter(|_| !asked.is_empty()) else {
                launcher.show_indexed_results(indexing).await;
                return;
            };
            let listing = launcher
                .show_root_results(epoch, search, &query, at, asked, &mut cancelled)
                .await;
            launcher.show_indexed_results(indexing).await;
            // Commands whose granted folder was still being listed are asked
            // again once it is, after every other result was shown.
            for (command, data, listed) in listing.unwrap_or_default() {
                if until_cancelled(listed, &mut cancelled).await.is_none() {
                    return;
                }
                let asked = launcher
                    .show_one_root_result(epoch, search, &query, at, command, data, &mut cancelled)
                    .await;
                if asked.is_none() {
                    return;
                }
            }
        }
    }

    /// Tells the launcher how strict root search's matching is now: the
    /// Launcher page's choice as the window holds it, pushed as the query
    /// field changes. It applies on the next keystroke — the list the
    /// current query has already made stays as it is — and its late
    /// answers re-rank with it. Until one is pushed, the default (High)
    /// applies.
    pub fn set_search_sensitivity(&self, sensitivity: SearchSensitivity) {
        let mut state = self.lock();
        if state.sensitivity != sensitivity {
            state.sensitivity = sensitivity;
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
            .filter(|package| state.runs(package))
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .indexed_result_commands()
                    .into_iter()
                    // One whose required preferences are unset is not
                    // asked: it says "Needs setup" instead (see `setup`).
                    .filter(move |command| !self.needs_setup(package, command.manifest_id()))
                    .map(move |command| (command, data.clone()))
            })
            .collect();
        state.indexes.begin_asking(commands)
    }

    /// Asks the enabled commands that supply results ahead of the query
    /// for those results, if they have not been asked since root search
    /// was last shown (coming back marks them stale): what the blank
    /// query's list needs below the pins (#199), and what the quick slots
    /// pinning one of them resolve through — a cold visit of root
    /// search's home would otherwise list none of them, since the indexed
    /// results are asked for otherwise only once a query is typed. The
    /// query stays as it is and nothing is searched; await the returned
    /// future to list them, which resolves the slots holding them.
    pub fn resolve_root_home(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut guard = self.lock();
        let state = &mut *guard;
        let commands: Vec<_> = state
            .packages
            .iter()
            .filter(|package| state.runs(package))
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
        let asking = state.indexes.begin_asking(commands);
        drop(guard);
        let launcher = self.clone();
        async move { launcher.show_indexed_results(asking).await }
    }

    /// Asks each of `commands` in turn for its results ahead of the query
    /// and keeps them, listing them in root search if it is on screen,
    /// whatever the query is by then. A command disabled or replaced
    /// meanwhile contributes nothing.
    async fn show_indexed_results(
        &self,
        commands: Vec<(CommandRegistration, Option<PackageData>)>,
    ) {
        self.show_indexed_results_with(commands, relist_root).await;
    }

    /// [`Launcher::show_indexed_results`], listing root search again with
    /// `relist`.
    async fn show_indexed_results_with(
        &self,
        commands: Vec<(CommandRegistration, Option<PackageData>)>,
        relist: fn(&mut State, &str),
    ) {
        for (command, data) in commands {
            let (answer, applications) = match self.runtime() {
                Ok(runtime) => (
                    runtime
                        .indexed_results_with(&command.component, data.clone())
                        .await,
                    Some(runtime.applications()),
                ),
                Err(error) => (Err(error), None),
            };
            let carried = {
                let mut state = self.lock();
                let state = &mut *state;
                if data.as_ref().and_then(PackageData::stopped).is_some() {
                    continue;
                }
                state.indexes.answer(&command, answer);
                // Their applications' icons are refreshed (#172).
                application_icons::listed(state);
                // Pins made before applications had stable identities
                // resolve to the results listed for them now.
                let carried = applications.is_some_and(|applications| {
                    quick_slots::carry_over(state, &command.id, applications.as_ref())
                });
                // While the query's list is held, its publication ranks the
                // answer with everything else (#201): the rows shown stay
                // the previous query's.
                if state.holding.is_none()
                    && let Some(query) = state.view.query().map(str::to_owned)
                {
                    relist(state, &query);
                }
                carried
            };
            if carried {
                self.record_carried_over().await;
            }
        }
    }

    /// Forgets the results supplied ahead of the query by commands that are
    /// no longer enabled, are paused, or were replaced.
    fn forget_indexes(state: &mut State) {
        let indexing: Vec<PathBuf> = state
            .packages
            .iter()
            .filter(|package| state.runs(package))
            .flat_map(|package| package.indexed_result_commands())
            .map(|command| command.component)
            .collect();
        state
            .indexes
            .retain(|component| indexing.iter().any(|kept| kept == component));
        application_icons::listed(state);
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

    /// Opens `target`, named `name`, with `application` or the system's
    /// handler, through the system the `system.open` host function acts
    /// on, off the calling thread, and reports whether it opened.
    async fn open_target(
        &self,
        epoch: u64,
        target: String,
        application: Option<String>,
        name: String,
    ) {
        let system = self.system();
        let applications = self.runtime().ok().map(|runtime| runtime.applications());
        let opened = off_thread(move || {
            // An installed application's id is opened by its source.
            let application = match (application, applications) {
                (Some(application), Some(applications)) => Some(crate::applications::opener(
                    applications.as_ref(),
                    &application,
                )),
                (application, _) => application,
            };
            crate::system::System::open(system.as_ref(), &target, application.as_deref())
        })
        .await;
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        state.view.status = match opened {
            Ok(()) => Status::Result(format!("Opened {name}")),
            Err(problem) => Status::Error(format!("Could not open {name}: {problem}")),
        };
    }

    /// Begins the search for `query`: the field shows it at once, and the
    /// query's list is published once every provider asked has answered or
    /// its budget ended (#201, see `publishing`) — until then the rows
    /// shown stay the previous query's. Results computed for an earlier
    /// query are gone once the list is published, and the calls still
    /// asking for them are cancelled. Returns the commands to ask for what
    /// they compute, and what resolves once this search is replaced too, or
    /// root search is left.
    fn search(
        &self,
        state: &mut State,
        query: &str,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        Vec<(CommandRegistration, Option<PackageData>)>,
    ) {
        state.search_epoch += 1;
        let (alive, cancelled) = tokio::sync::oneshot::channel();
        // Dropping the earlier search's cancels its pending calls.
        state.search_alive = Some(alive);
        // A command's answer to the query sent is not an answer to this one.
        if state.sent_from.take().is_some_and(|sent| sent != query) {
            state.view.status = Status::Idle;
        }
        // What the query is, beyond its words, is understood once per
        // change of it (#195).
        state.typed = typed_query::analyze(query, state.home.as_deref());
        state.view.screen = Screen::Root {
            query: query.to_owned(),
        };
        // A merge still coalescing belongs to the query that was; this
        // query's publication relists whatever it would have.
        state.merge = None;
        state.staged.clear();
        let asked = self.ask_for_root_results(state, query);
        if asked.is_empty() {
            // No provider is asked: the list is published at once, the
            // metadata ranked as it always was (a blank query asks none).
            state.computed.clear();
            state.holding = None;
            let (rows, entries) = root_rows(state, query);
            state.view.selected = aliases::first_choice(&entries);
            state.view.rows = rows;
            state.entries = entries;
            state.published = query.to_owned();
        } else {
            // The query's list is held (#201): the rows shown stay the
            // previous query's while the field's own query runs ahead of
            // them.
            state.holding = Some(publishing::Holding {
                query: query.to_owned(),
                awaiting: asked
                    .iter()
                    .map(|(command, _)| command.component.clone())
                    .collect(),
                deadline: state
                    .clock
                    .now()
                    .saturating_add(publishing::milliseconds(publishing::BUDGET)),
            });
            // The system's clock only moves by time passing, so a thread
            // waits the budget out; a clock a test advances publishes what
            // is due through `Clock::on_change`.
            self.wait_for(publishing::BUDGET);
        }
        (cancelled, asked)
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
            .filter(|package| state.runs(package))
            .flat_map(|package| {
                let data = self
                    .installation
                    .as_ref()
                    .map(|installation| installation.data.owned_by(&package.identity));
                package
                    .root_result_commands()
                    .into_iter()
                    // One whose required preferences are unset is not
                    // asked: it says "Needs setup" instead (see `setup`).
                    .filter(move |command| !self.needs_setup(package, command.manifest_id()))
                    .map(move |command| (command, data.clone()))
            })
            .collect()
    }

    /// Asks each of `commands` in turn for its root results for `query`,
    /// asked about at `at` (see [`WallTime`]), unless the query, the
    /// search or the screen has changed meanwhile.
    ///
    /// Once `cancelled` resolves (the search was replaced, or root search
    /// was left), the pending call is dropped, which cancels it in the
    /// runtime, and no further command is asked. The runtime serves calls one
    /// at a time, so a command that is slow or hangs still delays the
    /// commands asked after it until then (timeouts are #18); it no longer
    /// hides the answers of those asked before it — they are staged for the
    /// query's list, which the budget publishes without waiting for it
    /// (#201, see [`Launcher::set_query`]).
    async fn show_root_results(
        &self,
        epoch: u64,
        search: u64,
        query: &str,
        at: WallTime,
        commands: Vec<(CommandRegistration, Option<PackageData>)>,
        cancelled: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Option<Vec<Listing>> {
        let mut listing = Vec::new();
        for (command, data) in commands {
            let asked = self
                .show_one_root_result(
                    epoch,
                    search,
                    query,
                    at,
                    command.clone(),
                    data.clone(),
                    cancelled,
                )
                .await?;
            if let Some(listed) = asked {
                listing.push((command, data, listed));
            }
        }
        Some(listing)
    }

    /// Asks `command` for its root results for `query`, asked about at
    /// `at`, and keeps them for the query's list (#201): staged while the
    /// list is held, and merged into it once it is published — coalesced,
    /// within 16 ms, with any answer that arrives close after. Nothing is
    /// kept if the query, the search or the screen changed meanwhile (then
    /// `None`: ask nothing more). Answers what resolves once the granted
    /// folder its package's answer was still waiting for is listed, if it
    /// was.
    async fn show_one_root_result(
        &self,
        epoch: u64,
        search: u64,
        query: &str,
        at: WallTime,
        command: CommandRegistration,
        data: Option<PackageData>,
        cancelled: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Option<Option<ListedFuture>> {
        let answer = match self.runtime() {
            Ok(runtime) => {
                let call = runtime.root_results_with(&command.component, query, at, data.clone());
                until_cancelled(call, cancelled).await?
            }
            Err(error) => Err(error),
        };
        let mut state = self.lock_if_current(epoch)?;
        if state.search_epoch != search || state.view.query() != Some(query) {
            return None;
        }
        let state = &mut *state;
        // A command disabled or replaced meanwhile contributes nothing; it
        // has answered all the same, so the list need not wait for it.
        if data.as_ref().and_then(PackageData::stopped).is_some() {
            publishing::answered(state, &command.component);
            return Some(None);
        }
        let owner = owner(&state.packages, &command.component).map(|p| p.identity.key());
        let listed = match (&owner, &state.files) {
            (Some(owner), Some(files)) => files
                .listed(owner)
                .map(|listed| Box::pin(listed) as ListedFuture),
            _ => None,
        };
        let component = command.component.clone();
        let files = state.files.clone();
        let computed = computed_results(command, owner.as_deref(), files.as_ref(), query, answer);
        // The system icons of the files it found (#142), unless the window
        // has each row's load as it draws the row (#165).
        if !looks::loads_as_shown(state) {
            file_search::want_icons(state, computed.iter().map(|computed| &computed.entry));
        }
        if state.holding.is_some() {
            // The query's list is still held (#201): the answer waits for
            // it, staged, and is listed when the list is published.
            state.staged.retain(|staged| staged.component != component);
            state.staged.extend(computed);
        } else {
            // The answer is late: it merges into the published list,
            // coalesced with any that arrives close after (#201).
            state
                .computed
                .retain(|computed| computed.component != component);
            state.computed.extend(computed);
            self.merge_soon(state);
        }
        publishing::answered(state, &component);
        Some(listed)
    }

    /// How the window presents the rows of the view: each row's kind,
    /// alias, hotkey and title matches, and the section labels over them
    /// (see [`Presentation`]). Read-only; root search alone is projected.
    pub fn presentation(&self) -> Presentation {
        presentation::presentation(&self.lock())
    }

    /// The view and its presentation, read together: the same rows, row
    /// for row, however the launcher changes in the background — what a
    /// frame draws from.
    pub fn presented_view(&self) -> (LauncherView, Presentation) {
        let state = self.lock();
        (state.view.clone(), presentation::presentation(&state))
    }

    /// The view and what the window draws of its whole list, read
    /// together (#165): the section labels, whether every row is a
    /// fallback, whether a row shows a date. A window that draws only the
    /// rows in view reads this each frame and each drawn row's own
    /// presentation with [`Launcher::present_row`], so the work behind a
    /// frame does not grow with the list.
    pub fn presented_list(&self) -> (LauncherView, ListPresentation) {
        let state = self.lock();
        (state.view.clone(), presentation::list_presentation(&state))
    }

    /// The presentation of the row at `index` as the window draws it now
    /// (the default for an index past the rows), the same as
    /// [`Launcher::presentation`] gives it. Once the window said it draws
    /// only the rows in view ([`Launcher::load_icons_as_shown`]), this also
    /// starts loading what the row's icons need (#142): the rows out of
    /// view request none (#165).
    pub fn present_row(&self, index: usize) -> RowPresentation {
        let state = self.lock();
        if looks::loads_as_shown(&state) {
            presentation::want_row_icons(&state, index);
        }
        presentation::row_presentation(&state, index)
    }

    /// From now on, the icons of an open command's rows, and of its items'
    /// actions, load as the window draws them — each row's as
    /// [`Launcher::present_row`] presents it, an item's actions' as the
    /// Actions panel lists them ([`Launcher::item_actions`]) — rather than
    /// all as the list opens (#165). The launcher window says so when it
    /// opens: it draws only the rows in view.
    pub fn load_icons_as_shown(&self) {
        looks::load_as_shown(&mut self.lock());
    }

    /// The selected row's index; `None` when nothing is selected. Cheaper
    /// than reading the whole view, for input that only asks this.
    pub fn selected(&self) -> Option<usize> {
        self.lock().view.selected
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

    /// Whether the selected row opens Pane's Settings window. Activating
    /// it does nothing in the launcher: the window opens or focuses its
    /// one Settings window, whatever opened it (the row, the ellipsis menu
    /// or the shortcut).
    pub fn selected_opens_settings(&self) -> bool {
        let state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index));
        matches!(entry, Some(Entry::Settings))
    }

    /// Where in Pane's Settings window the selected root row is handled,
    /// if it is handled there (#168): the Settings row opens the window;
    /// "Manage Extensions" opens it at the extensions; and the install
    /// rows open its install flow from a folder, npm or Git — Settings is
    /// where extensions are installed and managed. The launcher window
    /// asks this before activating the row, and does not activate it when
    /// it is handled in Settings.
    pub fn selected_settings_target(&self) -> Option<SettingsTarget> {
        let state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index))?;
        match entry {
            Entry::Settings => Some(SettingsTarget::Settings),
            Entry::Manage => Some(SettingsTarget::Extensions),
            Entry::InstallFromFolder => Some(SettingsTarget::InstallFromFolder),
            Entry::AskNpm => Some(SettingsTarget::InstallFromNpm),
            Entry::AskGit => Some(SettingsTarget::InstallFromGit),
            _ => None,
        }
    }

    /// Leaves an open form or custom view for its command's list, or an open
    /// command, package preview or the extension list for root search. A
    /// custom view is closed. On root search it clears the query. The
    /// answer is whether something was left: on root search with an empty
    /// query there is nothing left to back out of — `false`, which the
    /// window takes as the end of the Escape chain (the specification's
    /// order ends there by hiding the launcher).
    pub fn back(&self) -> bool {
        let mut state = self.lock();
        match &state.view.screen {
            Screen::Form(_)
                if matches!(
                    state.form,
                    Some(OpenForm {
                        purpose: FormPurpose::Screen(_),
                        ..
                    })
                ) =>
            {
                // The command's own screen: leaving it leaves the command.
                self.show_root(&mut state, None);
            }
            Screen::Form(_) => {
                let form = state.form.take().expect("a form is open");
                if !self.return_from_actions_flow(&mut state) {
                    state.next_screen();
                    state.view = form.return_to;
                }
                state.view.status = Status::Idle;
            }
            Screen::CustomView(_) => self.return_from_custom_view(&mut state, Status::Idle),
            Screen::Confirm { .. } => self.leave_confirm(&mut state),
            Screen::Hotkey { command, .. } => {
                let command = command.clone();
                self.leave_hotkey(&mut state, &command);
                state.view.status = Status::Idle;
            }
            Screen::PauseDetails { identity, .. } => {
                let identity = identity.clone();
                self.show_extensions_at(
                    &mut state,
                    |entry| matches!(entry, Entry::PauseDetails(shown) if *shown == identity),
                );
            }
            Screen::NetworkDetails { identity, .. } => {
                let identity = identity.clone();
                self.show_extensions_at(
                    &mut state,
                    |entry| matches!(entry, Entry::NetworkDetails(shown) if *shown == identity),
                );
            }
            Screen::ProgramDetails { identity, .. } => {
                let identity = identity.clone();
                self.show_extensions_at(
                    &mut state,
                    |entry| matches!(entry, Entry::ProgramDetails(shown) if *shown == identity),
                );
            }
            Screen::BuildDetails { identity, .. } => {
                let identity = identity.clone();
                self.show_extensions_at(
                    &mut state,
                    |entry| matches!(entry, Entry::BuildDetails(shown) if *shown == identity),
                );
            }
            Screen::ExtensionLog { identity } => {
                let identity = identity.clone();
                self.show_extensions_at(
                    &mut state,
                    |entry| matches!(entry, Entry::ExtensionLog(shown) if *shown == identity),
                );
            }
            Screen::RuntimeDetails { .. } => {
                self.show_extensions_at(&mut state, |entry| matches!(entry, Entry::RuntimeDetails));
            }
            // Escape clears the command's search before leaving it, as it
            // clears root search's query.
            Screen::CommandSearch { query } if !query.is_empty() => {
                self.clear_search_in_command(&mut state);
            }
            Screen::Command
            | Screen::CommandSearch { .. }
            | Screen::Package { .. }
            | Screen::Extensions { .. } => self.show_root(&mut state, None),
            Screen::Root { query } => {
                if !query.is_empty() {
                    self.search(&mut state, "");
                } else {
                    // Root search, an empty query: nothing to back out of.
                    return false;
                }
            }
        }
        true
    }

    /// Test support: shows the extension list as a screen of its own,
    /// wherever the launcher now is, its rows driven by
    /// [`Launcher::select`] and [`Launcher::activate_selected`], its
    /// confirmations and details screens returning to it. Pane itself never
    /// shows it (#168, ADR 0043): Settings runs each operation through
    /// [`Launcher::run_extension_operation`], and the "Manage Extensions"
    /// command opens Settings.
    #[doc(hidden)]
    pub fn manage_extensions(&self) {
        let mut state = self.lock();
        state.list_entered = true;
        self.show_extensions(&mut state);
    }

    /// The selected action: what Enter, or the window's footer button, does
    /// with the selected row now (see [`SelectedAction`]). One definition
    /// for the label, the availability and the binding the window shows;
    /// dispatch is the one both inputs already take —
    /// [`Launcher::activate_selected`], or [`Launcher::submit_form`] on a
    /// form — so a click and a key press cannot diverge.
    pub fn selected_action(&self) -> SelectedAction {
        selected_action(&self.lock())
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
        // A use of the selected root result, recorded once its action
        // dispatches (#199, see `learned`): read before activation, which
        // changes the screen.
        let learned = entry
            .as_ref()
            .and_then(|entry| learned::use_of(&state, entry));
        // The status line is about this action from now on.
        state.sent_from = None;
        let pending = match entry {
            Some(entry) => self.activation(&mut state, entry),
            None => Pending::Nothing,
        };
        let work = self.pending_work(&state, pending);
        drop(state);
        // The row's action was dispatched, not refused: the use is
        // recorded off this thread, and the list on screen is not
        // re-sorted for it.
        if let Some((id, query)) = learned {
            self.record_use(&id, query.as_deref());
        }
        work
    }

    /// The work [`Launcher::activation`] left, as a future to await: what
    /// activating a row, or running an extension's operation
    /// ([`Launcher::run_extension_operation`]), does after the lock.
    fn pending_work(
        &self,
        state: &State,
        pending: Pending,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        let epoch = state.screen_epoch;
        let open = state.open.clone();
        // A call into the package belongs to its generation as of now, not
        // as of when the returned future first runs.
        let called = match &pending {
            Pending::Open(Opening { component, .. })
            | Pending::Send(aliases::Sending {
                opening: Opening { component, .. },
                ..
            }) => Some(component),
            Pending::Run(_) | Pending::CustomView(..) => open.as_ref(),
            _ => None,
        };
        let data = called.and_then(|component| self.data_in(state, component));
        let launcher = self.clone();
        async move {
            match pending {
                Pending::Nothing => {}
                Pending::Open(opening) => launcher.launch_opening(epoch, opening, data).await,
                Pending::Send(sending) => {
                    launcher.launch_opening(epoch, sending.opening, data).await
                }
                Pending::OpenApplication { id, name } => {
                    launcher.open_application(epoch, id, name).await
                }
                Pending::OpenTarget {
                    target,
                    application,
                    name,
                } => launcher.open_target(epoch, target, application, name).await,
                Pending::Run(callback) => {
                    if let Some(component) = open {
                        launcher.run_action(epoch, component, callback, data).await
                    }
                }
                Pending::OpenUrl(url) => launcher.open_url(epoch, url).await,
                Pending::Own(work) => launcher.do_own(epoch, work).await,
                Pending::ClearCache(identity) => launcher.clear_cache(epoch, identity).await,
                Pending::CustomView(item_id, info) => {
                    if let Some(component) = open {
                        launcher
                            .open_custom_view(epoch, component, item_id, info, data)
                            .await
                    }
                }
                Pending::Develop(identity, start) => {
                    launcher.finish_developing(identity, start).await
                }
                Pending::Change(change) => launcher.finish_change(epoch, change).await,
                Pending::Reload(reload) => launcher.finish_reload(epoch, reload).await,
                Pending::HotkeyChange(change) => launcher.finish_hotkey_change(change).await,
                Pending::ChoiceChange(change) => launcher.finish_choice_change(change).await,
                Pending::UpdateToggle(toggle) => launcher.finish_update_toggle(toggle).await,
                Pending::Uninstall(uninstall) => launcher.finish_uninstall(epoch, uninstall).await,
                Pending::DeleteRetained(retained) => {
                    launcher.finish_delete_retained(epoch, retained).await
                }
                Pending::Install(install) => launcher.finish_install(epoch, install).await,
                Pending::Acquire(id) => launcher.retry_acquiring(&id).await,
                Pending::InstallUpdate => launcher.install_application_update().await,
                Pending::CheckUpdate => launcher.check_application_update_again().await,
                Pending::OpenLogFolder => launcher.open_log_folder().await,
                Pending::StopSharing(identity) => launcher.stop_sharing_folder(identity).await,
            }
        }
    }

    /// Does at once what activating `entry` does while the launcher is
    /// locked, and returns the work left for [`Launcher::activate_selected`]'s
    /// future. Every kind of row is named here, so a new one has to say what
    /// activating it does.
    fn activation(&self, state: &mut State, entry: Entry) -> Pending {
        match entry {
            Entry::Send(sending) => match &sending.unavailable {
                Some(reason) => {
                    state.view.status = Status::Error(reason.clone());
                    Pending::Nothing
                }
                None => {
                    state.sent_from = state.view.query().map(str::to_owned);
                    state.view.status = Status::Running;
                    Pending::Send(sending)
                }
            },
            Entry::Broken(problem) | Entry::Unavailable(problem) => {
                state.view.status = Status::Error(problem);
                Pending::Nothing
            }
            Entry::Copy(text) => {
                state.view.status = Status::Result(format!("Copied {text} to the clipboard"));
                Pending::Nothing
            }
            Entry::Form(item_id, form) => {
                open_form(state, item_id, form);
                Pending::Nothing
            }
            // Any scheme, as Raycast opens it (ADR 0037): the extension is
            // trusted, and a filter here would protect nothing.
            Entry::OpenUrl(url) => {
                state.view.status = Status::Running;
                Pending::OpenUrl(url)
            }
            Entry::StopSharingFolder(identity) => Pending::StopSharing(identity),
            Entry::Manage => {
                state.list_entered = true;
                self.show_extensions(state);
                Pending::Nothing
            }
            Entry::AskClearCache(identity) => {
                self.show_clear_cache(state, &identity);
                Pending::Nothing
            }
            Entry::AskClearClipboardHistory(identity) => {
                self.show_clear_clipboard_history(state, &identity);
                Pending::Nothing
            }
            Entry::ClearClipboardHistory(identity) => {
                self.clear_clipboard_history_of(state, &identity);
                Pending::Nothing
            }
            Entry::ResetConfirmations(identity) => {
                self.reset_confirmations_row(state, &identity);
                Pending::Nothing
            }
            Entry::NetworkDetails(identity) => {
                self.show_network_details(state, &identity);
                Pending::Nothing
            }
            Entry::ProgramDetails(identity) => {
                self.show_program_details(state, &identity);
                Pending::Nothing
            }
            Entry::PauseDetails(identity) => {
                self.show_pause_details(state, &identity);
                Pending::Nothing
            }
            Entry::RuntimeDetails => {
                self.show_runtime_details(state);
                Pending::Nothing
            }
            Entry::RestartRuntime => {
                self.restart_runtime(state);
                Pending::Nothing
            }
            Entry::Develop(identity) => match self.begin_developing(state, &identity) {
                Some(start) => Pending::Develop(identity, start),
                None => Pending::Nothing,
            },
            Entry::StopDeveloping(identity) => {
                self.end_developing(state, &identity);
                self.refresh(state);
                Pending::Nothing
            }
            Entry::BuildDetails(identity) => {
                self.show_build_details(state, &identity);
                Pending::Nothing
            }
            Entry::BuildAgain(identity) => {
                self.build_again(state, &identity);
                Pending::Nothing
            }
            Entry::ExtensionLog(identity) => {
                self.show_extension_log(state, &identity);
                Pending::Nothing
            }
            Entry::AskUninstall(identity) => {
                let closure = dependencies::required_dependents(&state.packages, &identity);
                if closure.is_empty() {
                    self.show_uninstall(state, &identity);
                } else {
                    self.show_uninstall_dependents(state, &identity, closure);
                }
                Pending::Nothing
            }
            Entry::Uninstall(identity, saved) => self
                .begin_uninstall(state, vec![identity], saved)
                .map_or(Pending::Nothing, Pending::Uninstall),
            Entry::UninstallAll(identity, shown, saved) => self
                .begin_uninstall_all(state, identity, &shown, saved)
                .map_or(Pending::Nothing, Pending::Uninstall),
            Entry::AskDeleteRetained(identity) => {
                self.show_delete_retained(state, &identity);
                Pending::Nothing
            }
            Entry::DeleteRetained(identity) => self
                .begin_delete_retained(state, identity)
                .map_or(Pending::Nothing, Pending::DeleteRetained),
            Entry::Cancel => {
                self.leave_confirm(state);
                Pending::Nothing
            }
            Entry::AskHotkey(command) => {
                self.show_hotkey(state, &command);
                Pending::Nothing
            }
            Entry::RemoveHotkey(command) => self
                .remove_hotkey(state, &command)
                .map_or(Pending::Nothing, Pending::HotkeyChange),
            Entry::AskAlias(command) => {
                self.show_alias_form(state, &command);
                Pending::Nothing
            }
            Entry::ToggleFallback(command) => {
                Pending::ChoiceChange(self.toggle_fallback(state, &command))
            }
            Entry::ForgetChoices(command) => {
                Pending::ChoiceChange(self.forget_choices(state, &command))
            }
            Entry::Toggle(identity) => {
                // The package's state when the user pressed, not when the
                // future runs.
                let enable = state.package(&identity).is_some_and(|p| !p.enabled);
                let closure = match enable {
                    true => Vec::new(),
                    false => dependencies::required_dependents(&state.packages, &identity),
                };
                if closure.iter().any(|dependent| dependent.enabled) {
                    self.show_disable_dependents(state, &identity, closure);
                    Pending::Nothing
                } else {
                    self.begin_change(state, vec![identity], enable)
                        .map_or(Pending::Nothing, Pending::Change)
                }
            }
            Entry::ToggleUpdates(which) => self
                .begin_update_toggle(state, which)
                .map_or(Pending::Nothing, Pending::UpdateToggle),
            Entry::DisableAll(identity, shown) => self
                .begin_disable_all(state, identity, &shown)
                .map_or(Pending::Nothing, Pending::Change),
            Entry::Reload(identity) => self
                .begin_reload(state, identity, reload::Attempt::Reload)
                .map_or(Pending::Nothing, Pending::Reload),
            Entry::Retry(identity) => self
                .begin_reload(state, identity, reload::Attempt::Retry)
                .map_or(Pending::Nothing, Pending::Reload),
            Entry::Install(request, mode, assumptions) => self
                .begin_install(state, request, mode, assumptions)
                .map_or(Pending::Nothing, Pending::Install),
            Entry::Acquire(id) => {
                state.view.status = Status::Running;
                Pending::Acquire(id)
            }
            Entry::InstallUpdate => {
                state.view.status = Status::Running;
                Pending::InstallUpdate
            }
            Entry::CheckUpdate => {
                state.view.status = Status::Running;
                Pending::CheckUpdate
            }
            Entry::OpenLogFolder => {
                state.view.status = Status::Running;
                Pending::OpenLogFolder
            }
            Entry::AskNpm => {
                self.show_npm_form(state);
                Pending::Nothing
            }
            Entry::AskGit => {
                self.show_git_form(state);
                Pending::Nothing
            }
            // The window acts on these, not the launcher.
            Entry::InstallFromFolder | Entry::ChooseFolder(_) | Entry::Settings => Pending::Nothing,
            Entry::Open(opening) => {
                if opening.no_view {
                    Launcher::begin_run(state);
                } else {
                    state.view.status = Status::Running;
                }
                Pending::Open(opening)
            }
            Entry::Run(callback) => {
                state.view.status = Status::Running;
                Pending::Run(callback)
            }
            Entry::Actions(listed) => match listed.actions[0].callback() {
                Some(callback) => {
                    state.view.status = Status::Running;
                    Pending::Run(callback.to_owned())
                }
                // A primary action that opens a submenu (#140): the window
                // opens the Actions panel at it ([`Launcher::open_submenu`]).
                None => Pending::Nothing,
            },
            Entry::NoActions => {
                state.view.status = Status::Error(item_actions::NO_ACTIONS.into());
                Pending::Nothing
            }
            Entry::CustomView(item_id, info) => {
                state.view.status = Status::Running;
                Pending::CustomView(item_id, info)
            }
            Entry::OpenApplication { id, name } => {
                state.view.status = Status::Running;
                Pending::OpenApplication { id, name }
            }
            Entry::OpenTarget {
                target,
                application,
                name,
            } => {
                state.view.status = Status::Running;
                Pending::OpenTarget {
                    target,
                    application,
                    name,
                }
            }
            // A document opens, a program or script is revealed: file
            // search's Enter never runs one (ADR 0037).
            Entry::File(file) => {
                let work = own_actions::primary(file);
                own_actions::begin(state, &work);
                Pending::Own(work)
            }
            Entry::ClearCache(identity) => {
                state.view.status = Status::Running;
                Pending::ClearCache(identity)
            }
        }
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
        let change = self.begin_change(&mut state, vec![identity.clone()], enabled);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(change) = change {
                launcher.finish_change(epoch, change).await;
            }
        }
    }

    /// Applies the user's choice to enable or disable the packages with
    /// `identities` (the one asked about first), to be recorded together by
    /// [`Launcher::finish_change`]. Changes none of them, explains why and
    /// returns `None` if one is not installed or something else is happening
    /// to it; returns `None` without a word while another change to one is
    /// being recorded.
    fn begin_change(
        &self,
        state: &mut State,
        identities: Vec<PackageIdentity>,
        enabled: bool,
    ) -> Option<Change> {
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        if let Some(missing) = identities.iter().find(|i| state.package(i).is_none()) {
            let error = PackageError::NotInstalled(missing.clone());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        let claims: Vec<(PackageIdentity, Changing)> = identities
            .iter()
            .map(|identity| (identity.clone(), Changing::Recording))
            .collect();
        match state.claim_all(&claims) {
            Ok(()) => {}
            // A second enabling or disabling while one is recorded is
            // ignored without a word, as pressing Enter twice would do.
            Err((_, Changing::Recording)) => return None,
            Err((identity, busy)) => {
                let message = format!("{} {}", state.title_of(&identity), busy.doing());
                state.view.status = Status::Error(message);
                return None;
            }
        }
        for identity in &identities {
            self.apply_enabled(state, identity, enabled);
        }
        state.view.status = Status::Running;
        Some(Change {
            identities,
            enabled,
        })
    }

    /// Records a change begun by [`Launcher::begin_change`] in one write,
    /// undoing all of it if it cannot be recorded.
    async fn finish_change(&self, epoch: u64, change: Change) {
        let Change {
            identities,
            enabled,
        } = change;
        let store = self
            .installation
            .as_ref()
            .expect("begin_change checked there is an installation")
            .store
            .clone();
        let recorded = {
            let identities = identities.clone();
            off_thread(move || {
                let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                store.set_enabled_all(&identities, enabled)
            })
            .await
        };
        let mut state = self.lock();
        for identity in &identities {
            state.release(identity);
        }
        let status = match recorded {
            Ok(()) => {
                let titles: Vec<String> = identities.iter().map(|i| state.title_of(i)).collect();
                match (enabled, titles.as_slice()) {
                    (true, _) => Status::Result(format!("Enabled {}", platform::join(&titles))),
                    (false, [title]) => Status::Result(format!("Disabled {title}")),
                    (false, [title, dependents @ ..]) => {
                        Status::Result(dependents::disabled(title, dependents))
                    }
                    (false, []) => Status::Idle,
                }
            }
            Err(error) => {
                for identity in &identities {
                    self.apply_enabled(&mut state, identity, !enabled);
                }
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
        let components: Vec<PathBuf> = package
            .commands()
            .into_iter()
            .map(|command| command.component)
            .collect();
        if let Some(installation) = &self.installation {
            installation.data.set_enabled(identity, enabled);
        }
        // Disabling or enabling ends a pause: the package starts afresh
        // (recording the choice forgets it too).
        self.unpause(state, identity);
        // Its hotkeys are released while it is disabled.
        self.sync_hotkeys(state);
        if !enabled {
            // Its development ends, with a build that is running.
            self.developing.end(Some(identity));
            // Its results kept for root search go, and so does an answer
            // from it being awaited.
            Launcher::forget_indexes(state);
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
        self.refresh(state);
    }

    fn start_running(&self) -> u64 {
        let mut state = self.lock();
        state.view.status = Status::Running;
        state.screen_epoch
    }

    /// Records `installed` as the managed copy of its package: added, or
    /// replacing the copy before it, whose instances stop. Returns whether a
    /// command of the replaced copy is open; the caller leaves it, since its
    /// state is not carried over to the new code.
    fn put_installed(&self, state: &mut State, installed: InstalledPackage) -> bool {
        // New code has not failed.
        self.unpause(state, &installed.identity);
        let Some(package) = state
            .packages
            .iter_mut()
            .find(|package| package.identity == installed.identity)
        else {
            // An identity installed again after it was uninstalled may save
            // data again, and data retained for it is its own again.
            state
                .retained
                .retain(|retained| retained.identity != installed.identity);
            if let Some(installation) = &self.installation {
                installation
                    .data
                    .set_enabled(&installed.identity, installed.enabled);
            }
            // Preference values retained from an earlier copy follow this
            // one's declarations, as an update's do (see `setup`).
            if let Ok(manifest) = &installed.manifest {
                self.carry_preferences(&installed.identity, manifest);
            }
            state.packages.push(installed);
            self.sync_hotkeys(state);
            // A package that uses the file index starts it (#175), whether
            // or not root search is refreshed after.
            self.sync_file_index(state);
            return false;
        };
        // The replaced copy's code no longer runs: its generation ended
        // before its folder was removed ([`Launcher::retire`]).
        let replaced: Vec<PathBuf> = package
            .commands()
            .into_iter()
            .map(|c| c.component)
            .collect();
        if let Ok(runtime) = self.runtime() {
            runtime.forget(replaced.iter().cloned());
        }
        // The values of preferences still declared are kept; those
        // undeclared, or of a type they no longer fit, go (see `setup`).
        let carried = installed.manifest.as_ref().ok().cloned();
        *package = installed;
        if let Some(manifest) = carried {
            let identity = package.identity.clone();
            self.carry_preferences(&identity, &manifest);
        }
        // A command the new copy no longer has releases its hotkey.
        self.sync_hotkeys(state);
        // The new copy may use the file index, or no longer use it.
        self.sync_file_index(state);
        // The replaced copy's results are asked for afresh.
        state
            .indexes
            .retain(|component| !replaced.iter().any(|old| old == component));
        state
            .open
            .as_ref()
            .is_some_and(|open| replaced.contains(open))
    }

    /// Reads the package `request` names off the calling thread (for npm,
    /// downloading it), then has the runtime check each component without
    /// running it.
    async fn read_and_check(
        &self,
        request: install::Request,
    ) -> Result<SourcePackage, PackageError> {
        self.read_and_check_from(self.sources.clone(), request)
            .await
    }

    /// Like [`Launcher::read_and_check`], reading the package from
    /// `sources` rather than this launcher's own: the updater's thread,
    /// which holds the registry a development build was given after the
    /// launcher was built (a [`WeakLauncher`] keeps the sources as they
    /// were when it was made).
    pub(in crate::launcher) async fn read_and_check_from(
        &self,
        sources: install::Sources,
        request: install::Request,
    ) -> Result<SourcePackage, PackageError> {
        let mut package = off_thread(move || sources.read(&request)).await?;
        let checked = self.check_components(&package).await?;
        package.note_imports(checked);
        Ok(package)
    }

    /// Like [`Launcher::read_and_check`], for a package staged in `folder`
    /// (a development build) that belongs to the source `identity`.
    async fn read_and_check_staged(
        &self,
        folder: PathBuf,
        identity: PackageIdentity,
    ) -> Result<SourcePackage, PackageError> {
        let mut package = off_thread(move || SourcePackage::read_staged(&folder, identity)).await?;
        let checked = self.check_components(&package).await?;
        package.note_imports(checked);
        Ok(package)
    }

    /// Has the runtime check each component of `package` without running
    /// it; whether any imports `wasi:http` (it can make web requests), and
    /// whether any imports `pane:extension/programs` (it can run system
    /// programs).
    async fn check_components(
        &self,
        package: &SourcePackage,
    ) -> Result<crate::runtime::Checked, PackageError> {
        let mut imports = crate::runtime::Checked::default();
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
            let checked = checked.map_err(|error| PackageError::Component {
                command: name.clone(),
                error,
            })?;
            imports |= checked;
        }
        Ok(imports)
    }

    /// Shows root search with an empty query: this build's commands, then
    /// the installed packages' commands, then the install row. Selects the
    /// command with component `select` if given, else the first row.
    fn show_root(&self, state: &mut State, select: Option<PathBuf>) {
        state.list_entered = false;
        self.note_setup_needed(state);
        state.root = self.root_results(state);
        state.sent_from = None;
        state.computed.clear();
        // What a held query's search had staged is gone with it (#201): the
        // empty query's list is published at once.
        state.staged.clear();
        state.holding = None;
        state.merge = None;
        state.indexes.stale();
        state.typed = None;
        state.published = String::new();
        let (rows, entries) = root_rows(state, "");
        let selected = select
            .and_then(|component| {
                entries.iter().position(
                    |entry| matches!(entry, Entry::Open(opening) if opening.component == component),
                )
            })
            .or_else(|| aliases::first_choice(&entries));
        self.leave_command(state);
        state.entries = entries;
        state.view = LauncherView {
            rows,
            selected,
            status: match (
                &state.store_problem,
                state.quick_slots.unreadable(),
                state.learned.unreadable(),
            ) {
                (Some(problem), _, _) => Status::Error(problem.clone()),
                (None, Some(problem), _) => Status::Error(quick_slots::unreadable_report(problem)),
                (None, None, Some(problem)) => Status::Error(learned::unreadable_report(problem)),
                (None, None, None) => Status::Idle,
            },
            ..LauncherView::new(
                Screen::Root {
                    query: String::new(),
                },
                "Pane",
            )
        };
        self.show_kept_development_status(state);
    }

    /// Shows root search with an empty query, leaving whatever screen is
    /// open — the state a summoned launcher starts from, with its search
    /// to focus. The Open Pane hotkey's show path uses this when the
    /// launcher was left on a screen with no search of its own, and the
    /// Launcher page's root-search choice uses it on every reopening.
    pub fn show_root_search(&self) {
        let mut state = self.lock();
        self.show_root(&mut state, None);
    }

    /// Whether the view on screen can be restored by a reopened launcher:
    /// the parent specification's provisional default reopens what the
    /// user left, when it is still a view there is something to return
    /// to. Root search always is, and so are the screens of Pane's own
    /// flows (a package's preview, details and confirmations); the
    /// extension list is not, since it is the flow Settings drives and the
    /// launcher window has no screen for it (#168); a command's view — its list, its search, a form or
    /// a custom view of it — is only while the command's package is still
    /// installed and enabled, since a removed or disabled extension
    /// leaves nothing to restore and a reopening launcher returns safely
    /// to root search instead. A command not installed as a package — one
    /// registered with Pane at start — has no package to lose and is
    /// always restorable.
    pub fn restorable_view(&self) -> bool {
        let state = self.lock();
        if matches!(state.view.screen, Screen::Extensions { .. }) {
            return false;
        }
        // Only a command's view has something to lose. A command not
        // installed as a package — one registered with Pane at start —
        // owns nothing that can be removed, so it is always restorable;
        // an installed one is while its package stays installed and
        // enabled, since a removed or disabled extension leaves nothing
        // to return to and a reopening launcher goes safely to root
        // search instead. Pane's own forms, opened with no command, are
        // Pane's to keep.
        match state.open.as_ref() {
            None => true,
            Some(component) => state
                .packages
                .iter()
                .find(|package| component.starts_with(&package.location))
                .is_none_or(|package| package.enabled),
        }
    }

    /// Updates root search or the extension list on screen after a package
    /// changed; other screens show no package state.
    fn refresh(&self, state: &mut State) {
        // The file index runs while a package that uses it may run.
        self.sync_file_index(state);
        match &state.view.screen {
            Screen::Root { .. } => self.refresh_root(state),
            Screen::Extensions { .. } => self.refresh_extensions(state),
            Screen::Command
            | Screen::CommandSearch { .. }
            | Screen::Package { .. }
            | Screen::Form(_)
            | Screen::CustomView(_)
            | Screen::Confirm { .. }
            | Screen::Hotkey { .. } => {}
            // Its lines stay, also once development ended: the window reads
            // them as they are.
            Screen::ExtensionLog { .. } => {}
            Screen::RuntimeDetails { .. } => self.keep_runtime_details(state),
            // Once the build succeeded or development ended, the extension
            // list; else the latest failure. The screen epoch is kept.
            Screen::BuildDetails { identity, .. } => {
                let identity = identity.clone();
                let epoch = state.screen_epoch;
                self.show_build_details(state, &identity);
                state.screen_epoch = epoch;
            }
            // Once the package is no longer paused (retried, reloaded,
            // disabled), the extension list, keeping the screen epoch as
            // refreshing does.
            Screen::PauseDetails { identity, .. } if !state.paused.is_paused(identity) => {
                let identity = identity.clone();
                let epoch = state.screen_epoch;
                self.show_extensions_at(
                    state,
                    |entry| matches!(entry, Entry::Reload(shown) if *shown == identity),
                );
                state.screen_epoch = epoch;
            }
            Screen::PauseDetails { .. } => {}
            // What it reached since, or the extension list once it is gone,
            // keeping the screen epoch.
            Screen::NetworkDetails { identity, .. } => {
                let identity = identity.clone();
                let epoch = state.screen_epoch;
                self.show_network_details(state, &identity);
                state.screen_epoch = epoch;
            }
            // What it ran since, or the extension list once it is gone,
            // keeping the screen epoch.
            Screen::ProgramDetails { identity, .. } => {
                let identity = identity.clone();
                let epoch = state.screen_epoch;
                self.show_program_details(state, &identity);
                state.screen_epoch = epoch;
            }
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
        self.note_setup_needed(state);
        state.root = self.root_results(state);
        // A command that was disabled, paused or replaced contributes
        // nothing more; one enabled again answers from the next change of the
        // query.
        let computing: Vec<PathBuf> = state
            .packages
            .iter()
            .filter(|package| state.runs(package))
            .flat_map(|package| package.root_result_commands())
            .map(|command| command.component)
            .collect();
        state
            .computed
            .retain(|computed| computing.contains(&computed.component));
        state
            .staged
            .retain(|staged| computing.contains(&staged.component));
        Launcher::forget_indexes(state);
        let query = state.view.query().unwrap_or_default();
        // While the query's list is held (#201), the rows shown stay the
        // previous query's: what changed is ranked when it is published.
        if state.holding.is_some() {
            return;
        }
        let (rows, entries) = root_rows(state, query);
        let selected = selected_id
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .or_else(|| aliases::first_choice(&entries));
        state.entries = entries;
        state.view.rows = rows;
        state.view.selected = selected;
    }

    /// Every root result, in root search order: this build's commands, the
    /// enabled packages' commands, the packages that cannot load, then
    /// Pane's own rows.
    fn root_results(&self, state: &State) -> Vec<RootResult> {
        let mut results = Vec::new();
        let mut add = |row: Row,
                       entry: Entry,
                       package: Option<&str>,
                       target,
                       keywords: &[String],
                       when: CommandWhen,
                       matches: CommandMatches| {
            let alias = state.aliases.chosen.active_alias(&row.id);
            let keys = Keys::new(&row.title, row.subtitle.as_deref(), package)
                .with_keywords(keywords)
                .with_alias(alias);
            // A command's row, available or not, is a registered command a
            // quick slot can hold by its id; Pane's own rows are not.
            let pin = matches!(entry, Entry::Open(_) | Entry::Unavailable(_))
                .then(|| PinTarget::Command(row.id.clone()));
            results.push(RootResult {
                row,
                entry,
                keys,
                target,
                pin,
                when,
                matches,
            });
        };
        let command = |(command, unavailable): (CommandRegistration, Option<Unavailable>),
                       no_view: bool| {
            let entry = match &unavailable {
                Some(reason) => Entry::Unavailable(reason.reason().to_owned()),
                None => Entry::Open(Opening::of(&command, no_view, LaunchSource::RootSearch)),
            };
            // A subtitle the command set replaces its manifest's.
            let subtitle = state
                .subtitles
                .chosen
                .subtitle_of(&command.id)
                .map(str::to_owned)
                .or(command.subtitle);
            let row = Row {
                id: command.id,
                title: command.title,
                subtitle,
                unavailable,
            };
            (row, entry)
        };
        // A disabled package contributes nothing to root search.
        let enabled = || state.packages.iter().filter(|package| package.enabled);
        for built in self.commands.iter() {
            let (row, entry) = command((built.clone(), None), false);
            add(
                row,
                entry,
                None,
                None,
                &built.keywords,
                built.when,
                built.matches,
            );
        }
        for package in enabled() {
            // Its commands are found by its title too, even those that show
            // a subtitle of their own.
            let title = package.title();
            // A paused package's commands stay listed, saying why they do
            // not run.
            let paused = state
                .paused
                .is_paused(&package.identity)
                .then(|| Unavailable::Paused(paused_reason(&title)));
            // A root provider has no row: its results answer instead.
            for (registration, unavailable) in package.launchable_commands() {
                let unavailable = paused
                    .clone()
                    .or(unavailable.map(Unavailable::OnThisSystem));
                let no_view = package.mode_of(registration.manifest_id())
                    == crate::packages::CommandMode::NoView;
                let (when, matches) = (registration.when, registration.matches);
                let target = aliases::Target {
                    registration: registration.clone(),
                    identity: package.identity.clone(),
                    unavailable: unavailable.clone(),
                    no_view,
                };
                let keywords = registration.keywords.clone();
                let (row, entry) = command((registration, unavailable), no_view);
                add(
                    row,
                    entry,
                    Some(&title),
                    Some(target),
                    &keywords,
                    when,
                    matches,
                );
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
                add(
                    row,
                    Entry::Broken(problem),
                    None,
                    None,
                    &[],
                    CommandWhen::Always,
                    CommandMatches::Title,
                );
            }
        }
        if self.installation.is_some() {
            let row = Row {
                id: INSTALL_FROM_FOLDER.into(),
                title: "Install extension from folder…".into(),
                subtitle: Some("Choose a local extension package to install".into()),
                unavailable: None,
            };
            add(
                row,
                Entry::InstallFromFolder,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
            let row = Row {
                id: INSTALL_FROM_NPM.into(),
                title: "Install extension from npm…".into(),
                subtitle: Some("Download an extension package published to npm".into()),
                unavailable: None,
            };
            add(
                row,
                Entry::AskNpm,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
            let row = Row {
                id: INSTALL_FROM_GIT.into(),
                title: "Install extension from Git…".into(),
                subtitle: Some("Fetch an extension package from a Git repository".into()),
                unavailable: None,
            };
            add(
                row,
                Entry::AskGit,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
        }
        // A default extension Pane could not acquire can be tried again;
        // the row is gone while one is being acquired, or once it is
        // installed.
        if self.defaults.is_some() {
            for (id, title, why) in state.acquisitions.retryable() {
                let row = Row {
                    id: format!("acquire:{id}"),
                    title: format!("Set up {title}"),
                    subtitle: Some(why),
                    unavailable: None,
                };
                add(
                    row,
                    Entry::Acquire(id),
                    None,
                    None,
                    &[],
                    CommandWhen::Always,
                    CommandMatches::Title,
                );
            }
        }
        // Pane's own update, when a check found one the user can choose to
        // install, or failed in a way that can be tried again.
        if self.application.is_some() {
            for (row, entry) in state.updates.rows() {
                add(
                    row,
                    entry,
                    None,
                    None,
                    &[],
                    CommandWhen::Always,
                    CommandMatches::Title,
                );
            }
        }
        // That Pane quit unexpectedly last time, until the user dismisses
        // it or opens the log folder (see `crash_notice`).
        for (row, entry) in state.crash.rows() {
            add(
                row,
                entry,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
        }
        // Retained data is managed there too, while nothing is installed.
        if self.installation.is_some() && !(state.packages.is_empty() && state.retained.is_empty())
        {
            let row = Row {
                id: MANAGE_EXTENSIONS.into(),
                title: "Manage Extensions".into(),
                subtitle: Some("Configure, update and remove extensions in Settings".into()),
                unavailable: None,
            };
            add(
                row,
                Entry::Manage,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
        }
        // Pane's Settings window is the app's to open; the row is listed
        // whatever is installed, since Settings is reachable without any
        // extension (the ellipsis menu and the local shortcut open it
        // too).
        {
            let row = Row {
                id: SETTINGS.into(),
                title: "Settings…".into(),
                subtitle: Some("Open Pane's settings window".into()),
                unavailable: None,
            };
            add(
                row,
                Entry::Settings,
                None,
                None,
                &[],
                CommandWhen::Always,
                CommandMatches::Title,
            );
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
    ///
    /// Pane's own alias form is applied at once instead (see `aliases`);
    /// the future records it. Pane's argument form launches its command
    /// with the values given, or takes focus to a required field left
    /// empty (see `argument_form`); the future runs the command.
    pub fn submit_form(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let state = &mut *state;
        // The Setup screen saves the preferences it asks for, then launches
        // the command it held back (see `setup`).
        let setup = self.begin_setup_submit(state);
        let arguments = self.submit_arguments(state);
        let alias_change = match (&state.view.screen, &state.form) {
            (
                Screen::Form(_),
                Some(OpenForm {
                    purpose: FormPurpose::Alias(command),
                    ..
                }),
            ) => {
                let command = command.clone();
                self.submit_alias(state, &command)
            }
            _ => None,
        };
        // Pane's own npm and Git forms preview the package they name; the
        // form stays until the preview replaces it, and Back meanwhile
        // discards it.
        let named = match (&state.view.screen, &mut state.form) {
            (
                Screen::Form(form),
                Some(
                    open @ OpenForm {
                        purpose: FormPurpose::Npm | FormPurpose::Git,
                        submitting: false,
                        ..
                    },
                ),
            ) => {
                open.submitting = true;
                state.view.status = Status::Running;
                let git = matches!(open.purpose, FormPurpose::Git);
                form.fields.first().map(|field| (git, field.value.clone()))
            }
            _ => None,
        };
        let submission = match (&state.view.screen, &mut state.form, &state.open) {
            (
                Screen::Form(form),
                Some(
                    open @ OpenForm {
                        purpose: FormPurpose::Item(_) | FormPurpose::Screen(_),
                        ..
                    },
                ),
                Some(component),
            ) if !open.submitting => {
                let (FormPurpose::Item(item_id) | FormPurpose::Screen(item_id)) = &open.purpose
                else {
                    unreachable!("matched above");
                };
                let item_id = item_id.clone();
                open.submitting = true;
                let values: Vec<FieldValue> = form
                    .fields
                    .iter()
                    .map(|field| FieldValue {
                        id: field.id.clone(),
                        value: field.value.clone(),
                    })
                    .collect();
                Some((component.clone(), item_id, values))
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
            if let Some(setup) = setup {
                launcher.finish_setup(epoch, setup).await;
            }
            if let Some(submitted) = arguments {
                launcher.launch_submitted(submitted).await;
            }
            if let Some(change) = alias_change {
                launcher.finish_choice_change(change).await;
            }
            match named {
                Some((false, spec)) => launcher.preview_npm(&spec).await,
                Some((true, spec)) => launcher.preview_git(&spec).await,
                None => {}
            }
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

    /// Shows why Pane paused the installed package with `identity`: how it
    /// failed, its version and the full diagnostics, with a row that retries
    /// it. A package that is not paused (it was retried meanwhile) shows the
    /// extension list instead.
    fn show_pause_details(&self, state: &mut State, identity: &PackageIdentity) {
        let (Some(package), Some(pause)) = (state.package(identity), state.paused.of(identity))
        else {
            self.show_extensions(state);
            return;
        };
        let title = package.title();
        let mut details = vec![
            format!("{}.", pause.after.failure(&title)),
            format!("From {identity}"),
        ];
        if let Some(version) = &pause.version {
            details.push(format!("Version: {version}"));
        }
        details.push(
            "Pane runs none of its code until you retry it, reload or update it, or disable and \
             enable it. Its settings and saved data are kept."
                .into(),
        );
        details.extend(
            pause
                .why
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        );
        let retry = Row {
            id: format!("retry:{}", identity.key()),
            title: pause.after.retry_title(&title),
            subtitle: Some("Start it again".into()),
            unavailable: None,
        };
        state.next_screen();
        state.entries = vec![Entry::Retry(identity.clone())];
        let screen = Screen::PauseDetails {
            identity: identity.clone(),
            details,
        };
        state.view =
            LauncherView::new(screen, pausing::details_title(&title)).with_rows(vec![retry]);
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
        state.next_screen();
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
        if cleared.is_ok() {
            // Its web images went with its cache: a list naming them
            // downloads them again.
            let loads = self.lock().icon_loads.clone();
            loads.forget(&identity.key());
        }
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
        // Unknown (the runtime did not answer in time) counts as running.
        let still_running = match self.runtime() {
            Ok(runtime) => runtime_barrier(runtime)
                .await
                .is_none_or(|running| running.iter().any(|path| components.contains(path))),
            Err(_) => false,
        };
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
            Question::ClearClipboardHistory(identity) => {
                self.show_extensions_at_clear_clipboard_history(state, &identity)
            }
            Question::Uninstall(identity) | Question::UninstallDependents(identity) => self
                .show_extensions_at(
                    state,
                    |entry| matches!(entry, Entry::AskUninstall(asked) if *asked == identity),
                ),
            Question::DeleteRetained(identity) => self.show_extensions_at(
                state,
                |entry| matches!(entry, Entry::AskDeleteRetained(asked) if *asked == identity),
            ),
            Question::DisableDependents(identity) => self.show_extensions_at(
                state,
                |entry| matches!(entry, Entry::Toggle(asked) if *asked == identity),
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
                state.next_screen();
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
        state.next_screen();
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
        // Its search in progress, if any, is stopped.
        state.searching = None;
        state.open = None;
        state.open_command = None;
        state.launch = LaunchRecord::default();
        state.form = None;
        state.next_screen();
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

    /// Has the open command in `component` handle `callback`, which the
    /// user chose, and shows its answer; once it answered (with an error of
    /// its own too), asks for its tree again and lists it, keeping the
    /// selection on the same item (ADR 0036's envelope).
    async fn run_action(
        &self,
        epoch: u64,
        component: PathBuf,
        callback: String,
        data: Option<PackageData>,
    ) {
        if let Some(problem) = self.updating(&component) {
            // As opening a command: its package's code is being replaced (an
            // update Pane applies by itself), which would stop the action the
            // user is about to wait on, so it is refused rather than started
            // and then stopped by the replacement.
            let mut state = self.lock();
            if state.screen_epoch == epoch {
                state.view.status = Status::Error(problem);
            }
            return;
        }
        let command = self.lock().open_command.clone();
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .handle_event_with(
                        &component,
                        command.as_deref(),
                        &callback,
                        "{}",
                        data.clone(),
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        // The command handled the event, whatever it answered: its tree may
        // have changed. A crash, or a call that was stopped, did not.
        let handled = matches!(
            result,
            Ok(_) | Err(CallError::Guest(_) | CallError::Unreadable(_))
        );
        {
            let Some(mut state) = self.lock_if_current(epoch) else {
                return;
            };
            let state = &mut *state;
            let ended = stopped(state, &component, &data);
            let list_again = handled && ended.is_none();
            state.view.status = match (ended, result) {
                // Stopped while it was running: its answer is not shown.
                (Some(problem), _) => Status::Error(problem),
                // The answer shows nothing: the command said what it had
                // to through a toast or a HUD (#141).
                (None, Ok(_)) => Status::Idle,
                // An error it answered with is a failure toast.
                (None, Err(CallError::Guest(message))) => {
                    self.show_failure(state, &component, command.as_deref(), message);
                    Status::Idle
                }
                (None, Err(error)) => Status::Error(error.to_string()),
            };
            if !list_again {
                return;
            }
        }
        self.list_again(epoch, component, data).await;
    }

    /// Asks the open command in `component` for its tree again, after it
    /// handled an event, and lists it while its screen is still the one on
    /// display, keeping the selection on the same item. The status stays the
    /// event's answer, unless the tree cannot be had, which is then shown
    /// with the list as it was.
    async fn list_again(&self, epoch: u64, component: PathBuf, data: Option<PackageData>) {
        // The record its screen was opened with: the same each time.
        let (launch, command) = {
            let state = self.lock();
            (state.launch.clone(), state.open_command.clone())
        };
        let answer = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .render_launched_with(&component, command.as_deref(), &launch, data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        let state = &mut *state;
        if state.open.as_ref() != Some(&component) {
            return;
        }
        if let Some(problem) = stopped(state, &component, &data) {
            state.view.status = Status::Error(problem);
            return;
        }
        match answer {
            Ok(view) => self.relist(state, &component, view),
            Err(error) => state.view.status = Status::Error(error.to_string()),
        }
    }

    /// Shows `view`, the open command's tree drawn again, keeping the
    /// selection on the same item when it is still listed (else at the same
    /// place). While the command's search field holds text, what it found
    /// stays listed, and `view` is kept for when the text is cleared.
    fn relist(&self, state: &mut State, component: &Path, view: View) {
        if search_files::browsing(state) {
            // Search Files lists the file index, not the command's own
            // list (#177).
            state.view.title = view.title;
            return;
        }
        let extra = looks::remember(state, component, &view.items);
        let list = self.command_list(state, component, view.items);
        let searching = match &state.view.screen {
            Screen::Command => false,
            Screen::CommandSearch { query } => !query.trim().is_empty(),
            _ => return,
        };
        self.report_extra_accessories(state, extra, false);
        state.view.title = view.title;
        if searching {
            if let Some(search) = state.searching.as_mut() {
                search.keep(list);
            }
            return;
        }
        let shown = state.view.selected;
        let selected_id = shown
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        let selected = selected_id
            .and_then(|id| list.rows.iter().position(|row| row.id == id))
            .or_else(|| {
                let last = list.rows.len().checked_sub(1)?;
                Some(shown?.min(last))
            });
        state.view.rows = list.rows;
        state.entries = list.entries;
        state.view.selected = selected;
        self.report_unbound(state);
    }

    /// The open command's own list for `items`, its tree's: the package's
    /// folder rows first when it may read a granted folder.
    fn command_list(&self, state: &State, component: &Path, items: Vec<Item>) -> CommandList {
        let mut list = CommandList::of(items);
        if let Some(package) = owner(&state.packages, component)
            && folder_access(package)
        {
            let (pane_rows, pane_entries) = files::folder_rows(state, &package.identity);
            list.rows.splice(0..0, pane_rows);
            list.entries.splice(0..0, pane_entries);
        }
        list
    }

    async fn open_command(&self, epoch: u64, opening: Opening, data: Option<PackageData>) {
        let Opening {
            component,
            command,
            search,
            launch,
            initial_search,
            ..
        } = opening;
        // Which command its screen is, for the preferences it reads.
        let launch = LaunchRecord {
            command: Some(command.clone()),
            ..launch
        };
        if let Some(problem) = self.updating(&component) {
            // Its package's code is being replaced (an update): opening
            // the command now would be stopped by the replacement, so it
            // is refused rather than interrupted.
            let mut state = self.lock();
            if state.screen_epoch == epoch {
                state.view.status = Status::Error(problem);
            }
            return;
        }
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .render_launched_with(&component, Some(command.as_str()), &launch, data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        // The launcher is unlocked before the search it opens with runs.
        let searching = {
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
            if end == Some(End::Paused) {
                // Paused before it was asked (by its hotkey), or while it was
                // opening (this opening crashed or could not start, which said
                // so).
                if state.view.status == Status::Running {
                    state.view.status = Status::Error(paused(&state, &component));
                }
                return;
            }
            let mut guard = state;
            let state = &mut *guard;
            let mut searching = None;
            match result {
                Ok(view) => {
                    let extra = looks::remember(state, &component, &view.items);
                    let CommandList { rows, entries } =
                        self.command_list(state, &component, view.items);
                    state.open_command = Some(command.clone());
                    let screen = if search {
                        state.searching = Some(command_search::Searching::new(command));
                        Screen::CommandSearch {
                            query: String::new(),
                        }
                    } else {
                        state.searching = None;
                        Screen::Command
                    };
                    state.entries = entries;
                    state.open = Some(component);
                    state.launch = launch;
                    state.next_screen();
                    state.view = LauncherView::new(screen, view.title).with_rows(rows);
                    state.reported_unbound = Vec::new();
                    // Pane's registered Files command lists the file index
                    // itself (#177): Recently Used, or the text it opened
                    // with.
                    let files = search
                        && match (state.open.clone(), state.open_command.clone()) {
                            (Some(component), Some(command)) => {
                                self.begin_search_files(state, &component, &command)
                            }
                            _ => false,
                        };
                    self.report_unbound(state);
                    self.report_extra_accessories(state, extra, true);
                    // A command whose screen is a form (#149) shows it at once;
                    // Back from it leaves the command.
                    if let Some(ScreenForm { id, form }) = view.form {
                        open_form_for(state, FormPurpose::Screen(id), form);
                    } else if let Some(text) = initial_search.filter(|_| search) {
                        // Opened with text in its field ("Search Files for
                        // “…”"): searched at once, as if typed.
                        searching = self.search_in_command(state, &text);
                    } else if files {
                        searching = self.ask_files(state, "", 0);
                    }
                }
                Err(error) => state.view.status = Status::Error(error.to_string()),
            }
            searching
        };
        if let Some(searching) = searching {
            searching.await;
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

    /// What an update or reload of the package with `identity` does to the
    /// old code once the new copy is recorded, before the old copy's folder
    /// is removed: its generation ends, which stops its pending calls (the
    /// new code runs in a new one), and its helpers are ended and reaped, so
    /// no running program keeps the folder in use (Windows would refuse to
    /// remove it).
    fn retire(&self, identity: &PackageIdentity) -> impl FnOnce(&Path) + Send + 'static {
        let data = self.installation.as_ref().map(|i| i.data.clone());
        let runtime = self.runtime().ok().cloned();
        let identity = identity.clone();
        move |old: &Path| {
            if let Some(data) = data {
                data.replace_code(&identity);
            }
            if let Some(runtime) = runtime {
                runtime.stop_helpers_in(old);
            }
        }
    }

    fn runtime(&self) -> Result<&Runtime, CallError> {
        self.runtime.as_ref().map_err(Clone::clone)
    }

    /// Locks the state only if the screen is still the one of `epoch`.
    fn lock_if_current(&self, epoch: u64) -> Option<MutexGuard<'_, State>> {
        let state = self.lock();
        (state.screen_epoch == epoch).then_some(state)
    }

    /// Locks the launcher's state. If a thread panicked while holding it
    /// (such as the runtime thread crashing while it noted a failure), the
    /// state is taken over and first put back into a known state (see
    /// [`Launcher::recover_state`]).
    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                self.state.clear_poison();
                self.recover_state(&mut state);
                state
            }
        }
    }

    /// Tells the window that the launcher changed in the background.
    /// Through the shared development configuration, so the threads started
    /// before `with_development` wired the channel — the updater, the
    /// scheduler, the services — reach the window as well (#49's smoke: the
    /// update applied and the status line never showed it).
    fn changed(&self) {
        self.developing.changed();
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
/// ("<title> is disabled"), its code replaced, it was uninstalled or Pane
/// paused it. `None` while it lasts, and for a command built into Pane.
fn stopped(state: &State, component: &Path, data: &Option<PackageData>) -> Option<String> {
    match data.as_ref()?.stopped()? {
        End::Disabled => Some(disabled(state, component)),
        End::Replaced => Some(CallError::Replaced.to_string()),
        End::Uninstalled => Some(CallError::Uninstalled.to_string()),
        End::Paused => Some(paused(state, component)),
        // The launcher's data is never fenced: only the runtime's copy is.
        End::Abandoned => None,
    }
}

/// "<title> is paused after an error; …", for the package `component`
/// belongs to.
fn paused(state: &State, component: &Path) -> String {
    match owner(&state.packages, component) {
        Some(package) => paused_reason(&package.title()),
        None => CallError::Paused.to_string(),
    }
}

/// "<title> is disabled", for the package `component` belongs to.
fn disabled(state: &State, component: &Path) -> String {
    match owner(&state.packages, component) {
        Some(package) => format!("{} is disabled", package.title()),
        None => CallError::Disabled.to_string(),
    }
}

/// Replaces the command view with `form`, which belongs to item `item_id`.
/// The command's row entries stay, for when the form closes.
fn open_form(state: &mut State, item_id: String, form: Form) {
    open_form_for(state, FormPurpose::Item(item_id), form);
}

/// Replaces the command view with `form`, which `purpose` submits: each
/// field starts with the value the tree gives it (a text field's text, a
/// choice's option), else empty or with the first option.
fn open_form_for(state: &mut State, purpose: FormPurpose, form: Form) {
    let fields = form
        .fields
        .into_iter()
        .map(|field| {
            let value = match &field.kind {
                FieldKind::Text { .. } | FieldKind::Password { .. } | FieldKind::Path { .. } => {
                    field.value.clone().unwrap_or_default()
                }
                FieldKind::Choice(choices) => field
                    .value
                    .as_ref()
                    .filter(|value| choices.iter().any(|choice| &choice.id == *value))
                    .or_else(|| choices.first().map(|choice| &choice.id))
                    .cloned()
                    .unwrap_or_default(),
            };
            FormField {
                id: field.id,
                label: field.label,
                kind: field.kind,
                value,
                error: None,
                description: None,
                required: false,
            }
        })
        .collect();
    let form_view = LauncherView::new(
        Screen::Form(FormView {
            fields,
            submit_label: form.submit_label,
            setup: None,
        }),
        form.title,
    );
    let return_to = std::mem::replace(&mut state.view, form_view);
    state.form = Some(OpenForm {
        purpose,
        return_to,
        submitting: false,
    });
    state.next_screen();
}

/// The user's home folder, which `~` in a typed path resolves to
/// (#195), as the environment names it: `USERPROFILE` on Windows, `HOME`
/// elsewhere. The file index's configuration replaces it with the home it
/// is built over (see [`Launcher::with_file_index`]), which is the same
/// folder on a real Pane; the typed folder the user names is resolved with
/// the same home ([`crate::typed_folder`]).
pub(crate) fn home_folder() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from)
}

/// When a query is asked about, by the clock root search's own dates are
/// shown by (a test's, in a test): the moment the user stopped at the
/// query, and how far the local time there is from UTC, so a command can
/// answer about the local date or time (#196). The whole search shares
/// one moment, whatever a slow command delays.
fn asked_at(state: &State) -> WallTime {
    let milliseconds = state.clock.now();
    WallTime {
        milliseconds,
        offset: state.clock.local_offset(milliseconds),
    }
}

/// What a listed row can show to tell itself apart from another row
/// with the same folded title (#197): an installed command names its
/// package's source after its subtitle, as the send rows already do. An
/// indexed result's own subtitle is what its provider gave to tell it
/// apart (an application's distinction, a quicklink's target), a
/// computed result's section names the command that computed it, and a
/// file row shows its folder, so none of them adds anything.
enum Told {
    /// The package the command was installed from.
    Source(PackageIdentity),
    /// Nothing: the row already tells itself apart.
    Apart,
}

/// What kind of thing a root result is, as the comparator ranks kinds:
/// commands above links, above applications, above files. Folders,
/// fallbacks and the like are never ranked by the comparator — they keep
/// their places below the results — so they take the lowest kind, which
/// the order names for files.
fn kind(entry: &Entry) -> search::Kind {
    match presentation::kind(entry) {
        Some(presentation::RowKind::Command) => search::Kind::Command,
        Some(presentation::RowKind::Link) => search::Kind::Link,
        Some(presentation::RowKind::Application) => search::Kind::Application,
        _ => search::Kind::File,
    }
}

/// The rows of root search for `query`, and what activating each does: the
/// results computed from it, then the root results matching it, in the
/// comparator's order — those supplied ahead of the query ranked with
/// them, also for the blank query, whose own order is the no-query
/// order (#199) — then the rows declared for the address or path the
/// query is, then the computed results that open a file, then the rows
/// explaining why a command could not supply them.
fn root_rows(state: &State, query: &str) -> (Vec<Row>, Vec<Entry>) {
    let blank = query.trim().is_empty();
    // What the comparator ranks: the root results — Pane's own rows and
    // every package's commands, one provider in the order root search
    // lists them — then each command that supplies results ahead of the
    // query, in the order they were first asked. The blank query ranks
    // them too (#199): what the user opens most rises to the top.
    let candidates: Vec<(&RootResult, search::Kind, usize)> = state
        .root
        .iter()
        // A command declared for URL-like or path-like queries is never
        // matched by title: it is listed for such a query alone, below
        // (see `typed_query`).
        .filter(|result| result.matches == CommandMatches::Title)
        // A command may show with a blank query alone, or only while the
        // user searches (its `when`, #195).
        .filter(|result| result.when.listed(blank))
        .map(|result| (result, kind(&result.entry), 0))
        .chain(
            state
                .indexes
                .grouped_results()
                .map(|(provider, result)| (result, kind(&result.entry), provider + 1)),
        )
        .collect();
    // What root search learned about them, as ranking sees it now
    // (#199): each recorded result's decayed frecency and counting
    // queries, by its row id — a command's or an indexed result's own.
    let learned = state.learned.chosen.ranked(state.clock.now());
    let keys = |&(result, kind, provider)| Candidate {
        keys: &result.keys,
        kind,
        provider,
        learned: learned.get(&result.row.id),
    };
    let parsed = Query::new(query);
    let named = |index: &usize| parsed.is_alias_of(&candidates[*index].0.keys);
    let matches: Vec<usize> =
        search::ranked_matches(&parsed, candidates.iter().map(keys), state.sensitivity)
            .into_iter()
            .filter(|index| !named(index))
            .collect();
    // A command the query names by its alias is hoisted above every
    // other row, computed results included.
    let by_alias: Vec<usize> = (0..candidates.len()).filter(named).collect();
    // The row a listed candidate becomes, with what it can show to tell
    // itself apart: an installed command's package source.
    let found = |index: usize| {
        let told = candidates[index]
            .0
            .target
            .as_ref()
            .map(|target| Told::Source(target.identity.clone()))
            .unwrap_or(Told::Apart);
        (
            candidates[index].0.row.clone(),
            candidates[index].0.entry.clone(),
            told,
        )
    };
    // A command the query names by its alias is launched from its alias.
    let by_its_alias = |index: usize| {
        let (row, entry, told) = found(index);
        let entry = match entry {
            Entry::Open(mut opening) => {
                opening.launch.source = LaunchSource::Alias;
                Entry::Open(opening)
            }
            entry => entry,
        };
        (row, entry, told)
    };
    let failures = state
        .indexes
        .failures()
        .filter(|_| !blank)
        .map(|(row, entry)| (row.clone(), entry.clone()));
    let (files, computed): (Vec<&Computed>, Vec<&Computed>) = state
        .computed
        .iter()
        .partition(|computed| computed.in_files);
    let computed_row =
        |computed: &Computed| (computed.row.clone(), computed.entry.clone(), Told::Apart);
    let apart = |(row, entry)| (row, entry, Told::Apart);
    // What the user's alias names comes first, even before computed
    // results; files found for the query follow what is found by title,
    // since a folder can hold many; the fallbacks, which the user chooses
    // when anything else is listed, come last.
    let listed: Vec<(Row, Entry, Told)> = aliases::rows_sending_after_alias(state, query)
        .into_iter()
        .map(apart)
        .chain(by_alias.into_iter().map(by_its_alias))
        .chain(computed.into_iter().map(computed_row))
        .chain(matches.into_iter().map(found))
        // The rows declared for the address or path the query is (#195),
        // below the results found by title and above the files.
        .chain(typed_query::rows(state).into_iter().map(apart))
        .chain(files.into_iter().map(computed_row))
        .chain(failures.map(apart))
        .chain(aliases::fallback_rows(state, query).into_iter().map(apart))
        .collect();
    // Rows that share a folded title each show what tells them apart.
    let folded: Vec<String> = listed
        .iter()
        .map(|(row, _, _)| search::fold(&row.title))
        .collect();
    let mut rows: Vec<Row> = Vec::with_capacity(listed.len());
    let mut entries: Vec<Entry> = Vec::with_capacity(listed.len());
    for ((row, entry, told), title) in listed.into_iter().zip(&folded) {
        let shared = folded.iter().filter(|other| *other == title).count() > 1;
        let row = match (told, shared) {
            (Told::Source(identity), true) => Row {
                subtitle: Some(match row.subtitle {
                    Some(subtitle) => format!("{subtitle} · {identity}"),
                    None => identity.to_string(),
                }),
                ..row
            },
            _ => row,
        };
        rows.push(row);
        entries.push(entry);
    }
    (rows, entries)
}

/// Lists root search's rows for `query` again after results arrived: the
/// best match stays selected, now that better ones may be first, and a row
/// the user moved to stays selected. The query's list must be published
/// (#201): while it is held, the rows shown stay the previous query's.
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
        .or_else(|| aliases::first_choice(&entries));
    state.view.rows = rows;
    state.entries = entries;
}

/// The rows for `command`'s `answer` to `query`: its results, or one
/// explaining why it failed. A result that opens a file is shown with the
/// file's own name and folder, as the host found it in the latest listing
/// of `owner`'s granted folder or in the folder the user typed (#204),
/// whatever the extension titled it; one the host does not know is left
/// out.
fn computed_results(
    command: CommandRegistration,
    owner: Option<&str>,
    files: Option<&FileAccess>,
    query: &str,
    answer: Result<Vec<ComputedResult>, CallError>,
) -> Vec<Computed> {
    let computed = |row: Row, entry: Entry, detail: Option<ComputedDetail>| Computed {
        component: command.component.clone(),
        query: query.to_owned(),
        command_title: command.title.clone(),
        answer: detail,
        in_files: matches!(entry, Entry::File(_)),
        row,
        entry,
    };
    match answer {
        Ok(results) => {
            // At most `file_search::ROOT_FILE_ROWS` of the file index's
            // entries, then a row searching them all (#175).
            let mut indexed = 0;
            let mut listed: Vec<Computed> = results
                .into_iter()
                .filter_map(|result| {
                    let ComputedResult {
                        listing,
                        action,
                        answer,
                    } = result;
                    // Only a result whose action copies is an answer's
                    // card: a detail on another action is not shown.
                    let detail = answer.filter(|_| matches!(action, RootAction::Copy(_)));
                    let (listing, entry) = match action {
                        RootAction::Copy(text) => (listing, Entry::Copy(text)),
                        RootAction::OpenUrl(url) => (listing, Entry::OpenUrl(url)),
                        RootAction::OpenFile(id) => {
                            let row_id = format!("{}:{}", command.id, listing.id);
                            let (row, file) =
                                files::file_row(files?, owner?, &command.component, id, row_id)?;
                            if file.indexed {
                                if indexed == file_search::ROOT_FILE_ROWS {
                                    return None;
                                }
                                indexed += 1;
                            }
                            return Some(computed(row, Entry::File(file), None));
                        }
                    };
                    Some(computed(
                        Row::listed(listing, Some(&command.id)),
                        entry,
                        detail,
                    ))
                })
                .collect();
            if indexed > 0
                && let Some((row, entry)) = file_search::search_all_row(&command, query)
            {
                listed.push(Computed {
                    in_files: true,
                    ..computed(row, entry, None)
                });
            }
            // A typed folder that holds more entries than Pane lists says
            // so with a row of its own at the end of them (#204).
            let typed = listed
                .iter()
                .any(|computed| matches!(&computed.entry, Entry::File(file) if file.typed));
            let partial = match (files, owner) {
                (Some(files), Some(owner)) => files.typed().partial(owner),
                _ => false,
            };
            if typed && partial {
                listed.push(Computed {
                    in_files: true,
                    ..computed(
                        Row {
                            id: format!("{}:typed-more", command.id),
                            title: "…and more entries".into(),
                            subtitle: Some(format!(
                                "Pane lists a folder's first {} entries",
                                crate::typed_folder::MAX_ENTRIES
                            )),
                            unavailable: None,
                        },
                        // The row cannot be activated: it says something,
                        // it does not do anything.
                        Entry::NoActions,
                        None,
                    )
                });
            }
            listed
        }
        Err(error) => {
            let row = Row {
                id: format!("{}:failed", command.id),
                title: command.title.clone(),
                subtitle: Some(format!("Could not answer: {error}")),
                unavailable: None,
            };
            let problem = format!("{} could not answer “{query}”: {error}", command.title);
            vec![computed(row, Entry::Broken(problem), None)]
        }
    }
}

/// What resolves once a granted folder's listing ends.
type ListedFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A command whose answer waited for its granted folder's listing, its
/// data, and what resolves once the listing ends.
type Listing = (CommandRegistration, Option<PackageData>, ListedFuture);

/// Whether `package` asks for access to a folder the user grants it.
fn folder_access(package: &InstalledPackage) -> bool {
    package
        .manifest
        .as_ref()
        .is_ok_and(|manifest| manifest.folder_access)
}

/// Awaits `call`, unless `cancelled` resolves first (its sender was used or
/// dropped): then `call` is dropped unfinished, and the answer is `None`.
async fn until_cancelled<T>(
    call: impl Future<Output = T>,
    cancelled: &mut tokio::sync::oneshot::Receiver<()>,
) -> Option<T> {
    let mut call = std::pin::pin!(call);
    std::future::poll_fn(|cx| {
        if Pin::new(&mut *cancelled).poll(cx).is_ready() {
            return std::task::Poll::Ready(None);
        }
        call.as_mut().poll(cx).map(Some)
    })
    .await
}

/// The components with a live instance, once the runtime handled every
/// request sent before this; `None` when it cannot say (it stopped, or its
/// thread failed) or does not within [`crate::runtime::UNRESPONSIVE_LIMIT`]
/// (a guest call ahead of it waits for long), so that a management action
/// waiting on it never waits forever.
async fn runtime_barrier(runtime: &Runtime) -> Option<Vec<PathBuf>> {
    let running = runtime.try_running();
    let timer = off_thread(|| std::thread::sleep(crate::runtime::UNRESPONSIVE_LIMIT));
    let (mut running, mut timer) = (std::pin::pin!(running), std::pin::pin!(timer));
    std::future::poll_fn(|cx| {
        if let std::task::Poll::Ready(running) = running.as_mut().poll(cx) {
            return std::task::Poll::Ready(running.ok());
        }
        timer.as_mut().poll(cx).map(|()| None)
    })
    .await
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
