//! Reloading one installed package through the launcher's public interface:
//! its code is replaced from its source folder while Pane and other packages
//! keep running, its settings are kept, a replacement that does not pass
//! the install checks leaves the working code in place, and one that fails
//! to start is reported with Retry. Real guests from `cargo xtask guests`,
//! in Rust, JavaScript and TypeScript.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{Key, Launcher, PackageIdentity, Runtime, Screen, Status, ViewEvent};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage extensions…";

/// Writes a package folder titled `title` with one command, "Open <title>",
/// whose component `command.wasm` is a copy of the guest `name`.
fn package(folder: &Path, title: &str, name: &str) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        format!(
            r#"{{
  "manifestVersion": 1,
  "title": "{title}",
  "apiVersion": "0.1",
  "commands": [{{ "id": "open", "title": "Open {title}", "component": "command.wasm" }}]
}}"#
        ),
    )
    .unwrap();
    rebuild(folder, name);
    folder.to_path_buf()
}

/// Replaces the source folder's component with a copy of the guest `name`,
/// as a new build of the package would.
fn rebuild(folder: &Path, name: &str) {
    fs::copy(guest(name), folder.join("command.wasm")).unwrap();
}

struct Dirs {
    sources: TempDir,
    data: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        }
    }

    fn source(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn launcher_on(&self, runtime: Runtime) -> Launcher {
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
    }

    fn launcher(&self) -> Launcher {
        self.launcher_on(Runtime::start().unwrap())
    }

    /// A launcher with the package `name` installed from a folder holding
    /// the guest `guest`, and that package's identity.
    fn installed(&self, name: &str, guest: &str) -> (Launcher, PathBuf, PackageIdentity) {
        let launcher = self.launcher();
        let folder = package(&self.source(name), name, guest);
        block_on(launcher.install_package(&folder));
        let identity = PackageIdentity::local(&folder).unwrap();
        (launcher, folder, identity)
    }
}

