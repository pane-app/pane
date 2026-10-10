//! Pane's Switch Windows sample: a command that lists the open windows
//! ([`pane_extension::windows`], #263) — the ones Alt+Tab would show,
//! each with its title, its application's name and icon, and whether it
//! is minimized, maximized, on another desktop or elevated — one item for
//! each, which switches to it and says what it answered in a toast
//! ("Switched to notes.txt - Notepad"), or why it could not ("Switch to
//! notes.txt - Notepad: that window closed since it was listed"). Typing
//! in the command's search field filters the windows by their titles and
//! their applications' names.
//!
//! The sample calls each host function as it is: it does not close Pane's
//! window first, as the Switch Windows default extension does
//! (guests/switch-windows), so a test can read what it answered — and a
//! test drives this sample with a fake of the windows, so nothing it
//! switches to is a real window. The JavaScript and TypeScript samples
//! answer the same.
#![no_std]

use pane_extension::alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::search::SearchResult;
use pane_extension::windows::{self, Window};
use pane_extension::{Command, Icon, Item, List, NoCustomView};

/// The command's id in `pane.json`.
const SWITCH: &str = "switch-windows";

struct SwitchWindowsSample;
pane_extension::export!(SwitchWindowsSample);
pane_extension::search::export!(SwitchWindowsSample);

/// The error's message, as the answer the launcher shows.
fn explain(error: windows::WindowsError) -> String {
    error.message().to_string()
}

/// One listed window as an item: its title, its application's name and
/// icon, with "on another desktop" said of one that is. Running it
/// switches to the window and says what that answered.
fn item(window: &Window) -> Item {
    let (id, title, name) = (window.id.clone(), window.title.clone(), subtitle(window));
    let icon = window.icon.clone().map(Icon::file);
    let listed = Item::new(window.id.clone(), window.title.clone())
        .subtitle(name)
        .on_action(move || async move { answer(&title, windows::activate(&id)) });
    match icon {
        Some(icon) => listed.icon(icon),
        None => listed,
    }
}

/// The second line of a listed window: its application's name, saying
/// "on another desktop" of a window on one.
fn subtitle(window: &Window) -> String {
    if window.elsewhere {
        format!("{} — on another desktop", window.application_name)
    } else {
        window.application_name.clone()
    }
}

/// Says what switching to the window titled `title` answered in a toast:
/// a success when it came to the front, a failure saying why when it did
/// not.
fn answer(title: &str, switched: Result<(), windows::WindowsError>) -> Result<(), String> {
    let toast = match switched {
        Ok(()) => Toast::success(format!("Switched to {title}")),
        Err(error) => Toast::failure(format!("Switch to {title}: {}", error.message())),
    };
    show_toast(toast);
    Ok(())
}

impl Command for SwitchWindowsSample {
    type CustomView = NoCustomView;

    /// The windows Alt+Tab would show, in z-order with the front
    /// application's first; running an item switches to it. With none
    /// listed, the list says so.
    async fn render() -> Result<List, String> {
        let listed = windows::list_windows().map_err(explain)?;
        if listed.is_empty() {
            return Ok(List::new("Switch Windows").item(
                Item::new("none", "No windows are open")
                    .subtitle("Nothing Alt+Tab would show is listed"),
            ));
        }
        Ok(List::new("Switch Windows").items(listed.iter().map(item)))
    }

    /// Runs the search result the user chose: the window its id names,
    /// switched to.
    async fn run_search_result(id: String) -> Result<(), String> {
        // The list is asked for again, to say which window was switched
        // to; one that is gone says so.
        let title = windows::list_windows()
            .map_err(explain)?
            .iter()
            .find(|window| window.id == id)
            .map(|window| window.title.clone())
            .unwrap_or_else(|| "a window that is gone".into());
        answer(&title, windows::activate(&id))
    }
}

impl pane_extension::search::Guest for SwitchWindowsSample {
    /// The windows whose title or application's name matches what is
    /// typed, in the order they were listed.
    async fn search(command: String, query: String) -> Result<Vec<SearchResult>, String> {
        if command != SWITCH {
            return Err(format!("unknown command: {command}"));
        }
        let typed = query.trim().to_lowercase();
        let listed = windows::list_windows().map_err(explain)?;
        let found = listed
            .iter()
            .filter(|window| {
                window.title.to_lowercase().contains(&typed)
                    || window.application_name.to_lowercase().contains(&typed)
            })
            .map(|window| SearchResult {
                id: window.id.clone(),
                title: window.title.clone(),
                subtitle: Some(subtitle(window)),
                // A window, not a file of a granted folder.
                file: None,
            })
            .collect();
        Ok(found)
    }
}
