//! Pausing an installed package that keeps failing, through the launcher's
//! public interface: three crashes within five minutes, or a start that fails, pause
//! it; Pane then runs none of its code, lists its commands with why, keeps
//! its saved data and the pause across a restart, and offers Retry. An
//! error the extension answers with is an ordinary outcome and never pauses
//! it. Every check runs against the settings sample's "Crash" in Rust,
//! JavaScript and TypeScript, real guests from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    CallError, FormError, Launcher, PackageIdentity, Row, Runtime, Screen, Status, Unavailable,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles, to_root};

const COMMAND: &str = "Greeting";

/// A settings sample package: the same command in each language.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-settings",
    component: "sample_settings.wasm",
    title: "Settings sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-settings-js",
    component: "sample_settings_js.wasm",
    title: "JavaScript settings sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-settings-ts",
    component: "sample_settings_ts.wasm",
    title: "TypeScript settings sample",
};

fn assembled(fixture: &Fixture) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(fixture.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    assembled
}

/// Copies the assembled settings sample package of `fixture` into `folder`.
fn settings_package(fixture: &Fixture, folder: &Path) -> PathBuf {
    let assembled = assembled(fixture);
    fs::create_dir_all(folder).unwrap();
    fs::copy(assembled.join("pane.json"), folder.join("pane.json")).unwrap();
    fs::copy(
        assembled.join(fixture.component),
        folder.join(fixture.component),
    )
    .unwrap();
    folder.to_path_buf()
}

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
        Launcher::with_packages(
            Ok(self.runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
    }

    /// A second launcher on the same records, with a runtime of its own:
    /// one on `runtime` would take its window and feedback host functions,
    /// so later calls' toasts would not reach the launcher a test drives.
    fn reader(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
    }

    /// A launcher with the settings sample of `fixture` installed, and its
    /// identity and source folder.
    fn installed(&self, fixture: &Fixture) -> (Launcher, PackageIdentity, PathBuf) {
        let launcher = self.launcher();
        let folder = settings_package(fixture, &self.sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        (launcher, identity, folder)
    }

    /// Whether any component of the installed package `identity` runs.
    fn is_running(&self, launcher: &Launcher, identity: &PackageIdentity) -> bool {
        let location = launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == *identity)
            .unwrap()
            .location;
        block_on(self.runtime.running())
            .iter()
            .any(|component| component.starts_with(&location))
    }
}

fn row(launcher: &Launcher, title: &str) -> Row {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// From root search, opens the command and runs its item titled `item`,
/// returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, item: &str) -> Status {
    to_root(launcher);
    select_title(launcher, COMMAND);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view().status
    );
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Activates the extension manager's row titled `title`, returning the
/// outcome.
fn press(launcher: &Launcher, title: &str) -> Status {
    manage(launcher);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn error(status: Status) -> String {
    match status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// Crashes the command `times` times, each reported as a crash without
/// pausing it.
fn crash(launcher: &Launcher, times: usize) {
    for _ in 0..times {
        let message = error(run(launcher, "Crash"));
        assert!(message.starts_with("The extension crashed: "), "{message}");
    }
}

/// Whether root search lists the command as paused rather than runnable.
fn is_paused(launcher: &Launcher) -> bool {
    to_root(launcher);
    row(launcher, COMMAND).unavailable.is_some()
}

fn three_crashes_pause_the_package_until_retry(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, identity, _) = dirs.installed(fixture);
    let title = fixture.title;
    assert_eq!(
        run(&launcher, "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );

    crash(&launcher, 2);
    assert!(!is_paused(&launcher));
    let toast = error(run(&launcher, "Crash"));
    assert!(
        toast.starts_with(&format!(
            "{title} crashed 3 times within 5 minutes and is paused"
        )),
        "{toast}"
    );
    assert!(
        toast.ends_with("Retry it, or see why, in Manage extensions."),
        "{toast}"
    );
    // Its open command closed, and none of its code runs.
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(!dirs.is_running(&launcher, &identity));

    // Its command stays listed, saying why it does not run.
    let command = row(&launcher, COMMAND);
    let reason = command.unavailable.expect("the command says it is paused");
    let Unavailable::Paused(reason) = reason else {
        panic!("expected a pause, got {reason:?}");
    };
    assert_eq!(
        reason,
        format!("{title} is paused after an error; retry it in Manage extensions")
    );
    select_title(&launcher, COMMAND);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().status, Status::Error(reason.clone()));
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(!dirs.is_running(&launcher, &identity));

    // The extension list says so, with Retry and the details.
    manage(&launcher);
    let subtitle = row(&launcher, title).subtitle.unwrap();
    assert!(
        subtitle.starts_with("Enabled · Paused after crashing"),
        "{subtitle}"
    );
    assert_eq!(
        row(&launcher, &format!("Retry {title}"))
            .subtitle
            .as_deref(),
        Some("Paused: it crashed 3 times within 5 minutes; start it again")
    );
    press(&launcher, &format!("Why {title} is paused"));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::PauseDetails { .. }));
    assert_eq!(view.title, format!("Why {title} is paused"));
    let details = view.details();
    assert_eq!(
        details[0],
        format!("{title} crashed 3 times within 5 minutes.")
    );
    assert!(
        details.contains(&"Version: 0.1.0".to_string()),
        "{details:?}"
    );
    assert!(
        details.iter().any(|line| line.starts_with(
            "Crashed 3 times within 5 minutes; the last time: The extension crashed: "
        )),
        "{details:?}"
    );
    assert_eq!(titles(&launcher), [format!("Retry {title}")]);
    // Escape returns to the extension list, on that row.
    launcher.back();
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.rows[view.selected.unwrap()].title,
        format!("Why {title} is paused")
    );

    // The pause holds after a restart.
    block_on(launcher.records_written());
    let restarted = dirs.launcher();
    assert!(is_paused(&restarted));
    manage(&restarted);
    assert!(titles(&restarted).contains(&format!("Retry {title}")));

    // Retry, from the details, starts it again, with its saved data kept.
    press(&restarted, &format!("Why {title} is paused"));
    block_on(restarted.activate_selected());
    assert_eq!(
        restarted.view().status,
        Status::Result(format!("Started {title}"))
    );
    assert!(matches!(restarted.view().screen, Screen::Extensions { .. }));
    assert!(!is_paused(&restarted));
    assert_eq!(
        run(&restarted, "Greet me"),
        Status::Result("Good day to you".into())
    );
    // And it is no longer paused after another restart.
    block_on(restarted.records_written());
    assert!(!is_paused(&dirs.reader()));
}