/// From root search, opens the command titled `command`, returning the
/// title of the view it shows.
fn open(launcher: &Launcher, command: &str) -> String {
    launcher.back();
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command, "{command}: {:?}", view.status);
    view.title
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    open(launcher, command);
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    launcher.back();
    launcher.back();
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// Activates the extension manager's row titled `row`, returning the
/// outcome.
fn press(launcher: &Launcher, row: &str) -> Status {
    manage(launcher);
    select_title(launcher, row);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// (guest, the answer of its "Say hello") of each sample language.
const RUST: (&str, &str) = ("sample_rust", "Hello from the Rust guest");
const JAVASCRIPT: (&str, &str) = ("sample_js", "Hello from the JavaScript guest");
const TYPESCRIPT: (&str, &str) = ("sample_ts", "Hello from the TypeScript guest");

fn reloading_replaces_the_code_that_runs(from: (&str, &str), to: (&str, &str)) {
    let dirs = Dirs::new();
    let (launcher, folder, _) = dirs.installed("Dev", from.0);
    assert_eq!(
        run(&launcher, "Open Dev", "Say hello"),
        Status::Result(from.1.into())
    );

    rebuild(&folder, to.0);
    assert_eq!(
        press(&launcher, "Reload Dev"),
        Status::Result("Reloaded Dev".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    assert_eq!(
        run(&launcher, "Open Dev", "Say hello"),
        Status::Result(to.1.into())
    );
}

#[test]
fn reloading_replaces_rust_code_with_javascript() {
    reloading_replaces_the_code_that_runs(RUST, JAVASCRIPT);
}

#[test]
fn reloading_replaces_javascript_code_with_typescript() {
    reloading_replaces_the_code_that_runs(JAVASCRIPT, TYPESCRIPT);
}

#[test]
fn reloading_replaces_typescript_code_with_rust() {
    reloading_replaces_the_code_that_runs(TYPESCRIPT, RUST);
}

fn settings_are_kept_through_a_reload(settings_guest: &str) {
    let dirs = Dirs::new();
    let (launcher, folder, _) = dirs.installed("Dev", settings_guest);
    assert_eq!(
        run(&launcher, "Open Dev", "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );

    rebuild(&folder, settings_guest);
    assert_eq!(
        press(&launcher, "Reload Dev"),
        Status::Result("Reloaded Dev".into())
    );

    assert_eq!(
        run(&launcher, "Open Dev", "Greet me"),
        Status::Result("Good day to you".into())
    );
    assert_eq!(launcher.view().title, "Greeting: formal");
}

#[test]
fn a_rust_package_keeps_its_settings_through_a_reload() {
    settings_are_kept_through_a_reload("sample_settings");
}

#[test]
fn a_javascript_package_keeps_its_settings_through_a_reload() {
    settings_are_kept_through_a_reload("sample_settings_js");
}

#[test]
fn a_typescript_package_keeps_its_settings_through_a_reload() {
    settings_are_kept_through_a_reload("sample_settings_ts");
}

#[test]
fn a_replacement_that_fails_its_checks_leaves_the_working_code_running() {
    let dirs = Dirs::new();
    let (launcher, folder, _) = dirs.installed("Dev", "sample_rust");
    // An older API shape, then a source-only package, then no package.
    type Break = fn(&Path);
    let broken: [(&str, Break); 3] = [
        ("an older extension API shape", |folder| {
            rebuild(folder, "old_api")
        }),
        ("source-only package", |folder| {
            fs::remove_file(folder.join("command.wasm")).unwrap()
        }),
        ("no pane.json", |folder| {
            fs::remove_file(folder.join("pane.json")).unwrap()
        }),
    ];
    for (reason, break_it) in broken {
        break_it(&folder);

        let message = error(press(&launcher, "Reload Dev"));
        assert!(message.starts_with("Dev was not reloaded: "), "{message}");
        assert!(message.contains(reason), "{message}");
        assert!(
            message.ends_with("It keeps running its installed code."),
            "{message}"
        );
        assert!(!titles(&launcher).iter().any(|t| t.starts_with("Retry")));
        assert_eq!(
            run(&launcher, "Open Dev", "Say hello"),
            Status::Result("Hello from the Rust guest".into())
        );
    }
}

#[test]
fn a_replacement_that_fails_to_start_is_reported_and_retried() {
    let dirs = Dirs::new();
    let (launcher, folder, _) = dirs.installed("Dev", "sample_rust");
    rebuild(&folder, "failing_start");

    let message = error(press(&launcher, "Reload Dev"));
    assert!(
        message
            .starts_with("Reloaded Dev, but it failed to start; its earlier code is not restored."),
        "{message}"
    );
    assert!(message.contains("\"Why Dev is paused\""), "{message}");
    // The failure is listed with Retry and its diagnostics.
    let view = launcher.view();
    assert!(titles(&launcher).contains(&"Retry starting Dev".to_string()));
    assert!(
        view.rows[0]
            .subtitle
            .as_deref()
            .is_some_and(|s| s.starts_with("Enabled · Failed to start"))
    );
    press(&launcher, "Why Dev is paused");
    let details = launcher.view().details().to_vec();
    assert_eq!(details[0], "Dev could not start.");
    assert!(
        details
            .iter()
            .any(|line| line.starts_with("The extension crashed: ")),
        "{details:?}"
    );
    manage(&launcher);
    let view = launcher.view();
    assert!(
        view.rows[0]
            .subtitle
            .as_deref()
            .is_some_and(|s| s.starts_with("Enabled · Failed to start")),
        "{:?}",
        view.rows[0]
    );

    // The older code is not restored, and what the failed start saved is
    // kept: Retry starts the same code, which now starts.
    assert_eq!(
        press(&launcher, "Retry starting Dev"),
        Status::Result("Started Dev".into())
    );
    assert!(!titles(&launcher).iter().any(|t| t.starts_with("Retry")));
    // The Retry row is gone; the selection moves to the Reload row before it.
    let view = launcher.view();
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Reload Dev")
    );
    assert_eq!(open(&launcher, "Open Dev"), "Started");

    // Fixing it and reloading works as ever.
    rebuild(&folder, "sample_rust");
    assert_eq!(
        press(&launcher, "Reload Dev"),
        Status::Result("Reloaded Dev".into())
    );
    assert_eq!(open(&launcher, "Open Dev"), "Rust sample");
}

#[test]
fn disabling_a_package_that_failed_to_start_forgets_the_failure() {
    let dirs = Dirs::new();
    let (launcher, folder, identity) = dirs.installed("Dev", "sample_rust");
    rebuild(&folder, "failing_start");
    error(press(&launcher, "Reload Dev"));
    assert!(titles(&launcher).contains(&"Retry starting Dev".to_string()));

    // Disabling stops it; enabled again, it starts fresh when opened, so
    // the earlier failure no longer describes it.
    block_on(launcher.set_enabled(&identity, false));
    block_on(launcher.set_enabled(&identity, true));
    manage(&launcher);
    assert!(!titles(&launcher).iter().any(|t| t.starts_with("Retry")));
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some(format!("Enabled · {identity}").as_str())
    );
}

/// Shows the package in `folder` and starts updating the installed copy
/// from it, returning the update to await.
fn begin_update(launcher: &Launcher, folder: &Path) -> impl Future<Output = ()> {
    block_on(launcher.preview_package(folder));
    select_title(launcher, "Update");
    launcher.activate_selected()
}

#[test]
fn a_package_being_updated_is_not_reloaded_meanwhile() {
    let dirs = Dirs::new();
    let (launcher, folder, identity) = dirs.installed("Dev", "sample_rust");
    rebuild(&folder, "sample_js");

    let update = begin_update(&launcher, &folder);
    block_on(launcher.reload(&identity));
    assert_eq!(
        launcher.view().status,
        Status::Error("Dev is updating".into())
    );
    block_on(update);
    assert_eq!(launcher.view().status, Status::Result("Updated Dev".into()));

    // Once updated, it reloads as ever.
    rebuild(&folder, "sample_ts");
    assert_eq!(
        press(&launcher, "Reload Dev"),
        Status::Result("Reloaded Dev".into())
    );
    assert_eq!(open(&launcher, "Open Dev"), "TypeScript sample");
}

#[test]
fn a_package_being_reloaded_is_not_updated_or_toggled_meanwhile() {
    let dirs = Dirs::new();
    let (launcher, folder, identity) = dirs.installed("Dev", "sample_rust");
    rebuild(&folder, "sample_js");
    block_on(launcher.preview_package(&folder));
    select_title(&launcher, "Update");

    let reload = launcher.reload(&identity);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Error("Dev is reloading".into())
    );
    block_on(launcher.set_enabled(&identity, false));
    assert_eq!(
        launcher.view().status,
        Status::Error("Dev is reloading".into())
    );
    assert!(launcher.packages()[0].enabled);
    block_on(reload);

    assert_eq!(open(&launcher, "Open Dev"), "JavaScript sample");
}

#[test]
fn a_view_the_guest_refuses_is_not_a_failure_to_start() {
    let dirs = Dirs::new();
    let (launcher, folder, _) = dirs.installed("Dev", "sample_rust");
    // Its view answers with an ordinary error, as one that needs the user
    // to sign in first would: the code started.
    rebuild(&folder, "refusing_view");

    assert_eq!(
        press(&launcher, "Reload Dev"),
        Status::Result("Reloaded Dev".into())
    );
    assert!(!titles(&launcher).iter().any(|t| t.starts_with("Retry")));
    assert!(
        launcher.view().rows[0]
            .subtitle
            .as_deref()
            .is_some_and(|s| !s.contains("Failed to start")),
        "{:?}",
        launcher.view().rows[0]
    );
}

#[test]
fn a_disabled_package_is_not_reloaded() {
    let dirs = Dirs::new();
    let (launcher, _, identity) = dirs.installed("Dev", "sample_rust");
    block_on(launcher.set_enabled(&identity, false));
    manage(&launcher);
    assert_eq!(
        titles(&launcher),
        [
            "Dev",
            "Clear cache of Dev",
            "Uninstall Dev",
            "Update extensions automatically"
        ],
        "no Reload row"
    );

    block_on(launcher.reload(&identity));

    assert_eq!(
        launcher.view().status,
        Status::Error("Dev is disabled; enable it to reload it".into())
    );
}

/// Opens the color picker of the sample command titled `command` and
/// presses Right, which chooses the next color.
fn open_color(launcher: &Launcher, command: &str) {
    open(launcher, command);
    select_title(launcher, "Choose a color");
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::CustomView(_)));
    block_on(launcher.send_view_event(ViewEvent::Key(Key::Right)));
}

