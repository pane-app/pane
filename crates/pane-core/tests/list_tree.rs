//! A command's list reaches Pane through ADR 0036's envelope: `render`
//! answers a versioned JSON tree, and choosing an item hands the callback id
//! its action names to `handle-event`, after which Pane asks for the tree
//! again. These checks drive the launcher with the `trees` fixture, whose
//! JSON is written by hand rather than by an SDK, so that Pane's reading is
//! checked against what an SDK never writes: fields Pane does not know, a
//! newer version, a view Pane cannot show, and trees and answers it cannot
//! read. The samples' suites check the same envelope through each SDK.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{CallError, CommandRegistration, Launcher, Runtime, Screen, Status};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;

use feedback::shown;
use guests::guest;

/// The fixture's items, (id, title), in their first order.
const ITEMS: [(&str, &str); 8] = [
    ("first", "First"),
    ("reverse", "Reverse the list"),
    ("remove", "Remove this item"),
    ("newer", "Draw a newer tree"),
    ("grid", "Draw a grid"),
    ("unreadable", "Draw an unreadable tree"),
    ("bad-answer", "Answer unreadably"),
    ("quiet", "Answer nothing"),
];

struct Pane {
    runtime: Runtime,
    launcher: Launcher,
}

impl Pane {
    /// A launcher offering the fixture as a command built into Pane, with
    /// its runtime.
    fn new() -> Pane {
        let runtime = Runtime::start().unwrap();
        let launcher = Launcher::new(
            Ok(runtime.clone()),
            vec![CommandRegistration {
                id: "trees".into(),
                title: "Trees".into(),
                subtitle: None,
                component: component(),
                takes_query: false,
                search: false,
                keywords: Vec::new(),
                when: pane_core::CommandWhen::Always,
                matches: pane_core::CommandMatches::Title,
            }],
        );
        Pane { runtime, launcher }
    }

    /// The launcher with the fixture's command opened.
    fn opened() -> Pane {
        let pane = Pane::new();
        // The blank query lists the command and Pane's own rows in the
        // no-query order (#199): "Settings…" comes before "Trees", so the
        // fixture's row is chosen by its title.
        let index = pane
            .launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == "Trees")
            .expect("the fixture's command is listed");
        pane.launcher.select(index);
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.launcher.view().screen, Screen::Command);
        pane
    }

    /// The ids of the rows listed, in order.
    fn ids(&self) -> Vec<String> {
        self.launcher
            .view()
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect()
    }

    /// The id of the selected row.
    fn selected(&self) -> Option<String> {
        let view = self.launcher.view();
        view.selected.map(|index| view.rows[index].id.clone())
    }

    /// Chooses the item `id` and returns what the user is shown once it
    /// answered and the list was drawn again: its toast, or the status
    /// line.
    fn choose(&self, id: &str) -> Status {
        let index = self
            .ids()
            .iter()
            .position(|listed| listed == id)
            .unwrap_or_else(|| panic!("no item {id} in {:?}", self.ids()));
        self.launcher.select(index);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    fn title(&self) -> String {
        self.launcher.view().title
    }
}

fn component() -> PathBuf {
    guest("tree_fixture")
}

