//! Acquiring Pane's default extensions at first setup (see `defaults`):
//! what the status line says while Pane fetches their pinned revisions,
//! and the rows that try a failed one again.
//!
//! Acquisition goes on in the background: the window, root search, the
//! install rows and the extension list stay usable while it runs and after
//! it fails. Each default extension Pane is missing is acquired in turn:
//! its pinned commit is fetched from its repository with Pane's own Git
//! client, then installed through the same path a package from a folder
//! takes — read and checked, planned, claimed and written into a managed
//! copy — with the identity of its default extension, so the normal
//! mechanisms (disable, uninstall, extension data) apply to it unchanged.
//! First setup asks first (#283, ADR 0045): on a data folder whose first
//! setup has not chosen, [`Launcher::first_setup`] lists the official
//! extensions this system sets up, by their pins' titles and
//! descriptions, with the defaults ticked (the choice screen, `choice`),
//! and nothing is fetched until the user chooses Set Up. The choice is
//! recorded with the packages: a default the user unticked is declined,
//! as an uninstall declines one, and a declined default is never acquired
//! by itself; Settings' Extensions group offers every official extension
//! not installed ([`Launcher::official_extensions`], and an Install
//! operation each), and installing one drops it from the declined. Once
//! the choice is made, first setup never asks again: a default that is
//! missing and not declined — one whose acquisition failed, or one a
//! later Pane added — is acquired by itself at each start. An install
//! set up before the choice existed, holding a default already, is not
//! asked either.
//!
//! A default extension the user uninstalled is not acquired again: the
//! user's choice — whether its data was kept or not, the uninstall
//! declines it — with disabling the other opt-out; one never installed
//! is, once first setup's choice is made.
//!
//! A default extension's pinned revision is never re-acquired as an
//! update: the pin is the release's tested revision, and what a later
//! release does with its repository's newer tags is that release's work
//! (#269). The Git fields its record keeps say where it came from.

use std::future::Future;
use std::sync::Arc;

use super::install::Stopped;
use super::{DOWNLOADS_DIR, Entry, Launcher, Mode, Row, Screen, State, Status, off_thread};
use crate::defaults::DefaultExtension;
use crate::packages::{PackageIdentity, SourcePackage};

/// The default extensions this launcher acquires at first setup, with the
/// pins this Pane release names them by — only the pins whose platform
/// this system is, the gate of the pins file applied where the pins are
/// taken into the launcher, so a default of another system is neither
/// listed nor fetched (a pin that names no platform is every system's).
#[derive(Clone)]
pub(in crate::launcher) struct Defaults {
    extensions: Arc<[DefaultExtension]>,
}

impl Defaults {
    pub(in crate::launcher) fn new(extensions: Vec<DefaultExtension>) -> Defaults {
        Defaults {
            // The platform gate of the pins file, applied where the pins
            // are taken — before any repository is fetched — so a
            // Windows-only default is never fetched, never listed as a
            // failed acquisition and never offered as a retry row on
            // another system.
            extensions: extensions
                .into_iter()
                .filter(|extension| extension.runs_here())
                .collect(),
        }
    }
}

/// First setup's choice of the official extensions (#283), as the
/// launcher holds it: the store's record of it, and whether this session
/// asked.
#[derive(Default)]
pub(in crate::launcher) struct FirstSetup {
    /// Whether the user made the choice (the store's record).
    chosen: bool,
    /// The ids of the default extensions the user declined: unticked at
    /// first setup, or uninstalled (the store's record).
    declined: Vec<String>,
    /// Whether this session showed the choice: while it is not made, root
    /// search then offers it again (the user may have left it).
    asked: bool,
}

impl FirstSetup {
    /// The choice as the store records it.
    pub(in crate::launcher) fn recorded(chosen: bool, declined: Vec<String>) -> FirstSetup {
        FirstSetup {
            chosen,
            declined,
            asked: false,
        }
    }

    /// Whether the user declined the default extension `id`.
    fn declines(&self, id: &str) -> bool {
        self.declined.iter().any(|declined| declined == id)
    }

    /// Holds the choice as made, the defaults `declined` declined, while
    /// the store records it ([`Launcher::finish_first_setup`]).
    pub(in crate::launcher) fn choose(&mut self, declined: &[String]) {
        self.chosen = true;
        for id in declined {
            if !self.declines(id) {
                self.declined.push(id.clone());
            }
        }
    }

