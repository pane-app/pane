//! Pane's own updates in the launcher (#54): what the status line says
//! while Pane checks for one and installs it by the user's choice, and
//! the rows that offer it (see `application_update`, where the check, the
//! download and the swap are).
//!
//! Pane checks once, when it starts, in the background: the window, root
//! search and Manage extensions stay usable. A newer version it finds is
//! told as a row in root search — with the version, what installing does
//! and that the user chooses — and as a word on the status line; nothing
//! is downloaded until the user picks the row. A check that fails is
//! explained the same way, with a row that tries it again; an install
//! that fails is explained, everything left as it was, with the row ready
//! to try again.
//!
//! Pane never restarts itself: an install replaces the program so the new
//! version is used the next time Pane starts, as the row says.

use std::future::Future;
use std::path::PathBuf;

use super::{Launcher, Status, off_thread};
use crate::Target;
use crate::application_update::{self, Found, Offer, Program};
use crate::defaults::ArtifactSource;
use crate::launcher::Entry;
use crate::launcher::Row;

/// This launcher's application: the version of Pane it runs, where its
/// updates come from (the artifact source the default extensions come
/// from too) and the program an update replaces. A launcher without one
/// (another system's updater is not wired yet, or the program could not
/// be named) checks for nothing and offers nothing.
#[derive(Clone)]
pub(in crate::launcher) struct Application {
    version: String,
    source: ArtifactSource,
    program: Program,
}

/// What the last check for a Pane update found, and what is running now.
#[derive(Default)]
pub(in crate::launcher) struct Updates {
    /// What the last check found.
    found: Notice,
    /// A check is running now: the row that tries a failed one again is
    /// not listed meanwhile.
    checking: bool,
    /// The offer is being installed now: its row is not listed meanwhile.
    installing: bool,
}

/// What the last check for a Pane update found.
#[derive(Default)]
enum Notice {
    /// No check has finished, or it found nothing to tell the user.
    #[default]
    None,
    /// A newer version, found and offered.
    Offered(Offer),
    /// The check failed: why, with the row that tries it again.
    Failed(String),
}

impl Updates {
    /// Takes the checking for one flow — the start of Pane, or the row
    /// that tries a failed check again — so two cannot run at once.
    fn begin_check(&mut self) -> bool {
        if self.checking {
            false
        } else {
            self.checking = true;
            true
        }
    }

    /// Ends the check, so a row that tries a failed one again is listed.
    fn end_check(&mut self) {
        self.checking = false;
    }

    /// The offer to install, if the last check found one and it is not
    /// being installed.
    fn offer(&self) -> Option<Offer> {
        match (&self.found, self.installing) {
            (Notice::Offered(offer), false) => Some(offer.clone()),
            _ => None,
        }
    }

    /// The rows root search offers of Pane's own update: the offer the
    /// user can choose to install, or a check that failed and can be
    /// tried again.
    pub(in crate::launcher) fn rows(&self) -> Vec<(Row, Entry)> {
        match self.offer() {
            Some(offer) => vec![(
                Row {
                    id: "pane.update".into(),
                    title: format!("Update Pane to {}", offer.version),
                    subtitle: Some(
                        "Your extensions and settings are kept; the new version is used the \
                         next time Pane starts"
                            .into(),
                    ),
                    unavailable: None,
                },
                Entry::InstallUpdate,
            )],
            None => match (&self.found, self.checking) {
                (Notice::Failed(why), false) => vec![(
                    Row {
                        id: "pane.check-update".into(),
                        title: "Check for a Pane update".into(),
                        subtitle: Some(why.clone()),
                        unavailable: None,
                    },
                    Entry::CheckUpdate,
                )],
                _ => Vec::new(),
            },
        }
    }
}

impl Launcher {
    /// This launcher checking for Pane application updates: the version
    /// `version` of Pane it runs, updated from `source` — the same
    /// artifact source the default extensions are acquired from —
    /// replacing the program at `program` when the user chooses to
    /// install. What an earlier update left in the program's folder also
    /// goes, which a Pane starting does. A path that names no program
    /// file wires no updater at all.
    pub fn with_application_update(
        self,
        version: &str,
        source: ArtifactSource,
        program: PathBuf,
    ) -> Self {
        let application = Program::at(program).ok().map(|program| {
            program.clean_install_folder();
            Application {
                version: version.to_owned(),
                source,
                program,
            }
        });
        Launcher {
            application,
            ..self
        }
    }

