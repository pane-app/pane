//! The choice of a collection's extensions (#308, ADR 0044). A Git
//! repository — or local folder — whose root holds `pane-collection.json`
//! is a collection, and naming it, without an extension's id, opens the
//! choice: the extensions it offers, each listed with what its own
//! manifest says of it (its icon, title, description and version; the
//! index holds none of those), none ticked. The user ticks some or all of
//! them — Space and the row's check mark tick the selected extension, the
//! arrows move the selection as any list's — and chooses Install: each
//! ticked extension is then previewed and installed as any extension's
//! own install is, one after another, in the collection's order, with its
//! own identity, managed copy and record.
//!
//! Choosing several is a convenience, never a unit (ADR 0044): one that
//! cannot be installed is explained and the others continue, and when the
//! run ends the choice lists what was installed and what was not.
//! Nothing of the choice itself is recorded. Activating an extension's
//! row opens the ordinary preview of that one extension, whose Back
//! returns to the choice, before anything is installed.
//!
//! For a collection fetched from Git, the revision Pane fetched is held
//! while the choice is open: every extension is previewed and installed
//! from that one revision, so however many are ticked, the revision is
//! fetched once.
//!
//! First setup's choice of the official extensions (#283, ADR 0045) is
//! the same screen over the default extensions' pins: each listed with
//! the title and description its pin gives (nothing is fetched to list
//! them), all of them ticked — they are the defaults — and the Set Up
//! row after them, selected, so Enter sets the defaults up. Activating an
//! extension's row ticks or unticks it, as Space does; Set Up records the
//! choice (what was unticked is declined) and returns to root search,
//! where the ticked ones are set up as first setup sets them up
//! (`acquire`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use super::install::{self, Request};
use super::{
    Entry, Launcher, LauncherView, Pending, Row, Screen, State, Status, first_index, off_thread,
};
use crate::git::GitOrigin;
use crate::icons::Icon;
use crate::platform;

/// The id of the choice's Install row, after the extension rows.
const INSTALL_ROW: &str = "install";

/// The id of first setup's Set Up row, after the extension rows (#283).
const SET_UP_ROW: &str = "set-up";

/// First setup's title (#283).
const FIRST_SETUP_TITLE: &str = "Pane's Official Extensions";

/// The lines under first setup's title (#283): where the extensions come
/// from, and that what is left out stays available.
const FIRST_SETUP_LINES: [&str; 2] = [
    "Pane sets these up from their own repositories, at the releases this version of Pane \
     was tested with",
    "Untick what you do not want: you can install it later from the Extensions group in \
     Settings",
];

/// One extension of a collection as the choice lists it: the id that names
/// it, and what its own manifest says of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceExtension {
    /// The id that names the extension in its collection (ADR 0044): the
    /// part of its identity after `#`, and the id its row carries.
    pub id: String,
    /// Its title, from its own manifest; the id where the manifest cannot
    /// be read (the row is explained when its preview is opened).
    pub title: String,
    /// Its description, from its own manifest.
    pub description: Option<String>,
    /// Its version, from its own manifest.
    pub version: Option<String>,
    /// Its icon, from its own manifest, resolved against its folder in
    /// the collection; a first-letter tile of its title where it has none.
    pub icon: Icon,
}

/// What the run of the ticked extensions came to for one of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceOutcome {
    /// The run is installing it now.
    Installing,
    /// It was installed.
    Installed,
    /// It was not, and why.
    Refused(String),
}

/// What the choice screen shows: the extensions to tick, which are ticked,
/// what the run came to for each, and the lines under the title — the
/// collection's own, or, while the run installs one, that extension's
/// preview of what it uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceView {
    /// The extensions, in the collection's order: the rows before the
    /// Install row.
    pub extensions: Vec<ChoiceExtension>,
    /// Which are ticked, aligned with `extensions`.
    pub ticked: Vec<bool>,
    /// What the run came to, aligned with `extensions`: `None` where it
    /// has not reached an extension, and everywhere before it begins.
    pub outcomes: Vec<Option<ChoiceOutcome>>,
    /// The lines under the title.
    pub details: Vec<String>,
}

