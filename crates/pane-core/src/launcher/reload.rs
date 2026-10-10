//! Reloading one installed package while Pane and other packages keep
//! running (ADR 0004).
//!
//! A reload has two stages, reported differently:
//!
//! 1. The package is read again from its source folder and checked as an
//!    install would check it, without running it. If that fails, nothing
//!    changes: the installed code stays in use and the reason is shown.
//! 2. Otherwise the checked package replaces the managed copy, the old
//!    instances stop (closing a command of it that is open), and the
//!    replacement starts: each of its commands available here is started
//!    and asked for its view. If one fails to initialize (a trap, or a
//!    component that cannot load or be instantiated; not an error the guest
//!    answers with), its instances are stopped again and the package is
//!    paused as failed to start (see `pausing`), with Retry; the older code
//!    is not restored.
//!
//! Settings belong to the package identity, so they are kept throughout.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{Changing, Launcher, State, Status, off_thread, owner, pausing};
use crate::packages::{PackageError, PackageIdentity, Pause, PauseCause};
use crate::runtime::CallError;

/// What a reload does.
#[derive(Clone, Copy)]
pub(super) enum Attempt {
    /// Replace the package's code from its source folder, then start it.
    Reload,
    /// Start again the package's code, which Pane paused after it failed.
    Retry,
}

/// A reload or retry begun by [`Launcher::begin_reload`].
pub(super) struct Reload {
    identity: PackageIdentity,
    attempt: Attempt,
    /// Where the replacement is, if not in the package's source folder: a
    /// development build's staging folder.
    staged: Option<PathBuf>,
}

impl Reload {
    /// A reload of the package with `identity` from the package in
    /// `staged`, for which the caller has claimed the package.
    pub(super) fn staged(identity: PackageIdentity, staged: PathBuf) -> Reload {
        Reload {
            identity,
            attempt: Attempt::Reload,
            staged: Some(staged),
        }
    }
}

/// How a reload ended.
pub(super) struct Reloaded {
    /// The screen epoch the outcome belongs to.
    pub epoch: u64,
    pub status: Status,
    /// Whether the replacement became the installed copy (it may then have
    /// failed to start); not when it failed its checks.
    pub replaced: bool,
}

