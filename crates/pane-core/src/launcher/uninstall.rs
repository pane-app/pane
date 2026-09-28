//! Uninstalling one installed package, with the user's explicit choice to
//! keep or delete its saved data (its extension settings and content).
//!
//! Choosing a row of the confirmation applies at once in this launcher: the
//! package leaves root search, the extension list and the operation
//! targets, its instances stop, an open command of it closes and it can no
//! longer save data. Then Pane, never the package's code:
//!
//! 1. records it as uninstalled in `installed.json`, with a record of the
//!    identity whose saved data is kept when the user keeps it. If this
//!    fails nothing else changes and the package is back as it was. This is
//!    the point after which the package is uninstalled;
//! 2. removes its managed copy. A folder still in use (Windows) is listed
//!    for removal at the next start, as an update's replaced copy is;
//! 3. removes its cache and local credentials, and its settings and content
//!    too when the user deletes them. Data that could not be removed stays
//!    recorded as kept, so it is not lost track of.
//!
//! Recording first means a failure leaves either an installed package with
//! all its data or an uninstalled one whose leftovers are on record, never
//! an installed package whose data was deleted. A failure of step 2 or 3 is
//! explained rather than reported as a
//! successful uninstall. The source folder and anything outside Pane's data
//! folder are never touched. Kept data belongs to the package identity:
//! installing the same source again finds it, another source never does.

use std::future::Future;
use std::path::PathBuf;

use super::{
    Changing, Entry, Launcher, LauncherView, Question, Row, Screen, State, Status, off_thread,
};
use crate::extension_data::{DataKind, ExtensionData};
use crate::packages::{
    InstalledPackage, Leftover, PackageError, PackageIdentity, Pause, SavedData,
};

/// An uninstall begun by [`Launcher::begin_uninstall`]: the package, already
/// removed from the launcher, and where it was, to put it back if its
/// uninstall cannot be recorded.
pub(super) struct Uninstall {
    package: InstalledPackage,
    index: usize,
    saved: SavedData,
    /// Why Pane had paused it, if it had.
    pause: Option<Pause>,
}

