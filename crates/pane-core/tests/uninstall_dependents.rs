//! Uninstalling a package other installed packages require, through the
//! launcher's public interface: before anything changes, Pane lists every
//! installed package that requires it (directly or through each other,
//! disabled ones included, optional users excluded) with each one's saved
//! data, and offers Uninstall all with the saved-data choice of a single
//! uninstall, or Cancel. Cancel changes nothing; Uninstall all removes
//! exactly the shown set as uninstalling each would, in one record, and
//! reports a copy or data it could not remove against the package it
//! belongs to. Installing the dependency again installs it alone. Real
//! guests from `cargo xtask guests`: the settings sample (declaring a
//! dependency) and the operations fixture serve as packages.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{
    Launcher, PackageIdentity, Question, RetainedData, Runtime, SavedData, Screen, Status,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest_file as guest;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage extensions…";

struct Dirs {
    sources: TempDir,
    data: TempDir,
    runtime: Runtime,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
        }
    }

    /// A launcher on this data folder; a new one is a restart of Pane.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    /// Writes an operations fixture package in folder `name`, titled
    /// "Package <name>", publishing `echo` 1 and declaring `dependencies`
    /// (JSON array contents).
    fn fixture(&self, name: &str, dependencies: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("operations_fixture.wasm"),
            folder.join("fixture.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package {name}",
                "apiVersion": "0.1",
                "operations": [{{ "id": "echo", "version": 1, "component": "fixture.wasm" }}],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Writes the Rust settings sample in folder `name`, titled `title`,
    /// declaring `dependencies`: its Greeting command keeps a greeting
    /// style in its settings.
    fn settings(&self, name: &str, title: &str, dependencies: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("sample_settings.wasm"),
            folder.join("sample_settings.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "{title}",
                "apiVersion": "0.1",
                "commands": [
                    {{ "id": "greeting", "title": "Greeting", "component": "sample_settings.wasm" }}
                ],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }
}

/// A required dependency on sibling folder `folder`'s `echo` 1.
fn needs(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}",
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

/// An optional dependency on sibling folder `folder`'s `echo` 1.
fn uses(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}", "optional": true,
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

fn press(launcher: &Launcher, title: &str) -> Status {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn to_root(launcher: &Launcher) {
    for _ in 0..3 {
        launcher.back();
    }
}

fn manage(launcher: &Launcher) {
    to_root(launcher);
    press(launcher, MANAGE_ROW);
    assert!(
        matches!(launcher.view().screen, Screen::Extensions { .. }),
        "{:?}",
        launcher.view().screen
    );
}

/// Opens the Greeting command and runs its item `item`, returning what it
/// showed: its toast, or the status line.
fn greet(launcher: &Launcher, item: &str) -> Status {
    to_root(launcher);
    press(launcher, "Greeting");
    assert_eq!(launcher.view().screen, Screen::Command);
    press(launcher, item);
    shown(launcher)
}

/// Installs, in folders a to d: Package a; Package c, which requires a;
/// Settings b, which requires c (installing it installs a and c with it);
/// Package d, which only uses a if installed. Settings b saves the formal
/// greeting, so it has saved data and an instance of it runs.
fn four(dirs: &Dirs) -> Launcher {
    dirs.fixture("a", "");
    dirs.fixture("c", &needs("a"));
    let b = dirs.settings("b", "Settings b", &needs("c"));
    let d = dirs.fixture("d", &uses("a"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&b));
    block_on(launcher.install_package(&d));
    assert_eq!(
        installed(&launcher),
        ["Package a", "Package c", "Settings b", "Package d"]
    );
    assert_eq!(
        greet(&launcher, "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );
    launcher
}

/// The titles of the installed packages, in installed order.
fn installed(launcher: &Launcher) -> Vec<String> {
    launcher
        .packages()
        .into_iter()
        .map(|package| package.title())
        .collect()
}

/// The managed copy of the installed package with `identity`.
fn location(launcher: &Launcher, identity: &PackageIdentity) -> PathBuf {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == *identity)
        .unwrap()
        .location
}

/// Whether an instance of Settings b's component is running.
fn b_runs(dirs: &Dirs) -> bool {
    block_on(dirs.runtime.running())
        .iter()
        .any(|path| path.ends_with("sample_settings.wasm"))
}

/// The keys of the settings kept for the package in folder `name`, read
/// from Pane's settings file.
fn settings_of(dirs: &Dirs, name: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(dirs.packages_dir().join("settings.json")) else {
        return Vec::new();
    };
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["packages"][dirs.identity(name).key()]
        .as_object()
        .map(|values| values.keys().cloned().collect())
        .unwrap_or_default()
}

/// Presses "Uninstall <title>" in the extension list, which asks first.
fn ask_to_uninstall(launcher: &Launcher, title: &str) {
    manage(launcher);
    press(launcher, &format!("Uninstall {title}"));
}

const KEEP_ALL_3: &str = "Uninstall all 3 and keep saved data";
const DELETE_ALL_3: &str = "Uninstall all 3 and delete saved data";

#[test]
fn uninstalling_a_required_dependency_shows_its_dependents_and_their_data_and_cancel_keeps_them() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);

    ask_to_uninstall(&launcher, "Package a");

    let view = launcher.view();
    assert_eq!(
        view.screen,
        Screen::Confirm {
            question: Question::UninstallDependents(dirs.identity("a")),
            details: view.details().to_vec(),
        }
    );
    assert_eq!(
        view.title,
        "Uninstall Package a and the extensions that require it?"
    );
    let details = view.details().to_vec();
    for line in [
        format!(
            "Package c, which requires Package a · {}",
            dirs.identity("c")
        ),
        format!(
            "Settings b, which requires Package c · {}",
            dirs.identity("b")
        ),
        "Saved data: Package a none · Package c none · Settings b 1 setting".to_string(),
    ] {
        assert!(details.contains(&line), "{line:?} not in {details:#?}");
    }
    // An optional integration is not affected.
    assert!(
        !details.iter().any(|line| line.contains("Package d")),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), [KEEP_ALL_3, DELETE_ALL_3, "Cancel"]);
    assert_eq!(view.selected, Some(0));
    assert!(b_runs(&dirs));

    assert_eq!(press(&launcher, "Cancel"), Status::Idle);
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    // Back on the row the user pressed.
    assert_eq!(
        view.rows[view.selected.unwrap()].title,
        "Uninstall Package a"
    );

    // Escape cancels too.
    press(&launcher, "Uninstall Package a");
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    let all = ["Package a", "Package c", "Settings b", "Package d"];
    assert_eq!(installed(&launcher), all);
    assert!(b_runs(&dirs));
    assert_eq!(settings_of(&dirs, "b"), ["greeting-style"]);
    let restarted = dirs.launcher();
    assert_eq!(installed(&restarted), all, "nothing was recorded");
    assert!(restarted.retained_data().is_empty());
}

#[test]
fn uninstall_all_keeping_saved_data_removes_the_shown_set_and_the_dependency_comes_back_alone() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    let copies: Vec<PathBuf> = ["a", "c", "b"]
        .into_iter()
        .map(|name| location(&launcher, &dirs.identity(name)))
        .collect();
    ask_to_uninstall(&launcher, "Package a");

    assert_eq!(
        press(&launcher, KEEP_ALL_3),
        Status::Result(
            "Uninstalled Package a and the 2 extensions that require it: Package c and \
             Settings b; their settings and content are kept"
                .into()
        )
    );
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(installed(&launcher), ["Package d"]);
    assert!(!b_runs(&dirs));
    to_root(&launcher);
    assert!(!titles(&launcher).contains(&"Greeting".to_string()));
    for copy in &copies {
        assert!(!copy.exists(), "{} was removed", copy.display());
    }
    // Only the package with saved data keeps a record of it.
    let retained = [RetainedData {
        identity: dirs.identity("b"),
        title: "Settings b".into(),
    }];
    assert_eq!(launcher.retained_data(), retained);
    assert_eq!(settings_of(&dirs, "b"), ["greeting-style"]);
    // The source folders are untouched.
    for name in ["a", "b", "c"] {
        assert!(dirs.folder(name).join("pane.json").is_file());
    }

    // It is on record; installing the dependency again installs it alone.
    let restarted = dirs.launcher();
    assert_eq!(installed(&restarted), ["Package d"]);
    block_on(restarted.install_package(&dirs.folder("a")));
    assert_eq!(installed(&restarted), ["Package d", "Package a"]);
    assert_eq!(installed(&dirs.launcher()), ["Package d", "Package a"]);
    assert_eq!(restarted.retained_data(), retained);

    // Settings b installed again finds its settings.
    block_on(restarted.install_package(&dirs.folder("b")));
    assert_eq!(
        installed(&restarted),
        ["Package d", "Package a", "Package c", "Settings b"]
    );
    assert_eq!(
        greet(&restarted, "Greet me"),
        Status::Result("Good day to you".into())
    );
}

