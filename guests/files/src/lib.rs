//! Pane's file search, a default extension: **Search Files**. The user
//! grants it one folder through Pane's own "Choose folder…" row in its
//! command (its `pane.json` sets `"folderAccess": true`). Once open, the
//! command owns the launcher's search field (`"search": true`): it lists
//! the files of that folder whose names (or folders) match what is typed,
//! and so does root search (`"rootResults": true`). Pane gives each file
//! its actions (Open, Reveal in Explorer, Open With…, Copy Path, Copy File,
//! Move to Recycle Bin; for a program or script, Enter reveals it and only
//! Run runs it) and performs them itself.
//!
//! Pane's host owns the grant, lists the folder under its scan limits and
//! checks each file again before acting on it; the extension only matches
//! the listing Pane gives it and names files by the ids Pane gave them,
//! never by a path. Disabling the package removes its results and stops
//! any listing.
#![no_std]

mod matching;

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::files::{self, FolderState};
use pane_guest::root::{RootAction, RootResult};
use pane_guest::search::SearchResult;
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct Files;
pane_guest::export!(Files);
pane_guest::root::export!(Files);
pane_guest::search::export!(Files);

/// What Pane lists, in its own limits.
fn policy() -> String {
    let limits = files::limits();
    format!(
        "Regular files of the folder and its subfolders, {} deep, at most {} files and {} \
         entries looked at; not hidden files, links or folders Pane cannot read",
        limits.depth, limits.files, limits.entries
    )
}

/// Runs the action of the item `item_id`: a toast saying what is searched.
async fn act(item_id: &str) -> Result<(), String> {
    match item_id {
        "policy" => {
            show_toast(Toast::success(policy()));
            Ok(())
        }
        _ => Err(format!("unknown item: {item_id}")),
    }
}

/// The files of the granted folder `query` finds, best first, each as the
/// id Pane gave it and its path below the folder; none while no folder is
/// granted or Pane is still listing it (it asks again when it is done).
fn found(query: &str) -> Result<Vec<(String, String)>, String> {
    let listing = match files::list_folder()
        .map_err(|problem| format!("cannot search the granted folder: {problem}"))?
    {
        FolderState::Ready(listing) => listing,
        FolderState::NotGranted | FolderState::Listing => return Ok(Vec::new()),
    };
    Ok(matching::matching(&listing.files, query)
        .into_iter()
        .map(|file| (file.id.clone(), file.relative.clone()))
        .collect())
}

impl Command for Files {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Search Files").item(
            Item::new("policy", "What is searched")
                .subtitle(policy())
                .on_action(|| act("policy")),
        ))
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl pane_guest::search::Guest for Files {
    /// The files the text typed in Search Files' field finds, each named by
    /// the id Pane gave it: Pane lists it with its own name and folder, and
    /// gives it its file actions.
    async fn search(_command: String, query: String) -> Result<Vec<SearchResult>, String> {
        Ok(found(&query)?
            .into_iter()
            .map(|(id, relative)| SearchResult {
                title: matching::last_name(&relative).into(),
                id: relative,
                subtitle: None,
                file: Some(id),
            })
            .collect())
    }
}

impl pane_guest::root::Guest for Files {
    /// The files the query typed in root search finds, each opening the
    /// file (Pane gives it the same actions as in Search Files).
    async fn results_for(query: String) -> Result<Vec<RootResult>, String> {
        Ok(found(&query)?
            .into_iter()
            .map(|(id, relative)| RootResult {
                // Pane shows the file's own name and folder, whatever these
                // say.
                title: matching::last_name(&relative).into(),
                id: relative,
                subtitle: None,
                action: RootAction::OpenFile(id),
            })
            .collect())
    }
}