/// What a choice lists: a collection's extensions (#308), each read and
/// installed from where the collection is, or first setup's official
/// extensions (#283), set up by their pins once the choice is made.
#[derive(Clone)]
enum Of {
    Collection(Source),
    FirstSetup,
}

/// A collection as the choice reads it (#308), or first setup's official
/// extensions (#283): what the choice lists, and what the screen shows of
/// it.
pub(in crate::launcher) struct Listed {
    /// What the choice lists: a collection, with how each extension is
    /// read and installed, or first setup's official extensions.
    of: Of,
    /// The screen's title: the repository's or folder's name.
    title: String,
    /// The lines under the title: where the collection came from and how
    /// the list is used.
    details: Vec<String>,
    /// The extensions, in the collection's order.
    extensions: Vec<ChoiceExtension>,
}

/// Where a collection is, and how each of its extensions is read and
/// installed.
#[derive(Clone)]
pub(in crate::launcher) enum Source {
    /// The local folder holding it.
    Folder(PathBuf),
    /// The revision Pane fetched from Git and holds while the choice is
    /// open: each extension read from it with its Git identity and origin
    /// (ADR 0044), never fetched again. Where the server filters (#311),
    /// `partial` is the fetch the revision was fetched in parts by: the
    /// files of the extensions chosen are fetched from it, into the same
    /// download; `None` where the whole revision was fetched.
    Fetched {
        download: Arc<crate::downloads::Download>,
        origin: GitOrigin,
        partial: Option<Arc<crate::git::PartialFetch>>,
    },
}

impl Source {
    /// The partial fetch the revision was fetched in parts by, where the
    /// server filters (#311): the files of the extensions chosen from the
    /// revision are fetched from it; `None` where the whole revision was
    /// fetched, and for a local folder.
    fn partial(&self) -> Option<&Arc<crate::git::PartialFetch>> {
        match self {
            Source::Folder(_) => None,
            Source::Fetched { partial, .. } => partial.as_ref(),
        }
    }

    /// The request that reads and installs the extension `id`: as the
    /// choice's own preview and the run's installs name it.
    fn request(&self, id: &str) -> Request {
        match self {
            Source::Folder(folder) => Request::Collection(folder.clone(), id.to_owned()),
            Source::Fetched {
                download,
                origin,
                partial,
            } => Request::FetchedExtension {
                download: download.clone(),
                origin: origin.clone(),
                id: id.to_owned(),
                partial: partial.clone(),
            },
        }
    }
}

impl Listed {
    /// The collection `collection` at the local folder `root`, as the
    /// choice lists it: each extension's own manifest read for what its
    /// row shows of it.
    pub(in crate::launcher) fn folder(
        root: &Path,
        collection: &crate::collections::Collection,
    ) -> Listed {
        Listed {
            of: Of::Collection(Source::Folder(root.to_path_buf())),
            title: crate::packages::folder_name(root),
            details: vec![
                format!("Source: local folder {}", root.display()),
                HOW.into(),
            ],
            extensions: extensions_of(root, collection),
        }
    }

    /// The collection `collection` at the root of the revision Pane
    /// fetched from Git and holds (`download`), as the choice lists it.
    /// `origin` is where the revision was fetched from, recorded with each
    /// extension installed from it, and `partial` is the fetch it was
    /// fetched in parts by, where the server filters (#311): the files of
    /// the extensions chosen are fetched from it, into the same download.
    pub(in crate::launcher) fn git(
        download: Arc<crate::downloads::Download>,
        origin: &GitOrigin,
        partial: Option<Arc<crate::git::PartialFetch>>,
        collection: &crate::collections::Collection,
    ) -> Listed {
        let repository = origin.repository.name();
        let mut details = vec![format!("Source: Git repository {repository}")];
        details.extend(install::git_lines(origin, None));
        details.push(HOW.into());
        let folder = download.folder().to_path_buf();
        Listed {
            of: Of::Collection(Source::Fetched {
                download,
                origin: origin.clone(),
                partial,
            }),
            // The repository's own name, as a local collection's title is
            // its folder's: the details say where the collection is.
            title: crate::packages::folder_name(std::path::Path::new(repository)),
            details,
            extensions: extensions_of(&folder, collection),
        }
    }

