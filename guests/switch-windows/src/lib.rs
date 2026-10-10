//! Pane's Switch Windows, a default extension (ADR 0040): a command
//! listing the windows Alt+Tab would show, so the user finds a window by
//! typing — its title, its application's name and icon — and switches to
//! it with Enter. The host lists the windows (`pane_extension::windows`)
//! in z-order with the front application's first, so switching back to
//! what the user was in before opening Pane is one keystroke; typing
//! filters by title and application name, as the command's search. Enter
//! brings the chosen window to the front, restoring it if it is
//! minimized, and a window that closed says so. Window actions (close,
//! minimize, maximize, restore, keep on top) are a later slice.
#![no_std]

use pane_extension::alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use pane_extension::feedback::{ToastStyle, show_hud};
use pane_extension::search::SearchResult;
use pane_extension::window::{PopToRootType, close};
use pane_extension::windows::{self, Window};
use pane_extension::{Command, Icon, Item, List, NoCustomView};

/// The command's id in `pane.json`.
const SWITCH: &str = "switch-windows";

struct Switch;
pane_extension::export!(Switch);
pane_extension::search::export!(Switch);

/// The error's message, as the answer the launcher shows.
fn explain(error: windows::WindowsError) -> String {
    error.message().to_string()
}

/// One listed window: its title, its application's name, and the
/// application's icon, with "on another desktop" said of one that is.
fn item(window: &Window) -> Item {
    let (id, name) = (window.id.clone(), subtitle(window));
    let icon = window.icon.clone().map(Icon::file);
    let listed = Item::new(window.id.clone(), window.title.clone())
        .subtitle(name)
        .on_action(move || switch(id));
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

/// Switches to the window `id` names: Pane's window closes first, so the
/// window can take the foreground, then the host brings it to the front,
/// restoring it if it is minimized. A window that closed, or one that did
/// not come to the front, says why in a HUD.
async fn switch(id: String) -> Result<(), String> {
    close(false, PopToRootType::Default);
    if let Err(error) = windows::activate(&id) {
        show_hud(error.message(), ToastStyle::Failure);
    }
    Ok(())
}

impl Command for Switch {
    type CustomView = NoCustomView;

    /// The windows Alt+Tab would show, in z-order with the front
    /// application's first; Enter switches to one. With none to show —
    /// nothing else is open — the list says so.
    async fn render() -> Result<List, String> {
        let listed = windows::list_windows().map_err(explain)?;
        if listed.is_empty() {
            return Ok(List::new("Switch Windows").item(
                Item::new("none", "No other windows are open")
                    .subtitle("The window you were in is the only one"),
            ));
        }
        Ok(List::new("Switch Windows").items(listed.iter().map(item)))
    }

    /// Runs the search result the user chose: the window its id names,
    /// switched to.
    async fn run_search_result(id: String) -> Result<(), String> {
        switch(id).await
    }
}

impl pane_extension::search::Guest for Switch {
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