#[test]
fn three_crashes_pause_a_rust_package_until_retry() {
    three_crashes_pause_the_package_until_retry(&RUST);
}

#[test]
fn three_crashes_pause_a_javascript_package_until_retry() {
    three_crashes_pause_the_package_until_retry(&JAVASCRIPT);
}

#[test]
fn three_crashes_pause_a_typescript_package_until_retry() {
    three_crashes_pause_the_package_until_retry(&TYPESCRIPT);
}

fn errors_the_extension_answers_with_never_pause_it(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, _, _) = dirs.installed(fixture);
    for _ in 0..5 {
        assert_eq!(
            error(run(&launcher, "Greet me")),
            "The extension reported an error: No greeting style is saved yet; choose one first"
        );
    }
    assert!(!is_paused(&launcher));
}

#[test]
fn errors_a_rust_package_answers_with_never_pause_it() {
    errors_the_extension_answers_with_never_pause_it(&RUST);
}

#[test]
fn errors_a_javascript_package_answers_with_never_pause_it() {
    errors_the_extension_answers_with_never_pause_it(&JAVASCRIPT);
}

#[test]
fn errors_a_typescript_package_answers_with_never_pause_it() {
    errors_the_extension_answers_with_never_pause_it(&TYPESCRIPT);
}

#[test]
fn crashes_are_counted_afresh_after_a_restart() {
    let dirs = Dirs::new();
    let (launcher, _, _) = dirs.installed(&RUST);
    crash(&launcher, 2);
    drop(launcher);
    let restarted = dirs.launcher();
    crash(&restarted, 2);
    assert!(!is_paused(&restarted));
}

#[test]
fn other_packages_keep_running_while_one_is_paused() {
    let dirs = Dirs::new();
    let (launcher, _, _) = dirs.installed(&RUST);
    let other = settings_package(&JAVASCRIPT, &dirs.sources.path().join("other"));
    let manifest = fs::read_to_string(other.join("pane.json")).unwrap();
    let manifest = manifest.replace("\"title\": \"Greeting\"", "\"title\": \"Other greeting\"");
    fs::write(other.join("pane.json"), manifest).unwrap();
    block_on(launcher.install_package(&other));
    crash(&launcher, 2);
    error(run(&launcher, "Crash"));

    assert!(is_paused(&launcher));
    // The other package's command is not paused, and runs.
    assert_eq!(row(&launcher, "Other greeting").unavailable, None);
    select_title(&launcher, "Other greeting");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
    select_title(&launcher, "Use a casual greeting");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Saved the casual greeting".into())
    );
}

