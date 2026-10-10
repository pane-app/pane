//! Disabling and re-enabling installed packages through the launcher's public
//! interface: a disabled package contributes nothing and runs nothing, stays
//! disabled across restarts, and keeps its settings for when it is enabled
//! again. Every check runs against the settings sample in Rust, JavaScript
//! and TypeScript, real guests from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{CallError, Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

const CREATE_ROW: &str = "Create Extension…";
const IMPORT_ROW: &str = "Import Extension…";
const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const MANAGE_ROW: &str = "Manage Extensions";
const SETTINGS_ROW: &str = "Settings…";

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

/// Copies the assembled settings sample package of `fixture` into `folder`,
/// a package with its own identity. `title` replaces the package title.
fn settings_package(fixture: &Fixture, folder: &Path, title: &str) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(fixture.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
    let original = format!("\"{}\"", fixture.title);
    assert!(manifest.contains(&original), "{manifest}");
    let manifest = manifest.replace(&original, &format!("\"{title}\""));
    fs::write(folder.join("pane.json"), manifest).unwrap();
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

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder; a new one is a restart of Pane.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Runtime::start(), vec![], self.packages_dir())
    }
}

fn subtitles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.subtitle.unwrap_or_default())
        .collect()
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "{command} opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    launcher.back();
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::Extensions { .. }),
        "{:?}",
        launcher.view().screen
    );
}

/// Enables or disables the package on row `index` of the extension manager.
fn toggle(launcher: &Launcher, index: usize) -> Status {
    manage(launcher);
    launcher.select(index);
    block_on(launcher.activate_selected());
    launcher.view().status
}

fn enabled(launcher: &Launcher) -> Vec<(PackageIdentity, bool)> {
    launcher
        .packages()
        .into_iter()
        .map(|package| (package.identity, package.enabled))
        .collect()
}