    /// First setup's choice of the official extensions `pins` names, the
    /// default extensions this system sets up (#283): each listed by what
    /// its pin says — its title, its description and the release it sets
    /// up — before anything is fetched, with a first-letter tile.
    pub(in crate::launcher) fn first_setup(pins: &[crate::defaults::DefaultExtension]) -> Listed {
        Listed {
            of: Of::FirstSetup,
            title: FIRST_SETUP_TITLE.into(),
            details: FIRST_SETUP_LINES
                .iter()
                .map(|line| (*line).into())
                .collect(),
            extensions: pins
                .iter()
                .map(|pin| ChoiceExtension {
                    id: pin.id.clone(),
                    title: pin.title.clone(),
                    description: pin.description.clone(),
                    version: Some(pin.tag.trim_start_matches('v').to_owned()),
                    icon: Icon::letter_of(&pin.title),
                })
                .collect(),
        }
    }
}

/// The line the choice's details end with: how the list is used, and that
/// choosing several is a convenience, not a unit (ADR 0044).
const HOW: &str = "Tick the extensions to install: each one installs on its own, with its own \
                   preview and record";

/// The extensions of `collection` at `root`, one choice row each.
fn extensions_of(root: &Path, collection: &crate::collections::Collection) -> Vec<ChoiceExtension> {
    collection
        .extensions()
        .iter()
        .map(|entry| {
            let listed = crate::packages::ListedExtension::of(root, entry);
            let title = listed.title.unwrap_or_else(|| entry.id.clone());
            ChoiceExtension {
                id: entry.id.clone(),
                icon: listed.icon.unwrap_or_else(|| Icon::letter_of(&title)),
                title,
                description: listed.description,
                version: listed.version,
            }
        })
        .collect()
}

/// The choice in progress: what its collection listed, which of its
/// extensions are ticked, and where the run of the ticked ones stands.
/// Held while the choice screen — or the preview of one of its
/// extensions, opened from it — is on show.
pub(in crate::launcher) struct Choice {
    of: Of,
    title: String,
    details: Vec<String>,
    extensions: Vec<ChoiceExtension>,
    /// Which are ticked, aligned with `extensions`: none, at first, for a
    /// collection; every one, the defaults, for first setup.
    ticked: Vec<bool>,
    /// The run of the ticked extensions, once Install begins it.
    run: Option<Run>,
}

impl Choice {
    /// The choice of `listed`'s extensions, no run: a collection's with
    /// none ticked, first setup's with every one ticked — they are the
    /// default extensions (#283).
    fn listing(listed: Listed) -> Choice {
        let Listed {
            of,
            title,
            details,
            extensions,
        } = listed;
        let ticked = matches!(of, Of::FirstSetup);
        Choice {
            of,
            title,
            details,
            ticked: vec![ticked; extensions.len()],
            extensions,
            run: None,
        }
    }

    /// The request that reads and installs the extension `id`, as the
    /// choice's own preview and the run's installs name it; `None` where
    /// the collection lists no such extension, and for first setup, whose
    /// extensions are set up by their pins (#283).
    fn request(&self, id: &str) -> Option<Request> {
        let Of::Collection(source) = &self.of else {
            return None;
        };
        self.extensions
            .iter()
            .any(|extension| extension.id == id)
            .then(|| source.request(id))
    }

    /// Whether this is first setup's choice of the official extensions
    /// (#283).
    pub(in crate::launcher) fn is_first_setup(&self) -> bool {
        matches!(self.of, Of::FirstSetup)
    }

    /// Whether the extension `id` is ticked.
    pub(in crate::launcher) fn ticks(&self, id: &str) -> bool {
        self.extensions
            .iter()
            .zip(&self.ticked)
            .any(|(extension, ticked)| extension.id == id && *ticked)
    }
}

/// The run of the choice's ticked extensions, as the choice keeps it.
struct Run {
    /// Whether the run is still going: while it is, the extensions' rows
    /// are not previewed and nothing is ticked.
    going: bool,
    /// What the run came to, aligned with `Choice::extensions`.
    outcomes: Vec<Option<ChoiceOutcome>>,
    /// The preview lines on show under the title: the extension the run
    /// is installing, as any extension's preview shows what it uses.
    details: Vec<String>,
}