impl Launcher {
    /// Uninstalls the installed package with `identity`, keeping or deleting
    /// its saved data (see the module documentation). Its cache and local
    /// credentials are removed either way; its source folder is not touched
    /// and none of its code runs. Await the returned future for the outcome.
    ///
    /// While the package is being reloaded, updated or uninstalled, this
    /// explains why it does nothing.
    pub fn uninstall(
        &self,
        identity: &PackageIdentity,
        saved: SavedData,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let uninstall = self.begin_uninstall(&mut state, identity.clone(), saved);
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(uninstall) = uninstall {
                launcher.finish_uninstall(epoch, uninstall).await;
            }
        }
    }

    /// Asks whether to uninstall the installed package with `identity`,
    /// saying what is removed, what saved data it has and what is never
    /// touched, with a choice to keep or delete that data.
    pub(super) fn show_uninstall(&self, state: &mut State, identity: &PackageIdentity) {
        let Some(installation) = &self.installation else {
            return;
        };
        let title = state.title_of(identity);
        let choice = |id: &str, title: &str, subtitle: &str| Row {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            unavailable: None,
        };
        state.screen_epoch += 1;
        state.entries = vec![
            Entry::Uninstall(identity.clone(), SavedData::Keep),
            Entry::Uninstall(identity.clone(), SavedData::Delete),
            Entry::Cancel,
        ];
        let source = match identity.local_folder() {
            Some(folder) => format!("Its source folder {}", folder.display()),
            None => "Its source".into(),
        };
        let details = vec![
            format!("From {identity}"),
            "Pane removes its installed copy, its cache and its credentials on this computer, \
             and the extension does not run. Deleting a credential does not sign you out of an \
             online service."
                .into(),
            saved_data(&installation.data, identity),
            format!("{source} and files it saved elsewhere are not touched."),
        ];
        let screen = Screen::Confirm {
            question: Question::Uninstall(identity.clone()),
            details,
        };
        state.view = LauncherView::new(screen, format!("Uninstall {title}?")).with_rows(vec![
            choice(
                "keep",
                "Uninstall and keep saved data",
                "Keep its settings and content; installing it again from the same source \
                 restores them",
            ),
            choice(
                "delete",
                "Uninstall and delete saved data",
                "Delete its settings and content too",
            ),
            choice("cancel", "Cancel", "Keep it installed"),
        ]);
    }

    /// Removes the package with `identity` from this launcher at once, to be
    /// uninstalled by [`Launcher::finish_uninstall`]: it leaves root search,
    /// the extension list and the operation targets, its instances stop, an
    /// open command of it closes and it can no longer save data. Explains
    /// why not and returns `None` if there is no such package or something
    /// else is happening to it.
    pub(super) fn begin_uninstall(
        &self,
        state: &mut State,
        identity: PackageIdentity,
        saved: SavedData,
    ) -> Option<Uninstall> {
        let Some(installation) = &self.installation else {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        };
        let Some(index) = state.packages.iter().position(|p| p.identity == identity) else {
            state.view.status = Status::Error(PackageError::NotInstalled(identity).to_string());
            return None;
        };
        if !state.claim(&identity, Changing::Uninstalling) {
            return None;
        }
        let package = state.packages.remove(index);
        installation.data.uninstall(&identity);
        // Its development ends, with a build that is running.
        self.developing.end(Some(&identity));
        let components: Vec<PathBuf> = package
            .commands()
            .into_iter()
            .map(|command| command.component)
            .collect();
        if let Ok(runtime) = self.runtime() {
            // Operation components too: every instance of its managed copy.
            let mut all = components.clone();
            if let Ok(manifest) = &package.manifest {
                all.extend(
                    manifest
                        .operations
                        .iter()
                        .map(|operation| package.location.join(&operation.component)),
                );
            }
            runtime.forget(all);
        }
        // Its pause goes with it, in memory only: if the uninstall cannot be
        // recorded, it is put back, as `installed.json` still has it.
        let pause = state.paused.forget(&identity);
        // Its hotkeys are released now, and forgotten once it is uninstalled.
        self.sync_hotkeys(state);
        // Its results kept for root search go, and so does an answer from it
        // being awaited.
        Launcher::forget_indexes(state);
        let open = state
            .open
            .as_ref()
            .is_some_and(|open| components.contains(open));
        if open {
            self.show_root(state, None);
        } else {
            self.refresh(state);
        }
        state.view.status = Status::Running;
        Some(Uninstall {
            package,
            index,
            saved,
            pause,
        })
    }

    /// Uninstalls the package removed by [`Launcher::begin_uninstall`] from
    /// Pane's files, putting it back if that cannot be recorded, and shows
    /// the outcome.
    pub(super) async fn finish_uninstall(&self, epoch: u64, uninstall: Uninstall) {
        let Uninstall {
            package,
            index,
            saved,
            pause,
        } = uninstall;
        let identity = package.identity.clone();
        let title = package.title();
        let installation = self
            .installation
            .clone()
            .expect("begin_uninstall checked there is an installation");
        // The runtime has dropped its instances before its files go; its
        // pending calls were stopped when its generation ended.
        if let Ok(runtime) = self.runtime() {
            runtime.running().await;
        }
        let keeps = |kind| installation.data.count(kind, &identity) != Ok(0);
        let retain = (saved == SavedData::Keep
            && (keeps(DataKind::Settings) || keeps(DataKind::Content)))
        .then(|| title.clone());
        let recorded = {
            let store = installation.store.clone();
            let identity = identity.clone();
            off_thread(move || {
                let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                store.uninstall(&identity, retain)
            })
            .await
        };
        let status = match recorded {
            Err(error) => {
                let mut state = self.lock();
                let enabled = package.enabled;
                let at = index.min(state.packages.len());
                state.packages.insert(at, package);
                installation.data.reinstate(&identity, enabled);
                // Still paused, as `installed.json` still records it.
                if let Some(pause) = pause {
                    installation.data.pause(&identity);
                    state.paused.restore(identity.clone(), pause);
                }
                self.sync_hotkeys(&mut state);
                self.end_uninstall(
                    &mut state,
                    epoch,
                    &identity,
                    Status::Error(format!(
                        "Could not uninstall {title}: {error}. It is still installed and \
                         nothing was deleted."
                    )),
                );
                return;
            }
            Ok(leftover) => {
                let forget_hotkeys = self.forget_hotkeys_of(&mut self.lock(), &identity);
                let forget_aliases = self.forget_aliases_of(&mut self.lock(), &identity);
                let (problems, retained) = {
                    let data = installation.data.clone();
                    let store = installation.store.clone();
                    let identity = identity.clone();
                    let title = title.clone();
                    off_thread(move || {
                        let mut problems = data.remove_uninstalled(&identity, saved);
                        if let Some(Err(error)) = forget_hotkeys.map(|forget| forget()) {
                            problems.push(format!("could not forget its hotkeys: {error}"));
                        }
                        if let Some(Err(error)) = forget_aliases.map(|forget| forget()) {
                            problems.push(format!("could not forget its aliases: {error}"));
                        }
                        let store = &mut store.lock().unwrap_or_else(|p| p.into_inner());
                        let recorded = store.retained().iter().any(|r| r.identity == identity);
                        if !recorded
                            && data.holds_any(&identity)
                            && let Err(error) = store.retain(&identity, title)
                        {
                            problems.push(format!(
                                "could not record that some of its data is kept: {error}"
                            ));
                        }
                        (problems, store.retained())
                    })
                    .await
                };
                self.lock().retained = retained;
                outcome(&title, saved, problems, leftover)
            }
        };
        let mut state = self.lock();
        self.end_uninstall(&mut state, epoch, &identity, status);
    }

    /// Ends an uninstall with `status`, shown if the screen is still the one
    /// of `epoch`: from its confirmation, on the extension list.
    fn end_uninstall(
        &self,
        state: &mut State,
        epoch: u64,
        identity: &PackageIdentity,
        status: Status,
    ) {
        state.release(identity);
        if state.screen_epoch != epoch {
            self.refresh(state);
            return;
        }
        match &state.view.screen {
            Screen::Confirm {
                question: Question::Uninstall(asked),
                ..
            } if asked == identity => {
                let identity = identity.clone();
                self.show_extensions_at(
                    state,
                    |entry| matches!(entry, Entry::AskUninstall(asked) if *asked == identity),
                );
            }
            _ => self.refresh(state),
        }
        state.view.status = status;
    }
}

