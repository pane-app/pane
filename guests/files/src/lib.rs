//! Pane's file search, a default extension: the user grants it one folder
//! through Pane's own "Choose folder…" row in its command (its `pane.json`
//! sets `"folderAccess": true`), and typing into root search lists the files
//! in that folder whose names (or folders) match; invoking one opens it with
//! the system's handler for its type. Pane's host owns the grant, lists the
//! folder under its scan limits and checks each file again before opening
//! it; the extension only matches the listing Pane gives it and answers
//! `open-file` results with the ids Pane gave the files. Disabling the
//! package removes its results and stops any listing.
#![no_std]

mod matching;

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::files::{self, FolderState};
use pane_guest::root::{RootAction, RootResult};
use pane_guest::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct Files;
pane_guest::export!(Files);
pane_guest::root::export!(Files);

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

impl Command for Files {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Files").item(
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

impl pane_guest::root::Guest for Files {
    /// The files of the granted folder the query finds, each opening the
    /// file; none while no folder is granted or Pane is still listing it
    /// (it asks again when it is done).
    async fn results_for(query: String) -> Result<Vec<RootResult>, String> {
        let listing = match files::list_folder()
            .map_err(|problem| format!("cannot search the granted folder: {problem}"))?
        {
            FolderState::Ready(listing) => listing,
            FolderState::NotGranted | FolderState::Listing => return Ok(Vec::new()),
        };
        Ok(matching::matching(&listing.files, &query)
            .into_iter()
            .map(|file| RootResult {
                id: file.relative.clone(),
                // Pane shows the file's own name and folder, whatever these
                // say.
                title: matching::last_name(&file.relative).into(),
                subtitle: None,
                action: RootAction::OpenFile(file.id.clone()),
            })
            .collect())
    }
}
