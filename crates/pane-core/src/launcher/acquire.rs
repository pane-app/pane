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
//! A default extension the user uninstalled, whose data Pane keeps, is
//! not acquired again: the user's choice, with disabling the documented
//! opt-out; one never installed is.
//!
//! A default extension's pinned revision is never re-acquired as an
//! update: the pin is the release's tested revision, and what a later
//! release does with its repository's newer tags is that release's work
//! (#269). The Git fields its record keeps say where it came from.

use std::future::Future;
use std::sync::Arc;

use super::install::Stopped;
use super::{DOWNLOADS_DIR, Launcher, Mode, Status, off_thread};
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

    /// Whether the default extension `extension` is missing: not
    /// installed, and with no retained data of an uninstall the user
    /// chose. A disabled default extension is installed, so it is never
    /// re-acquired: disabling is the opt-out.
    fn default_missing(&self, extension: &DefaultExtension) -> bool {
        let identity = PackageIdentity::default_extension(&extension.id);
        let state = self.lock();
        !state.packages.iter().any(|p| p.identity == identity)
            && !state.retained.iter().any(|r| r.identity == identity)
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