fn color(launcher: &Launcher) -> String {
    launcher
        .view()
        .custom_view()
        .expect("a view is open")
        .frame
        .value
        .clone()
}

#[test]
fn reloading_one_package_leaves_another_running_with_its_state() {
    let dirs = Dirs::new();
    let runtime = Runtime::start().unwrap();
    let launcher = dirs.launcher_on(runtime.clone());
    let reloaded = package(&dirs.source("dev"), "Dev", "sample_rust");
    let other = package(&dirs.source("other"), "Other", "sample_js");
    block_on(launcher.install_package(&reloaded));
    block_on(launcher.install_package(&other));
    open_color(&launcher, "Open Other");
    let chosen = color(&launcher);

    rebuild(&reloaded, "sample_ts");
    block_on(launcher.reload(&PackageIdentity::local(&reloaded).unwrap()));

    // Still open, with the color chosen before, and still answering.
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::CustomView(_)));
    assert_eq!(view.status, Status::Result("Reloaded Dev".into()));
    assert_eq!(color(&launcher), chosen);
    block_on(launcher.send_view_event(ViewEvent::Key(Key::Right)));
    assert_ne!(color(&launcher), chosen);
    assert_eq!(block_on(runtime.view_count()), 1);
    assert_eq!(
        run(&launcher, "Open Dev", "Say hello"),
        Status::Result("Hello from the TypeScript guest".into())
    );
}

