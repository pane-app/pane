//! Several actions per item (#137) through the launcher's public interface,
//! with the actions sample in Rust, JavaScript and TypeScript, real guests
//! `cargo xtask guests` assembles: Enter, Ctrl+Enter and Ctrl+Shift+Enter run
//! an item's first three actions and nothing for a missing one; an item
//! without actions cannot be activated; the Actions panel's list keeps the
//! sections, the destructive style and the shortcuts Pane binds, and
//! filters by title with the sections flattened; a shortcut runs its action
//! with its modifiers matched exactly and on its own system only; a
//! shortcut that is one of Pane's keys, the user's rebinds included, is not
//! bound and is reported while the package is developed. Prior art:
//! `result_actions.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{
    Binding, ItemActions, Keyboard, KeyboardAction, Launcher, NavigationBindings, PackageIdentity,
    PaneKeys, Runtime, Screen, Status,
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

/// The titles of "Alpha note"'s actions, in order.
const ALPHA_ACTIONS: [&str; 9] = [
    "Open",
    "Copy",
    "Rename",
    "Duplicate",
    "Archive",
    "Copy Link",
    "Reveal",
    "Open Menu",
    "Delete",
];

/// "Reveal"'s shortcut on this system, and the other systems' ones, which
/// bind nothing here.
fn reveal_keys() -> (&'static str, [&'static str; 2]) {
    if cfg!(target_os = "windows") {
        ("ctrl-shift-e", ["cmd-shift-r", "ctrl-shift-l"])
    } else if cfg!(target_os = "macos") {
        ("cmd-shift-r", ["ctrl-shift-e", "ctrl-shift-l"])
    } else {
        ("ctrl-shift-l", ["ctrl-shift-e", "cmd-shift-r"])
    }
}

fn binding(id: &str) -> Binding {
    Binding::parse(id).unwrap()
}

/// A build that never runs: development only has to be on.
struct NoBuild;

impl Builder for NoBuild {
    fn build_for(&self, _folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(NoBuild))
    }
}

impl Build for NoBuild {
    fn command(&self) -> String {
        "no build".into()
    }

    fn ignores(&self, _path: &Path) -> bool {
        true
    }

    fn run(&self, _job: &BuildJob) -> BuildOutcome {
        BuildOutcome::Stopped
    }
}

/// One test's Pane, with one language's sample installed.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    launcher: Launcher,
    identity: PackageIdentity,
}

