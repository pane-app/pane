//! Pane's clipboard history, a default extension: once the user turns it
//! on in its command, Pane keeps the text they copy on this computer, and
//! the command lists it, newest first; Enter on an item copies it again or
//! deletes it. It starts off, and can be paused, resumed and turned off
//! again; disabling the extension stops it too. Programs can be excluded by
//! their file name. Items are kept for 7 days unless the user chooses
//! another time, and Pane deletes them then, whether this command runs or
//! not; the recent ones can be deleted together, all of them with Clear
//! (history stays on), or all of them with history turned off.
//!
//! Pane's host does the watching and keeping (`pane:extension/clipboard-history`):
//! this command only shows the history and the user's controls, and nothing
//! of it runs while the clipboard changes.
#![no_std]

use pane_guest::alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use pane_guest::clipboard_history::{self as history, Capture, Entry, HistoryStatus};
use pane_guest::{
    Choice, Command, CustomView, Field, FieldKind, FieldValue, Form, FormError, Item, List,
    NoCustomView, TextField,
};

struct ClipboardHistory;
pane_guest::export!(ClipboardHistory);

// The items that turn keeping on, pause it, resume it and turn it off. Each
// does only that, so running one again (Pane draws the list again after
// each, so only a stale callback can) changes nothing more.
const TURN_ON: &str = "turn-on";
const PAUSE: &str = "pause";
const RESUME: &str = "resume";
const TURN_OFF: &str = "turn-off";
/// The item whose form excludes a program.
const EXCLUDE: &str = "exclude";
/// The prefix of the item that no longer excludes the program named by the
/// rest.
const INCLUDE: &str = "include:";
/// The item that deletes every kept item.
const CLEAR: &str = "clear";
/// The item that turns keeping off and deletes every kept item.
const TURN_OFF_AND_CLEAR: &str = "turn-off-and-clear";
/// The item whose form chooses how long items are kept.
const RETENTION: &str = "retention";
/// The item whose form deletes the items copied recently.
const DELETE_RECENT: &str = "delete-recent";
/// The prefix of a kept item, followed by its id.
const ENTRY: &str = "entry:";
/// The item shown while nothing is kept.
const EMPTY: &str = "empty";

/// The longest title of a kept item, in characters.
const TITLE_CHARS: usize = 80;

/// How long items can be kept, in seconds.
const RETENTIONS: [u64; 5] = [3600, 86_400, 7 * 86_400, 30 * 86_400, 90 * 86_400];

/// How recent the items deleted together can be, in seconds, and what that
/// is called.
const RECENT: [(u64, &str); 3] = [(900, "15 minutes"), (3600, "hour"), (86_400, "day")];