/// The run the Install row begins: where the collection is, and which of
/// its extensions were ticked, in the collection's order.
pub(in crate::launcher) struct Begun {
    source: Source,
    ids: Vec<String>,
}

impl Launcher {
    /// Shows the choice of the collection `listed` (#308): the extensions
    /// it offers, none ticked, the Install row after them.
    pub(in crate::launcher) fn show_choice(&self, state: &mut State, listed: Listed) {
        self.leave_command(state);
        state.view.selected = None;
        state.view.status = Status::Idle;
        let first_setup = matches!(listed.of, Of::FirstSetup);
        let rows = listed.extensions.len();
        state.choice = Some(Choice::listing(listed));
        // First setup's Set Up row is selected, after the extensions, so
        // Enter sets the defaults up (#283).
        if first_setup {
            state.view.selected = Some(rows);
        }
        self.choice_view(state);
    }

    /// The choice screen, as the choice in progress now lists it: one row
    /// per extension, and the Install row after them while no run is
    /// going; once it is, the rows stay and each says what the run came
    /// to for it. Keeps the row selected.
    pub(in crate::launcher) fn choice_view(&self, state: &mut State) {
        let Some(choice) = state.choice.as_ref() else {
            return;
        };
        let mut rows = Vec::with_capacity(choice.extensions.len() + 1);
        let mut entries = Vec::with_capacity(choice.extensions.len() + 1);
        let first_setup = choice.is_first_setup();
        for extension in &choice.extensions {
            rows.push(Row {
                id: extension.id.clone(),
                title: extension.title.clone(),
                subtitle: extension.description.clone(),
                unavailable: None,
            });
            // First setup's rows tick (#283); a collection's preview.
            entries.push(if first_setup {
                Entry::TickChoice(extension.id.clone())
            } else {
                Entry::PreviewChoice(extension.id.clone())
            });
        }
        if first_setup {
            let ticked = choice.ticked.iter().filter(|ticked| **ticked).count();
            let subtitle = match ticked {
                0 => "Set up none of them: Pane starts with no extensions".into(),
                1 => "Set up the ticked extension".into(),
                ticked => format!("Set up the {ticked} ticked extensions"),
            };
            rows.push(Row {
                id: SET_UP_ROW.into(),
                title: "Set Up".into(),
                subtitle: Some(subtitle),
                unavailable: None,
            });
            entries.push(Entry::SetUpDefaults);
        } else if choice.run.is_none() {
            let ticked = choice.ticked.iter().filter(|ticked| **ticked).count();
            let subtitle = match ticked {
                0 => "Tick the extensions to install".into(),
                1 => "Copy the ticked extension into Pane and add its commands".into(),
                ticked => {
                    format!("Copy the {ticked} ticked extensions into Pane and add their commands")
                }
            };
            rows.push(Row {
                id: INSTALL_ROW.into(),
                title: "Install".into(),
                subtitle: Some(subtitle),
                unavailable: None,
            });
            entries.push(Entry::InstallChoice);
        }
        // While the run installs one extension, its preview is on show
        // under the title; the collection's own lines show before it
        // begins, and the last preview the run showed stays once it ends.
        // The rows say what came of each extension as the run came to it,
        // from its first refusal — one refused before its preview shows,
        // with no preview lines — as from its installs.
        let (outcomes, details) = match &choice.run {
            Some(run) => (run.outcomes.clone(), run.details.clone()),
            _ => (vec![None; choice.extensions.len()], choice.details.clone()),
        };
        // Before the run has shown a preview, the collection's own lines
        // stay under the title.
        let details = if details.is_empty() {
            choice.details.clone()
        } else {
            details
        };
        let selected = state
            .view
            .selected
            .filter(|at| *at < rows.len())
            .or_else(|| first_index(&rows));
        let status = state.view.status.clone();
        state.entries = entries;
        state.view = LauncherView {
            rows,
            selected,
            status,
            ..LauncherView::new(
                Screen::Choice(ChoiceView {
                    extensions: choice.extensions.clone(),
                    ticked: choice.ticked.clone(),
                    outcomes,
                    details,
                }),
                choice.title.clone(),
            )
        };
    }

