//! Pane's actions sample: a list whose items carry several actions (#137).
//! "Alpha note" has nine, in an untitled section and the "Edit", "Share"
//! and "Danger" sections: Enter runs "Open", Ctrl+Enter "Copy" and
//! Ctrl+Shift+Enter "Rename", and Ctrl+K lists them all. Its shortcuts show
//! the rules Pane binds them by: one per system ("Reveal"), one that is
//! Pane's own Ctrl+K and so is never bound ("Open Menu"), one that Pane
//! leaves free until the user gives one of its keys Ctrl+Shift+Y
//! ("Archive"), and a destructive "Delete". "Beta note" has one action, so
//! Ctrl+Enter does nothing there, and "Gamma note" has none, so it cannot
//! be activated. Every action answers its title and the item's
//! ("Open: Alpha note"); the JavaScript and TypeScript samples answer the
//! same.
//!
//! "Delta note" shows submenus (#140): "Open With…" gives its entries at
//! once, in sections, two with shortcuts and a destructive one; "Move to
//! List…" gives them when it opens, in a section that counts how many times
//! it was asked ("Asked 1 time"); and "Tag…" fails when it opens ("The tags
//! could not be loaded"). An entry answers what it did and the item's
//! ("Open With Notepad: Delta note", "Move to Later: Delta note").
#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::{
    Action, Command, CustomView, FieldValue, FormError, Item, List, Modifier, NoCustomView,
    Shortcut, Submenu,
};

use Modifier::{Cmd, Ctrl, Shift};

struct Actions;
pane_guest::export!(Actions);

/// What the action titled `title` of the item titled `item` answers.
async fn answer(title: &str, item: &str) -> Result<String, String> {
    Ok(format!("{title}: {item}"))
}

/// The action titled `title` of the item titled `item`: it answers both.
fn action(title: &'static str, item: &'static str) -> Action {
    Action::new(title, move || answer(title, item))
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
                                Ok::<String, String>(format!("Move to {list}: {delta}"))
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
        ]))
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
