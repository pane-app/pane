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
//!    answers with, #16), its instances are stopped again and the package
//!    is reported as failed to start, with Retry; the older code is not
//!    restored.
//!
//! Settings belong to the package identity, so they are kept throughout.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;

use super::{Changing, Launcher, State, Status, off_thread};
use crate::packages::{PackageError, PackageIdentity};
use crate::runtime::CallError;

/// Why each package whose reloaded code failed to start failed. Such a
/// package stays installed with that code, offering Retry; nothing older is
/// restored. The mark lasts until the package starts, is replaced or is
/// disabled.
#[derive(Default)]
pub(super) struct StartFailures(HashMap<PackageIdentity, String>);

impl StartFailures {
    pub(super) fn record(&mut self, identity: PackageIdentity, why: String) {
        self.0.insert(identity, why);
    }

    pub(super) fn forget(&mut self, identity: &PackageIdentity) {
        self.0.remove(identity);
    }

    /// Why the package with `identity` failed to start, if it did.
    pub(super) fn of(&self, identity: &PackageIdentity) -> Option<&str> {
        self.0.get(identity).map(String::as_str)
    }
}

/// What a reload does.
#[derive(Clone, Copy)]
pub(super) enum Attempt {
    /// Replace the package's code from its source folder, then start it.
    Reload,
    /// Start again the package's code, whose start failed.
    Retry,
}

/// A reload or retry begun by [`Launcher::begin_reload`].
pub(super) struct Reload {
    identity: PackageIdentity,
    attempt: Attempt,
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

    /// Starts again the package with `identity`, whose reloaded code failed
    /// to start, without reading its source folder again.
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
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        let Some(package) = state.package(&identity) else {
            state.view.status = Status::Error(PackageError::NotInstalled(identity).to_string());
            return None;
        };
        if !package.enabled {
            state.view.status = Status::Error(format!(
                "{} is disabled; enable it to reload it",
                package.title()
            ));
            return None;
        }
        if !state.claim(&identity, Changing::Reloading) {
            return None;
        }
        state.view.status = Status::Running;
        Some(Reload { identity, attempt })
    }

    /// Carries out a reload begun by [`Launcher::begin_reload`]. The outcome
    /// is shown if the user is still on the screen it started from, or was
    /// taken to root search because a command of the package closed.
    pub(super) async fn finish_reload(&self, epoch: u64, reload: Reload) {
        let Reload { identity, attempt } = reload;
        let title = self.title_of(&identity);
        let epoch = match attempt {
            Attempt::Retry => {
                self.lock().failed.forget(&identity);
                epoch
            }
            Attempt::Reload => match self.replace(epoch, &identity).await {
                Ok(epoch) => epoch,
                Err(error) => {
                    let message = format!(
                        "{title} was not reloaded: {error}. It keeps running its installed code."
                    );
                    self.end_reload(epoch, &identity, Status::Error(message));
                    return;
                }
            },
        };
        // The replacement may have another title.
        let title = self.title_of(&identity);
        let status = match (self.start(&identity).await, attempt) {
            (Ok(()), Attempt::Reload) => Status::Result(format!("Reloaded {title}")),
            (Ok(()), Attempt::Retry) => Status::Result(format!("Started {title}")),
            (Err(error), _) => {
                let mut state = self.lock();
                let message = error.to_string();
                // The log: the diagnostics, such as a trap's backtrace, also
                // go to Pane's standard error.
                eprintln!("pane: {title} failed to start: {message}");
                state.failed.record(identity.clone(), message);
                let failed = match attempt {
                    Attempt::Reload => format!("Reloaded {title}, but it failed to start"),
                    Attempt::Retry => format!("{title} failed to start again"),
                };
                // The diagnostics can be long, so they are shown under Retry
                // in the extension list rather than here.
                Status::Error(format!(
                    "{failed}; its earlier code is not restored. Retry, or fix it and reload \
                     it; the diagnostics are under \"Retry starting {title}\"."
                ))
            }
        };
        self.end_reload(epoch, &identity, status);
    }

    /// Checks the package in its source folder and makes it the installed
    /// copy, stopping the old instances. Returns the screen epoch the
    /// outcome belongs to: a new one if an open command of the package
    /// closed for root search.
    async fn replace(&self, epoch: u64, identity: &PackageIdentity) -> Result<u64, String> {
        let Some(folder) = identity.local_folder() else {
            return Err("it has no local source folder to reload from".into());
        };
        let folder = folder.to_path_buf();
        let package = self
            .read_and_check(folder)
            .await
            .map_err(|error| error.to_string())?;
        let store = self
            .installation
            .as_ref()
            .expect("begin_reload checked there is an installation")
            .store
            .clone();
        let installed = off_thread(move || {
            let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
            store.update(&package)
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
    /// is not started, and that is not a failure.
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
            match runtime.get_view_with(component, data).await {
                // Only a fatal initialization is a failure to start: an
                // error the guest answers with, such as "sign in first", is
                // an ordinary outcome of code that started (#16).
                Ok(_) | Err(CallError::Disabled | CallError::Guest(_)) => {}
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

    fn title_of(&self, identity: &PackageIdentity) -> String {
        self.lock().title_of(identity)
    }
}