#[test]
fn uninstall_all_deleting_saved_data_deletes_each_ones_settings() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    ask_to_uninstall(&launcher, "Package a");

    assert_eq!(
        press(&launcher, DELETE_ALL_3),
        Status::Result(
            "Uninstalled Package a and the 2 extensions that require it: Package c and \
             Settings b, and deleted their saved data"
                .into()
        )
    );
    assert_eq!(installed(&launcher), ["Package d"]);
    assert!(settings_of(&dirs, "b").is_empty());
    assert!(launcher.retained_data().is_empty());
    assert!(dirs.launcher().retained_data().is_empty());
}

#[test]
fn a_disabled_dependent_is_listed_and_uninstalled_with_it() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    block_on(launcher.set_enabled(&dirs.identity("b"), false));

    ask_to_uninstall(&launcher, "Package c");
    let details = launcher.view().details().to_vec();
    let line = format!(
        "Settings b (disabled), which requires Package c · {}",
        dirs.identity("b")
    );
    assert!(details.contains(&line), "{line:?} not in {details:#?}");
    assert_eq!(
        press(&launcher, "Uninstall all 2 and keep saved data"),
        Status::Result(
            "Uninstalled Package c and Settings b, which requires it; their settings and \
             content are kept"
                .into()
        )
    );
    assert_eq!(installed(&launcher), ["Package a", "Package d"]);
}

