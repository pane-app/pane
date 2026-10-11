//! Reloading one installed package while Pane and other packages keep
//! running (ADR 0004).
//!
//! A reload has two stages, reported differently:
//!
//! 1. The package is read again from its source folder and checked as an
//!    install would check it, without running it. If that fails, nothing
//!    changes: the installed code stays in use and the reason is shown.
//! 2. Otherwise the checked package replaces the managed copy, the old
//!    instances stop, and the replacement starts: each of its commands
//!    available here is started and asked for its view. If one fails to
//!    initialize (a trap, or a component that cannot load or be
//!    instantiated; not an error the guest answers with), its instances
//!    are stopped again and the package is paused as failed to start
//!    (see `pausing`), with Retry; the older code is not restored.
//!
//! The old code hands its in-memory state to the new one when it opted in
//! (ADR 0041's state handoff, #159), and the command screen that was open
//! is reopened on the new code with its launch record, whether or not the
//! package opted in: `take_handoff`/`stage_handoff` and `take_reopen`/
//! [`Launcher::reopen`] below, which an update the user chose runs too.
//!
//! Settings belong to the package identity, so they are kept throughout.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{Changing, Launcher, Opening, Screen, State, Status, off_thread, owner, pausing};
use crate::packages::{
    CommandMode, InstalledPackage, Manifest, PackageError, PackageIdentity, Pause, PauseCause,
};
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