#[test]
fn disabling_a_paused_package_ends_the_pause() {
    let dirs = Dirs::new();
    let (launcher, identity, _) = dirs.installed(&RUST);
    crash(&launcher, 2);
    error(run(&launcher, "Crash"));
    assert!(is_paused(&launcher));

    block_on(launcher.set_enabled(&identity, false));
    block_on(launcher.set_enabled(&identity, true));
    assert!(!is_paused(&launcher));
    block_on(launcher.records_written());
    assert!(!is_paused(&dirs.reader()));
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
}

#[test]
fn reloading_a_paused_package_ends_the_pause() {
    let dirs = Dirs::new();
    let (launcher, _, _) = dirs.installed(&RUST);
    crash(&launcher, 2);
    error(run(&launcher, "Crash"));

    assert_eq!(
        press(&launcher, "Reload Settings sample"),
        Status::Result("Reloaded Settings sample".into())
    );
    assert!(!is_paused(&launcher));
    block_on(launcher.records_written());
    assert!(!is_paused(&dirs.reader()));
}

#[test]
fn a_reload_that_fails_to_start_pauses_the_package_across_a_restart() {
    let dirs = Dirs::new();
    let (launcher, _, folder) = dirs.installed(&RUST);
    // The failing-start fixture traps the first time it is asked for its
    // view, then starts.
    let failing =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/failing_start.wasm");
    fs::copy(failing, folder.join(RUST.component)).unwrap();
    let message = error(press(&launcher, "Reload Settings sample"));
    assert!(
        message.starts_with("Reloaded Settings sample, but it failed to start"),
        "{message}"
    );
    assert!(is_paused(&launcher));

    let restarted = dirs.launcher();
    assert!(is_paused(&restarted));
    assert_eq!(
        press(&restarted, "Retry starting Settings sample"),
        Status::Result("Started Settings sample".into())
    );
    assert!(!is_paused(&restarted));
}

/// Types `query` into root search and waits for the commands asked.
fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

#[test]
fn a_root_result_provider_that_keeps_crashing_is_paused_and_asked_no_more() {
    let dirs = Dirs::new();
    let folder = dirs.sources.path().join("faulty");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/faulty.wasm"),
        folder.join("faulty.wasm"),
    )
    .unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Faulty",
  "apiVersion": "0.1",
  "commands": [{ "id": "faulty", "title": "Faulty", "component": "faulty.wasm", "rootResults": true }]
}"#,
    )
    .unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    to_root(&launcher);

    // The provider traps on "trap"; root search asks it on each change of
    // the query, in the background, and lists why it could not answer.
    for _ in 0..2 {
        search(&launcher, "trap");
        let failed = row(&launcher, "Faulty");
        assert!(
            failed
                .subtitle
                .as_deref()
                .is_some_and(|s| s.starts_with("Could not answer: The extension crashed")),
            "{failed:?}"
        );
        search(&launcher, "");
    }
    search(&launcher, "trap");
    let toast = error(launcher.view().status);
    assert!(
        toast.starts_with("Faulty crashed 3 times within 5 minutes and is paused"),
        "{toast}"
    );
    // It is asked no more: its command is listed as paused, with no row
    // saying it could not answer.
    search(&launcher, "");
    search(&launcher, "trap");
    assert_eq!(launcher.view().rows, []);
    search(&launcher, "faulty");
    let rows = launcher.view().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].unavailable.is_some(), "{rows:?}");
}

/// Pauses the Rust settings sample of a new launcher with three crashes,
/// with the pause on record.
fn paused(dirs: &Dirs) -> (Launcher, PackageIdentity, PathBuf) {
    let (launcher, identity, folder) = dirs.installed(&RUST);
    crash(&launcher, 2);
    error(run(&launcher, "Crash"));
    assert!(is_paused(&launcher));
    block_on(launcher.records_written());
    (launcher, identity, folder)
}

#[test]
fn enabling_a_package_ends_a_pause_on_record() {
    let dirs = Dirs::new();
    let (launcher, identity, _) = paused(&dirs);
    drop(launcher);
    // As if a pause had been recorded as the user disabled the package.
    let registry = dirs.data.path().join("extensions/installed.json");
    let text = fs::read_to_string(&registry).unwrap();
    assert!(text.contains("\"paused\""), "{text}");
    let disabled = text.replacen("\"dir\"", "\"disabled\": true,\n      \"dir\"", 1);
    fs::write(&registry, disabled).unwrap();

    let restarted = dirs.launcher();
    block_on(restarted.set_enabled(&identity, true));
    assert!(!is_paused(&restarted));
    block_on(restarted.records_written());
    assert!(
        !fs::read_to_string(&registry)
            .unwrap()
            .contains("\"paused\"")
    );
    assert!(!is_paused(&dirs.reader()));
}

