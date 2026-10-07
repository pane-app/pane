//! Acquiring Pane's default extensions at first setup (see `defaults`):
//! what the status line says while Pane downloads them, and the rows that
//! try a failed one again.
//!
//! Acquisition goes on in the background: the window, root search, the
//! install rows and Manage extensions stay usable while it runs and after
//! it fails. Each default extension Pane is missing is acquired in turn:
//! its payload is downloaded from Pane's own downloads with progress and
//! retries, then installed through the same path a package from a folder
//! takes — read and checked, planned, claimed and written into a managed
//! copy — with the identity of its default extension, so the normal
//! mechanisms (disable, uninstall, extension data) apply to it unchanged.
//! A default extension the user uninstalled, whose data Pane keeps, is
//! not acquired again: the user's choice, with disabling the documented
//! opt-out; one never installed is.

use std::future::Future;
use std::sync::Arc;
use std::time::SystemTime;

use super::install::Stopped;
use super::{ACQUIRED_DIR, DOWNLOADS_DIR, Launcher, Mode, Status, off_thread};
use crate::defaults::{ArtifactSource, DefaultExtension};
use crate::packages::{PackageIdentity, SourcePackage};

/// The default extensions this launcher acquires at first setup, and the
/// artifact source it acquires them from.
#[derive(Clone)]
pub(in crate::launcher) struct Defaults {
    source: ArtifactSource,
    extensions: Arc<[DefaultExtension]>,
}

impl Defaults {
    pub(in crate::launcher) fn new(
        source: ArtifactSource,
        extensions: Vec<DefaultExtension>,
    ) -> Defaults {
        Defaults {
            source,
            extensions: extensions.into(),
        }
    }
}

/// A default extension Pane could not acquire.
pub(in crate::launcher) struct FailedAcquisition {
    id: String,
    title: String,
    why: String,
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
    /// acquired: each as (id, title, why).
    pub(in crate::launcher) fn retryable(&self) -> Vec<(String, String, String)> {
        if self.in_flight {
            Vec::new()
        } else {
            self.failed
                .iter()
                .map(|failed| (failed.id.clone(), failed.title.clone(), failed.why.clone()))
                .collect()
        }
    }
}

/// "<n>% of <size>", the progress of a payload, from its bytes so far and
/// the size its index entry gave; "…" when the size is not known. Used by
/// acquiring a default extension's payload and downloading a Pane update
/// alike, so both say progress the same way.
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
    /// This launcher acquiring the default extensions `extensions` from
    /// `source` at first setup, into managed copies, as
    /// [`Launcher::acquire_defaults`] does. Without this, no default
    /// extension is acquired; the extensions Pane's application build
    /// acquires are its own choice (a release build takes Pane's published
    /// downloads, a development build `PANE_ARTIFACTS`, the tests' and
    /// smokes' own source on this computer).
    pub fn with_defaults(self, source: ArtifactSource, extensions: Vec<DefaultExtension>) -> Self {
        let defaults = Defaults::new(source, extensions);
        Launcher {
            defaults: Some(defaults),
            ..self
        }
    }

    /// Acquires each default extension this launcher offers and is
    /// missing, downloading its payload with progress and retries and
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
        let (Some(defaults), Some(dir)) = (
            self.defaults.clone(),
            self.installation.as_ref().map(|i| i.dir.clone()),
        ) else {
            return;
        };
        // The flow is taken before anything runs, so a second call while
        // one is running (the window starts one; a retry row starts
        // another) does nothing rather than race it.
        if !self.lock().acquisitions.begin_flow() {
            return;
        }
        // A payload a Pane stopped downloading a day ago is abandoned.
        crate::defaults::remove_abandoned_parts(&dir.join(ACQUIRED_DIR), SystemTime::now());
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

    /// Acquires the payload of `extension` and installs it, telling the
    /// status line what is happening; on failure, why, with the row that
    /// tries again left in root search.
    async fn acquire_one(&self, extension: &DefaultExtension) -> Result<(), String> {
        let (defaults, installation) = match (self.defaults.clone(), self.installation.as_ref()) {
            (Some(defaults), Some(installation)) => (defaults, installation),
            _ => return Err("this launcher installs no packages".into()),
        };
        let (id, title) = (extension.id.clone(), extension.title.clone());
        // Its own copies of both: the download thread below takes the
        // originals.
        let recording = (id.clone(), title.clone());
        let failed = move |launcher: &Launcher, why: String| {
            let (id, title) = (recording.0.clone(), recording.1.clone());
            let why = format!("Could not set up the {title}: {why}");
            // The reason also goes to Pane's own standard error, where the
            // smokes collect it: the status line shows it to the user, but
            // a screenshot cannot be read back.
            eprintln!("pane: {why}");
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
        // The download runs off the thread; the status line follows it.
        let source = defaults.source.clone();
        let downloads = installation.dir.join(DOWNLOADS_DIR);
        let acquired = installation.dir.join(ACQUIRED_DIR);
        let (state, changes) = (self.state.clone(), self.developing.changes());
        let telling = title.clone();
        let telling = move |bytes: u64, total: u64| {
            let text = format!("Acquiring the {telling}: {}", progress(bytes, total));
            let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
            state.view.status = Status::Progress(text);
            drop(state);
            if let Some(changes) = &changes {
                changes.changed();
            }
        };
        let asked = id.clone();
        let fetched = off_thread(move || {
            crate::defaults::fetch(&source, &asked, &downloads, &acquired, &telling)
        })
        .await;
        let fetched = match fetched {
            Ok(fetched) => fetched,
            Err(why) => return failed(self, why.to_string()),
        };
        // The payload is installed as a package from a folder is: read and
        // checked, planned, claimed and written into a managed copy.
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
