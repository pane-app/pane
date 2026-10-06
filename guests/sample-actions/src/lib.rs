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
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::{
    Action, Command, CustomView, FieldValue, FormError, Item, List, Modifier, NoCustomView,
    Shortcut,
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