fn handled(callback: &str) -> Status {
    Status::Result(format!("Handled {callback} with {{}}"))
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn a_tree_with_fields_pane_does_not_know_lists_its_items() {
    let pane = Pane::opened();

    let view = pane.launcher.view();
    assert_eq!(view.title, "Drawn 1 times");
    let rows: Vec<(String, String, Option<String>)> = view
        .rows
        .into_iter()
        .map(|row| (row.id, row.title, row.subtitle))
        .collect();
    let expected: Vec<(String, String, Option<String>)> = ITEMS
        .iter()
        .map(|&(id, title)| (id.into(), title.into(), Some(format!("Callback cb-{id}"))))
        .collect();
    assert_eq!(rows, expected);
    assert_eq!(view.selected, Some(0));
}

#[test]
fn choosing_an_item_hands_its_callback_to_the_command_and_draws_the_list_again() {
    let pane = Pane::opened();

    assert_eq!(pane.choose("first"), handled("cb-first"));
    // The toast says it: the `status` text the command still answers is
    // not shown in the status line.
    assert_eq!(pane.launcher.view().status, Status::Idle);

    // The callback is the one the tree named, not the item's id, and the
    // list was asked for again.
    assert_eq!(pane.title(), "Drawn 2 times");
    assert_eq!(pane.launcher.view().screen, Screen::Command);
    assert_eq!(pane.choose("first"), handled("cb-first"));
    assert_eq!(pane.title(), "Drawn 3 times");
}

#[test]
fn the_selection_stays_on_the_same_item_when_the_list_is_drawn_again() {
    let pane = Pane::opened();

    assert_eq!(pane.choose("reverse"), handled("cb-reverse"));

    let mut reversed: Vec<String> = ITEMS.iter().map(|&(id, _)| id.into()).collect();
    reversed.reverse();
    assert_eq!(pane.ids(), reversed);
    assert_eq!(pane.selected().as_deref(), Some("reverse"));
}

#[test]
fn an_item_that_leaves_the_list_leaves_the_selection_in_its_place() {
    let pane = Pane::opened();

    assert_eq!(pane.choose("remove"), handled("cb-remove"));

    assert!(!pane.ids().contains(&"remove".to_string()));
    assert_eq!(pane.launcher.view().selected, Some(2));
    assert_eq!(pane.selected().as_deref(), Some("newer"));
}

#[test]
fn a_tree_naming_a_newer_version_still_draws_what_pane_knows() {
    let pane = Pane::opened();

    assert_eq!(pane.choose("newer"), handled("cb-newer"));

    // Drawn from the version 2 tree.
    assert_eq!(pane.title(), "Drawn 2 times");
    let ids: Vec<String> = ITEMS.iter().map(|&(id, _)| id.into()).collect();
    assert_eq!(pane.ids(), ids);
    assert_eq!(pane.choose("first"), handled("cb-first"));
    assert_eq!(pane.title(), "Drawn 3 times");
}

#[test]
fn a_view_pane_cannot_show_is_the_views_failure_and_the_list_stays() {
    let pane = Pane::opened();

    let message = error(pane.choose("grid"));

    assert_eq!(
        message,
        "Pane could not read what the extension answered: it shows a `grid` view, which this \
         version of Pane cannot show"
    );
    assert_eq!(pane.title(), "Drawn 1 times");
    assert_eq!(pane.ids().len(), ITEMS.len());
    // The command carries on: its next tree is a list again.
    assert_eq!(pane.choose("first"), handled("cb-first"));
    assert_eq!(pane.title(), "Drawn 3 times");
}

#[test]
fn an_unreadable_tree_fails_the_view_without_counting_as_a_crash() {
    let pane = Pane::opened();

    let message = error(pane.choose("unreadable"));

    assert!(
        message.starts_with("Pane could not read what the extension answered: its view: "),
        "{message}"
    );
    // Not a trap: the instance that answered it is still running, and the
    // list stays as it was.
    assert_eq!(block_on(pane.runtime.running()), vec![component()]);
    assert_eq!(pane.ids().len(), ITEMS.len());
    assert_eq!(pane.choose("first"), handled("cb-first"));
}

#[test]
fn an_unreadable_tree_when_the_command_opens_is_shown_as_a_failed_view() {
    let pane = Pane::new();
    // The instance the launcher opens next answers an unreadable tree.
    assert_eq!(
        block_on(
            pane.runtime
                .handle_event(&component(), "cb-unreadable", "{}")
        ),
        Ok(pane_core::Answer::default())
    );

    // The blank query lists the fixture's row behind "Settings…" in the
    // no-query order (#199): the command is chosen by its title.
    let index = pane
        .launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Trees")
        .expect("the fixture's command is listed");
    pane.launcher.select(index);
    block_on(pane.launcher.activate_selected());

    assert!(
        matches!(pane.launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        pane.launcher.view().screen
    );
    let message = error(pane.launcher.view().status);
    assert!(
        message.starts_with("Pane could not read what the extension answered: its view: "),
        "{message}"
    );
    assert_eq!(block_on(pane.runtime.running()), vec![component()]);
    // Opened again, it lists its items.
    block_on(pane.launcher.activate_selected());
    assert_eq!(pane.launcher.view().screen, Screen::Command);
}

#[test]
fn an_answer_pane_cannot_read_is_the_actions_failure_and_the_list_is_drawn_again() {
    let pane = Pane::opened();

    let message = error(pane.choose("bad-answer"));

    assert!(
        message.starts_with("Pane could not read what the extension answered: its answer: "),
        "{message}"
    );
    assert_eq!(pane.title(), "Drawn 2 times");
    assert_eq!(block_on(pane.runtime.running()), vec![component()]);
}

#[test]
fn an_answer_without_text_shows_none() {
    let pane = Pane::opened();

    assert_eq!(pane.choose("quiet"), Status::Idle);

    assert_eq!(pane.title(), "Drawn 2 times");
}

#[test]
fn the_runtime_reads_the_tree_and_runs_an_item_by_its_callback() {
    let runtime = Runtime::start().unwrap();

    let view = block_on(runtime.render(&component())).unwrap();
    assert_eq!(view.title, "Drawn 1 times");
    let first = &view.items[0];
    assert_eq!(
        first.action().and_then(|action| action.callback()),
        Some("cb-first")
    );
    assert_eq!(
        first.action().and_then(|action| action.title.as_deref()),
        Some("Run")
    );

    // Running an item draws the list, then hands its action's callback over.
    assert_eq!(
        block_on(runtime.run_item(&component(), "first")),
        Ok(pane_core::Answer::default())
    );
    assert_eq!(
        block_on(runtime.run_item(&component(), "missing")),
        Err(CallError::Guest("unknown item: missing".into()))
    );
}