    /// Ticks or unticks the extension of the open choice whose id is
    /// `id`, selecting its row. Whether there was one to tick, and no run
    /// of the ticked extensions is going.
    pub fn toggle_choice_tick(&self, id: &str) -> bool {
        let mut state = self.lock();
        self.tick_choice(&mut state, id)
    }

    /// Ticks or unticks the extension `id` of the open choice, as
    /// [`Launcher::toggle_choice_tick`] does: what Space, the check mark
    /// and — on first setup's choice (#283) — the row itself do.
    pub(in crate::launcher) fn tick_choice(&self, state: &mut State, id: &str) -> bool {
        let Some(choice) = state.choice.as_mut() else {
            return false;
        };
        if choice.run.is_some() {
            return false;
        }
        let Some(at) = choice
            .extensions
            .iter()
            .position(|extension| extension.id == id)
        else {
            return false;
        };
        choice.ticked[at] = !choice.ticked[at];
        // A refusal that asked for ticks answered itself once one is made.
        if matches!(state.view.status, Status::Error(_)) {
            state.view.status = Status::Idle;
        }
        state.view.selected = Some(at);
        self.choice_view(state);
        true
    }

    /// First setup's Set Up row (#283): records the choice — each default
    /// left unticked is declined, as an uninstall declines one — and
    /// returns to root search, where the ticked ones are set up in the
    /// background. The launcher holds the choice as made at once, so
    /// root search no longer offers it. [`Pending`] for the activation.
    pub(in crate::launcher) fn set_up_chosen(&self, state: &mut State) -> Pending {
        let Some(choice) = state
            .choice
            .as_ref()
            .filter(|choice| choice.is_first_setup())
        else {
            return Pending::Nothing;
        };
        if self.installation.is_none() {
            state.view.status = Status::Error("this launcher does not install packages".into());
            return Pending::Nothing;
        }
        let declined: Vec<String> = choice
            .extensions
            .iter()
            .zip(&choice.ticked)
            .filter(|(_, ticked)| !**ticked)
            .map(|(extension, _)| extension.id.clone())
            .collect();
        state.first_setup.choose(&declined);
        state.view.status = Status::Idle;
        self.show_root(state, None);
        Pending::FirstSetup(declined)
    }

    /// The id of the extension of the open choice whose row is selected,
    /// if one is: what Space ticks (the Install row selects none).
    pub fn selected_choice_extension(&self) -> Option<String> {
        let state = self.lock();
        let Screen::Choice(choice) = &state.view.screen else {
            return None;
        };
        state
            .view
            .selected
            .and_then(|at| choice.extensions.get(at))
            .map(|extension| extension.id.clone())
    }

    /// The ordinary preview of the extension `id` of the open choice, as
    /// activating its row opens it (#308): the preview any extension's
    /// install shows, whose Back returns to the choice. [`Pending`] for
    /// the activation.
    pub(in crate::launcher) fn preview_choice(&self, state: &mut State, id: &str) -> Pending {
        let Some(choice) = state.choice.as_ref() else {
            return Pending::Nothing;
        };
        if choice.run.as_ref().is_some_and(|run| run.going) {
            // The ticked extensions are being installed; nothing else is
            // previewed meanwhile.
            state.view.status = Status::Error("The ticked extensions are being installed".into());
            return Pending::Nothing;
        }
        let Some(request) = choice.request(id) else {
            return Pending::Nothing;
        };
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        Pending::PreviewChoice(request)
    }