fn a_disabled_package_leaves_root_search_and_stays_disabled_after_a_restart(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    assert_eq!(
        titles(&launcher),
        [
            "Greeting",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );

    manage(&launcher);
    let identity = PackageIdentity::local(&folder).unwrap();
    // After the packages' rows, one per enabled package reloads it, then one
    // per package clears its cache, then one per package uninstalls it.
    assert_eq!(
        titles(&launcher),
        [
            "Settings sample",
            "Reload Settings sample",
            "Clear cache of Settings sample",
            "Uninstall Settings sample",
            "Hotkey for Greeting",
            "Alias for Greeting",
            "Develop Settings sample",
            "Update extensions automatically"
        ]
    );
    assert_eq!(launcher.view().rows[0].id, identity.key());
    assert!(subtitles(&launcher)[0].starts_with("Enabled"));

    assert_eq!(
        toggle(&launcher, 0),
        Status::Result("Disabled Settings sample".into())
    );
    assert!(subtitles(&launcher)[0].starts_with("Disabled"));
    launcher.back();
    assert_eq!(
        titles(&launcher),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, CREATE_ROW, IMPORT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );

    // A restart without a runtime: listing runs no guest and keeps the choice.
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let restarted = Launcher::with_packages(unavailable, vec![], dirs.packages_dir());
    assert_eq!(
        titles(&restarted),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, CREATE_ROW, IMPORT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );
    assert_eq!(enabled(&restarted), [(identity.clone(), false)]);
    manage(&restarted);
    assert!(subtitles(&restarted)[0].starts_with("Disabled"));
}

fn re_enabling_after_a_restart_restores_the_saved_settings(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    assert_eq!(
        run(&launcher, "Greeting", "Greet me"),
        Status::Error(
            "The extension reported an error: No greeting style is saved yet; choose one first"
                .into()
        )
    );
    assert_eq!(
        run(&launcher, "Greeting", "Use a formal greeting"),
        Status::Result("Saved the formal greeting".into())
    );
    toggle(&launcher, 0);
    drop(launcher);

    let restarted = dirs.launcher();
    assert_eq!(
        toggle(&restarted, 0),
        Status::Result("Enabled Settings sample".into())
    );
    restarted.back();
    assert_eq!(
        titles(&restarted),
        [
            "Greeting",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    assert_eq!(
        run(&restarted, "Greeting", "Greet me"),
        Status::Result("Good day to you".into())
    );
    assert_eq!(restarted.view().title, "Greeting: formal");
    assert_eq!(
        enabled(&restarted),
        [(PackageIdentity::local(&folder).unwrap(), true)]
    );
}

fn copies_with_the_same_title_are_enabled_and_keep_settings_by_identity(fixture: &Fixture) {
    let dirs = Dirs::new();
    let published = settings_package(fixture, &dirs.source("published"), "Greeter");
    let development = settings_package(fixture, &dirs.source("development"), "Greeter");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&published));
    block_on(launcher.install_package(&development));
    launcher.back();
    // Each copy saves its own choice.
    launcher.select(0);
    block_on(launcher.activate_selected());
    select_title(&launcher, "Use a formal greeting");
    block_on(launcher.activate_selected());
    launcher.back();
    launcher.select(1);
    block_on(launcher.activate_selected());
    select_title(&launcher, "Use a casual greeting");
    block_on(launcher.activate_selected());

    manage(&launcher);
    assert_eq!(
        titles(&launcher),
        [
            "Greeter",
            "Greeter",
            "Reload Greeter",
            "Reload Greeter",
            "Clear cache of Greeter",
            "Clear cache of Greeter",
            "Uninstall Greeter",
            "Uninstall Greeter",
            "Hotkey for Greeting",
            "Hotkey for Greeting",
            "Alias for Greeting",
            "Alias for Greeting",
            "Develop Greeter",
            "Develop Greeter",
            "Update extensions automatically"
        ]
    );
    let published_id = PackageIdentity::local(&published).unwrap();
    let development_id = PackageIdentity::local(&development).unwrap();
    let ids: Vec<String> = launcher.view().rows.into_iter().map(|r| r.id).collect();
    assert_eq!(ids[..2], [published_id.key(), development_id.key()]);
    // The row says which copy it is.
    assert!(subtitles(&launcher)[0].contains(&published_id.to_string()));

    toggle(&launcher, 1);
    assert_eq!(
        enabled(&launcher),
        [
            (published_id.clone(), true),
            (development_id.clone(), false)
        ]
    );
    // The enabled copy is the one left in root search, with its own setting.
    assert_eq!(
        run(&launcher, "Greeting", "Greet me"),
        Status::Result("Good day to you".into())
    );

    toggle(&launcher, 0);
    let restarted = dirs.launcher();
    assert_eq!(
        titles(&restarted),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, CREATE_ROW, IMPORT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );
    // Enabling one copy leaves the other disabled.
    toggle(&restarted, 1);
    assert_eq!(
        enabled(&restarted),
        [(published_id, false), (development_id, true)]
    );
    assert_eq!(
        run(&restarted, "Greeting", "Greet me"),
        Status::Result("Hi there".into())
    );
}

fn disabling_through_the_api_closes_the_package_command_and_updating_keeps_it_disabled(
    fixture: &Fixture,
) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);

    block_on(launcher.set_enabled(&identity, false));

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Disabled Settings sample".into())
    );
    assert_eq!(
        titles(&launcher),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, CREATE_ROW, IMPORT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );

    // An update replaces the code, not the user's choice.
    block_on(launcher.preview_package(&folder));
    assert!(
        launcher
            .view()
            .details()
            .contains(&"Disabled: enable it in Settings".to_owned()),
        "{:?}",
        launcher.view().details()
    );
    block_on(launcher.activate_selected());
    assert_eq!(enabled(&launcher), [(identity.clone(), false)]);
    assert_eq!(
        titles(&launcher),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, CREATE_ROW, IMPORT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );
    assert_eq!(enabled(&dirs.launcher()), [(identity, false)]);
}

fn enabling_an_identity_that_is_not_installed_is_explained(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();

    block_on(launcher.set_enabled(&PackageIdentity::local(&folder).unwrap(), true));

    match launcher.view().status {
        Status::Error(message) => assert!(message.starts_with("Nothing is installed from")),
        other => panic!("expected an error, got {other:?}"),
    }
}

