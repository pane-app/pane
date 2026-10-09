//! Disabling a package other installed packages require, through the
//! launcher's public interface: before anything changes, Pane lists the
//! packages that require it (directly or through each other, optional ones
//! excluded) with Disable all and Cancel. Cancel changes nothing; Disable
//! all disables exactly what was shown, stops their running instances and
//! keeps their settings; enabling the dependency again enables it alone.
//! Pane pausing a dependency after it failed is not the user disabling it
//! and disables nothing else. Real guests from `cargo xtask guests`: the
//! settings sample (declaring a dependency), the Rust operations sample and
//! the operations fixture serve as packages.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Question, Runtime, Screen, Status};
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

const MANAGE_ROW: &str = "Manage Extensions";

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

    /// A restart with a runtime of its own, which loads components afresh.
    fn restarted(&self) -> Launcher {
        Launcher::with_packages(Runtime::start(), vec![], self.packages_dir())
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

    /// Copies the assembled Rust operations sample into folder
    /// `sample-operations`.
    fn operations_sample(&self) -> PathBuf {
        let assembled = guest("packages").join("sample-operations");
        let folder = self.folder("sample-operations");
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
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

/// Presses the extension list's row of the package titled `title`, which
/// enables or disables it, or asks first.
fn toggle(launcher: &Launcher, title: &str) -> Status {
    manage(launcher);
    press(launcher, title)
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

/// Each installed package's title with whether it is enabled.
fn enabled(launcher: &Launcher) -> Vec<(String, bool)> {
    launcher
        .packages()
        .into_iter()
        .map(|package| (package.title(), package.enabled))
        .collect()
}

fn all_enabled(launcher: &Launcher) -> bool {
    launcher.packages().iter().all(|package| package.enabled)
}

/// Installs, in folders a to d: Package a; Package c, which requires a;
/// Settings b, which requires c (installing it installs a and c with it);
/// Package d, which only uses a if installed. Settings b saves the formal
/// greeting, so an instance of it runs.
fn four(dirs: &Dirs) -> Launcher {
    dirs.fixture("a", "");
    dirs.fixture("c", &needs("a"));
    let b = dirs.settings("b", "Settings b", &needs("c"));
    let d = dirs.fixture("d", &uses("a"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&b));
    block_on(launcher.install_package(&d));
    assert_eq!(
        enabled(&launcher),
        [
            ("Package a".into(), true),
            ("Package c".into(), true),
            ("Settings b".into(), true),
            ("Package d".into(), true),
        ]
    );
    assert_eq!(
        greet(&launcher, "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );
    launcher
}

/// Whether an instance of Settings b's component is running.
fn b_runs(dirs: &Dirs) -> bool {
    block_on(dirs.runtime.running())
        .iter()
        .any(|path| path.ends_with("sample_settings.wasm"))
}

#[test]
fn disabling_a_required_dependency_shows_its_dependents_first_and_cancel_changes_nothing() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    assert!(b_runs(&dirs));

    assert_eq!(toggle(&launcher, "Package a"), Status::Idle);

    let view = launcher.view();
    assert_eq!(
        view.screen,
        Screen::Confirm {
            question: Question::DisableDependents(dirs.identity("a")),
            details: view.details().to_vec(),
        }
    );
    assert_eq!(
        view.title,
        "Disable Package a and the extensions that require it?"
    );
    let details = view.details().join("\n");
    assert!(
        details.contains(&format!(
            "Package c, which requires Package a · {}",
            dirs.identity("c")
        )),
        "{details}"
    );
    assert!(
        details.contains(&format!(
            "Settings b, which requires Package c · {}",
            dirs.identity("b")
        )),
        "{details}"
    );
    // An optional integration is not affected.
    assert!(!details.contains("Package d"), "{details}");
    assert_eq!(titles(&launcher), ["Disable all 3", "Cancel"]);
    assert_eq!(view.selected, Some(0));
    // Nothing has changed yet.
    assert!(all_enabled(&launcher));
    assert!(b_runs(&dirs));

    assert_eq!(press(&launcher, "Cancel"), Status::Idle);
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    // Back on the row the user pressed.
    let view = launcher.view();
    assert_eq!(view.rows[view.selected.unwrap()].title, "Package a");
    assert!(all_enabled(&launcher));
    assert!(b_runs(&dirs));

    // Escape cancels too.
    press(&launcher, "Package a");
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert!(all_enabled(&launcher));
    assert!(all_enabled(&dirs.launcher()), "nothing was recorded");
}

#[test]
fn disable_all_disables_the_shown_set_stops_it_and_keeps_its_settings() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    toggle(&launcher, "Package a");

    assert_eq!(
        press(&launcher, "Disable all 3"),
        Status::Result(
            "Disabled Package a and the 2 extensions that require it: Package c and Settings b"
                .into()
        )
    );
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    let expected = [
        ("Package a".to_string(), false),
        ("Package c".to_string(), false),
        ("Settings b".to_string(), false),
        ("Package d".to_string(), true),
    ];
    assert_eq!(enabled(&launcher), expected);
    // Settings b's instance stopped and its command left root search.
    assert!(!b_runs(&dirs));
    to_root(&launcher);
    assert!(!titles(&launcher).contains(&"Greeting".to_string()));
    // It is on record.
    assert_eq!(enabled(&dirs.launcher()), expected);

    // Enabling the dependency enables it alone, also after a restart.
    let restarted = dirs.launcher();
    assert_eq!(
        toggle(&restarted, "Package a"),
        Status::Result("Enabled Package a".into())
    );
    let alone = [
        ("Package a".to_string(), true),
        ("Package c".to_string(), false),
        ("Settings b".to_string(), false),
        ("Package d".to_string(), true),
    ];
    assert_eq!(enabled(&restarted), alone);
    // Read by a launcher of its own: one on the same runtime would take
    // the toasts `restarted`'s calls show.
    assert_eq!(enabled(&dirs.restarted()), alone);

    // Settings b kept its settings.
    assert_eq!(
        toggle(&restarted, "Settings b"),
        Status::Result("Enabled Settings b".into())
    );
    assert_eq!(
        greet(&restarted, "Greet me"),
        Status::Result("Good day to you".into())
    );
}

#[test]
fn a_dependent_enabled_while_the_question_is_shown_is_asked_about_again() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    toggle(&launcher, "Package a");
    // Package c is disabled already, so it is not disabled again.
    let details = launcher.view().details().join("\n");
    assert!(
        details.contains("Already disabled: Package c, which requires Package a"),
        "{details}"
    );
    assert_eq!(titles(&launcher), ["Disable all 2", "Cancel"]);

    // Meanwhile Package c is enabled again.
    block_on(launcher.set_enabled(&dirs.identity("c"), true));
    assert_eq!(
        press(&launcher, "Disable all 2"),
        Status::Error(
            "What disabling Package a affects changed since it was shown; check it again and \
             choose Disable all once more"
                .into()
        )
    );
    assert!(all_enabled(&launcher));
    assert_eq!(titles(&launcher), ["Disable all 3", "Cancel"]);

    press(&launcher, "Disable all 3");
    assert_eq!(
        enabled(&launcher),
        [
            ("Package a".into(), false),
            ("Package c".into(), false),
            ("Settings b".into(), false),
            ("Package d".into(), true),
        ]
    );
}

#[test]
fn a_dependent_disabled_while_the_question_is_shown_leaves_the_rest_to_disable() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    toggle(&launcher, "Package a");
    block_on(launcher.set_enabled(&dirs.identity("c"), false));

    assert_eq!(
        press(&launcher, "Disable all 3"),
        Status::Result("Disabled Package a and Settings b, which requires it".into())
    );
    assert!(!all_enabled(&launcher));
    assert!(enabled(&launcher).contains(&("Package d".into(), true)));
}

#[test]
fn a_package_disabled_while_the_question_is_shown_disables_nothing_even_with_a_new_dependent() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    toggle(&launcher, "Package a");
    assert_eq!(titles(&launcher), ["Disable all 2", "Cancel"]);

    // Meanwhile Package a is disabled, and Package c, which requires it and
    // was not shown to be disabled, is enabled again.
    block_on(launcher.set_enabled(&dirs.identity("a"), false));
    block_on(launcher.set_enabled(&dirs.identity("c"), true));
    assert_eq!(
        press(&launcher, "Disable all 2"),
        Status::Error("Package a is disabled already".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(
        enabled(&launcher),
        [
            ("Package a".into(), false),
            ("Package c".into(), true),
            ("Settings b".into(), true),
            ("Package d".into(), true),
        ]
    );
}

#[test]
fn packages_requiring_each_other_are_disabled_together() {
    let dirs = Dirs::new();
    dirs.fixture("x", &needs("y"));
    let y = dirs.fixture("y", &needs("x"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&y));
    toggle(&launcher, "Package x");
    assert_eq!(titles(&launcher), ["Disable all 2", "Cancel"]);
    assert_eq!(
        press(&launcher, "Disable all 2"),
        Status::Result("Disabled Package x and Package y, which requires it".into())
    );
    assert!(launcher.packages().iter().all(|package| !package.enabled));
}

#[test]
fn a_package_only_optionally_used_is_disabled_at_once() {
    let dirs = Dirs::new();
    dirs.fixture("a", "");
    let d = dirs.fixture("d", &uses("a"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&dirs.folder("a")));
    block_on(launcher.install_package(&d));

    assert_eq!(
        toggle(&launcher, "Package a"),
        Status::Result("Disabled Package a".into())
    );
    assert_eq!(
        enabled(&launcher),
        [("Package a".into(), false), ("Package d".into(), true)]
    );
}

/// Damages the managed copy of the Rust operations sample, so that it
/// cannot start.
fn damage(launcher: &Launcher, identity: &PackageIdentity) {
    let location = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == *identity)
        .unwrap()
        .location;
    fs::write(location.join("sample_operations.wasm"), b"not a component").unwrap();
}

#[test]
fn pane_pausing_a_required_dependency_disables_nothing_else_and_its_dependent_waits() {
    let dirs = Dirs::new();
    dirs.operations_sample();
    let caller = dirs.settings(
        "caller",
        "Settings caller",
        r#"{ "id": "greeter", "source": "local:../sample-operations",
             "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&caller));
    damage(&launcher, &dirs.identity("sample-operations"));
    drop(launcher);

    let restarted = dirs.restarted();
    to_root(&restarted);
    let status = press(&restarted, "Call from Rust");
    assert!(
        matches!(&status, Status::Error(e) if e.contains("could not start and is paused")),
        "{status:?}"
    );
    // Paused, not disabled; its dependent stays enabled, and nothing asked.
    assert!(all_enabled(&restarted));
    manage(&restarted);
    assert!(matches!(restarted.view().screen, Screen::Extensions { .. }));

    // Its dependent now waits for the paused dependency, rather than
    // having its calls refused (#152): its command stays listed, saying
    // what it needs, and comes back once the dependency is retried or
    // reloaded.
    to_root(&restarted);
    let greeting = restarted
        .view()
        .rows
        .iter()
        .find(|row| row.title == "Greeting")
        .unwrap()
        .clone();
    assert_eq!(
        greeting.unavailable,
        Some(pane_core::Unavailable::Waiting(
            "Needs Rust operations sample, which is paused".into()
        ))
    );

    // The user disabling the paused package still asks about its dependent.
    press(&restarted, "Rust operations sample");
    assert_eq!(
        restarted.view().title,
        "Disable Rust operations sample and the extensions that require it?"
    );
    assert_eq!(titles(&restarted), ["Disable all 2", "Cancel"]);
}

#[test]
fn a_disable_all_that_cannot_be_recorded_changes_none_of_them() {
    let dirs = Dirs::new();
    let launcher = four(&dirs);
    // `installed.json` cannot be replaced by a file while a folder is there.
    let registry = dirs.packages_dir().join("installed.json");
    let text = fs::read(&registry).unwrap();
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    fs::write(registry.join("blocker"), "").unwrap();

    toggle(&launcher, "Package a");
    let status = press(&launcher, "Disable all 3");
    assert!(
        matches!(&status, Status::Error(e)
            if e.starts_with("Could not update Pane's installed extensions")),
        "{status:?}"
    );
    assert!(all_enabled(&launcher));
    to_root(&launcher);
    assert!(titles(&launcher).contains(&"Greeting".to_string()));

    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, text).unwrap();
    assert!(all_enabled(&dirs.launcher()));
}
