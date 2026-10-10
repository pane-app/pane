//! The folder a package is granted, through Pane's own rows in its
//! commands, and the rows of the files its listing found (see
//! `crate::files`), whose actions Pane performs (see `own_actions`); and
//! the window's keys over a typed folder's entries (#204), Tab completing
//! the query to a folder of them and Shift+Tab removing the query's last
//! path component.
//!
//! A package whose manifest sets `"folderAccess": true` has Pane's "Choose
//! folder…" row first in each of its commands, and "Stop sharing …" once a
//! folder is granted. The window answers "Choose folder…" with the system's
//! folder picker and hands the choice to [`Launcher::grant_folder`]; the
//! extension takes no part in it and never sees the path.

use std::future::Future;
use std::path::{Path, PathBuf};

use super::{Entry, Launcher, Row, State, Status, off_thread, owner, typed_query};
use crate::files::FileAccess;
use crate::links;
use crate::packages::PackageIdentity;

/// A file of the file index (#175), of a package's granted folder, or of
/// the folder the user typed (#204), as a row lists it: root search's file
/// results and Search Files' results. Pane performs its actions itself
/// (see `own_actions`), checking it again first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileRow {
    /// The identity key of the package whose search or listing found it.
    pub(super) owner: String,
    /// The id Pane gave it in that search or listing.
    pub(super) id: String,
    /// Its own name, as Pane found it.
    pub(super) name: String,
    /// Whether it is a program or script, as found: Enter reveals it
    /// instead of opening it.
    pub(super) program: bool,
    /// The component of the command that found it.
    pub(super) component: PathBuf,
    /// Found in the file index, rather than a granted folder's listing or
    /// the folder the user typed.
    pub(super) indexed: bool,
    /// An entry of the folder the user typed (#204), rather than of the
    /// file index or a granted folder's listing: Tab completes the query
    /// to its path when it is a folder.
    pub(super) typed: bool,
    /// A folder (the index and a typed folder find folders): Enter opens
    /// it in the file manager.
    pub(super) folder: bool,
    /// Its path, for its system icon (#142) and Tab's completion: the
    /// index's and the typed folder's entries only.
    pub(super) path: Option<PathBuf>,
}

/// The row for the file with id `id` that a search of the file index by the
/// package with identity key `owner` found, or else of that package's
/// granted folder's latest listing, or else of the folder the user typed
/// (#204), as Pane names it (its own name, and its folder: `~/…` for the
/// index and a typed folder, "File in" the granted folder's), with
/// `row_id`, found by the command in `component`; `None` for an id Pane did
/// not give.
pub(super) fn file_row(
    files: &FileAccess,
    owner: &str,
    component: &Path,
    id: String,
    row_id: String,
) -> Option<(Row, FileRow)> {
    if let Some(entry) = files.indexer().known(owner, &id) {
        let row = Row {
            id: row_id,
            title: entry.name.clone(),
            subtitle: Some(entry.folder),
            unavailable: None,
        };
        let file = FileRow {
            owner: owner.to_owned(),
            id,
            name: entry.name,
            program: entry.program,
            component: component.to_path_buf(),
            indexed: true,
            typed: false,
            folder: entry.kind == crate::file_index::EntryKind::Folder,
            path: Some(entry.path),
        };
        return Some((row, file));
    }
    if let Some(known) = files.known(owner, &id) {
        let row = Row {
            id: row_id,
            title: known.name.clone(),
            subtitle: Some(format!("File in {}", known.within)),
            unavailable: None,
        };
        let file = FileRow {
            owner: owner.to_owned(),
            id,
            name: known.name,
            program: known.program,
            component: component.to_path_buf(),
            indexed: false,
            typed: false,
            folder: false,
            path: None,
        };
        return Some((row, file));
    }
    // An entry of the folder the user typed (#204).
    let known = files.typed().known(owner, &id)?;
    let row = Row {
        id: row_id,
        title: known.name.clone(),
        subtitle: Some(known.within),
        unavailable: None,
    };
    let file = FileRow {
        owner: owner.to_owned(),
        id,
        name: known.name,
        program: known.program,
        component: component.to_path_buf(),
        indexed: false,
        typed: true,
        folder: known.folder,
        path: Some(known.path),
    };
    Some((row, file))
}

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

    /// The query Tab completes the selected row to (#204): the row is a
    /// folder of the entries listed for the folder the query typed, and
    /// the completion is that folder's path with a separator after it, so
    /// the query lists the folder's own entries. `None` when the selected
    /// row is not one, so Tab goes on to focus the next field (the
    /// argument fields, #205).
    pub fn typed_folder_completion(&self) -> Option<String> {
        let state = self.lock();
        let index = state.view.selected?;
        let Entry::File(file) = state.entries.get(index)? else {
            return None;
        };
        if !file.typed || !file.folder {
            return None;
        }
        let separator = if cfg!(windows) { "\\" } else { "/" };
        Some(format!("{}{separator}", file.path.as_ref()?.display()))
    }

    /// The query with its last path component removed and a separator
    /// kept after what remains, what Shift+Tab types (#204): the query
    /// names the typed folder above. `None` when the query is not a
    /// path, so Shift+Tab goes on to focus the previous field.
    pub fn typed_path_parent(&self) -> Option<String> {
        let state = self.lock();
        if !matches!(state.typed.as_ref()?, typed_query::TypedQuery::Path(_)) {
            return None;
        }
        typed_query::parent(state.view.query()?)
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
}
