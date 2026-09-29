//! The folder a package is granted, through Pane's own rows in its
//! commands, and opening the files its listing found (see `crate::files`).
//!
//! A package whose manifest sets `"folderAccess": true` has Pane's "Choose
//! folder…" row first in each of its commands, and "Stop sharing …" once a
//! folder is granted. The window answers "Choose folder…" with the system's
//! folder picker and hands the choice to [`Launcher::grant_folder`]; the
//! extension takes no part in it and never sees the path.

use std::future::Future;
use std::path::{Path, PathBuf};

use super::{Entry, Launcher, Row, State, Status, off_thread, owner};
use crate::links;
use crate::packages::PackageIdentity;

/// Pane's rows at the top of a command of the package with `identity`,
/// which asks for access to a folder: choosing it, and taking it back once
/// granted.
pub(super) fn folder_rows(state: &State, identity: &PackageIdentity) -> (Vec<Row>, Vec<Entry>) {
    let title = state.title_of(identity);
    let granted = state
        .files
        .as_ref()
        .and_then(|files| files.granted(&identity.key()));
    let mut rows = vec![Row {
        id: "pane.choose-folder".into(),
        title: "Choose folder…".into(),
        subtitle: Some(match &granted {
            Some(folder) => format!(
                "Pane lets {title} list only {} · Enter chooses another",
                folder.display()
            ),
            None => format!("Pane lets {title} list only a folder you choose; none yet"),
        }),
        unavailable: None,
    }];
    let mut entries = vec![Entry::ChooseFolder(identity.clone())];
    if granted.is_some() {
        rows.push(Row {
            id: "pane.stop-sharing-folder".into(),
            title: format!("Stop sharing the folder with {title}"),
            subtitle: Some("The folder itself is not changed".into()),
            unavailable: None,
        });
        entries.push(Entry::StopSharingFolder(identity.clone()));
    }
    (rows, entries)
}

/// Replaces Pane's folder rows at the top of the open command of the package
/// with `identity`, if it is on screen, after its grant changed.
fn refresh_folder_rows(state: &mut State, identity: &PackageIdentity) {
    let shown = state
        .open
        .as_ref()
        .and_then(|component| owner(&state.packages, component))
        .is_some_and(|package| package.identity == *identity);
    if !shown {
        return;
    }
    let old = state
        .entries
        .iter()
        .take_while(|entry| matches!(entry, Entry::ChooseFolder(_) | Entry::StopSharingFolder(_)))
        .count();
    let (rows, entries) = folder_rows(state, identity);
    state.view.rows.splice(0..old, rows);
    state.entries.splice(0..old, entries);
    state.view.selected = Some(0);
}

impl Launcher {
    /// The package whose folder the selected row asks the user to choose,
    /// if it does. Activating it does nothing in the launcher: the window
    /// asks for a folder with the system's picker and calls
    /// [`Launcher::grant_folder`].
    pub fn folder_to_choose(&self) -> Option<PackageIdentity> {
        let state = self.lock();
        let entry = state
            .view
            .selected
            .and_then(|index| state.entries.get(index));
        match entry {
            Some(Entry::ChooseFolder(identity)) => Some(identity.clone()),
            _ => None,
        }
    }

    /// Grants the package with `identity` the folder `folder`, which the user
    /// chose with the system's picker: Pane checks it (not the file system's
    /// root, the home folder itself, a hidden folder or a network path) and
    /// records it in its own `folders.json`, replacing an earlier grant. The
    /// status says how it went.
    pub fn grant_folder(
        &self,
        identity: &PackageIdentity,
        folder: &Path,
    ) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        let identity = identity.clone();
        let folder = folder.to_path_buf();
        let files = {
            let mut state = self.lock();
            state.view.status = Status::Running;
            state.files.clone()
        };
        async move {
            let granted = match files {
                Some(files) => {
                    let owner = identity.key();
                    off_thread(move || files.grant(&owner, &folder)).await
                }
                None => Err("Pane's extension runtime is unavailable".into()),
            };
            launcher.show_grant(&identity, granted);
        }
    }

    fn show_grant(&self, identity: &PackageIdentity, granted: Result<PathBuf, String>) {
        let mut state = self.lock();
        let title = state.title_of(identity);
        state.view.status = match granted {
            Ok(folder) => {
                let name = links::file_name(&folder.to_string_lossy());
                Status::Result(format!("{title} may now list “{name}”"))
            }
            Err(problem) => Status::Error(format!("Pane did not grant the folder: {problem}")),
        };
        refresh_folder_rows(&mut state, identity);
    }

    /// Takes back the folder granted to the package with `identity`.
    pub(super) async fn stop_sharing_folder(&self, identity: PackageIdentity) {
        let files = {
            let mut state = self.lock();
            state.view.status = Status::Running;
            state.files.clone()
        };
        let revoked = match files {
            Some(files) => {
                let owner = identity.key();
                off_thread(move || files.revoke(&owner)).await
            }
            None => Err("Pane's extension runtime is unavailable".into()),
        };
        let mut state = self.lock();
        let title = state.title_of(&identity);
        state.view.status = match revoked {
            Ok(()) => Status::Result(format!("{title} no longer lists a folder")),
            Err(problem) => Status::Error(problem),
        };
        refresh_folder_rows(&mut state, &identity);
    }

    /// Opens the file with id `id` from the latest listing of the package
    /// with identity key `owner`, named `name`, once the host has checked it
    /// again, off the calling thread, and reports the outcome while the
    /// screen is the one it was opened from. The status names the file the
    /// host found, not what the extension called it.
    pub(super) async fn open_file(&self, epoch: u64, owner: String, id: String, name: String) {
        let links = self.links.clone();
        let files = self.lock().files.clone();
        let opened = off_thread(move || {
            let files = files.ok_or("Pane's extension runtime is unavailable")?;
            let path = files.checked_file(&owner, &id)?;
            links.open_file(&path)
        })
        .await;
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        state.view.status = match opened {
            Ok(()) => Status::Result(format!("Opened {name}")),
            Err(reason) => Status::Error(format!("Could not open {name}: {reason}")),
        };
    }
}