impl Pane {
    /// Pane with `fixture`'s sample installed from a source folder, and
    /// development available (not on).
    fn with(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (changes, _) = pane_core::changes::channel();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_development(Arc::new(NoBuild), changes);
        let folder = copy(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        Pane {
            identity: PackageIdentity::local(&folder).unwrap(),
            _sources: sources,
            _data: data,
            launcher,
        }
    }

    /// Opens the sample's command from root search.
    fn open(&self, fixture: &Fixture) {
        block_on(self.launcher.set_query(fixture.command));
        select_title(&self.launcher, fixture.command);
        block_on(self.launcher.activate_selected());
        let view = self.launcher.view();
        assert_eq!(
            (&view.screen, view.title.as_str()),
            (&Screen::Command, "Actions sample"),
            "{:?}",
            view.status
        );
    }

    fn status(&self) -> Status {
        self.launcher.view().status
    }

    /// What the user is shown of the last outcome: an action's toast, or
    /// the status line.
    fn shown(&self) -> Status {
        shown(&self.launcher)
    }

    fn actions(&self) -> ItemActions {
        self.launcher
            .item_actions()
            .expect("the selected item has actions")
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

#[test]
fn enter_and_the_action_chords_run_the_first_three_actions() {
    for fixture in ALL {
        let pane = Pane::with(fixture);
        pane.open(fixture);
        let launcher = &pane.launcher;
        select_title(launcher, "Alpha note");

        // The footer names the primary action.
        let primary = launcher.selected_action();
        assert_eq!((primary.label.as_str(), primary.available), ("Open", true));

        block_on(launcher.activate_selected());
        assert_eq!(
            pane.shown(),
            answered("Open: Alpha note"),
            "{}",
            fixture.title
        );
        block_on(launcher.run_selected_action(1));
        assert_eq!(pane.shown(), answered("Copy: Alpha note"));
        block_on(launcher.run_selected_action(2));
        assert_eq!(pane.shown(), answered("Rename: Alpha note"));
        // Pane drew the list again after each, keeping the selection.
        assert_eq!(pane.actions().title, "Alpha note");

        // An item with one action: the chords run nothing.
        select_title(launcher, "Beta note");
        assert_eq!(launcher.selected_action().label, "Open");
        block_on(launcher.run_selected_action(1));
        block_on(launcher.run_selected_action(2));
        assert_eq!(pane.shown(), answered("Rename: Alpha note"), "nothing ran");
        block_on(launcher.activate_selected());
        assert_eq!(pane.shown(), answered("Open: Beta note"));
    }
}

#[test]
fn an_item_without_actions_cannot_be_activated_and_says_so() {
    for fixture in ALL {
        let pane = Pane::with(fixture);
        pane.open(fixture);
        let launcher = &pane.launcher;
        select_title(launcher, "Gamma note");

        let primary = launcher.selected_action();
        assert_eq!(
            (primary.label.as_str(), primary.available),
            ("No actions", false)
        );
        assert_eq!(launcher.item_actions(), None);
        block_on(launcher.activate_selected());
        assert!(
            matches!(pane.status(), Status::Error(why) if why.contains("cannot be activated")),
            "{:?}",
            pane.status()
        );
        block_on(launcher.run_selected_action(1));
        assert!(matches!(pane.status(), Status::Error(_)), "nothing ran");
    }
}

#[test]
fn the_panel_lists_sections_shortcuts_and_the_destructive_style() {
    let (reveal, _) = reveal_keys();
    for fixture in ALL {
        let pane = Pane::with(fixture);
        pane.open(fixture);
        select_title(&pane.launcher, "Alpha note");
        let actions = pane.actions();

        assert_eq!(actions.target, "alpha");
        let titles: Vec<&str> = actions.actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ALPHA_ACTIONS, "{}", fixture.title);
        let sections: Vec<Option<&str>> = actions
            .actions
            .iter()
            .map(|action| action.section.as_deref())
            .collect();
        assert_eq!(
            sections,
            [
                None,
                None,
                Some("Edit"),
                Some("Edit"),
                Some("Edit"),
                Some("Share"),
                Some("Share"),
                Some("Share"),
                Some("Danger"),
            ]
        );
        let destructive: Vec<&str> = actions
            .actions
            .iter()
            .filter(|action| action.destructive)
            .map(|action| action.title.as_str())
            .collect();
        assert_eq!(destructive, ["Delete"]);
        let shortcuts: Vec<Option<String>> = actions
            .actions
            .iter()
            .map(|action| action.shortcut.as_ref().map(Binding::id))
            .collect();
        let bound = |id: &str| Some(binding(id).id());
        assert_eq!(
            shortcuts,
            [
                None,
                None,
                bound("ctrl-r"),
                bound("ctrl-d"),
                bound("ctrl-shift-y"),
                bound("ctrl-shift-c"),
                bound(reveal),
                // Pane's own Ctrl+K: listed, without the shortcut.
                None,
                bound("ctrl-x"),
            ]
        );
        let menu = &actions.actions[7];
        assert!(
            menu.unbound
                .as_ref()
                .is_some_and(|why| why.contains("in Pane")),
            "{:?}",
            menu.unbound
        );

        // Typing filters by title, with the sections flattened.
        assert_eq!(actions.matching("copy"), [1, 5]);
        assert_eq!(actions.matching("  DELETE "), [8]);
        assert!(actions.matching("zzz").is_empty());

        // The panel runs an action on its item, while it is still selected.
        block_on(pane.launcher.run_item_action("alpha", 8));
        assert_eq!(pane.shown(), answered("Delete: Alpha note"));
        block_on(pane.launcher.run_item_action("beta", 0));
        assert_eq!(
            pane.shown(),
            answered("Delete: Alpha note"),
            "another item's action runs nothing"
        );
        block_on(pane.launcher.run_item_action("alpha", 9));
        assert_eq!(pane.shown(), answered("Delete: Alpha note"), "no tenth");
    }
}

#[test]
fn a_shortcut_runs_its_action_with_its_modifiers_exactly_on_its_system() {
    let (reveal, elsewhere) = reveal_keys();
    for fixture in ALL {
        let pane = Pane::with(fixture);
        pane.open(fixture);
        select_title(&pane.launcher, "Alpha note");
        let actions = pane.actions();

        let index = actions.bound_to(&binding("ctrl-shift-c")).unwrap();
        block_on(pane.launcher.run_selected_action(index));
        assert_eq!(pane.shown(), answered("Copy Link: Alpha note"));
        let index = actions.bound_to(&binding(reveal)).unwrap();
        block_on(pane.launcher.run_selected_action(index));
        assert_eq!(pane.shown(), answered("Reveal: Alpha note"));
        let index = actions.bound_to(&binding("ctrl-x")).unwrap();
        block_on(pane.launcher.run_selected_action(index));
        assert_eq!(pane.shown(), answered("Delete: Alpha note"));

        // Exactly these modifiers: no more, no fewer.
        for other in [
            "ctrl-c",
            "ctrl-alt-shift-c",
            "ctrl-shift-r",
            "ctrl-shift-d",
            "x",
        ] {
            assert_eq!(actions.bound_to(&binding(other)), None, "{other}");
        }
        // Another system's shortcut binds nothing here.
        for other in elsewhere {
            assert_eq!(actions.bound_to(&binding(other)), None, "{other}");
        }
        // Pane's own Ctrl+K is not the action's.
        assert_eq!(actions.bound_to(&binding("ctrl-k")), None);
    }
}

#[test]
fn a_shortcut_the_user_gave_a_pane_action_is_not_bound() {
    for fixture in ALL {
        let pane = Pane::with(fixture);
        pane.open(fixture);
        select_title(&pane.launcher, "Alpha note");
        let archive = binding("ctrl-shift-y");
        assert_eq!(pane.actions().bound_to(&archive), Some(4), "free at first");

        // The user rebinds Dismiss launcher to the archive's keys.
        let mut keyboard = Keyboard::default_for_this_system();
        keyboard
            .checked_set(KeyboardAction::DismissLauncher, archive.clone())
            .unwrap();
        pane.launcher
            .set_pane_keys(PaneKeys::new(&keyboard, NavigationBindings::None));
        let actions = pane.actions();
        assert_eq!(actions.bound_to(&archive), None);
        let action = &actions.actions[4];
        assert_eq!(action.title, "Archive");
        assert_eq!(action.shortcut, None);
        assert!(
            action
                .unbound
                .as_ref()
                .is_some_and(|why| why.contains("dismisses the launcher")),
            "{:?}",
            action.unbound
        );
        // The action stays listed and runs from the panel.
        block_on(pane.launcher.run_item_action("alpha", 4));
        assert_eq!(pane.shown(), answered("Archive: Alpha note"));

        let unbound: Vec<(String, String)> = pane
            .launcher
            .unbound_shortcuts()
            .into_iter()
            .map(|shortcut| (shortcut.item, shortcut.action))
            .collect();
        assert_eq!(
            unbound,
            [
                ("Alpha note".to_owned(), "Archive".to_owned()),
                ("Alpha note".to_owned(), "Open Menu".to_owned()),
            ]
        );

        // Given back, the keys are the action's again.
        pane.launcher.set_pane_keys(PaneKeys::default());
        assert_eq!(pane.actions().bound_to(&archive), Some(4));
    }
}

#[test]
fn development_mode_reports_the_shortcuts_pane_does_not_bind() {
    for fixture in ALL {
        let pane = Pane::with(fixture);
        // Not developed: opening says nothing.
        pane.open(fixture);
        assert_eq!(pane.status(), Status::Idle);
        while !matches!(pane.launcher.view().screen, Screen::Root { .. }) {
            pane.launcher.back();
        }

        block_on(pane.launcher.start_developing(&pane.identity));
        assert!(
            pane.launcher.development(&pane.identity).is_some(),
            "{:?}",
            pane.status()
        );
        pane.open(fixture);
        let Status::Error(report) = pane.status() else {
            panic!("no report: {:?}", pane.status());
        };
        assert!(report.contains("“Open Menu” of “Alpha note”"), "{report}");
        assert!(report.contains("in Pane"), "{report}");
        assert!(!report.contains("Archive"), "{report}");

        // The same list drawn again after an action is not reported again.
        select_title(&pane.launcher, "Alpha note");
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.shown(), answered("Open: Alpha note"));
        pane.launcher.stop_developing(&pane.identity);
    }
}