    /// Begins installing every ticked extension of the open choice, one
    /// after another in the collection's order: the Install row.
    /// [`Pending`] for the activation.
    pub(in crate::launcher) fn begin_choice_run(&self, state: &mut State) -> Pending {
        if self.installation.is_none() {
            state.view.status = Status::Error("this launcher does not install packages".into());
            return Pending::Nothing;
        }
        let Some(choice) = state.choice.as_mut() else {
            return Pending::Nothing;
        };
        let Of::Collection(source) = &choice.of else {
            return Pending::Nothing;
        };
        let source = source.clone();
        let ids: Vec<String> = choice
            .extensions
            .iter()
            .zip(&choice.ticked)
            .filter(|(_, ticked)| **ticked)
            .map(|(extension, _)| extension.id.clone())
            .collect();
        if ids.is_empty() {
            state.view.status =
                Status::Error("No extensions are ticked: tick the ones to install".into());
            return Pending::Nothing;
        }
        let count = choice.extensions.len();
        choice.run = Some(Run {
            going: true,
            outcomes: vec![None; count],
            details: Vec::new(),
        });
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        self.choice_view(state);
        Pending::ChoiceRun(Begun { source, ids })
    }

    /// Runs the choice's ticked extensions (`begun`, from the Install
    /// row), one after another, in the collection's order: each is
    /// previewed — the lines any extension's preview shows of what it
    /// uses — and then installed as its own package, with its own
    /// identity and record. One that cannot be installed is explained,
    /// and the others continue; when the run ends, the choice lists what
    /// was installed and what was not.
    ///
    /// The user may leave the choice while the run goes on: the
    /// extensions are installed either way, and only the screens the run
    /// would have shown are skipped.
    pub(in crate::launcher) async fn run_choice(&self, epoch: u64, begun: Begun) {
        let Begun { source, ids } = begun;
        let Some(store) = self
            .installation
            .as_ref()
            .map(|installation| installation.store.clone())
        else {
            return;
        };
        // Where the revision was fetched without its file contents (a
        // collection from a server that filters, #311), the ticked
        // extensions' files are fetched together before the run reads any
        // of them: one fetch serves every extension chosen from the same
        // revision (ADR 0044). A failure of that one fetch is what each
        // ticked extension is refused with — reading one would only find
        // its files missing, which would say source-only — and the rest
        // of the run is not reached.
        let mut refused = None;
        if let Some(partial) = source.partial() {
            let wanted = ids.clone();
            let partial = partial.clone();
            refused = off_thread(move || partial.fetch_extensions(&wanted))
                .await
                .err();
        }
        match refused {
            Some(why) => {
                for id in &ids {
                    self.note_choice(epoch, id, ChoiceOutcome::Refused(why.clone()));
                }
            }
            None => {
                for id in &ids {
                    let request = source.request(id);
                    match self
                        .install_choice_extension(epoch, &store, id, &request)
                        .await
                    {
                        Step::Installed => self.note_choice(epoch, id, ChoiceOutcome::Installed),
                        Step::Refused(why) => {
                            self.note_choice(epoch, id, ChoiceOutcome::Refused(why))
                        }
                    }
                }
            }
        }
        // The ending: the choice lists what was installed and what was
        // not, and the status line says which came of the run. The lines
        // under the title stay as the run left them — the preview of the
        // last extension it installed, as any extension's preview shows
        // what it uses — while the rows carry what came of each.
        let mut state = self.lock();
        if state.screen_epoch == epoch
            && state
                .choice
                .as_ref()
                .is_some_and(|choice| choice.run.is_some())
        {
            // The run is over: what its rows came to stay.
            if let Some(choice) = state.choice.as_mut()
                && let Some(run) = &mut choice.run
            {
                run.going = false;
            }
            state.view.status = state.choice.as_ref().map(ending).unwrap_or(Status::Idle);
            self.choice_view(&mut state);
        } else {
            self.refresh(&mut state);
        }
        self.changed();
    }

