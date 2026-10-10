//! Pane's file search, a default extension: **Search Files**, over Pane's
//! file index of the home folder (#126, #175; its `pane.json` sets
//! `"fileIndex": true`, so Pane keeps the index current while Files is
//! enabled, and stops watching the moment it is disabled). Root search asks
//! it (`"rootResults": true`) and lists the best few files after what is
//! found by title, with a row opening Search Files with the query typed;
//! once open, the command owns the launcher's search field
//! (`"search": true`) and lists more.
//!
//! Two more commands are declared for a path typed into root search
//! (#195, `"matches": "file-path"`), listed only for one and receiving
//! the resolved path as their launch record's fallback text: **Open**
//! (`open`) opens it with the system's handler — a program or script is
//! shown in the file manager instead, never run, as file search's own
//! Enter does (ADR 0037) — and **Reveal in File Explorer** (`reveal`)
//! shows it selected there. Each closes the window after it acts, as the
//! SDK's standard actions do.
//!
//! A path-like query ending in a separator lists the folder it names
//! (#204): Pane's host lists the folder's direct entries for the command
//! (`pane:extension/typed-folder`, at most 500, folders first and each in
//! name order), and it answers them as it does the index's files, by the
//! ids Pane gave them, whatever the extension titled them. A folder that
//! cannot be listed — not there, a file, not a path — lists nothing.
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
use pane_extension::system;
use pane_extension::typed_folder::{self, FolderEntry};
use pane_extension::window::{self, PopToRootType};
use pane_extension::{
    Command, CustomView, FieldValue, FormError, Item, LaunchRecord, List, NoCustomView,
};

struct Files;
pane_extension::export!(Files);
pane_extension::root::export!(Files);
pane_extension::search::export!(Files);

/// The most files root search lists (Pane lists 5 at most, then a row
/// searching them all).
const ROOT_RESULTS: u32 = 5;

/// The most files one search in Search Files' own field lists.
const SEARCH_RESULTS: u32 = 50;

/// The commands' ids in `pane.json`.
const OPEN: &str = "open";
const REVEAL: &str = "reveal";

/// The types whose names say that opening a file runs it: a program, a
/// script, a shortcut or an installer, the same on every system, as Pane's
/// host decides (`crates/pane-core/src/files.rs`, `program_named`). A path
/// typed into root search is checked by its name alone: a pure WASI guest
/// cannot read the file system, and the file index knows a program the
/// same way.
const PROGRAM_EXTENSIONS: &[&str] = &[
    // Windows
    "exe", "bat", "cmd", "com", "lnk", "js", "jse", "vbs", "vbe", "wsf", "wsh", "hta", "msi", "msp",
    "scr", "pif", "ps1", "cpl", "reg", "url", // macOS
    "app", "command", "tool", "terminal", "workflow", // Linux
    "desktop",
];

/// Whether `path`'s name alone says that opening it runs a program: its
/// type is one of [`PROGRAM_EXTENSIONS`], compared caselessly, or a folder
/// above it is a macOS application bundle (`.app`).
fn program_named(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    if extension.is_some_and(|extension| {
        PROGRAM_EXTENSIONS
            .iter()
            .any(|program| program.eq_ignore_ascii_case(extension))
    }) {
        return true;
    }
    // A folder above the path ending in `.app` is a macOS application
    // bundle.
    let mut folder = path;
    while let Some(at) = folder.rfind(['/', '\\']) {
        folder = &folder[..at];
        if folder.ends_with(".app") {
            return true;
        }
    }
    false
}

/// The resolved path a command declared for a typed one received as its
/// launch record's fallback text.
fn typed_path(launch: &LaunchRecord, what: &str) -> Result<String, String> {
    launch
        .fallback_text
        .clone()
        .ok_or_else(|| format!("{what} needs a file or folder path typed in root search"))
}

/// Open (#195): opens the path typed into root search with the system's
/// handler — a program or script is shown in the file manager instead,
/// never run, as file search's own Enter does (ADR 0037) — then closes the
/// window, as the SDK's standard actions do.
fn open_typed(launch: &LaunchRecord) -> Result<(), String> {
    let path = typed_path(launch, "Open")?;
    if program_named(&path) {
        system::reveal(&path)?;
    } else {
        system::open(&path, None)?;
    }
    window::close(false, PopToRootType::Default);
    Ok(())
}

/// Reveal in File Explorer (#195): shows the path typed into root search
/// selected in the file manager, then closes the window, as the SDK's
/// standard action does.
fn reveal_typed(launch: &LaunchRecord) -> Result<(), String> {
    let path = typed_path(launch, "Reveal in File Explorer")?;
    system::reveal(&path)?;
    window::close(false, PopToRootType::Default);
    Ok(())
}

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

/// The entries the index finds, best first, at most `limit`.
fn found(query: &str, limit: u32) -> Result<Vec<FileEntry>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    file_index::search(query, SearchOptions::first(limit))
        .map_err(|problem| format!("cannot search your files: {problem}"))
}

/// The entries of the folder the path typed into root search names, when
/// the query is a path ending in a separator (#204): `None` when it is
/// not one, or the folder cannot be listed — a missing folder lists
/// nothing.
fn typed(query: &str) -> Option<Vec<FolderEntry>> {
    let query = query.trim();
    if !query.ends_with(['/', '\\']) {
        return None;
    }
    // Pane resolves what the user typed: `~` to the home folder,
    // `file://` taken off. The bounds are Pane's, not the command's.
    typed_folder::list(query).ok().map(|listing| listing.entries)
}

impl Command for Files {
    type CustomView = NoCustomView;

    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            OPEN => open_typed(&launch),
            REVEAL => reveal_typed(&launch),
            other => Err(format!(
                "`{other}` opens a screen; it has no run entry point"
            )),
        }
    }

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
    /// Files); a query that is a path ending in a separator lists the
    /// folder's own entries instead (#204).
    async fn results_for(query: String, _at: WallTime) -> Result<Vec<RootResult>, String> {
        if let Some(entries) = typed(&query) {
            return Ok(entries
                .into_iter()
                .map(|entry| RootResult {
                    // Pane shows the entry's own name and folder, whatever
                    // these say.
                    title: entry.name,
                    id: entry.id.clone(),
                    subtitle: None,
                    action: RootAction::OpenFile(entry.id),
                    answer: None,
                })
                .collect());
        }
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
