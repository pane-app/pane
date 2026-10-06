//! Pane's actions sample: a list whose items carry several actions (#137),
//! and what a command does after it acts (#141).
//!
//! "Alpha note" has nine actions, in an untitled section and the "Edit",
//! "Share" and "Danger" sections: Enter runs "Open", Ctrl+Enter "Copy" and
//! Ctrl+Shift+Enter "Rename", and Ctrl+K lists them all. Its shortcuts show
//! the rules Pane binds them by: one per system ("Reveal"), one that is
//! Pane's own Ctrl+K and so is never bound ("Open Menu"), one that Pane
//! leaves free until the user gives one of its keys Ctrl+Shift+Y
//! ("Archive"), and a destructive "Delete". "Beta note" has one action, so
//! Ctrl+Enter does nothing there, and "Gamma note" has none, so it cannot
//! be activated. Every one of their actions shows a success toast with its
//! title and the item's ("Open: Alpha note").
//!
//! "Window" closes the window with each way the next showing may go
//! (`default`, `immediate`, `suspended`, and clearing root search), pops to
//! root search (clearing its search or not) and clears the search field;
//! "In the Background" launches the "Window functions" no-view command in
//! the background, where no window is shown, and that command's toast says
//! what each function answered there. "Feedback" shows a HUD (and a
//! failure HUD), shows a toast that is updated from animated ("Uploading…")
//! to success ("Uploaded") with Open and Retry actions, hides it, fails,
//! and sets and clears the command's row subtitle ("3 unread"). The no-view
//! commands "Spin" (it leaves an animated toast, which Pane hides when the
//! run ends) and "Stumble" (it fails) show what Pane does at a run's end.
//!
//! "Delta note" shows submenus (#140): "Open With…" gives its entries at
//! once, in sections, two with shortcuts and a destructive one; "Move to
//! List…" gives them when it opens, in a section that counts how many times
//! it was asked ("Asked 1 time"); and "Tag…" fails when it opens ("The tags
//! could not be loaded"). An entry shows what it did and the item's
//! ("Open With Notepad: Delta note", "Move to Later: Delta note").
//! The JavaScript and TypeScript samples do the same.
#![no_std]

use core::cell::Cell;
use core::sync::atomic::{AtomicU32, Ordering};

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::commands::{CommandRef, LaunchType, launch, set_subtitle};
use pane_guest::feedback::{ShownToast, Toast, ToastAction, ToastStyle, show_hud, show_toast};
use pane_guest::window::{PopToRootType, clear_search, close, pop_to_root};
use pane_guest::{
    Action, Command, CustomView, FieldValue, FormError, Item, LaunchRecord, List, Modifier,
    NoCustomView, Shortcut, Submenu,
};

use Modifier::{Cmd, Ctrl, Shift};

struct Actions;
pane_guest::export!(Actions);

/// What the action titled `title` of the item titled `item` shows.
async fn answer(title: &str, item: &str) -> Result<(), String> {
    show_toast(Toast::success(format!("{title}: {item}")));
    Ok(())
}

/// The action titled `title` of the item titled `item`: it shows both.
fn action(title: &'static str, item: &'static str) -> Action {
    Action::new(title, move || answer(title, item))
}

/// What a window function answered: nothing more when a window was shown
/// for the call, else the failure that none was.
fn windowed(shown: bool) -> Result<(), String> {
    if shown {
        Ok(())
    } else {
        Err("No window was shown".into())
    }
}

/// The "Window" item's action titled `title`, which runs `function`.
fn window_action(title: &'static str, function: fn() -> bool) -> Action {
    Action::new(title, move || async move { windowed(function()) })
}

/// The upload toast "Start Upload" showed, which "Finish Upload" and
/// "Hide Toast" change.
struct Upload(Cell<Option<ShownToast>>);

// SAFETY: a component's code runs on one thread.
unsafe impl Sync for Upload {}