impl Launcher {
    /// Reloads the installed package with `identity` from its source folder
    /// while Pane and other packages keep running: the package is checked as
    /// an install checks it, then replaces the installed copy and starts (see
    /// the module documentation). Its settings are kept. A command of it
    /// that is open closes; its state is not carried over. Await the
    /// returned future for the outcome.
    ///
    /// A disabled package is not reloaded. While the package is being
    /// reloaded, enabled or disabled, this does nothing.
    pub fn reload(&self, identity: &PackageIdentity) -> impl Future<Output = ()> + Send + 'static {
        self.reload_as(identity, Attempt::Reload)
    }

    /// Starts again the package with `identity`, which Pane paused after it
    /// failed (see `pausing`), without reading its source folder again. Its
    /// crashes are counted afresh.
    pub fn retry_start(
        &self,
        identity: &PackageIdentity,
    ) -> impl Future<Output = ()> + Send + 'static {
        self.reload_as(identity, Attempt::Retry)
    }

    fn reload_as(
        &self,
        identity: &PackageIdentity,
        attempt: Attempt,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let reload = self.begin_reload(&mut state, identity.clone(), attempt);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(reload) = reload {
                launcher.finish_reload(epoch, reload).await;
            }
        }
    }

    /// Checks that the package can be reloaded now, explaining why not: it
    /// is not, while it is being updated, enabled or disabled.
    pub(super) fn begin_reload(
        &self,
        state: &mut State,
        identity: PackageIdentity,
        attempt: Attempt,
    ) -> Option<Reload> {
        if let Err(problem) = self.changeable(state, &identity, "reload") {
            state.view.status = Status::Error(problem);
            return None;
        }
        if !state.claim(&identity, Changing::Reloading) {
            return None;
        }
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        Some(Reload {
            identity,
            attempt,
            staged: None,
        })
    }

    /// Why the command in `component` cannot be started now: its package's
    /// managed copy is being replaced by the update Pane applied by itself,
    /// which would stop the call the user is about to wait on. `None` when
    /// it can be started. The user is never interrupted mid-command by an
    /// automatic update: one waits for the package to be quiet, and this
    /// keeps a call started in the moment between that check and the
    /// replacement from being stopped by it. An update the user chose
    /// (the preview's Update row) replaces anyway, exactly as a reload
    /// does: opening the command is allowed and the replacement closes
    /// it.
    pub(super) fn updating(&self, component: &std::path::Path) -> Option<String> {
        let state = self.lock();
        let package = owner(&state.packages, component)?;
        match state.changing.get(&package.identity) {
            Some(Changing::BackgroundUpdating) => Some(format!(
                "{} is updating; open it again once that is done",
                package.title()
            )),
            _ => None,
        }
    }

    /// Why the package with `identity` cannot be changed by `verb` (reload,
    /// develop) now: this launcher installs nothing, it is not installed, or
    /// it is disabled.
    pub(super) fn changeable(
        &self,
        state: &State,
        identity: &PackageIdentity,
        verb: &str,
    ) -> Result<(), String> {
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            return Err(error.to_string());
        }
        let Some(package) = state.package(identity) else {
            return Err(PackageError::NotInstalled(identity.clone()).to_string());
        };
        if !package.enabled {
            return Err(format!(
                "{} is disabled; enable it to {verb} it",
                package.title()
            ));
        }
        Ok(())
    }

    /// Carries out a reload begun by [`Launcher::begin_reload`]. The outcome
    /// is shown if the user is still on the screen it started from, or was
    /// taken to root search because a command of the package closed.
    pub(super) async fn finish_reload(&self, epoch: u64, reload: Reload) {
        let identity = reload.identity.clone();
        let reloaded = self.carry_out(epoch, reload).await;
        self.end_reload(reloaded.epoch, &identity, reloaded.status);
    }

    /// Carries out a reload, returning how it ended without showing it or
    /// releasing the package.
    pub(super) async fn carry_out(&self, epoch: u64, reload: Reload) -> Reloaded {
        let Reload {
            identity,
            attempt,
            staged,
        } = reload;
        let title = self.title_of(&identity);
        // A Retry ends the pause, remembering it in case the runtime
        // cannot start the package.
        let mut before = None;
        let epoch = match attempt {
            Attempt::Retry => {
                before = self.unpause(&mut self.lock(), &identity);
                epoch
            }
            Attempt::Reload => match self.replace(epoch, &identity, staged.as_deref()).await {
                Ok(epoch) => epoch,
                Err(error) => {
                    let message = format!(
                        "{title} was not reloaded: {error}. It keeps running its installed code."
                    );
                    return Reloaded {
                        epoch,
                        status: Status::Error(message),
                        replaced: false,
                    };
                }
            },
        };
        // The replacement may have another title.
        let title = self.title_of(&identity);
        let status = match (self.start(&identity).await, attempt) {
            (Ok(()), Attempt::Reload) => Status::Result(format!("Reloaded {title}")),
            (Ok(()), Attempt::Retry) => Status::Result(format!("Started {title}")),
            // Pane's runtime, not the package, failed: nothing is paused for
            // it, and a Retry leaves the pause as it was.
            (Err(error @ CallError::RuntimeUnavailable(_)), _) => {
                let mut state = self.lock();
                let kept = match before {
                    Some(pause) => {
                        self.pause(&mut state, &identity, pause);
                        "it stays paused"
                    }
                    None => "it is not paused",
                };
                Status::Error(format!("{title} was not started: {error}; {kept}."))
            }
            (Err(error), _) => {
                let message = error.to_string();
                // The log: the diagnostics, such as a trap's backtrace, also
                // go to Pane's standard error.
                crate::diagnostic!("pane: {title} failed to start: {message}");
                {
                    let mut state = self.lock();
                    // Already paused by its own failure (it could not load)
                    // or meanwhile: those details are kept.
                    if !state.paused.is_paused(&identity) {
                        let pause = Pause {
                            after: PauseCause::FailedToStart,
                            why: message,
                            version: state.package(&identity).and_then(|p| p.version()),
                        };
                        self.pause(&mut state, &identity, pause);
                    }
                }
                // The pause is on record before the outcome is shown.
                self.records_written().await;
                // A package Pane is developing shows the failure as the
                // error overlay (see `error_overlay`) over what the
                // launcher is showing — the command the reload closed for
                // root search included — with a row that starts it again;
                // the pause and its details stay as they are. Not being
                // developed, the status line below is the presentation.
                {
                    let mut state = self.lock();
                    self.show_failed_start_overlay(&mut state, &identity, &error);
                }
                let failed = match attempt {
                    Attempt::Reload => format!("Reloaded {title}, but it failed to start"),
                    Attempt::Retry => format!("{title} failed to start again"),
                };
                // The diagnostics can be long, so they are shown on their own
                // screen rather than here.
                Status::Error(format!(
                    "{failed}; its earlier code is not restored. Retry, or fix it and reload \
                     it; the diagnostics are under \"{}\".",
                    pausing::details_title(&title)
                ))
            }
        };
        Reloaded {
            epoch,
            status,
            replaced: matches!(attempt, Attempt::Reload),
        }
    }

    /// Checks the package in its source folder (or in `staged`, keeping
    /// the source's identity) and makes it the installed copy, stopping the
    /// old instances. Returns the screen epoch the outcome belongs to: a
    /// new one if an open command of the package closed for root search.
    async fn replace(
        &self,
        epoch: u64,
        identity: &PackageIdentity,
        staged: Option<&Path>,
    ) -> Result<u64, String> {
        let package = match staged {
            Some(staged) => {
                self.read_and_check_staged(staged.to_path_buf(), identity.clone())
                    .await
            }
            None => {
                let Some(folder) = identity.local_folder() else {
                    return Err("it has no local source folder to reload from".into());
                };
                self.read_and_check(super::install::Request::Folder(folder.to_path_buf()))
                    .await
            }
        }
        .map_err(|error| error.to_string())?;
        let store = self
            .installation
            .as_ref()
            .expect("begin_reload checked there is an installation")
            .store
            .clone();
        let retire = self.retire(identity);
        let installed = off_thread(move || {
            let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
            store.update(&package, retire)
        })
        .await
        .map_err(|error| error.to_string())?;
        let mut state = self.lock();
        let first = installed.commands().first().map(|c| c.component.clone());
        if self.put_installed(&mut state, installed) {
            self.show_root(&mut state, first);
            return Ok(state.screen_epoch);
        }
        self.refresh(&mut state);
        Ok(epoch)
    }

    /// Starts each command of the package that is available on this system
    /// and asks it for its view. If one fails to initialize (it traps, or
    /// cannot load or be instantiated), the package's instances are stopped
    /// again, so a retry starts afresh; a view the guest refuses with an
    /// error of its own is not a failure. A package disabled meanwhile
    /// is not started, and that is not a failure; nor is one paused
    /// meanwhile, which says so itself.
    async fn start(&self, identity: &PackageIdentity) -> Result<(), CallError> {
        let components: Vec<PathBuf> = {
            let state = self.lock();
            state
                .package(identity)
                .map(|package| {
                    package
                        .available_commands()
                        .into_iter()
                        .filter(|(_, unavailable)| unavailable.is_none())
                        .map(|(command, _)| command.component)
                        .collect()
                })
                .unwrap_or_default()
        };
        let runtime = self.runtime()?;
        for component in &components {
            let data = self.data_of(component);
            match runtime.render_with(component, data).await {
                // Only a fatal initialization is a failure to start: an
                // error the guest answers with, such as "sign in first", is
                // an ordinary outcome of code that started (#16), and so is
                // a tree Pane cannot read.
                Ok(_)
                | Err(CallError::Disabled | CallError::Guest(_) | CallError::Unreadable(_)) => {}
                Err(error) => {
                    runtime.forget(components.iter().cloned());
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// Ends a reload with `status`, shown if the screen is still the one of
    /// `epoch`.
    fn end_reload(&self, epoch: u64, identity: &PackageIdentity, status: Status) {
        let mut state = self.lock();
        state.release(identity);
        self.refresh(&mut state);
        if state.screen_epoch == epoch {
            state.view.status = status;
        }
    }

    pub(super) fn title_of(&self, identity: &PackageIdentity) -> String {
        self.lock().title_of(identity)
    }
}
