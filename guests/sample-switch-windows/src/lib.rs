//! Pane's Switch Windows sample: a command that lists the open windows
//! ([`pane_extension::windows`], #263) — the ones Alt+Tab would show,
//! each with its title, its application's name and icon, and whether it
//! is minimized, maximized, on another desktop or elevated — one item for
//! each, which switches to it and says what it answered in a toast
//! ("Switched to notes.txt - Notepad"), or why it could not ("Switch to
//! notes.txt - Notepad: that window closed since it was listed"). The
//! command's screen is a designed List (#240), which Pane filters by the
//! text typed in its search field with the root-search matcher, each
//! item's keywords naming its application, so typing finds a window by
//! its title or its application's name.
//!
//! The sample calls each host function as it is: it does not close Pane's
//! window first, as the Switch Windows default extension does
//! (pane-app/switch-windows), so a test can read what it answered — and a
//! test drives this sample with a fake of the windows, so nothing it
//! switches to is a real window. The JavaScript and TypeScript samples
//! answer the same.
#![no_std]

use pane_extension::alloc::{
    format,
    string::{String, ToString},
};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::view::{Cx, IntoNode, item, list};
use pane_extension::windows::{self, Window};
use pane_extension::{Command, Icon, LaunchRecord};

/// The command's id in `pane.json`.
const SWITCH: &str = "switch-windows";

struct SwitchWindowsSample;
pane_extension::export!(SwitchWindowsSample);

/// One listed window as an item: its title, its application's name (as
/// the keywords the List's host filtering matches besides the title) and
/// icon, with "on another desktop" said of one that is. Activating it
/// switches to the window and says what that answered.
fn window(cx: &mut Cx<Windows>, listed: &Window) -> pane_extension::view::Item {
    let (id, title, name) = (listed.id.clone(), listed.title.clone(), subtitle(listed));
    let icon = listed.icon.clone().map(Icon::file);
    let mut held = item(id.clone())
        .title(title.clone())
        .subtitle(name)
        .keyword(listed.application_name.clone())
        .on_press(cx.listener(move |_: &mut Windows| {
            answer(&title, windows::activate(&id));
        }));
    if let Some(icon) = icon {
        held = held.icon(icon);
    }
    held
}

/// The second line of a listed window: its application's name, saying
/// "on another desktop" of a window on one.
fn subtitle(listed: &Window) -> String {
    if listed.elsewhere {
        format!("{} — on another desktop", listed.application_name)
    } else {
        listed.application_name.clone()
    }
}

/// Says what switching to the window titled `title` answered in a toast:
/// a success when it came to the front, a failure saying why when it did
/// not.
fn answer(title: &str, switched: Result<(), windows::WindowsError>) {
    let toast = match switched {
        Ok(()) => Toast::success(format!("Switched to {title}")),
        Err(error) => Toast::failure(format!("Switch to {title}: {}", error.message())),
    };
    show_toast(toast);
}

/// The view the command opens (#240): the windows Alt+Tab would show, in
/// z-order with the front application's first, read again each render —
/// so the switch's answer re-draws it. With none listed, the list says
/// so; the listing failing says why.
struct Windows;

impl pane_extension::view::View for Windows {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoNode {
        match windows::list_windows() {
            Ok(listed) if listed.is_empty() => list()
                .navigation_title("Switch Windows")
                .search_placeholder("Filter the windows…")
                .child(
                    pane_extension::view::empty_state("No windows are open")
                        .description("Nothing Alt+Tab would show is listed"),
                ),
            Ok(listed) => {
                let mut view = list()
                    .navigation_title("Switch Windows")
                    .search_placeholder("Filter the windows…");
                for listed in listed.iter() {
                    view = view.child(window(cx, listed));
                }
                view
            }
            Err(error) => list()
                .navigation_title("Switch Windows")
                .search_placeholder("Filter the windows…")
                .child(
                    pane_extension::view::empty_state(format!(
                        "The open windows could not be listed: {}",
                        error.message()
                    ))
                    .description("Nothing was switched to"),
                ),
        }
    }
}

impl Command for SwitchWindowsSample {
    /// The windows the command lists: a designed view (its pane.json says
    /// `"mode": "designed"`).
    type DesignedView = Windows;

    async fn render() -> Result<pane_extension::List, String> {
        Err(format!("`{SWITCH}` opens a designed view; it has no list"))
    }

    /// The view the command opens: the windows Alt+Tab would show, listed
    /// anew each render. Without this the trait's default refuses the
    /// open, and the command's screen never shows.
    async fn open_designed_view(command: String, _launch: LaunchRecord) -> Result<Windows, String> {
        if command == SWITCH {
            Ok(Windows)
        } else {
            Err(format!("unknown command: {command}"))
        }
    }
}