#[test]
fn a_pause_recorded_for_another_version_does_not_hold() {
    let dirs = Dirs::new();
    let (launcher, _, _) = paused(&dirs);
    drop(launcher);
    let registry = dirs.data.path().join("extensions/installed.json");
    let text = fs::read_to_string(&registry).unwrap();
    assert!(text.contains("\"version\": \"0.1.0\""), "{text}");
    fs::write(
        &registry,
        text.replace("\"version\": \"0.1.0\"", "\"version\": \"0.0.9\""),
    )
    .unwrap();
    assert!(!is_paused(&dirs.reader()));
}

#[test]
fn retrying_without_a_runtime_keeps_the_pause() {
    let dirs = Dirs::new();
    let (launcher, _, _) = paused(&dirs);
    drop(launcher);
    let unavailable = Launcher::with_packages(
        Err(CallError::RuntimeUnavailable("no engine".into())),
        vec![],
        dirs.data.path().join("extensions"),
    );
    // Pane's runtime failing is not the package's failure: it stays paused,
    // as it was, on record too.
    assert_eq!(
        press(&unavailable, "Retry Settings sample"),
        Status::Error(
            "Settings sample was not started: Extension runtime unavailable: no engine; it \
             stays paused."
                .into()
        )
    );
    assert!(is_paused(&unavailable));
    manage(&unavailable);
    assert!(titles(&unavailable).contains(&"Retry Settings sample".to_string()));
    press(&unavailable, "Why Settings sample is paused");
    assert_eq!(
        unavailable.view().details()[0],
        "Settings sample crashed 3 times within 5 minutes."
    );
    block_on(unavailable.records_written());
    assert!(is_paused(&dirs.reader()));
}

#[test]
fn a_component_that_cannot_load_is_paused_at_once() {
    let dirs = Dirs::new();
    let (launcher, identity, _) = dirs.installed(&RUST);
    let location = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity)
        .unwrap()
        .location;
    drop(launcher);
    // Its managed copy was damaged behind Pane's back; a new runtime loads
    // it afresh.
    fs::write(location.join(RUST.component), b"not a component").unwrap();
    let restarted = Launcher::with_packages(
        Runtime::start(),
        vec![],
        dirs.data.path().join("extensions"),
    );
    to_root(&restarted);
    select_title(&restarted, COMMAND);
    block_on(restarted.activate_selected());
    let toast = error(restarted.view().status);
    assert!(
        toast.starts_with("Settings sample could not start and is paused"),
        "{toast}"
    );
    assert!(is_paused(&restarted));
    manage(&restarted);
    assert!(
        titles(&restarted).contains(&"Retry starting Settings sample".to_string()),
        "{:?}",
        titles(&restarted)
    );
}

#[test]
fn an_uninstall_that_cannot_be_recorded_keeps_the_pause() {
    let dirs = Dirs::new();
    let (launcher, _, _) = paused(&dirs);
    // `installed.json` cannot be replaced by a file while a folder is there.
    let registry = dirs.data.path().join("extensions/installed.json");
    let text = fs::read(&registry).unwrap();
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    fs::write(registry.join("blocker"), "").unwrap();

    manage(&launcher);
    select_title(&launcher, "Uninstall Settings sample");
    block_on(launcher.activate_selected());
    select_title(&launcher, "Uninstall and keep saved data");
    block_on(launcher.activate_selected());
    let message = error(launcher.view().status);
    assert!(message.starts_with("Could not uninstall"), "{message}");
    // Still installed and still paused, as `installed.json` records it.
    assert!(is_paused(&launcher));
    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, text).unwrap();
    assert!(is_paused(&dirs.reader()));
}

fn an_error_thrown_from_a_form_is_an_error_not_a_crash(fixture: &Fixture) {
    let runtime = Runtime::start().unwrap();
    let component = assembled(fixture).join(fixture.component);
    // The sample has no form: submitting one is refused as a whole, which
    // JavaScript and TypeScript do by throwing an `Error`.
    assert_eq!(
        block_on(runtime.submit_form(&component, "nothing", vec![])),
        Err(CallError::Form(FormError {
            field: None,
            message: "unknown form: nothing".into()
        }))
    );
}

#[test]
fn an_error_thrown_from_a_rust_form_is_an_error_not_a_crash() {
    an_error_thrown_from_a_form_is_an_error_not_a_crash(&RUST);
}

#[test]
fn an_error_thrown_from_a_javascript_form_is_an_error_not_a_crash() {
    an_error_thrown_from_a_form_is_an_error_not_a_crash(&JAVASCRIPT);
}

#[test]
fn an_error_thrown_from_a_typescript_form_is_an_error_not_a_crash() {
    an_error_thrown_from_a_form_is_an_error_not_a_crash(&TYPESCRIPT);
}