    /// Checks the artifact source for a newer version of Pane and tells
    /// the user what it found: a row in root search and a word on the
    /// status line for an update the user can choose to install, or an
    /// explanation of why the check could not be made, with the row that
    /// tries it again. Nothing is downloaded, installed or restarted: only
    /// the index is read, and only this Pane's start runs it. Runs in the
    /// background; await the returned future to apply the result, as the
    /// window does at start. Calling it again while a check runs does
    /// nothing.
    pub fn check_application_update(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.check(false).await }
    }

    /// The check again, as the row that tries a failed one runs it: the
    /// user asked, so it answers even when there is nothing to offer.
    pub(in crate::launcher) async fn retry_checking_update(&self) {
        self.check(true).await;
    }

    /// One check, at Pane's start or asked for by the row that retries.
    async fn check(&self, asked: bool) {
        let Some(application) = self.application.clone() else {
            return;
        };
        // This Pane's own target: an entry for another system is
        // explained, not offered.
        let target = Target::current()
            .map(|target| target.id().to_owned())
            .unwrap_or_else(|| "a system Pane names no target for".into());
        if !self.lock().updates.begin_check() {
            return;
        }
        let source = application.source.clone();
        let version = application.version.clone();
        let checked =
            off_thread(move || application_update::check(&source, &version, &target)).await;
        let mut state = self.lock();
        state.updates.end_check();
        let status = match checked {
            Ok(Found::Offered(offer)) => {
                let version = offer.version.clone();
                state.updates.found = Notice::Offered(offer);
                Some(Status::Result(format!("Pane {version} is available")))
            }
            Ok(Found::UpToDate) => {
                state.updates.found = Notice::None;
                // A check the user asked for answers, even with nothing to
                // offer; the one at Pane's start stays quiet, as there is
                // nothing to tell.
                asked.then(|| Status::Result("Pane is up to date".into()))
            }
            Err(why) => {
                state.updates.found = Notice::Failed(why.clone());
                Some(Status::Error(format!(
                    "Could not check for a Pane update: {why}"
                )))
            }
        };
        self.refresh(&mut state);
        drop(state);
        if let Some(status) = status {
            self.show(status);
        }
    }

    /// Installs the offered update, as the row the user chose does:
    /// downloads the package with progress and retries, checks it, and
    /// replaces the program, which the new version is used by the next
    /// time Pane starts. On failure everything is as it was, explained,
    /// and the row stays to try again.
    pub(in crate::launcher) async fn install_application_update(&self) {
        let Some(application) = self.application.clone() else {
            return;
        };
        let Some(offer) = self.lock().updates.offer() else {
            return;
        };
        let version = offer.version.clone();
        {
            let mut state = self.lock();
            state.updates.installing = true;
            state.view.status = Status::Progress(format!("Downloading Pane {version}…"));
            self.refresh(&mut state);
        }
        self.changed();
        // The download runs off the thread; the status line follows it,
        // as acquiring a default extension's payload does.
        let (state, changes) = (self.state.clone(), self.developing.changes());
        let telling = version.clone();
        let telling = move |bytes: u64, total: u64| {
            let text = format!(
                "Downloading Pane {telling}: {}",
                super::acquire::progress(bytes, total)
            );
            let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
            state.view.status = Status::Progress(text);
            drop(state);
            if let Some(changes) = &changes {
                changes.changed();
            }
        };
        let source = application.source.clone();
        let for_swap = offer.clone();
        let fetched =
            off_thread(move || application_update::fetch(&source, &for_swap, &telling)).await;
        let package = match fetched {
            Ok(package) => package,
            Err(why) => return self.update_failed(&version, why),
        };
        {
            let mut state = self.lock();
            state.view.status = Status::Progress(format!("Installing Pane {version}…"));
        }
        self.changed();
        let swapped =
            off_thread(move || application_update::swap(&package, &offer, &application.program))
                .await;
        match swapped {
            Ok(()) => {
                let mut state = self.lock();
                state.updates.installing = false;
                state.updates.found = Notice::None;
                self.refresh(&mut state);
                drop(state);
                self.show(Status::Result(format!(
                    "Installed Pane {version}; the new version is used the next time Pane \
                     starts"
                )));
            }
            Err(why) => self.update_failed(&version, why),
        }
    }

    /// Explains a failed install: the row stays, nothing was changed, and
    /// the user can try again.
    fn update_failed(&self, version: &str, why: String) {
        {
            let mut state = self.lock();
            state.updates.installing = false;
            state.view.status = Status::Error(format!("Could not update Pane to {version}: {why}"));
            self.refresh(&mut state);
        }
        self.changed();
    }
}
