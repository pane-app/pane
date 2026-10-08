//! Pane's own updates in the launcher (#54): what the status line says
//! while Pane checks for one and installs it by the user's choice, and
//! the rows that offer it (see `application_update`, where the check, the
//! download and the swap are).
//!
//! Pane checks once, when it starts, in the background: the window, root
//! search and the extension list stay usable. A newer version it finds is
//! told as a row in root search — with the version, what installing does
//! and that the user chooses — and as a word on the status line; nothing
//! is downloaded until the user picks the row. A check that fails is
//! explained the same way, with a row that tries it again; an install
//! that fails is explained, everything left as it was, with the row ready
//! to try again.
//!
//! The Settings window's About page (#82) is a second view of the same
//! flow, not a second flow: `Launcher::application_update` reads the
//! same state the rows come from, and its "Check for updates" and
//! "Update Pane to <version>" rows run the same check
//! (`Launcher::check_application_update_again`) and the same install
//! (`Launcher::install_application_update`) the root rows run, so the
//! two entry points cannot disagree — whichever one found the offer,
//! both show it, and installing from either swaps the same program.
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
    /// Why installing the offered update last failed, if it did: the
    /// offer stays, ready to be chosen again, and the About page shows
    /// the failure beside it, as the status line does. Cleared when a
    /// new install of it begins, when one succeeds, and when a check
    /// replaces what it found.
    install_failed: Option<String>,
}

/// What the last check for a Pane update found.
#[derive(Default)]
enum Notice {
    /// No check has finished yet.
    #[default]
    None,
    /// The last check found this Pane new enough: nothing to offer, but
    /// not the same as no check having run — the About page says which
    /// it is. No row and no word on the status line, as before.
    Current,
    /// A newer version, found and offered.
    Offered(Offer),
    /// The offered update was installed: its version, which the next
    /// start of Pane runs. No row is listed for it, as nothing is left
    /// to choose; the status line and the About page say what happened.
    Installed(String),
    /// The check failed: why, with the row that tries it again.
    Failed(String),
}