    /// Installs one extension of the choice's run, `id` by its `request`:
    /// reads and checks it, plans its dependencies, shows its preview —
    /// the lines any extension's install shows of what it uses — while it
    /// installs, then installs it with the packages it is missing. What
    /// came of it.
    async fn install_choice_extension(
        &self,
        epoch: u64,
        store: &Arc<std::sync::Mutex<crate::packages::Store>>,
        id: &str,
        request: &Request,
    ) -> Step {
        let package = match self.read_and_check(request.clone()).await {
            Ok(package) => package,
            Err(error) => return Step::Refused(error.to_string()),
        };
        let (package, plan) = self.plan_dependencies(package).await;
        // Its preview, shown while it installs, as any extension's does.
        let title = package.manifest.title.clone();
        let lines = install::preview_lines(&package, &plan, None);
        self.note_choice_preview(epoch, id, &title, lines);
        if !plan.problems.is_empty() {
            return Step::Refused(install::problems(&plan).to_string());
        }
        let mut claimed = Vec::new();
        let outcome = self
            .install_plan(
                store.clone(),
                package,
                plan,
                &super::Mode::Install,
                None,
                &mut claimed,
            )
            .await;
        let mut state = self.lock();
        for identity in &claimed {
            state.release(identity);
        }
        match outcome {
            Ok(outcome) => {
                for dependency in outcome.dependencies {
                    self.put_installed(&mut state, dependency);
                }
                self.put_installed(&mut state, outcome.package);
                if state.screen_epoch != epoch {
                    // The user left the choice while this extension
                    // installed: the launcher's state catches up off its
                    // screen.
                    self.refresh(&mut state);
                }
                Step::Installed
            }
            Err(install::Stopped::Failed(failure)) => {
                // What it had already installed stays installed, and is
                // said (as any install's is).
                Step::Refused(self.install_left_behind(&mut state, &failure))
            }
            // The run shows each extension's plan as it installs it, so
            // there is no plan to have changed from; the arm cannot be
            // reached, but a run never panics over one that somehow is.
            Err(install::Stopped::Changed(_)) => Step::Refused(
                "What installing it needs changed; check it again and choose Install once more"
                    .into(),
            ),
        }
    }

    /// Notes that the run is installing the extension `id`, titled
    /// `title`: its preview `lines` on show under the title, and the
    /// status line naming it. While the choice is on show.
    fn note_choice_preview(&self, epoch: u64, id: &str, title: &str, lines: Vec<String>) {
        let mut state = self.lock();
        if state.screen_epoch != epoch {
            return;
        }
        let Some(choice) = state.choice.as_mut() else {
            return;
        };
        let Some(run) = choice.run.as_mut() else {
            return;
        };
        run.details = lines;
        if let Some(at) = choice
            .extensions
            .iter()
            .position(|extension| extension.id == id)
        {
            run.outcomes[at] = Some(ChoiceOutcome::Installing);
        }
        state.view.status = Status::Progress(format!("Installing {title}"));
        self.choice_view(&mut state);
        self.changed();
    }

    /// Notes what the run came to for the extension `id`, redrawing the
    /// choice while it is still on show; the extension is installed (or
    /// refused) either way.
    fn note_choice(&self, epoch: u64, id: &str, outcome: ChoiceOutcome) {
        let mut state = self.lock();
        if state.screen_epoch != epoch {
            return;
        }
        let Some(choice) = state.choice.as_mut() else {
            return;
        };
        let Some(run) = choice.run.as_mut() else {
            return;
        };
        if let Some(at) = choice
            .extensions
            .iter()
            .position(|extension| extension.id == id)
        {
            run.outcomes[at] = Some(outcome);
        }
        self.choice_view(&mut state);
        self.changed();
    }
}

/// What one step of the run came to.
enum Step {
    /// The extension was installed.
    Installed,
    /// It was not, and why.
    Refused(String),
}

/// The status line for the end of the run: what was installed, and what
/// was not — the choice's rows say what came of each extension.
fn ending(choice: &Choice) -> Status {
    let mut installed: Vec<String> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    if let Some(run) = &choice.run {
        for (extension, outcome) in choice.extensions.iter().zip(&run.outcomes) {
            match outcome {
                Some(ChoiceOutcome::Installed) => installed.push(extension.title.clone()),
                Some(ChoiceOutcome::Refused(_)) => refused.push(extension.title.clone()),
                _ => {}
            }
        }
    }
    match (installed.as_slice(), refused.as_slice()) {
        // None of the ticked extensions was installed; the choice's rows
        // say what came of each.
        ([], _) => {
            Status::Error("Nothing was installed: the choice lists what came of each".into())
        }
        (installed, []) => Status::Result(format!("Installed {}", platform::join(installed))),
        (installed, refused) => Status::Error(format!(
            "Installed {}, but not {}",
            platform::join(installed),
            platform::join(refused)
        )),
    }
}