    /// Records, as the store has, that the default extension `id` was
    /// installed again: no longer declined.
    pub(in crate::launcher) fn installed(&mut self, id: &str) {
        self.declined.retain(|declined| declined != id);
    }

    /// Takes the store's record again, after it changed (an uninstall
    /// declines a default).
    pub(in crate::launcher) fn reread(&mut self, chosen: bool, declined: Vec<String>) {
        self.chosen = chosen;
        self.declined = declined;
    }
}

/// One of Pane's official extensions as Settings' Extensions group lists
/// it (#283): each default extension this system sets up, by its pin,
/// and whether it is installed. One not installed — declined, or never
/// acquired — is offered there to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficialExtension {
    /// Its id: its identity is the default's, `default:<id>`.
    pub id: String,
    /// Its title, as its pin names it.
    pub title: String,
    /// What it does, as its pin says.
    pub description: Option<String>,
    /// Whether it is installed.
    pub installed: bool,
}

/// A default extension Pane could not acquire.
pub(in crate::launcher) struct FailedAcquisition {
    pub(in crate::launcher) id: String,
    pub(in crate::launcher) title: String,
    pub(in crate::launcher) why: String,
}

/// What the status line says of acquiring Pane's default extensions, and
/// which failed and can be tried again.
#[derive(Default)]
pub(in crate::launcher) struct Acquisitions {
    /// Whether Pane is acquiring a default extension now (a whole first
    /// setup, or one retried row); the rows that retry are not listed
    /// meanwhile.
    in_flight: bool,
    /// Each default extension Pane could not acquire.
    failed: Vec<FailedAcquisition>,
}

impl Acquisitions {
    /// Takes the acquisitions for one flow — a whole first setup, or one
    /// retried row — in one step, so two flows cannot run at once:
    /// whether this one started, or another is already running (in which
    /// case the rows that retry are not listed anyway).
    fn begin_flow(&mut self) -> bool {
        if self.in_flight {
            false
        } else {
            self.in_flight = true;
            true
        }
    }

    /// Ends the flow, so the rows that retry are listed again.
    fn end_flow(&mut self) {
        self.in_flight = false;
    }

    /// Records that the default extension with `id` was set up: the row
    /// that would try it again is gone.
    fn succeeded(&mut self, id: &str) {
        self.failed.retain(|failed| failed.id != id);
    }

    /// Records that the default extension could not be acquired,
    /// replacing what was recorded for it before.
    fn failed(&mut self, failed: FailedAcquisition) {
        match self
            .failed
            .iter_mut()
            .find(|recorded| recorded.id == failed.id)
        {
            Some(recorded) => *recorded = failed,
            None => self.failed.push(failed),
        }
    }

    /// The default extensions that can be tried again, while none is being
    /// acquired, each with its id, title and why it failed.
    pub(in crate::launcher) fn retryable(&self) -> Vec<&FailedAcquisition> {
        if self.in_flight {
            Vec::new()
        } else {
            self.failed.iter().collect()
        }
    }
}

/// "<n>% of <size>", the progress of a download, from its bytes so far and
/// the size its source gave; "…" when the size is not known. Used by
/// downloading a Pane update, so both say progress the same way. (A Git
/// fetch has no byte progress to follow: the status line says what is
/// being set up instead.)
pub(in crate::launcher) fn progress(bytes: u64, total: u64) -> String {
    if total == 0 {
        return "…".into();
    }
    let percent = bytes.saturating_mul(100) / total;
    format!("{percent}% of {}", of_size(total))
}

/// A size as people read it: "940 bytes", "116 KiB", "3.1 MiB".
fn of_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    match bytes {
        0..=1023 => format!("{bytes} bytes"),
        1024..=MIB => format!("{} KiB", bytes.div_ceil(KIB)),
        _ => format!("{:.1} MiB", bytes as f64 / MIB as f64),
    }
}

impl Launcher {
    /// This launcher acquiring the default extensions `extensions` at
    /// first setup, each at the commit this Pane release pins it by, into
    /// managed copies, as [`Launcher::acquire_defaults`] does. Without
    /// this, no default extension is acquired; the pins the application
    /// build gives are its own choice (the committed ones, or the ones
    /// `PANE_DEFAULTS` names in a development build). A pin whose
    /// platform this system is not is dropped here, before any repository
    /// is fetched — the pins file's platform gate.
    pub fn with_defaults(self, extensions: Vec<DefaultExtension>) -> Self {
        let defaults = Defaults::new(extensions);
        Launcher {
            defaults: Some(defaults),
            ..self
        }
    }

