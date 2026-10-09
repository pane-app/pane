//! Pane's files sample in Rust: the same contract as the Files default
//! extension (guests/files, "Search Files") and the JavaScript and
//! TypeScript samples. Its package's `pane.json` sets `"fileIndex": true`,
//! so Pane keeps its file index of the home folder current while the
//! sample is enabled, and the sample searches it with
//! `pane_extension::file_index`. Once open, the command owns the launcher's
//! search field (`"search": true`) and answers each text typed with the
//! entries the index finds, named by the ids Pane gave them; root search
//! gets the same entries as `open-file` results (`"rootResults": true`).
//! Pane shows each entry's own name and folder and gives it its file
//! actions (Open, Show in Explorer, Open With…, Copy Path, Copy File, Move
//! to Recycle Bin; for a program, Enter shows it and only Run runs it),
//! which it performs itself.
#![no_std]

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::file_index::{self, FileEntry, IndexState, SearchOptions};
use pane_extension::root::{RootAction, RootResult, WallTime};
use pane_extension::search::SearchResult;
use pane_extension::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct Sample;
pane_extension::export!(Sample);
pane_extension::root::export!(Sample);
pane_extension::search::export!(Sample);

/// The most entries one query lists.
const MAX_RESULTS: u32 = 20;

/// The entries the index finds for `query`, best first; none for a blank
/// query.
fn found(query: &str) -> Result<Vec<FileEntry>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    file_index::search(query, SearchOptions::first(MAX_RESULTS))
}

impl Command for Sample {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let status = file_index::status();
        let state = match status.state {
            IndexState::Off => "off",
            IndexState::Building => "being built",
            IndexState::Current => "current",
            IndexState::Stopped => "stopped",
        };
        Ok(
            List::new("Rust files sample").item(Item::new("status", "What is searched").subtitle(
                format!(
                    "Pane's file index, {state}: {} entries (Rust)",
                    status.entries
                ),
            )),
        )
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

impl pane_extension::search::Guest for Sample {
    async fn search(_command: String, query: String) -> Result<Vec<SearchResult>, String> {
        Ok(found(&query)?
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

impl pane_extension::root::Guest for Sample {
    async fn results_for(query: String, _at: WallTime) -> Result<Vec<RootResult>, String> {
        Ok(found(&query)?
            .into_iter()
            .map(|entry| RootResult {
                title: entry.name,
                id: entry.path,
                subtitle: None,
                action: RootAction::OpenFile(entry.id),
                answer: None,
            })
            .collect())
    }
}
