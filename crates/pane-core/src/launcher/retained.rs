//! Deleting retained data: the extension data Pane keeps for a package
//! identity that is not installed, recorded with the title it had when it
//! was uninstalled.
//!
//! The extension list shows one row per such identity, after the installed
//! packages' rows, naming its source and what is kept. Choosing it asks
//! first; confirming deletes every kind of data Pane keeps for that identity
//! (settings, content, and any cache or credentials an earlier removal could
//! not delete) and then drops its record from `installed.json`. Pane does this itself:
//! the package's code is gone, and nothing is downloaded or run. Other
//! identities' data, the source folder and anything outside Pane's data
//! folder are never touched.
//!
//! The record is dropped only once every kind is deleted, so data that
//! could not be deleted stays listed and can be deleted again once its file
//! is repaired. `installed.json` is read again first: if another Pane on the
//! same data folder installed the same source again meanwhile, the data is
//! its package's again and nothing is deleted.

use std::future::Future;
use std::time::Instant;

use super::off_thread;
use super::{Changing, Entry, Launcher, LauncherView, Question, Row, Screen, State, Status};
use crate::extension_data::{DataKind, ExtensionData};
use crate::packages::{PackageError, PackageIdentity, RetainedData, Standing};

impl Launcher {
    /// The identities that are not installed but whose extension data Pane
    /// keeps, each with its title when it was uninstalled, in the order they
    /// were uninstalled.
    pub fn retained_data(&self) -> Vec<RetainedData> {
        self.lock().retained.clone()
    }

    /// Deletes the retained data of `identity`, which is not installed,
    /// without running or downloading its extension (see the module
    /// documentation). Await the returned future for the outcome.
    pub fn delete_retained_data(
        &self,
        identity: &PackageIdentity,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let retained = self.begin_delete_retained(&mut state, identity.clone());
        let epoch = state.screen_epoch;
        drop(state);
        let launcher = self.clone();
        async move {
            if let Some(retained) = retained {
                launcher.finish_delete_retained(epoch, retained).await;
            }
        }
    }

    /// Asks whether to delete the retained data of `identity`, saying what
    /// is kept and what is never touched.
    pub(super) fn show_delete_retained(&self, state: &mut State, identity: &PackageIdentity) {
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
        state.next_screen();
        // Cancel first, so that Enter keeps the data, as uninstalling does.
        state.entries = vec![Entry::Cancel, Entry::DeleteRetained(identity.clone())];
        let source = match identity.local_folder() {
            Some(folder) => format!("Its source folder {}", folder.display()),
            None => "Its source".into(),
        };
        let kept = installation
            .data
            .kept_now(&DataKind::ALL)
            .describe(identity);
        let details = vec![
            format!("From {identity}"),
            format!(
                "{title} is not installed. Pane deletes the data it keeps for this source \
                 itself; the extension is not downloaded and does not run."
            ),
            format!("Retained data: {}", kept.unwrap_or_else(|| "none".into())),
            format!(
                "{source}, files it saved elsewhere and other extensions' data are not touched."
            ),
        ];
        let screen = Screen::Confirm {
            question: Question::DeleteRetained(identity.clone()),
            details,
        };
        state.view = LauncherView::new(screen, format!("Delete the retained data of {title}?"))
            .with_rows(vec![
                choice("cancel", "Cancel", "Keep it"),
                choice(
                    "delete",
                    "Delete retained data",
                    "Delete it now; installing it again from this source starts with nothing",
                ),
            ]);
    }

    /// Begins deleting the retained data of `identity`, explaining why not
    /// and returning `None` if it is installed, has no retained data or is
    /// already being deleted.
    pub(super) fn begin_delete_retained(
        &self,
        state: &mut State,
        identity: PackageIdentity,
    ) -> Option<RetainedData> {
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        if let Some(package) = state.package(&identity) {
            state.view.status = Status::Error(format!(
                "{} is installed: its data is deleted by uninstalling it",
                package.title()
            ));
            return None;
        }
        let Some(retained) = state
            .retained
            .iter()
            .find(|r| r.identity == identity)
            .cloned()
        else {
            state.view.status = Status::Error(format!("No data is retained for {identity}"));
            return None;
        };
        if !state.claim(&identity, Changing::DeletingRetained) {
            return None;
        }
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        Some(retained)
    }