    /// Acquires each default extension this launcher offers and is
    /// missing, fetching its pinned commit from its repository and
    /// installing it through the same path a package from a folder takes.
    /// Runs in the background: the launcher stays usable while it runs and
    /// after it fails, the status line says what it is doing, and each
    /// default extension that could not be acquired is offered again as a
    /// row in root search. Await the returned future to apply each result;
    /// calling it again while it runs does nothing.
    pub fn acquire_defaults(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.acquire_missing().await }
    }

    /// First setup, as Pane starts (#283): while the user has not made
    /// first setup's choice of the official extensions, shows it — the
    /// official extensions this system sets up, the defaults ticked, the
    /// Set Up row selected — and acquires nothing; Set Up records the
    /// choice and acquires what is ticked. Once the choice is made, it
    /// acquires each default extension that is missing and not declined,
    /// as [`Launcher::acquire_defaults`] does. The choice shows only over
    /// root search: over another screen (an install preview Pane was
    /// started with), root search offers it instead, as it does once the
    /// user leaves it. Await the returned future to apply each result.
    pub fn first_setup(&self) -> impl Future<Output = ()> + Send + 'static {
        let asks = {
            let mut state = self.lock();
            let asks = self.first_setup_pending(&state);
            if asks {
                // Root search offers it from now on, while it is not
                // made, whatever screen it was asked over.
                state.first_setup.asked = true;
                if matches!(state.view.screen, Screen::Root { .. }) {
                    self.show_first_setup(&mut state);
                }
            }
            asks
        };
        if asks {
            self.changed();
        }
        let launcher = self.clone();
        async move {
            if !asks {
                launcher.acquire_missing().await;
            }
        }
    }

    /// Whether first setup's choice is still to be made (#283): this
    /// launcher installs packages and sets default extensions up, the
    /// store records no choice, and no default extension is installed or
    /// has data kept — an install set up before the choice existed, which
    /// has its defaults already, is never asked.
    pub(in crate::launcher) fn first_setup_pending(&self, state: &State) -> bool {
        let offers = self
            .defaults
            .as_ref()
            .is_some_and(|defaults| !defaults.extensions.is_empty());
        let has_default = state
            .packages
            .iter()
            .map(|package| &package.identity)
            .chain(state.retained.iter().map(|retained| &retained.identity))
            .any(|identity| identity.default_id().is_some());
        self.installation.is_some() && offers && !state.first_setup.chosen && !has_default
    }

    /// Whether root search offers first setup's choice again: it was
    /// shown this session and is still to be made.
    pub(in crate::launcher) fn offers_first_setup(&self, state: &State) -> bool {
        state.first_setup.asked && self.first_setup_pending(state)
    }

    /// Shows first setup's choice (#283): the official extensions this
    /// system sets up, by their pins, the defaults ticked, and the Set Up
    /// row selected.
    pub(in crate::launcher) fn show_first_setup(&self, state: &mut State) {
        let Some(defaults) = self.defaults.clone() else {
            return;
        };
        let listed = super::choice::Listed::first_setup(&defaults.extensions);
        self.show_choice(state, listed);
    }

    /// Records first setup's choice — the ids of the defaults the user
    /// unticked, `declined` — and acquires the ticked ones, as first
    /// setup always acquired the defaults: in the background, the status
    /// line saying what is being set up, each failure offered again as a
    /// row. The launcher already shows the choice as made; a choice that
    /// cannot be recorded is explained, and first setup is offered again.
    pub(in crate::launcher) async fn finish_first_setup(&self, declined: Vec<String>) {
        let Some(installation) = self.installation.as_ref() else {
            return;
        };
        let store = installation.store.clone();
        let recorded = off_thread(move || {
            let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
            store
                .choose_first_setup(&declined)
                .map(|()| (store.first_setup_chosen(), store.declined_defaults()))
        })
        .await;
        match recorded {
            Ok((chosen, declined)) => {
                {
                    let mut state = self.lock();
                    state.first_setup.reread(chosen, declined);
                }
                self.acquire_missing().await;
            }
            Err(error) => {
                let mut state = self.lock();
                state.first_setup.chosen = false;
                state.first_setup.declined.clear();
                state.view.status = Status::Error(format!(
                    "Could not record which official extensions to set up: {error}"
                ));
                self.refresh(&mut state);
                drop(state);
                self.changed();
            }
        }
    }

    /// Pane's official extensions as Settings' Extensions group lists
    /// them (#283): each default extension this system sets up, in the
    /// pins' order, with whether it is installed. One not installed is
    /// offered there to install, declined or not.
    pub fn official_extensions(&self) -> Vec<OfficialExtension> {
        let Some(defaults) = self.defaults.clone() else {
            return Vec::new();
        };
        let state = self.lock();
        defaults
            .extensions
            .iter()
            .map(|extension| {
                let identity = PackageIdentity::default_extension(&extension.id);
                OfficialExtension {
                    id: extension.id.clone(),
                    title: extension.title.clone(),
                    description: extension.description.clone(),
                    installed: state.packages.iter().any(|p| p.identity == identity),
                }
            })
            .collect()
    }

    /// Whether the default extension `extension` is missing: not
    /// installed, not declined (unticked at first setup, or uninstalled),
    /// and with no retained data of an uninstall the user chose (as an
    /// earlier Pane recorded one). A disabled default extension is
    /// installed, so it is never re-acquired: disabling is the other
    /// opt-out.
    fn default_missing(&self, extension: &DefaultExtension) -> bool {
        let identity = PackageIdentity::default_extension(&extension.id);
        let state = self.lock();
        !state.packages.iter().any(|p| p.identity == identity)
            && !state.retained.iter().any(|r| r.identity == identity)
            && !state.first_setup.declines(&extension.id)
    }

    /// Settings' rows that install each official extension not installed
    /// (#283), in the pins' order: declined at first setup or by an
    /// uninstall, or never acquired. Each is acquired from its pin, as the
    /// row that tries a failed one again acquires it, and installing it
    /// drops it from the declined.
    pub(in crate::launcher) fn official_rows(&self, state: &State) -> Vec<(Row, Entry)> {
        let Some(defaults) = self.defaults.as_ref() else {
            return Vec::new();
        };
        defaults
            .extensions
            .iter()
            .filter(|extension| {
                let identity = PackageIdentity::default_extension(&extension.id);
                !state.packages.iter().any(|p| p.identity == identity)
            })
            .map(|extension| {
                let row = Row {
                    id: format!("install-official:{}", extension.id),
                    title: format!("Install {}", extension.title),
                    subtitle: Some(
                        extension
                            .description
                            .clone()
                            .unwrap_or_else(|| "One of Pane's official extensions".into()),
                    ),
                    unavailable: None,
                };
                (row, Entry::InstallOfficial(extension.id.clone()))
            })
            .collect()
    }

    /// Acquires every missing default extension in turn; the status line
    /// ends showing what was set up, or the last failure.
    async fn acquire_missing(&self) {
        let Some(defaults) = self.defaults.clone() else {
            return;
        };
        // The flow is taken before anything runs, so a second call while
        // one is running (the window starts one; a retry row starts
        // another) does nothing rather than race it.
        if !self.lock().acquisitions.begin_flow() {
            return;
        }
        let mut set_up: Option<String> = None;
        let mut more_than_one = false;
        let mut failure = None;
        for extension in defaults.extensions.iter() {
            if !self.default_missing(extension) {
                continue;
            }
            match self.acquire_one(extension).await {
                Ok(()) => {
                    if set_up.is_some() {
                        more_than_one = true;
                    }
                    set_up = Some(extension.title.clone());
                }
                Err(why) => failure = Some(why),
            }
        }
        // The flow ends before the status is set, so the rows that retry
        // what failed are listed again.
        {
            let mut state = self.lock();
            state.acquisitions.end_flow();
            self.refresh(&mut state);
        }
        if let Some(why) = failure {
            self.show(Status::Error(why));
        } else if more_than_one {
            self.show(Status::Result("Set up Pane's default extensions".into()));
        } else if let Some(title) = set_up {
            self.show(Status::Result(format!("Set up the {title}")));
        }
    }

    /// The row that tries a default extension again, after Pane could not
    /// acquire it: as [`Launcher::acquire_defaults`] acquires that one.
    pub(in crate::launcher) async fn retry_acquiring(&self, id: &str) {
        let Some(defaults) = self.defaults.clone() else {
            return;
        };
        let Some(extension) = defaults
            .extensions
            .iter()
            .find(|extension| extension.id == id)
            .cloned()
        else {
            return;
        };
        // As for a whole first setup: one flow at a time.
        if !self.lock().acquisitions.begin_flow() {
            return;
        }
        let acquired = self.acquire_one(&extension).await;
        {
            let mut state = self.lock();
            state.acquisitions.end_flow();
            self.refresh(&mut state);
        }
        match acquired {
            Ok(()) => self.show(Status::Result(format!("Set up the {}", extension.title))),
            Err(why) => self.show(Status::Error(why)),
        }
    }

    /// Acquires the pinned revision of `extension` and installs it,
    /// telling the status line what is being set up; on failure, why, with
    /// the row that tries again left in root search.
    async fn acquire_one(&self, extension: &DefaultExtension) -> Result<(), String> {
        let installation = match self.installation.as_ref() {
            Some(installation) => installation,
            None => return Err("this launcher installs no packages".into()),
        };
        let (id, title) = (extension.id.clone(), extension.title.clone());
        // Its own copies of both: the fetch thread below takes the
        // originals.
        let recording = (id.clone(), title.clone());
        let failed = move |launcher: &Launcher, why: String| {
            let (id, title) = (recording.0.clone(), recording.1.clone());
            let why = format!("Could not set up the {title}: {why}");
            // The reason also goes to Pane's own standard error, where the
            // smokes collect it: the status line shows it to the user, but
            // a screenshot cannot be read back.
            crate::diagnostic!("pane: {why}");
            launcher.show(Status::Error(why.clone()));
            let mut state = launcher.lock();
            state.acquisitions.failed(FailedAcquisition {
                id,
                title,
                why: why.clone(),
            });
            launcher.refresh(&mut state);
            Err(why)
        };
        {
            let mut state = self.lock();
            state.view.status = Status::Progress(format!("Acquiring the {title}…"));
        }
        self.changed();
        // The fetch runs off the thread. A Git fetch has no byte progress
        // to follow (its answer is one pack), so the status line says what
        // is being set up and nothing more until it ends.
        let pin = extension.clone();
        let downloads = installation.dir.join(DOWNLOADS_DIR);
        let fetched = off_thread(move || crate::defaults::fetch(&pin, &downloads)).await;
        let fetched = match fetched {
            Ok(fetched) => fetched,
            Err(why) => return failed(self, why.to_string()),
        };
        // The revision is installed as a package from a folder is: read
        // and checked, planned, claimed and written into a managed copy.
        let mut package = match off_thread(move || SourcePackage::read_default(fetched)).await {
            Ok(package) => package,
            Err(error) => return failed(self, error.to_string()),
        };
        match self.check_components(&package).await {
            Ok(checked) => package.note_imports(checked),
            Err(error) => return failed(self, error.to_string()),
        }
        let (package, plan) = self.plan_dependencies(package).await;
        let store = installation.store.clone();
        let mut claimed = Vec::new();
        let installed = self
            .install_plan(store, package, plan, &Mode::Install, None, &mut claimed)
            .await;
        let release = |launcher: &Launcher| {
            let mut state = launcher.lock();
            for identity in &claimed {
                state.release(identity);
            }
        };
        match installed {
            Ok(outcome) => {
                let mut state = self.lock();
                for dependency in outcome.dependencies {
                    self.put_installed(&mut state, dependency);
                }
                self.put_installed(&mut state, outcome.package);
                state.acquisitions.succeeded(&id);
                // The row that tried again is gone: what it asked for is
                // there.
                self.refresh(&mut state);
                drop(state);
                release(self);
                Ok(())
            }
            Err(Stopped::Changed(_)) => {
                release(self);
                failed(self, "what it needs changed; try again".into())
            }
            Err(Stopped::Failed(failure)) => {
                let mut state = self.lock();
                let message = self.install_left_behind(&mut state, &failure);
                drop(state);
                release(self);
                failed(self, message)
            }
        }
    }

    /// Shows `status` and tells the window, if any, that the launcher
    /// changed in the background, as acquiring a default extension or
    /// checking for a Pane update did.
    pub(in crate::launcher) fn show(&self, status: Status) {
        {
            let mut state = self.lock();
            state.view.status = status;
        }
        self.changed();
    }
}
