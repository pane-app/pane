//! Pane's file search, a default extension: **Search Files**, over Pane's
//! file index of the home folder (#126, #175; its `pane.json` sets
//! `"fileIndex": true`, so Pane keeps the index current while Files is
//! enabled, and stops watching the moment it is disabled). Root search asks
//! it (`"rootResults": true`) and lists the best few files after what is
//! found by title, with a row opening Search Files with the query typed;
//! once open, the command owns the launcher's search field
//! (`"search": true`) and lists more.
//!
//! Installed as Pane's default extension, Search Files is drawn by Pane
//! itself, as Raycast's File Search is (#177): Pane lists the index for it
//! — Recently Used before typing, a type dropdown, more rows as the list
//! scrolls, a detail with an image's preview and the file's Metadata —
//! and this command's own list is not shown nor its `search` asked. A copy
//! installed from a folder keeps them: its list says what is searched, and
//! its `search` answers the best 50 entries.
//!
//! Pane's host keeps the index, ranks the entries and checks each again
//! before acting on it; the extension only asks the index and names the
//! entries by the ids Pane gave them, never by a path. Pane gives each its
//! actions (Open, Show in Explorer, Open With…, Copy Path, Copy Name, Copy
//! File, Move to Recycle Bin) and performs them itself: Enter on a program shows it in
//! the file manager and only Run runs it.
#![no_std]

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::file_index::{self, FileEntry, IndexState, SearchOptions};
use pane_extension::root::{RootAction, RootResult, WallTime};
use pane_extension::search::SearchResult;
use pane_extension::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct Files;
pane_extension::export!(Files);
pane_extension::root::export!(Files);
pane_extension::search::export!(Files);

/// The most files root search lists (Pane lists 5 at most, then a row
/// searching them all).
const ROOT_RESULTS: u32 = 5;

/// The most files one search in Search Files' own field lists.
const SEARCH_RESULTS: u32 = 50;

/// What the index is doing, for people.
fn status() -> String {
    let status = file_index::status();
    let reason = status.reason.map(|reason| format!(" ({reason})"));
    let reason = reason.as_deref().unwrap_or("");
    match status.state {
        IndexState::Off => format!("File search is off{reason}"),
        IndexState::Building => format!(
            "Indexing your files… {} found so far{reason}",
            status.found.max(status.entries)
        ),
        IndexState::Current => format!(
            "{} files and folders of your home folder are indexed{reason}",
            status.entries
        ),
        IndexState::Stopped => format!("File search stopped{reason}"),
    }
}

/// Runs the action of the item `item_id`: a toast saying what is searched.
async fn act(item_id: &str) -> Result<(), String> {
    match item_id {
        "status" => {
            show_toast(Toast::success(status()));
            Ok(())
        }
        _ => Err(format!("unknown item: {item_id}")),
    }
}

/// The entries `query` finds, best first, at most `limit`.
fn found(query: &str, limit: u32) -> Result<Vec<FileEntry>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    file_index::search(query, SearchOptions::first(limit))
        .map_err(|problem| format!("cannot search your files: {problem}"))
}

impl Command for Files {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Search Files").item(
            Item::new("status", "What is searched")
                .subtitle(status())
                .on_action(|| act("status")),
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

impl pane_extension::search::Guest for Files {
    /// The entries the text typed in Search Files' field finds, each named
    /// by the id Pane gave it: Pane lists it with its own name and folder,
    /// and gives it its file actions.
    async fn search(_command: String, query: String) -> Result<Vec<SearchResult>, String> {
        Ok(found(&query, SEARCH_RESULTS)?
            .into_iter()
            .map(|entry| SearchResult {
                title: entry.name,
                id: entry.path,
                subtitle: None,
                file: Some(entry.id),
            })
            .collect())
    }
}

impl pane_extension::root::Guest for Files {
    /// The best few entries the query typed in root search finds, each
    /// opening the entry (Pane gives it the same actions as in Search
    /// Files).
    async fn results_for(query: String, _at: WallTime) -> Result<Vec<RootResult>, String> {
        Ok(found(&query, ROOT_RESULTS)?
            .into_iter()
            .map(|entry| RootResult {
                // Pane shows the entry's own name and folder, whatever
                // these say.
                title: entry.name,
                id: entry.path,
                subtitle: None,
                action: RootAction::OpenFile(entry.id),
                answer: None,
            })
            .collect())
    }
}