static UPLOAD: Upload = Upload(Cell::new(None));

/// Starts the upload: an animated toast, remembered.
fn start_upload() -> ShownToast {
    let shown = show_toast(Toast::animated("Uploading…"));
    UPLOAD.0.set(Some(shown));
    shown
}

/// The upload's toast once it is done: a success with Open and Retry.
fn uploaded() -> Toast {
    Toast::success("Uploaded")
        .message("notes.txt")
        .primary(
            ToastAction::new("Open", || async {
                show_toast(Toast::success("Opened the upload"));
                Ok(())
            })
            .shortcut(Shortcut::new([Ctrl, Shift], "o")),
        )
        .secondary(
            ToastAction::new("Retry", || async {
                start_upload().update(uploaded());
                Ok(())
            })
            .shortcut(Shortcut::new([Ctrl, Shift], "r")),
        )
}

/// The command `command` of this package, for a launch.
fn own(command: &str) -> CommandRef {
    CommandRef {
        source: None,
        command: command.into(),
    }
}

/// The action titled `title` of the item titled `item` that answers `said`
/// and the item's title.
fn answering(title: &'static str, said: &'static str, item: &'static str) -> Action {
    Action::new(title, move || answer(said, item))
}

/// How many times this instance was asked for "Move to List…"'s entries.
static LISTS_ASKED: AtomicU32 = AtomicU32::new(0);

/// "Delta note": its submenus.
fn delta() -> Item {
    let delta = "Delta note";
    Item::new("delta", delta)
        .subtitle("Submenus, given at once or asked for when opened")
        .actions([
            action("Open", delta),
            Action::submenu(
                "Open With…",
                Submenu::new("Open With").entries([
                    answering("Notepad", "Open With Notepad", delta)
                        .section("Editors")
                        .shortcut(Shortcut::new([Ctrl, Shift], "n")),
                    answering("WordPad", "Open With WordPad", delta).section("Editors"),
                    answering("Browser", "Open With Browser", delta)
                        .section("Other")
                        .shortcut(Shortcut::new([Ctrl, Shift], "b")),
                    action("Forget Applications", delta)
                        .section("Danger")
                        .destructive()
                        .shortcut(Shortcut::new([Ctrl, Shift], "d")),
                ]),
            ),
            Action::submenu(
                "Move to List…",
                Submenu::lazy("Move to List", move || async move {
                    let asked = LISTS_ASKED.fetch_add(1, Ordering::Relaxed) + 1;
                    let times = if asked == 1 { "time" } else { "times" };
                    let section = format!("Asked {asked} {times}");
                    let lists: Vec<Action> = ["Inbox", "Later", "Someday"]
                        .into_iter()
                        .map(|list| {
                            Action::new(list, move || async move {
                                show_toast(Toast::success(format!("Move to {list}: {delta}")));
                                Ok::<(), String>(())
                            })
                            .section(section.clone())
                        })
                        .collect();
                    Ok::<Vec<Action>, String>(lists)
                }),
            )
            .section("Organize"),
            Action::submenu(
                "Tag…",
                Submenu::lazy("Tags", || async {
                    Err::<Vec<Action>, String>("The tags could not be loaded".into())
                }),
            )
            .section("Organize"),
        ])
}

