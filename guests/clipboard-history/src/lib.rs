//! Pane's clipboard history, a default extension: Pane keeps the text the
//! user copies on this computer from the first start (ADR 0042), and the
//! command lists it, newest first. Each kept item has three actions: Paste
//! (Enter) pastes it into the application that was in front, or, where
//! Pane cannot paste yet, copies it and says so in a HUD; Copy copies it
//! again; Delete, destructive and last, deletes it. Recording can be paused
//! and resumed; disabling the extension stops it too.
//!
//! Pane's host does the watching and keeping (`pane:extension/clipboard-history`):
//! this command only shows the history, and nothing of it runs while the
//! clipboard changes. Pane draws this command in its own split view, with
//! a type dropdown and the record's Information, and offers the history's
//! other controls — retention, the applications whose copies are not
//! recorded, Clear History — in the Actions panel and on the extension's
//! Settings page, whose preferences (declared in `pane.json`) are the
//! history's own state (#166). This list is what the command shows where
//! Pane does not draw that view: a copy of the package installed from
//! another source.
#![no_std]

use pane_guest::actions::PASTE_FALLBACK;
use pane_guest::alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use pane_guest::clipboard_history::{self as history, Capture, Entry, HistoryStatus};
use pane_guest::feedback::{Toast, ToastStyle, show_hud, show_toast};
use pane_guest::system::{self, Clip, SystemError};
use pane_guest::window::{PopToRootType, close};
use pane_guest::{Action, Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct ClipboardHistory;
pane_guest::export!(ClipboardHistory);

/// The item that pauses recording, and the one that resumes it. Each does
/// only that, so running one again (only a stale callback can) changes
/// nothing more.
const PAUSE: &str = "pause";
const RESUME: &str = "resume";
/// The prefix of a kept item, followed by its id.
const ENTRY: &str = "entry:";

/// The longest title of a kept item, in characters.
const TITLE_CHARS: usize = 80;

fn item(id: &str, title: String, subtitle: String) -> Item {
    Item::new(id, title).subtitle(subtitle)
}

fn plural(count: u32, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// The item that pauses recording, or resumes it.
fn recording(status: &HistoryStatus) -> Item {
    let (id, title, subtitle) = match status.capture {
        Capture::On => (
            PAUSE,
            "Pause Recording",
            format!(
                "Recording · {} kept · Text you copy is kept on this computer",
                plural(status.items, "item", "items")
            ),
        ),
        Capture::Paused | Capture::Off => (
            RESUME,
            "Resume Recording",
            format!(
                "Paused · {} kept · Nothing you copy is kept until you resume",
                plural(status.items, "item", "items")
            ),
        ),
    };
    let subtitle = match &status.problem {
        Some(problem) => format!("{problem} · {subtitle}"),
        None => subtitle,
    };
    let id = String::from(id);
    item(&id, title.into(), subtitle).on_action(move || act(id))
}

/// The first line of `text` with content, trimmed and at most
/// [`TITLE_CHARS`] long.
fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.chars().count() <= TITLE_CHARS {
        return line.into();
    }
    let mut title: String = line.chars().take(TITLE_CHARS - 1).collect();
    title.push('…');
    title
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn age(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        _ if seconds < 2 * 86400 => "1 day ago".into(),
        _ => format!("{} days ago", seconds / 86400),
    }
}

fn entry_item(entry: &Entry) -> Item {
    let mut about = vec![age(entry.age_seconds)];
    if let Some(source) = &entry.source {
        about.push(format!("from {source}"));
    }
    let lines = entry.text.lines().count();
    if lines > 1 {
        about.push(format!("{lines} lines"));
    }
    about.push("Enter pastes it".into());
    let title = title_of(&entry.text);
    let (id, text) = (entry.id.clone(), entry.text.clone());
    let (copied, deleted) = (id.clone(), id.clone());
    item(&format!("{ENTRY}{}", entry.id), title, about.join(" · ")).actions([
        Action::new("Paste", move || paste(id, text)),
        Action::new("Copy", move || copy(copied)),
        Action::new("Delete", move || delete(deleted)).destructive(),
    ])
}

/// Paste: pastes the kept item `id`, whose text is `text`, into the
/// application that was in front before Pane, which closes the window;
/// where Pane cannot paste yet, copies it again instead (as Copy does),
/// closes the window and says so in a HUD.
async fn paste(id: String, text: String) -> Result<(), String> {
    match system::paste(&Clip::Text(text)) {
        Ok(()) => Ok(()),
        Err(SystemError::NotAvailable(_)) => {
            history::copy(&id)?;
            close(false, PopToRootType::Default);
            show_hud(PASTE_FALLBACK, ToastStyle::Success);
            Ok(())
        }
        Err(SystemError::Failed(why)) => Err(why),
    }
}

/// Copy: puts the kept item `id` on the clipboard again, closes the window
/// and says so in a HUD, as the standard Copy does.
async fn copy(id: String) -> Result<(), String> {
    history::copy(&id)?;
    close(false, PopToRootType::Default);
    show_hud("Copied to Clipboard", ToastStyle::Success);
    Ok(())
}

/// Delete: deletes the kept item `id`; one no longer kept is an error.
async fn delete(id: String) -> Result<(), String> {
    match history::delete_items(&[id])? {
        0 => Err("That item is no longer kept".into()),
        _ => {
            show_toast(Toast::success("Deleted the kept item"));
            Ok(())
        }
    }
}

/// Runs the action of the item `item_id` and shows a toast saying what it
/// did.
async fn act(item_id: String) -> Result<(), String> {
    let done = match item_id.as_str() {
        PAUSE => {
            history::set_capture(Capture::Paused)?;
            "Recording paused"
        }
        RESUME => {
            history::set_capture(Capture::On)?;
            "Recording resumed"
        }
        _ => return Err(format!("unknown item: {item_id}")),
    };
    show_toast(Toast::success(done.to_string()));
    Ok(())
}

impl Command for ClipboardHistory {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let status = history::status()?;
        let entries = history::entries()?;
        let mut items = vec![recording(&status)];
        items.extend(entries.iter().map(entry_item));
        Ok(List::new("Clipboard History").items(items))
    }

    /// A callback no item's action names: an item's action of a list
    /// drawn before, whose item is gone now.
    async fn run_search_result(id: String) -> Result<(), String> {
        act(id).await
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