#[test]
fn reloading_closes_an_open_view_of_the_package_and_starts_the_new_code() {
    let dirs = Dirs::new();
    let runtime = Runtime::start().unwrap();
    let launcher = dirs.launcher_on(runtime.clone());
    let folder = package(&dirs.source("dev"), "Dev", "sample_rust");
    block_on(launcher.install_package(&folder));
    open_color(&launcher, "Open Dev");
    assert_eq!(block_on(runtime.view_count()), 1);

    rebuild(&folder, "sample_js");
    block_on(launcher.reload(&PackageIdentity::local(&folder).unwrap()));

    // Its state is not carried over: the view closed with the old
    // instance, and root search selects the reloaded command.
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.status, Status::Result("Reloaded Dev".into()));
    assert_eq!(view.rows[view.selected.unwrap()].title, "Open Dev");
    assert_eq!(block_on(runtime.view_count()), 0);
    assert_eq!(open(&launcher, "Open Dev"), "JavaScript sample");
}

#[test]
fn a_command_that_opens_after_its_package_was_reloaded_is_not_shown() {
    let dirs = Dirs::new();
    let (launcher, folder, identity) = dirs.installed("Dev", "sample_rust");
    launcher.back();
    select_title(&launcher, "Open Dev");
    // Opening the copy from before the reload, answered after it.
    let opening = launcher.activate_selected();
    rebuild(&folder, "sample_js");
    block_on(launcher.reload(&identity));

    block_on(opening);

    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.status, Status::Result("Reloaded Dev".into()));
    assert_eq!(open(&launcher, "Open Dev"), "JavaScript sample");
}