/// "Saved data: …": how many settings and content records the package with
/// `identity` keeps, the data the user chooses to keep or delete.
fn saved_data(data: &ExtensionData, identity: &PackageIdentity) -> String {
    let kept = data
        .kept_now(&[DataKind::Settings, DataKind::Content])
        .describe(identity);
    format!("Saved data: {}", kept.unwrap_or_else(|| "none".into()))
}

/// The outcome of an uninstall that was recorded: a success only if every
/// file it was to remove is gone.
fn outcome(title: &str, saved: SavedData, mut problems: Vec<String>, left: Leftover) -> Status {
    match left {
        Leftover::None => {}
        Leftover::Listed(path, error) => problems.push(format!(
            "its installed copy in {} could not be removed yet ({error}); Pane removes it when \
             it next starts",
            path.display()
        )),
        Leftover::Unlisted(path, error) => problems.push(format!(
            "its installed copy in {} could not be removed ({error}); delete that folder \
             yourself",
            path.display()
        )),
    }
    if !problems.is_empty() {
        return Status::Error(format!("Uninstalled {title}, but {}.", problems.join("; ")));
    }
    Status::Result(match saved {
        SavedData::Keep => format!("Uninstalled {title}; its settings and content are kept"),
        SavedData::Delete => format!("Uninstalled {title} and deleted its saved data"),
    })
}