/// A time span such as "1 hour" or "7 days".
fn span(seconds: u64) -> String {
    let (count, one, many) = match seconds {
        _ if seconds % 86_400 == 0 => (seconds / 86_400, "day", "days"),
        _ if seconds % 3600 == 0 => (seconds / 3600, "hour", "hours"),
        _ if seconds % 60 == 0 => (seconds / 60, "minute", "minutes"),
        _ => (seconds, "second", "seconds"),
    };
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn choice(id: String, label: String) -> Choice {
    Choice { id, label }
}

/// The retention form. A form starts on its first choice, so the retention
/// now comes first: submitting the form unchanged changes nothing.
fn retention_form(current: u64) -> Form {
    let others = RETENTIONS
        .iter()
        .copied()
        .filter(|&seconds| seconds != current);
    Form {
        title: "Keep clipboard history items for".into(),
        fields: vec![Field {
            id: "retention".into(),
            label: "Keep each item for".into(),
            kind: FieldKind::Choice(
                core::iter::once(current)
                    .chain(others)
                    .map(|seconds| choice(seconds.to_string(), span(seconds)))
                    .collect(),
            ),
        }],
        submit_label: "Keep".into(),
    }
}

fn recent_form() -> Form {
    Form {
        title: "Delete recent clipboard history items".into(),
        fields: vec![Field {
            id: "since".into(),
            label: "Copied in the last".into(),
            kind: FieldKind::Choice(
                RECENT
                    .iter()
                    .map(|&(seconds, label)| choice(seconds.to_string(), label.into()))
                    .collect(),
            ),
        }],
        submit_label: "Delete".into(),
    }
}

/// The form an item opens: copy it again (first, so Enter twice copies)
/// or delete it.
fn entry_form(title: String) -> Form {
    Form {
        title,
        fields: vec![Field {
            id: "action".into(),
            label: "What to do with it".into(),
            kind: FieldKind::Choice(vec![
                choice("copy".into(), "Copy it again".into()),
                choice("delete".into(), "Delete it".into()),
            ]),
        }],
        submit_label: "OK".into(),
    }
}

fn item(id: &str, title: String, subtitle: String) -> Item {
    Item::new(id, title).subtitle(subtitle)
}

/// An item whose action is [`act`] with its id.
fn acting(id: &str, title: String, subtitle: String) -> Item {
    let id = String::from(id);
    let listed = item(&id, title, subtitle);
    listed.on_action(move || act(id))
}

fn plural(count: u32, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// The item that changes whether Pane keeps what is copied.
fn toggle(status: &HistoryStatus) -> Item {
    let (id, title, subtitle) = match status.capture {
        Capture::Off => (
            TURN_ON,
            "Turn on clipboard history",
            "Off · Pane keeps nothing you copy until you turn it on. Once on, it keeps the text \
             you copy on this computer; nothing is sent anywhere"
                .to_string(),
        ),
        Capture::On => (
            PAUSE,
            "Pause clipboard history",
            format!(
                "On · {} kept · Text you copy is kept on this computer",
                plural(status.items, "item", "items")
            ),
        ),
        Capture::Paused => (
            RESUME,
            "Resume clipboard history",
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
    acting(id, title.into(), subtitle)
}

fn exclude_form() -> Form {
    Form {
        title: "Exclude a program".into(),
        fields: vec![Field {
            id: "program".into(),
            label: "Program file name".into(),
            kind: FieldKind::Text(TextField {
                placeholder: Some("KeePass.exe".into()),
            }),
        }],
        submit_label: "Exclude".into(),
    }
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
    about.push("Enter copies or deletes it".into());
    let title = title_of(&entry.text);
    let form = entry_form(title.clone());
    item(&format!("{ENTRY}{}", entry.id), title, about.join(" · ")).form(form)
}

fn form_error(message: String) -> FormError {
    FormError {
        field: None,
        message,
    }
}

/// Runs the action of the item `item_id`; each item without a form runs
/// this with its id.
async fn act(item_id: String) -> Result<String, String> {
    let wanted = match item_id.as_str() {
        TURN_ON => Some((Capture::On, "Clipboard history is on")),
        PAUSE => Some((Capture::Paused, "Clipboard history is paused")),
        RESUME => Some((Capture::On, "Clipboard history is on again")),
        TURN_OFF => Some((Capture::Off, "Clipboard history is off")),
        _ => None,
    };
    if let Some((capture, done)) = wanted {
        history::set_capture(capture)?;
        return Ok(done.into());
    }
    if item_id == CLEAR {
        let cleared = history::clear()?;
        return Ok(format!(
            "Deleted {}",
            plural(cleared, "kept item", "kept items")
        ));
    }
    if item_id == TURN_OFF_AND_CLEAR {
        let cleared = history::turn_off_and_clear()?;
        return Ok(format!(
            "Clipboard history is off; deleted {}",
            plural(cleared, "kept item", "kept items")
        ));
    }
    if item_id == EMPTY {
        return Ok("Nothing is kept yet".into());
    }
    if let Some(program) = item_id.strip_prefix(INCLUDE) {
        let mut excluded = history::status()?.excluded;
        excluded.retain(|excluded| excluded != program);
        history::set_excluded(&excluded)?;
        return Ok(format!("Text copied from {program} is kept again"));
    }
    if let Some(id) = item_id.strip_prefix(ENTRY) {
        history::copy(id)?;
        return Ok("Copied to the clipboard".into());
    }
    Err(format!("unknown item: {item_id}"))
}

impl Command for ClipboardHistory {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let status = history::status()?;
        let mut items = vec![toggle(&status)];
        if status.capture != Capture::Off {
            items.push(acting(
                TURN_OFF,
                "Turn off clipboard history".into(),
                "Stops keeping what you copy; the kept items stay until you clear them".into(),
            ));
        }
        items.push(
            item(
                RETENTION,
                format!("Keep items for {}", span(status.retention_seconds)),
                "Older items are deleted, also while Pane is stopped or the extension is \
                 disabled · Enter changes it"
                    .into(),
            )
            .form(retention_form(status.retention_seconds)),
        );
        let excluded = match status.excluded.len() {
            0 => "None excluded".to_string(),
            count => format!("{count} excluded"),
        };
        items.push(
            item(
                EXCLUDE,
                "Exclude a program".into(),
                format!("Text copied from it is never kept · {excluded}"),
            )
            .form(exclude_form()),
        );
        items.extend(status.excluded.iter().map(|program| {
            acting(
                &format!("{INCLUDE}{program}"),
                format!("Stop excluding {program}"),
                format!("Text copied from {program} is not kept"),
            )
        }));
        let entries = history::entries()?;
        if !entries.is_empty() {
            items.push(acting(
                CLEAR,
                "Clear clipboard history".into(),
                format!(
                    "Deletes the {} kept; whether history is kept does not change",
                    plural(status.items, "item", "items")
                ),
            ));
            if status.capture != Capture::Off {
                items.push(acting(
                    TURN_OFF_AND_CLEAR,
                    "Turn off and delete clipboard history".into(),
                    format!(
                        "Deletes the {} kept and keeps nothing you copy from now on",
                        plural(status.items, "item", "items")
                    ),
                ));
            }
            items.push(
                item(
                    DELETE_RECENT,
                    "Delete recent items".into(),
                    "Deletes what you copied in the last 15 minutes, hour or day".into(),
                )
                .form(recent_form()),
            );
        }
        items.extend(entries.iter().map(entry_item));
        if entries.is_empty() && status.capture == Capture::On {
            items.push(acting(
                EMPTY,
                "Nothing kept yet".into(),
                "Text you copy from now on is listed here".into(),
            ));
        }
        Ok(List::new("Clipboard History").items(items))
    }

    /// A callback no item's action names runs as the action of that id, so
    /// a kept item's id (whose item opens a form) still copies it again.
    async fn run_search_result(id: String) -> Result<String, String> {
        act(id).await
    }

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        let value = |id: &str| {
            values
                .iter()
                .find(|value| value.id == id)
                .map_or("", |value| value.value.trim())
        };
        if let Some(id) = item_id.strip_prefix(ENTRY) {
            if value("action") == "delete" {
                return match history::delete_items(&[id.into()]).map_err(form_error)? {
                    0 => Err(form_error("That item is no longer kept".into())),
                    _ => Ok("Deleted the kept item".into()),
                };
            }
            history::copy(id).map_err(form_error)?;
            return Ok("Copied to the clipboard".into());
        }
        if item_id == RETENTION {
            let seconds: u64 = value("retention")
                .parse()
                .map_err(|_| form_error("Choose how long items are kept".into()))?;
            let before = history::status().map_err(form_error)?.items;
            history::set_retention(seconds).map_err(form_error)?;
            let after = history::status().map_err(form_error)?.items;
            let kept = format!("Items are kept for {}", span(seconds));
            return Ok(match before.saturating_sub(after) {
                0 => kept,
                deleted => format!(
                    "{kept}; deleted {}",
                    plural(deleted, "older item", "older items")
                ),
            });
        }
        if item_id == DELETE_RECENT {
            let seconds: u64 = value("since")
                .parse()
                .map_err(|_| form_error("Choose how recent the items are".into()))?;
            let ids: Vec<String> = history::entries()
                .map_err(form_error)?
                .into_iter()
                .filter(|entry| entry.age_seconds < seconds)
                .map(|entry| entry.id)
                .collect();
            let deleted = history::delete_items(&ids).map_err(form_error)?;
            return Ok(format!(
                "Deleted {}",
                plural(deleted, "kept item", "kept items")
            ));
        }
        if item_id != EXCLUDE {
            return Err(form_error(format!("unknown form: {item_id}")));
        }
        let program = value("program");
        let mut excluded = history::status().map_err(form_error)?.excluded;
        excluded.push(program.into());
        history::set_excluded(&excluded).map_err(|message| FormError {
            field: Some("program".into()),
            message,
        })?;
        Ok(format!("Text copied from {program} is not kept"))
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}
