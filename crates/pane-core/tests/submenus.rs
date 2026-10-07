//! Submenus in the Actions panel (#140) through the launcher's public
//! interface, with the actions sample in Rust, JavaScript and TypeScript,
//! real guests `cargo xtask guests` assembles. "Delta note" has a submenu
//! given at once ("Open With…"), one the command gives when it opens
//! ("Move to List…", whose section counts the times it was asked) and one
//! whose opening fails ("Tag…"): an eager submenu lists its entries and runs
//! the one chosen; a lazy one is asked once per opening, loads until it
//! answers and lists the answer, or shows why it failed; an answer arriving
//! after its submenu closed or the selection moved is discarded; typing
//! filters a submenu's entries with the sections flattened; choosing an
//! entry runs it once. Prior art: `item_actions.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    Binding, ItemActions, Keyboard, KeyboardAction, Launcher, NavigationBindings, OpenSubmenu,
    PaneKeys, Runtime, Screen, Status, SubmenuState,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::select_title;

/// One language's actions sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its command's title in root search.
    command: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-actions",
    title: "Actions sample",
    command: "Actions",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-actions-js",
    title: "JavaScript actions sample",
    command: "Actions (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-actions-ts",
    title: "TypeScript actions sample",
    command: "Actions (TypeScript)",
};

const ALL: [&Fixture; 3] = [&RUST, &JAVASCRIPT, &TYPESCRIPT];

/// "Delta note"'s id, the panel's target.
const DELTA: &str = "delta";

/// The places of "Delta note"'s actions.
const OPEN_WITH: usize = 1;
const MOVE_TO_LIST: usize = 2;
const TAG: usize = 3;

fn binding(id: &str) -> Binding {
    Binding::parse(id).unwrap()
}

/// One test's Pane, with one language's sample installed and its command
/// open, "Delta note" selected.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    launcher: Launcher,
}

impl Pane {
    fn open(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
        let folder = copy(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        block_on(launcher.set_query(fixture.command));
        select_title(&launcher, fixture.command);
        block_on(launcher.activate_selected());
        let view = launcher.view();
        assert_eq!(
            (&view.screen, view.title.as_str()),
            (&Screen::Command, "Actions sample"),
            "{:?}",
            view.status
        );
        select_title(&launcher, "Delta note");
        Pane {
            _sources: sources,
            _data: data,
            launcher,
        }
    }

    /// What the user is shown of the last outcome: an entry's toast
    /// (#141), or the status line.
    fn status(&self) -> Status {
        shown(&self.launcher)
    }

    fn actions(&self) -> ItemActions {
        self.launcher
            .item_actions()
            .expect("the selected item has actions")
    }

    fn submenu(&self) -> OpenSubmenu {
        self.launcher.submenu().expect("a submenu is open")
    }
}

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn copy(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

fn answered(text: &str) -> Status {
    Status::Result(text.into())
}

/// The titles of `submenu`'s entries.
fn titles(submenu: &OpenSubmenu) -> Vec<&str> {
    submenu
        .entries()
        .iter()
        .map(|entry| entry.title.as_str())
        .collect()
}

/// The sections of `submenu`'s entries.
fn sections(submenu: &OpenSubmenu) -> Vec<Option<&str>> {
    submenu
        .entries()
        .iter()
        .map(|entry| entry.section.as_deref())
        .collect()
}

#[test]
fn the_items_actions_say_which_open_a_submenu() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        let actions = pane.actions();
        let listed: Vec<(&str, bool)> = actions
            .actions
            .iter()
            .map(|action| (action.title.as_str(), action.submenu))
            .collect();
        assert_eq!(
            listed,
            [
                ("Open", false),
                ("Open With…", true),
                ("Move to List…", true),
                ("Tag…", true),
            ],
            "{}",
            fixture.title
        );
        // Typing at the top level filters the actions.
        assert_eq!(actions.matching("open"), [0, 1]);
        assert_eq!(actions.matching("list"), [MOVE_TO_LIST]);

        // An action that opens a submenu runs nothing from the list: the
        // window opens the panel at it.
        block_on(pane.launcher.run_selected_action(OPEN_WITH));
        block_on(pane.launcher.run_item_action(DELTA, MOVE_TO_LIST));
        assert_eq!(pane.status(), Status::Idle);
        assert_eq!(pane.launcher.submenu(), None);
        // The primary action is an ordinary one.
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.status(), answered("Open: Delta note"));
    }
}