/// A command screen that was on display when its package's code was
/// replaced, to be reopened on the new code (ADR 0041): only the command's
/// root view — a view or form it had pushed is not reopened, unless the
/// author restores it from a state handoff.
pub(super) struct Reopen {
    /// The command, opened as it was: with the launch record its screen was
    /// opened with.
    opening: Opening,
    /// The id of the row selected on the screen: selected again where the
    /// new code's list has it, the host-owned state kept by key.
    selected: Option<String>,
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
        let (epoch, reopen): (u64, Option<Reopen>) = match attempt {
            Attempt::Retry => {
                before = self.unpause(&mut self.lock(), &identity);
                (epoch, None)
            }
            Attempt::Reload => match self.replace(&identity, staged.as_deref()).await {
                Ok((epoch, reopen)) => (epoch, reopen),
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
        // The command screen that was on display reopens on the new code
        // (ADR 0041), whatever the start came to: a package that failed to
        // start is paused, and the reopen says so on root search.
        let epoch = match reopen {
            Some(reopen) => self.reopen(epoch, reopen).await,
            None => epoch,
        };
        Reloaded {
            epoch,
            status,
            replaced: matches!(attempt, Attempt::Reload),
        }
    }

    /// Checks the package in its source folder (or in `staged`, keeping
    /// the source's identity) and makes it the installed copy, stopping the
    /// old instances. Returns the screen epoch the outcome belongs to and
    /// the command screen to reopen on the new code, if one was on display
    /// (ADR 0041): a new one if an open command of the package closed for
    /// the reopen, as root search does for a replaced copy with no command
    /// to reopen.
    async fn replace(
        &self,
        identity: &PackageIdentity,
        staged: Option<&Path>,
    ) -> Result<(u64, Option<Reopen>), String> {
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
        // The old code is asked for the state it hands to the new one
        // (ADR 0041), now that the replacement passed its checks: before
        // its generation ends, and only for a replacement that is about to
        // be installed. It is staged once the copy is.
        let handoff = self.take_handoff(identity, &package.manifest).await;
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
        // Staged for the new code's first start, before anything is asked
        // of it: the old generation ended with the replacement above.
        self.stage_handoff(identity, &installed, handoff);
        let mut state = self.lock();
        let first = installed.commands().first().map(|c| c.component.clone());
        // The command screen that was on display is reopened on the new
        // code (ADR 0041); the screen leaves, its search stopping and its
        // form and view closing, until the reopen opens the command again
        // with its launch record. A replacement whose new code has no such
        // command goes to root search, as every replacement did before.
        let was_open = self.put_installed(&mut state, installed);
        let reopen = if was_open {
            let reopen = self.take_reopen(&mut state, identity);
            if reopen.is_none() {
                self.show_root(&mut state, first);
            }
            reopen
        } else {
            self.refresh(&mut state);
            None
        };
        Ok((state.screen_epoch, reopen))
    }

    /// Starts each command of the package that is available on this system
    /// and asks it for its view, and runs its activation entry point if
    /// its `pane.json` declares one (ADR 0041). If one fails to initialize
    /// (it traps, or cannot load or be instantiated), the package's
    /// instances are stopped again, so a retry starts afresh; a view the
    /// guest refuses with an error of its own is not a failure, and a trap
    /// in the activation entry point is a startup failure here, counted
    /// as one. A package disabled meanwhile is not started, and that is
    /// not a failure; nor is one paused meanwhile, which says so itself.
    async fn start(&self, identity: &PackageIdentity) -> Result<(), CallError> {
        let (components, activate): (Vec<PathBuf>, Option<PathBuf>) = {
            let state = self.lock();
            state
                .package(identity)
                .map(|package| {
                    let commands: Vec<PathBuf> = package
                        .available_commands()
                        .into_iter()
                        .filter(|(_, unavailable)| unavailable.is_none())
                        .map(|(command, _)| command.component)
                        .collect();
                    (
                        commands,
                        package
                            .manifest
                            .as_ref()
                            .ok()
                            .and_then(|m| m.activate.clone())
                            .map(|component| package.location.join(component)),
                    )
                })
                .unwrap_or((Vec::new(), None))
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
        // The activation entry point runs as the code starts: a trap in it
        // is a startup failure here, rather than the crash it counts as
        // elsewhere.
        if let Some(activate) = activate {
            let data = self.data_of(&activate);
            if let Err(error) = runtime.activate_with(&activate, data).await
                && !matches!(error, CallError::Disabled | CallError::Guest(_))
            {
                runtime.forget(components.iter().cloned());
                return Err(error);
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

    /// The state the old code of `identity` hands to the replacement whose
    /// manifest is `replacing` (ADR 0041, #159): each of the old code's
    /// running instances that exports `snapshot` and is idle — no call
    /// pending in it — is asked for one, within Pane's 1-second deadline
    /// and 1-megabyte limit, before the old generation ends. A snapshot
    /// whose component the new code no longer has is discarded. Kept in
    /// memory only; [`Launcher::stage_handoff`] stages it for the new code
    /// once the replacement is installed. Called only after the
    /// replacement passed its checks: one that fails changes nothing and
    /// takes no snapshot.
    pub(super) async fn take_handoff(
        &self,
        identity: &PackageIdentity,
        replacing: &Manifest,
    ) -> Vec<(PathBuf, Vec<u8>)> {
        let (named, data) = {
            let state = self.lock();
            let Some(package) = state.package(identity) else {
                return Vec::new();
            };
            let location = package.location.clone();
            let named: Vec<(PathBuf, PathBuf)> = match &package.manifest {
                Ok(manifest) => manifest
                    .components()
                    .map(|(_, component)| (component.to_path_buf(), location.join(component)))
                    .collect(),
                // A package whose manifest cannot be read has no code to
                // ask.
                Err(_) => Vec::new(),
            };
            let data = self
                .installation
                .as_ref()
                .map(|installation| installation.data.owned_by(identity));
            (named, data)
        };
        let Ok(runtime) = self.runtime() else {
            return Vec::new();
        };
        // Asked all at once, so a handoff of several components answers
        // within the one deadline rather than one per component.
        let asked: Vec<_> = named
            .iter()
            .map(|(_, component)| {
                let (runtime, component, data) = (runtime.clone(), component.clone(), data.clone());
                async move { runtime.snapshot_of(&component, data).await }
            })
            .collect();
        let answered = futures::future::join_all(asked).await;
        named
            .into_iter()
            .zip(answered)
            .filter_map(|((component, _), answer)| {
                // A snapshot with no matching component is discarded.
                if !replacing
                    .components()
                    .any(|(_, named)| named == component.as_path())
                {
                    return None;
                }
                answer.ok().flatten().map(|state| (component, state))
            })
            .collect()
    }

    /// Stages the `handoff` [`Launcher::take_handoff`] took for the new
    /// code `installed` (ADR 0041): each component of the new copy with a
    /// snapshot restores it on the instance's first start, before any
    /// other call into it. Called once the replacement is recorded — the
    /// old code's generation ended with it — and before the new code
    /// starts; only the generation the new code runs in restores the
    /// state, so nothing is handed over after, say, a disable followed by
    /// an enable.
    pub(super) fn stage_handoff(
        &self,
        identity: &PackageIdentity,
        installed: &InstalledPackage,
        handoff: Vec<(PathBuf, Vec<u8>)>,
    ) {
        if handoff.is_empty() {
            return;
        }
        let (Some(runtime), Some(installation)) = (self.runtime().ok(), self.installation.as_ref())
        else {
            return;
        };
        let generation = installation.data.owned_by(identity).generation().clone();
        let states = handoff
            .into_iter()
            .map(|(component, state)| (installed.location.join(component), state))
            .collect();
        runtime.stage_restores(&identity.key(), &generation, states);
    }

    /// Takes the command screen of the replaced package with `identity`
    /// that was on display (ADR 0041: it is reopened on the new code),
    /// leaving the screen: its search stops, its form and view close, and
    /// the screen epoch moves on, so no answer of the old code is shown.
    /// `None` when the new copy has no command of that id that opens a
    /// screen — the caller shows root search, as a replacement did before.
    /// What was on display is kept: the command's id, the launch record its
    /// screen was opened with, the text of its search and the row selected.
    pub(super) fn take_reopen(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<Reopen> {
        let command = state.open_command.clone()?;
        let launch = state.launch.clone();
        let search = match &state.view.screen {
            Screen::CommandSearch { query } => Some(query.clone()),
            _ => None,
        };
        // The selection is the command's root list's, kept by key: a view
        // or form the command pushed holds the root list's state to
        // return to, which is what the reopened screen keeps.
        let view = state
            .custom_view
            .as_ref()
            .map(|open| &open.return_to)
            .unwrap_or(&state.view);
        let selected = view
            .selected
            .and_then(|at| view.rows.get(at))
            .map(|row| row.id.clone());
        self.leave_command(state);
        // The new code's command of the same id, one that opens a screen:
        // only the command's root view is reopened; a view or form it had
        // pushed is not.
        let package = state.package(identity)?;
        let registration = package
            .commands()
            .into_iter()
            .find(|offered| offered.manifest_id() == command)?;
        if package.mode_of(&command) != CommandMode::View {
            return None;
        }
        let opening = Opening {
            component: registration.component.clone(),
            command,
            search: registration.search,
            no_view: false,
            launch,
            // Reopened with the text its search field held, where the new
            // code's command still searches.
            initial_search: search.filter(|_| registration.search),
        };
        Some(Reopen { opening, selected })
    }

    /// Reopens `reopen`'s command screen on the new code of its package
    /// (ADR 0041): with the launch record its screen was opened with, the
    /// text of its search typed again, and the row selected where the new
    /// code's list has it. The waiting and setup gates apply as any
    /// launch's. Returns the screen epoch the replacement's outcome
    /// belongs to.
    pub(super) async fn reopen(&self, epoch: u64, reopen: Reopen) -> u64 {
        let component = reopen.opening.component.clone();
        let selected = reopen.selected;
        let data = self.data_of(&component);
        self.launch_opening(epoch, reopen.opening, data).await;
        let mut state = self.lock();
        if state.screen_epoch > epoch {
            // The command's screen opened again, or its setup screen or its
            // argument form: the row selected before is selected where the
            // new list has it.
            if let Some(id) = selected
                && state.open.as_deref() == Some(component.as_path())
                && let Some(at) = state.view.rows.iter().position(|row| row.id == id)
            {
                state.view.selected = Some(at);
            }
            return state.screen_epoch;
        }
        // Nothing opened: the new code waits for what it needs, needs
        // setup that is not given, or failed to answer its view. Root
        // search, as a replacement that closed the screen, keeping what
        // the launch said on the status line.
        let status = std::mem::replace(&mut state.view.status, Status::Idle);
        self.show_root(&mut state, None);
        state.view.status = status;
        state.screen_epoch
    }

    pub(super) fn title_of(&self, identity: &PackageIdentity) -> String {
        self.lock().title_of(identity)
    }
}