/// What Pane knows of its own update, for a second surface of it beside
/// root search's rows (the Settings About page): one snapshot of the
/// state [`Launcher::application_update`] reads, so a page and the
/// launcher cannot disagree. Read only — nothing here checks, downloads
/// or installs anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationUpdate {
    /// No release source is configured: this Pane was given no artifact
    /// source to check (or its own program could not be named), so there
    /// is nothing to check, nothing to offer and no feed to claim. A
    /// page showing this explains the state rather than promising a
    /// check.
    Unconfigured,
    /// No check has finished yet — one may be about to run, or this
    /// Pane has not started one.
    Unchecked,
    /// A check is running now.
    Checking,
    /// The last check found this Pane new enough: no offer, no failure.
    Current,
    /// A newer version is offered to the user's choice: its version,
    /// with why installing it last failed, if it did — the offer stays,
    /// ready to be chosen again.
    Offered {
        version: String,
        failure: Option<String>,
    },
    /// The offered update is being installed now; the status line
    /// follows its progress, as it does every other progress.
    Installing { version: String },
    /// The offered update was installed: its version, which the next
    /// start of Pane runs. Pane never restarts itself.
    Installed { version: String },
    /// The check failed: why, with the check to try again.
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
    /// What Pane knows of its own update, read without entering any flow
    /// and without listing any row: the same state root search's update
    /// rows and the status line come from, as a second surface of it (the
    /// Settings About page) shows. Whether a release source is configured
    /// at all, what the last check found — a newer version it offers, or
    /// that this Pane is new enough, or why the check failed — and what
    /// is running now; the progress of an install is the status line's
    /// (see [`Launcher::view`]), which shows it as it shows every other
    /// progress. Nothing here checks, downloads or installs anything.
    pub fn application_update(&self) -> ApplicationUpdate {
        if self.application.is_none() {
            return ApplicationUpdate::Unconfigured;
        }
        let state = self.lock();
        let updates = &state.updates;
        // An install in flight comes first: the offer it is installing is
        // still the found notice, but what the page shows is the install.
        if updates.installing
            && let Notice::Offered(offer) = &updates.found
        {
            return ApplicationUpdate::Installing {
                version: offer.version.clone(),
            };
        }
        if updates.checking {
            return ApplicationUpdate::Checking;
        }
        match &updates.found {
            Notice::None => ApplicationUpdate::Unchecked,
            Notice::Current => ApplicationUpdate::Current,
            Notice::Offered(offer) => ApplicationUpdate::Offered {
                version: offer.version.clone(),
                failure: updates.install_failed.clone(),
            },
            Notice::Installed(version) => ApplicationUpdate::Installed {
                version: version.clone(),
            },
            Notice::Failed(why) => ApplicationUpdate::Failed(why.clone()),
        }
    }

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
    /// the index is read. Runs in the background; await the returned future
    /// to apply the result, as the window does at start. Calling it again
    /// while a check runs does nothing. This is the check at Pane's start:
    /// it stays quiet when there is nothing to tell, unlike the one the
    /// user asks for ([`Launcher::check_application_update_again`]).
    pub fn check_application_update(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.check(false).await }
    }

    /// The check for a Pane update the user asked for: the root result
    /// that tries a failed check again runs it, and the Settings About
    /// page's "Check for updates" row does too. It is the same check as
    /// the one at Pane's start ([`Launcher::check_application_update`])
    /// over the same source, except that it answers even when there is
    /// nothing to offer — the user asked, so up to date is an answer, not
    /// a silence. Nothing is downloaded or installed either way. Runs in
    /// the background; await the returned future to apply the result, as
    /// the window that offered the row does. Calling it again while a
    /// check runs does nothing.
    pub fn check_application_update_again(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.check(true).await }
    }

    /// One check, at Pane's start, asked for by the row that retries it
    /// or by the Settings About page's "Check for updates" row.
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
        // Whatever the check found replaces what the last one did, and an
        // install's failure with it: a new answer is the fresher word.
        state.updates.install_failed = None;
        let status = match checked {
            Ok(Found::Offered(offer)) => {
                let version = offer.version.clone();
                state.updates.found = Notice::Offered(offer);
                Some(Status::Result(format!("Pane {version} is available")))
            }
            Ok(Found::UpToDate) => {
                state.updates.found = Notice::Current;
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

    /// Installs the offered update, as the user chose it: the root
    /// result "Update Pane to <version>" is one choice, and the Settings
    /// About page's row is another. Downloads the package with progress
    /// and retries, checks it, and replaces the program, which the new
    /// version is used by the next time Pane starts. On failure
    /// everything is as it was, explained, and the offer stays to try
    /// again. Nothing is downloaded before the user's choice, and Pane
    /// never restarts itself. Runs in the background; await the returned
    /// future to apply the result, as the window that offered the row
    /// does.
    pub fn install_application_update(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.install_chosen_update().await }
    }

    /// The install the user chose; see
    /// [`Launcher::install_application_update`].
    async fn install_chosen_update(&self) {
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
            // A new attempt begins: whatever the last one failed with is
            // no longer the word on it.
            state.updates.install_failed = None;
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
                state.updates.found = Notice::Installed(version.clone());
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
    /// the user can try again. The reason is kept with the offer (see
    /// [`Updates::install_failed`]), so the Settings About page shows it
    /// beside the row that offers the install again.
    fn update_failed(&self, version: &str, why: String) {
        {
            let mut state = self.lock();
            state.updates.installing = false;
            state.updates.install_failed = Some(why.clone());
            state.view.status = Status::Error(format!("Could not update Pane to {version}: {why}"));
            self.refresh(&mut state);
        }
        self.changed();
    }
}