#[test]
fn an_eager_submenu_lists_its_entries_and_runs_the_one_chosen() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        let submenu = pane.submenu();
        assert_eq!(
            (
                submenu.target.as_str(),
                submenu.title.as_str(),
                submenu.depth
            ),
            (DELTA, "Open With", 1),
            "{}",
            fixture.title
        );
        assert_eq!(
            titles(&submenu),
            ["Notepad", "WordPad", "Browser", "Forget Applications"]
        );
        assert_eq!(
            sections(&submenu),
            [
                Some("Editors"),
                Some("Editors"),
                Some("Other"),
                Some("Danger")
            ]
        );
        let destructive: Vec<bool> = submenu
            .entries()
            .iter()
            .map(|entry| entry.destructive)
            .collect();
        assert_eq!(destructive, [false, false, false, true]);
        let shortcuts: Vec<Option<String>> = submenu
            .entries()
            .iter()
            .map(|entry| entry.shortcut.as_ref().map(Binding::id))
            .collect();
        let bound = |id: &str| Some(binding(id).id());
        assert_eq!(
            shortcuts,
            [
                bound("ctrl-shift-n"),
                None,
                bound("ctrl-shift-b"),
                bound("ctrl-shift-d")
            ]
        );
        assert_eq!(submenu.bound_to(&binding("ctrl-shift-b")), Some(2));
        assert_eq!(submenu.bound_to(&binding("ctrl-b")), None);
        // Opening it ran nothing.
        assert_eq!(pane.status(), Status::Idle);

        block_on(pane.launcher.run_submenu_entry(DELTA, 0));
        assert_eq!(pane.status(), answered("Open With Notepad: Delta note"));
        // The submenus closed with the choice; the list is drawn again,
        // the selection kept.
        assert_eq!(pane.launcher.submenu(), None);
        assert_eq!(pane.actions().title, "Delta note");

        // The destructive entry runs as any other.
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        block_on(pane.launcher.run_submenu_entry(DELTA, 3));
        assert_eq!(pane.status(), answered("Forget Applications: Delta note"));
    }
}

#[test]
fn choosing_an_entry_runs_it_once() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        let chosen = pane.launcher.run_submenu_entry(DELTA, 2);
        assert_eq!(pane.status(), Status::Running);
        // The same choice again, as a repeat would make it: nothing is open
        // to run it from.
        let again = pane.launcher.run_submenu_entry(DELTA, 2);
        block_on(chosen);
        assert_eq!(
            pane.status(),
            answered("Open With Browser: Delta note"),
            "{}",
            fixture.title
        );
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.status(), answered("Open: Delta note"));
        block_on(again);
        block_on(pane.launcher.run_submenu_entry(DELTA, 2));
        assert_eq!(
            pane.status(),
            answered("Open: Delta note"),
            "the repeats ran nothing"
        );
    }
}

#[test]
fn a_lazy_submenu_is_asked_once_per_opening_and_lists_the_answer() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        let asking = pane.launcher.open_submenu(DELTA, MOVE_TO_LIST);
        let submenu = pane.submenu();
        assert_eq!(
            (submenu.title.as_str(), &submenu.state),
            ("Move to List", &SubmenuState::Loading),
            "{}",
            fixture.title
        );
        assert!(submenu.entries().is_empty());
        block_on(asking);
        let submenu = pane.submenu();
        assert_eq!(titles(&submenu), ["Inbox", "Later", "Someday"]);
        assert_eq!(sections(&submenu), [Some("Asked 1 time"); 3]);
        // Asking drew nothing again and showed nothing.
        assert_eq!(pane.status(), Status::Idle);

        // Escape steps back to the item's actions; opening it again asks
        // again, once.
        assert!(pane.launcher.close_submenu());
        assert_eq!(pane.launcher.submenu(), None);
        assert!(!pane.launcher.close_submenu(), "nothing more to close");
        block_on(pane.launcher.open_submenu(DELTA, MOVE_TO_LIST));
        assert_eq!(sections(&pane.submenu()), [Some("Asked 2 times"); 3]);

        block_on(pane.launcher.run_submenu_entry(DELTA, 1));
        assert_eq!(pane.status(), answered("Move to Later: Delta note"));
        assert_eq!(pane.launcher.submenu(), None);

        // After the list was drawn again, it is asked again when opened.
        block_on(pane.launcher.open_submenu(DELTA, MOVE_TO_LIST));
        assert_eq!(sections(&pane.submenu()), [Some("Asked 3 times"); 3]);
    }
}

#[test]
fn a_failed_lazy_submenu_shows_why_and_stays_open() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        block_on(pane.launcher.open_submenu(DELTA, TAG));
        let submenu = pane.submenu();
        assert_eq!(submenu.title, "Tags", "{}", fixture.title);
        assert!(
            matches!(&submenu.state, SubmenuState::Failed(why)
                if why.contains("The tags could not be loaded")),
            "{:?}",
            submenu.state
        );
        assert!(submenu.entries().is_empty());
        // The status line says nothing, and nothing runs from it.
        assert_eq!(pane.status(), Status::Idle);
        block_on(pane.launcher.run_submenu_entry(DELTA, 0));
        assert_eq!(pane.status(), Status::Idle);
        assert!(pane.launcher.submenu().is_some(), "it stays open");

        // A failure is the command's answer, not a crash: the package
        // keeps working.
        assert!(pane.launcher.close_submenu());
        block_on(pane.launcher.open_submenu(DELTA, MOVE_TO_LIST));
        assert_eq!(titles(&pane.submenu()), ["Inbox", "Later", "Someday"]);
    }
}