    /// Deletes the retained data begun by
    /// [`Launcher::begin_delete_retained`], then shows the extension list
    /// with the outcome.
    pub(super) async fn finish_delete_retained(&self, epoch: u64, retained: RetainedData) {
        let RetainedData { identity, title } = retained;
        let installation = self
            .installation
            .clone()
            .expect("begin_delete_retained checked there is an installation");
        let (deleted, now_retained) = {
            let identity = identity.clone();
            off_thread(move || {
                // The store stays locked throughout, so this Pane cannot
                // install the same source again meanwhile and find half its
                // data.
                let mut store = installation.store.lock().unwrap_or_else(|p| p.into_inner());
                let deleted = match store.standing_on_disk(&identity) {
                    Err(error) => Deletion::Unchecked(error),
                    Ok(Standing::Retained) => {
                        let problems = installation.data.remove_retained(&identity);
                        if !problems.is_empty() {
                            Deletion::Partial(problems)
                        } else {
                            match store.forget_retained(&identity) {
                                Ok(()) => Deletion::Done,
                                Err(error) => Deletion::Unlisted(error),
                            }
                        }
                    }
                    // Another Pane on the same data folder installed it
                    // again, or deleted its data: nothing is deleted, and
                    // this Pane stops listing it.
                    Ok(Standing::Installed) => {
                        let _ = store.forget_retained(&identity);
                        Deletion::Installed
                    }
                    Ok(Standing::Neither) => {
                        let _ = store.forget_retained(&identity);
                        Deletion::Gone
                    }
                };
                (deleted, store.retained())
            })
            .await
        };
        let status = match deleted {
            Deletion::Done => Status::Result(format!("Deleted the retained data of {title}")),
            Deletion::Unlisted(error) => Status::Error(format!(
                "Deleted the retained data of {title}, but could not remove it from the list: \
                 {error}. It stays listed, keeping nothing, until it is deleted again."
            )),
            Deletion::Partial(problems) => {
                let files = if problems.len() == 1 {
                    "that file"
                } else {
                    "those files"
                };
                Status::Error(format!(
                    "Could not delete all the retained data of {title}: {}. What could not be \
                     deleted stays listed: repair or delete {files}, then delete it again.",
                    problems.join("; ")
                ))
            }
            Deletion::Unchecked(error) => Status::Error(format!(
                "Could not delete the retained data of {title}: {error}. Nothing was deleted."
            )),
            Deletion::Installed => Status::Error(format!(
                "{title} was installed again from the same source by another Pane using this \
                 data folder, so its data is in use and nothing was deleted"
            )),
            Deletion::Gone => Status::Error(format!(
                "The data of {title} is no longer kept, so nothing was deleted"
            )),
        };
        let mut state = self.lock();
        state.release(&identity);
        state.retained = now_retained;
        let asked = matches!(
            &state.view.screen,
            Screen::Confirm { question: Question::DeleteRetained(asked), .. } if *asked == identity
        );
        if state.screen_epoch != epoch || !asked {
            self.refresh(&mut state);
            if state.screen_epoch == epoch {
                state.view.status = status;
            }
            return;
        }
        self.show_extensions_at(
            &mut state,
            |entry| matches!(entry, Entry::AskDeleteRetained(asked) if *asked == identity),
        );
        state.view.status = status;
    }
}

/// How deleting retained data ended.
enum Deletion {
    /// Every kind is deleted and the record dropped.
    Done,
    /// Every kind is deleted, but the record could not be dropped.
    Unlisted(PackageError),
    /// These kinds could not be deleted; the record stays.
    Partial(Vec<String>),
    /// `installed.json` could not be read, so nothing was deleted.
    Unchecked(PackageError),
    /// The same source is installed again on disk; nothing was deleted.
    Installed,
    /// Nothing is kept for it on disk any more; nothing was deleted.
    Gone,
}

/// The extension list's rows for `retained`, one per identity in the order
/// they were uninstalled, each naming its source and what is kept.
pub(super) fn rows(retained: &[RetainedData], data: &ExtensionData) -> (Vec<Row>, Vec<Entry>) {
    if retained.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // Each kind's file is read once for every row; each row says how many
    // of its identity's values cannot be read on this computer (#130).
    let kept = data.kept_now(&DataKind::ALL);
    retained
        .iter()
        .map(|retained| {
            let kept = kept
                .describe_unreadable(&retained.identity)
                .unwrap_or_else(|| "nothing".into());
            let row = Row {
                id: format!("delete-retained:{}", retained.identity.key()),
                title: format!("Delete retained data of {}", retained.title),
                subtitle: Some(format!(
                    "Not installed · keeps {kept} · {}",
                    retained.identity
                )),
                unavailable: None,
            };
            (row, Entry::AskDeleteRetained(retained.identity.clone()))
        })
        .unzip()
}