#[test]
fn a_package_only_optionally_used_is_asked_about_alone() {
    let dirs = Dirs::new();
    dirs.fixture("a", "");
    let d = dirs.fixture("d", &uses("a"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&dirs.folder("a")));
    block_on(launcher.install_package(&d));

    ask_to_uninstall(&launcher, "Package a");
    assert!(matches!(
        launcher.view().screen,
        Screen::Confirm {
            question: Question::Uninstall(_),
            ..
        }
    ));
    assert_eq!(
        press(&launcher, "Uninstall and keep saved data"),
        Status::Result("Uninstalled Package a; its settings and content are kept".into())
    );
    assert_eq!(installed(&launcher), ["Package d"]);
}

#[test]
fn packages_requiring_each_other_are_uninstalled_together() {
    let dirs = Dirs::new();
    dirs.fixture("x", &needs("y"));
    let y = dirs.fixture("y", &needs("x"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&y));
    ask_to_uninstall(&launcher, "Package x");
    assert_eq!(
        titles(&launcher),
        [
            "Uninstall all 2 and keep saved data",
            "Uninstall all 2 and delete saved data",
            "Cancel"
        ]
    );
    assert_eq!(
        press(&launcher, "Uninstall all 2 and delete saved data"),
        Status::Result(
            "Uninstalled Package x and Package y, which requires it, and deleted their saved \
             data"
                .into()
        )
    );
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_dependent_that_appears_while_the_question_is_shown_is_asked_about_again() {
    let dirs = Dirs::new();
    dirs.fixture("a", "");
    let c = dirs.fixture("c", &needs("a"));
    let b = dirs.settings("b", "Settings b", "");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&c));
    block_on(launcher.install_package(&b));
    ask_to_uninstall(&launcher, "Package a");
    assert_eq!(titles(&launcher)[0], "Uninstall all 2 and keep saved data");

    // Meanwhile Settings b is reloaded from a folder where it requires
    // Package c.
    dirs.settings("b", "Settings b", &needs("c"));
    block_on(launcher.reload(&dirs.identity("b")));
    assert_eq!(
        press(&launcher, "Uninstall all 2 and keep saved data"),
        Status::Error(
            "What uninstalling Package a affects changed since it was shown; check it again \
             and choose Uninstall all once more"
                .into()
        )
    );
    assert_eq!(titles(&launcher), [KEEP_ALL_3, DELETE_ALL_3, "Cancel"]);
    assert_eq!(
        installed(&launcher),
        ["Package a", "Package c", "Settings b"]
    );

    assert_eq!(
        press(&launcher, KEEP_ALL_3),
        Status::Result(
            "Uninstalled Package a and the 2 extensions that require it: Package c and \
             Settings b; their settings and content are kept"
                .into()
        )
    );
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_dependent_uninstalled_while_the_question_is_shown_is_skipped() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    ask_to_uninstall(&launcher, "Package a");

    block_on(launcher.uninstall(&dirs.identity("b"), SavedData::Delete));
    assert_eq!(
        press(&launcher, KEEP_ALL_3),
        Status::Result(
            "Uninstalled Package a and Package c, which requires it; their settings and \
             content are kept"
                .into()
        )
    );
    assert_eq!(installed(&launcher), ["Package d"]);
}