#[test]
fn a_late_answer_is_discarded() {
    for fixture in ALL {
        let pane = Pane::open(fixture);

        // The panel closed before the command answered.
        let asking = pane.launcher.open_submenu(DELTA, MOVE_TO_LIST);
        pane.launcher.close_submenus();
        block_on(asking);
        assert_eq!(pane.launcher.submenu(), None, "{}", fixture.title);

        // The panel stepped back, and another submenu opened.
        let asking = pane.launcher.open_submenu(DELTA, MOVE_TO_LIST);
        assert!(pane.launcher.close_submenu());
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        block_on(asking);
        let submenu = pane.submenu();
        assert_eq!(submenu.title, "Open With");
        assert_eq!(titles(&submenu)[0], "Notepad");
        assert!(pane.launcher.close_submenu());

        // The selection moved to another item.
        let asking = pane.launcher.open_submenu(DELTA, MOVE_TO_LIST);
        select_title(&pane.launcher, "Gamma note");
        assert_eq!(pane.launcher.submenu(), None);
        block_on(asking);
        select_title(&pane.launcher, "Delta note");
        assert_eq!(pane.launcher.submenu(), None, "it shows nothing");
        assert_eq!(pane.status(), Status::Idle);

        // Each opening was asked, and the next one shows its own answer.
        block_on(pane.launcher.open_submenu(DELTA, MOVE_TO_LIST));
        assert_eq!(sections(&pane.submenu()), [Some("Asked 4 times"); 3]);
    }
}

#[test]
fn typing_filters_a_submenus_entries_with_the_sections_flattened() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        let submenu = pane.submenu();
        assert_eq!(submenu.matching("pad"), [0, 1], "{}", fixture.title);
        assert_eq!(submenu.matching("  BROWSER "), [2]);
        assert!(submenu.matching("zzz").is_empty());
        assert_eq!(submenu.matching(""), [0, 1, 2, 3]);
        // What matches across the Editors, Other and Danger sections is
        // listed as one.
        let matched = submenu.matching("r");
        assert_eq!(matched, [1, 2, 3]);
        let across: Vec<Option<&str>> = matched
            .iter()
            .map(|&index| submenu.entries()[index].section.as_deref())
            .collect();
        assert_eq!(across, [Some("Editors"), Some("Other"), Some("Danger")]);

        // A lazy submenu's answer filters the same way.
        assert!(pane.launcher.close_submenu());
        block_on(pane.launcher.open_submenu(DELTA, MOVE_TO_LIST));
        assert_eq!(pane.submenu().matching("LATER"), [1]);
    }
}

#[test]
fn only_an_action_that_opens_a_submenu_opens_one_on_its_own_item() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        // "Open" calls the command back; there is no fifth action; "beta"
        // is not the selected item.
        block_on(pane.launcher.open_submenu(DELTA, 0));
        block_on(pane.launcher.open_submenu(DELTA, 9));
        block_on(pane.launcher.open_submenu("beta", OPEN_WITH));
        assert_eq!(pane.launcher.submenu(), None, "{}", fixture.title);
        assert_eq!(pane.status(), Status::Idle);
        // An entry of another item's submenu runs nothing.
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        block_on(pane.launcher.run_submenu_entry("beta", 0));
        assert_eq!(pane.status(), Status::Idle);
        assert!(pane.launcher.submenu().is_some());
    }
}

#[test]
fn a_submenu_entrys_shortcut_follows_the_binding_rules() {
    for fixture in ALL {
        let pane = Pane::open(fixture);
        assert!(
            pane.launcher
                .unbound_shortcuts()
                .iter()
                .all(|shortcut| shortcut.item != "Delta note"),
            "{}: every entry's shortcut is bound",
            fixture.title
        );

        // The user gives one of Pane's actions the keys of "Browser".
        let mut keyboard = Keyboard::default_for_this_system();
        keyboard
            .checked_set(KeyboardAction::DismissLauncher, binding("ctrl-shift-b"))
            .unwrap();
        pane.launcher
            .set_pane_keys(PaneKeys::new(&keyboard, NavigationBindings::None));
        block_on(pane.launcher.open_submenu(DELTA, OPEN_WITH));
        let submenu = pane.submenu();
        let browser = &submenu.entries()[2];
        assert_eq!(browser.shortcut, None);
        assert!(
            browser
                .unbound
                .as_ref()
                .is_some_and(|why| why.contains("dismisses the launcher")),
            "{:?}",
            browser.unbound
        );
        assert_eq!(submenu.bound_to(&binding("ctrl-shift-b")), None);
        // It still runs from the submenu.
        block_on(pane.launcher.run_submenu_entry(DELTA, 2));
        assert_eq!(pane.status(), answered("Open With Browser: Delta note"));

        // The report names it under the action that opens its submenu.
        let unbound: Vec<(String, String)> = pane
            .launcher
            .unbound_shortcuts()
            .into_iter()
            .filter(|shortcut| shortcut.item == "Delta note")
            .map(|shortcut| (shortcut.item, shortcut.action))
            .collect();
        assert_eq!(
            unbound,
            [("Delta note".to_owned(), "Open With… › Browser".to_owned())]
        );
    }
}