impl Command for Actions {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let alpha = "Alpha note";
        let beta = "Beta note";
        Ok(List::new("Actions sample").items([
            Item::new("alpha", alpha)
                .subtitle("Several actions in sections, with shortcuts")
                .actions([
                    action("Open", alpha),
                    action("Copy", alpha),
                    action("Rename", alpha)
                        .section("Edit")
                        .shortcut(Shortcut::new([Ctrl], "r")),
                    action("Duplicate", alpha)
                        .section("Edit")
                        .shortcut(Shortcut::new([Ctrl], "d")),
                    action("Archive", alpha)
                        .section("Edit")
                        .shortcut(Shortcut::new([Ctrl, Shift], "y")),
                    action("Copy Link", alpha)
                        .section("Share")
                        .shortcut(Shortcut::new([Ctrl, Shift], "c")),
                    action("Reveal", alpha).section("Share").shortcut(
                        Shortcut::per_platform()
                            .windows([Ctrl, Shift], "e")
                            .macos([Cmd, Shift], "r")
                            .linux([Ctrl, Shift], "l"),
                    ),
                    action("Open Menu", alpha)
                        .section("Share")
                        .shortcut(Shortcut::new([Ctrl], "k")),
                    action("Delete", alpha)
                        .section("Danger")
                        .destructive()
                        .shortcut(Shortcut::new([Ctrl], "x")),
                ]),
            Item::new("beta", beta)
                .subtitle("One action")
                .action(action("Open", beta)),
            Item::new("gamma", "Gamma note").subtitle("No actions"),
            delta(),
            Item::new("window", "Window")
                .subtitle("Close, pop to root search, clear the search")
                .actions([
                    window_action("Close", || close(false, PopToRootType::Default)),
                    window_action("Close to Root Search", || {
                        close(false, PopToRootType::Immediate)
                    }),
                    window_action("Close and Keep Screen", || {
                        close(false, PopToRootType::Suspended)
                    }),
                    window_action("Close and Clear Root Search", || {
                        close(true, PopToRootType::Default)
                    }),
                    window_action("Pop to Root", || pop_to_root(false)),
                    window_action("Pop to Root and Clear Search", || pop_to_root(true)),
                    window_action("Clear Search", clear_search),
                    Action::new("In the Background", || async {
                        launch(&own("window-functions"), LaunchType::Background, &[], None)
                    }),
                ]),
            Item::new("feedback", "Feedback")
                .subtitle("A HUD, toasts and this command's subtitle")
                .actions([
                    Action::new("Show HUD", || async {
                        show_hud("Copied to Clipboard", ToastStyle::Success);
                        Ok(())
                    }),
                    Action::new("Show Failure HUD", || async {
                        show_hud("Could not copy", ToastStyle::Failure);
                        Ok(())
                    }),
                    Action::new("Start Upload", || async {
                        start_upload();
                        Ok(())
                    }),
                    Action::new("Finish Upload", || async {
                        match UPLOAD.0.get() {
                            Some(shown) => {
                                shown.update(uploaded());
                                Ok(())
                            }
                            None => Err("Nothing is uploading".into()),
                        }
                    }),
                    Action::new("Upload", || async {
                        start_upload().update(uploaded());
                        Ok(())
                    }),
                    Action::new("Hide Toast", || async {
                        if let Some(shown) = UPLOAD.0.take() {
                            shown.hide();
                        }
                        Ok(())
                    }),
                    Action::new("Fail", || async { Err("The upload failed".into()) }),
                    Action::new("Set Subtitle", || async { set_subtitle(Some("3 unread")) }),
                    Action::new("Clear Subtitle", || async { set_subtitle(None) }),
                ]),
        ]))
    }

    async fn run(command: String, _launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            // What each window function answers where it runs: in the
            // background, that no window was shown. Launched from root
            // search with its query typed, the close empties that query.
            "window-functions" => {
                let closed = close(true, PopToRootType::Default);
                let popped = pop_to_root(false);
                let cleared = clear_search();
                show_toast(Toast::success(format!(
                    "close: {closed}, pop to root: {popped}, clear search: {cleared}"
                )));
                Ok(())
            }
            // Leaves its toast in progress: Pane hides it once the run
            // ends.
            "spin" => {
                show_toast(Toast::animated("Spinning…"));
                Ok(())
            }
            "stumble" => Err("Stumbled on purpose".into()),
            other => Err(format!("`{other}` opens a screen")),
        }
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "The actions sample has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("The actions sample has no custom views".into())
    }
}