fn commands_built_into_pane_have_no_settings(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let command = pane_core::CommandRegistration {
        id: "greeting".into(),
        title: "Built-in greeting".into(),
        subtitle: None,
        component: folder.join(fixture.component),
        takes_query: false,
        search: false,
    };
    let launcher = Launcher::new(Runtime::start(), vec![command]);

    block_on(launcher.activate_selected());

    match launcher.view().status {
        Status::Error(message) => {
            assert!(message.contains("only installed packages"), "{message}")
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

fn unreadable_settings_are_explained_to_the_command_and_never_overwritten(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    block_on(dirs.launcher().install_package(&folder));
    let file = dirs.packages_dir().join("settings.json");
    fs::write(&file, "not settings").unwrap();

    let restarted = dirs.launcher();
    // The command's view reads the saved style.
    block_on(restarted.activate_selected());

    match restarted.view().status {
        Status::Error(message) => assert!(message.contains("Cannot read"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), "not settings");
}

/// Opens the installed Greeting command from root search and selects its
/// item titled `item`.
fn open_greeting_at(launcher: &Launcher, item: &str) {
    launcher.back();
    select_title(launcher, "Greeting");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
    select_title(launcher, item);
}

fn a_setting_saved_while_the_package_is_being_disabled_is_refused(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    open_greeting_at(&launcher, "Use a formal greeting");

    // The action runs after the user disabled the package but before the
    // choice is on disk.
    let saving = launcher.activate_selected();
    let disabling = launcher.set_enabled(&identity, false);
    block_on(saving);
    block_on(disabling);

    assert_eq!(
        launcher.view().status,
        Status::Result("Disabled Settings sample".into())
    );
    let saved = fs::read_to_string(dirs.packages_dir().join("settings.json")).unwrap_or_default();
    assert!(!saved.contains("formal"), "{saved}");
    let restarted = dirs.launcher();
    block_on(restarted.set_enabled(&identity, true));
    assert_eq!(
        run(&restarted, "Greeting", "Greet me"),
        Status::Error(
            "The extension reported an error: No greeting style is saved yet; choose one first"
                .into()
        )
    );
}

fn an_action_result_that_arrives_after_its_package_was_disabled_is_not_shown(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    open_greeting_at(&launcher, "Greet me");

    let greeting = launcher.activate_selected();
    let disabling = launcher.set_enabled(&identity, false);
    block_on(futures::future::join(greeting, disabling));

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Disabled Settings sample".into())
    );
}

fn a_second_change_while_one_is_pending_is_ignored(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();

    let disabling = launcher.set_enabled(&identity, false);
    let enabling = launcher.set_enabled(&identity, true);
    block_on(futures::future::join(disabling, enabling));

    assert_eq!(enabled(&launcher), [(identity.clone(), false)]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Disabled Settings sample".into())
    );
    // What Pane shows is what it recorded.
    assert_eq!(enabled(&dirs.launcher()), [(identity, false)]);
}

fn pressing_enter_twice_on_an_extension_row_changes_it_once(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    manage(&launcher);

    let first = launcher.activate_selected();
    let second = launcher.activate_selected();
    block_on(futures::future::join(first, second));

    assert_eq!(enabled(&launcher), [(identity.clone(), false)]);
    assert!(subtitles(&launcher)[0].starts_with("Disabled"));
    assert_eq!(enabled(&dirs.launcher()), [(identity.clone(), false)]);
    // Once the change is done, Enter changes it back.
    block_on(launcher.activate_selected());
    assert_eq!(enabled(&launcher), [(identity, true)]);
}

fn a_launcher_without_a_package_location_explains_it_cannot_disable(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = Launcher::new(Runtime::start(), vec![]);

    block_on(launcher.set_enabled(&PackageIdentity::local(&folder).unwrap(), false));

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Could not update Pane's installed extensions: this launcher does not install packages"
                .into()
        )
    );
}

/// Declares one test per check for each language's settings sample.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    a_disabled_package_leaves_root_search_and_stays_disabled_after_a_restart,
    re_enabling_after_a_restart_restores_the_saved_settings,
    copies_with_the_same_title_are_enabled_and_keep_settings_by_identity,
    disabling_through_the_api_closes_the_package_command_and_updating_keeps_it_disabled,
    enabling_an_identity_that_is_not_installed_is_explained,
    commands_built_into_pane_have_no_settings,
    unreadable_settings_are_explained_to_the_command_and_never_overwritten,
    a_setting_saved_while_the_package_is_being_disabled_is_refused,
    an_action_result_that_arrives_after_its_package_was_disabled_is_not_shown,
    a_second_change_while_one_is_pending_is_ignored,
    pressing_enter_twice_on_an_extension_row_changes_it_once,
    a_launcher_without_a_package_location_explains_it_cannot_disable,
);