#[test]
fn a_dependency_uninstalled_alone_while_the_question_is_shown_closes_it() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    ask_to_uninstall(&launcher, "Package a");

    // `Launcher::uninstall` uninstalls the package given alone.
    block_on(launcher.uninstall(&dirs.identity("a"), SavedData::Keep));
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(
        launcher.view().status,
        Status::Result("Uninstalled Package a; its settings and content are kept".into())
    );
    assert_eq!(
        installed(&launcher),
        ["Package c", "Settings b", "Package d"]
    );
}

#[test]
fn an_uninstall_all_that_cannot_be_recorded_uninstalls_none_of_them() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    // `installed.json` cannot be replaced by a file while a folder is there.
    let registry = dirs.packages_dir().join("installed.json");
    let text = fs::read(&registry).unwrap();
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    fs::write(registry.join("blocker"), "").unwrap();

    ask_to_uninstall(&launcher, "Package a");
    let status = press(&launcher, DELETE_ALL_3);
    match &status {
        Status::Error(e) => {
            assert!(
                e.starts_with("Could not uninstall Package a, Package c and Settings b: "),
                "{e}"
            );
            assert!(
                e.ends_with("They are all still installed and nothing was deleted."),
                "{e}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    let all = ["Package a", "Package c", "Settings b", "Package d"];
    assert_eq!(installed(&launcher), all);
    assert_eq!(settings_of(&dirs, "b"), ["greeting-style"]);
    assert_eq!(
        greet(&launcher, "Greet me"),
        Status::Result("Good day to you".into())
    );

    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, text).unwrap();
    assert_eq!(installed(&dirs.launcher()), all);
}

/// A managed copy Pane cannot remove, as Windows refuses for a file in use:
/// here, Package c's managed folder is made read-only, so the files in it
/// cannot be removed. Only Package c's failure is reported, against it.
#[cfg(unix)]
#[test]
fn a_copy_that_cannot_be_removed_is_reported_against_its_package() {
    use std::os::unix::fs::PermissionsExt;
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    let c = location(&launcher, &dirs.identity("c"));
    let others = [
        location(&launcher, &dirs.identity("a")),
        location(&launcher, &dirs.identity("b")),
    ];
    fs::set_permissions(&c, fs::Permissions::from_mode(0o555)).unwrap();

    ask_to_uninstall(&launcher, "Package a");
    let status = press(&launcher, KEEP_ALL_3);
    fs::set_permissions(&c, fs::Permissions::from_mode(0o755)).unwrap();
    match &status {
        Status::Error(e) => {
            assert!(
                e.starts_with(
                    "Uninstalled Package a and the 2 extensions that require it: Package c and \
                     Settings b, but for Package c, its installed copy in "
                ),
                "{e}"
            );
            assert!(e.ends_with("; Pane removes it when it next starts."), "{e}");
            assert!(!e.contains("for Package a"), "{e}");
            assert!(!e.contains("for Settings b"), "{e}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
    // All three are uninstalled, as recorded; only Package c's copy is left.
    assert_eq!(installed(&launcher), ["Package d"]);
    assert!(c.exists());
    for other in &others {
        assert!(!other.exists(), "{} was removed", other.display());
    }

    let restarted = dirs.launcher();
    assert_eq!(installed(&restarted), ["Package d"]);
    assert!(!c.exists(), "{} was removed at the next start", c.display());
}

#[test]
fn another_panes_install_on_the_same_folder_survives_an_uninstall() {
    let dirs = Dirs::new();
    dirs.fixture("a", "");
    let c = dirs.fixture("c", &needs("a"));
    let z = dirs.fixture("z", "");
    let y = dirs.fixture("y", "");
    let first = dirs.launcher();
    block_on(first.install_package(&c));
    block_on(first.install_package(&y));
    // A second Pane on the same data folder installs Package z meanwhile.
    let second = dirs.launcher();
    block_on(second.install_package(&z));
    assert_eq!(installed(&first), ["Package a", "Package c", "Package y"]);

    ask_to_uninstall(&first, "Package a");
    assert_eq!(
        press(&first, "Uninstall all 2 and delete saved data"),
        Status::Result(
            "Uninstalled Package a and Package c, which requires it, and deleted their saved \
             data"
                .into()
        )
    );
    // Uninstalling one package alone keeps it too.
    block_on(first.uninstall(&dirs.identity("y"), SavedData::Delete));
    assert!(first.packages().is_empty());

    let restarted = dirs.launcher();
    assert_eq!(installed(&restarted), ["Package z"]);
    assert!(
        location(&restarted, &dirs.identity("z"))
            .join("pane.json")
            .is_file()
    );
}

/// The folders granted in Pane's `folders.json`, by package key.
fn granted_folders(dirs: &Dirs) -> Vec<String> {
    let path = dirs.packages_dir().join("folders.json");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    record["folders"]
        .as_object()
        .map(|folders| folders.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn uninstall_all_forgets_the_folder_granted_to_each_package() {
    let dirs = Dirs::new();
    dirs.fixture("x", &needs("y"));
    let y = dirs.fixture("y", &needs("x"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&y));
    let shared = tempfile::tempdir().unwrap();
    for name in ["x", "y"] {
        let folder = shared.path().join(name);
        fs::create_dir(&folder).unwrap();
        block_on(launcher.grant_folder(&dirs.identity(name), &folder));
    }
    let mut both = vec![dirs.identity("x").key(), dirs.identity("y").key()];
    both.sort();
    let mut granted = granted_folders(&dirs);
    granted.sort();
    assert_eq!(granted, both);

    ask_to_uninstall(&launcher, "Package x");
    assert!(matches!(
        press(&launcher, "Uninstall all 2 and keep saved data"),
        Status::Result(_)
    ));
    assert!(launcher.packages().is_empty());
    // Pane's record, not their data: forgotten although data is kept.
    assert_eq!(granted_folders(&dirs), Vec::<String>::new());
    dirs.launcher();
    assert_eq!(granted_folders(&dirs), Vec::<String>::new());
}
