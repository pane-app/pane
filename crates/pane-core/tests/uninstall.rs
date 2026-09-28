//! Uninstalling an installed package through the launcher's public
//! interface: Manage extensions asks first, with an explicit choice to keep
//! or delete the package's saved data (its settings and content). Either way
//! Pane removes the managed copy, the cache and the local credentials,
//! without running the package, and never touches the source folder or the
//! user's own files. Kept data stays with the package identity, so
//! installing the same source again finds it. Every check runs against the
//! settings sample in Rust, JavaScript and TypeScript, real guests from
//! `cargo xtask guests`, which keeps one value of each kind of data.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    CallError, Launcher, PackageIdentity, RetainedData, Runtime, SavedData, Screen, Status,
};
use tempfile::TempDir;

const MANAGE_ROW: &str = "Manage extensions…";
const KEEP_ROW: &str = "Uninstall and keep saved data";
const DELETE_ROW: &str = "Uninstall and delete saved data";

/// What "Show what Pane keeps" answers once every kind of data is saved.
const EVERYTHING_KEPT: &str =
    "Style: formal · Note: Water the plants · Signed in: yes · Cached greeting: Good day to you";
/// What it answers after reinstalling a package whose saved data was kept:
/// the cache and the credential were removed with it.
const SAVED_DATA_KEPT: &str =
    "Style: formal · Note: Water the plants · Signed in: no · Cached greeting: none";
/// What it answers when nothing is kept.
const NOTHING_KEPT: &str = "Style: none · Note: none · Signed in: no · Cached greeting: none";

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

    /// The values of kind `file` (such as `settings.json`) kept for the
    /// package in `folder`, by key.
    fn values(&self, file: &str, folder: &Path) -> BTreeMap<String, String> {
        let Ok(text) = fs::read_to_string(self.packages_dir().join(file)) else {
            return BTreeMap::new();
        };
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let key = PackageIdentity::local(folder).unwrap().key();
        json["packages"][&key]
            .as_object()
            .map(|values| {
                values
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Which kinds of data are kept for the package in `folder`.
    fn kinds_kept(&self, folder: &Path) -> Vec<&'static str> {
        [
            "settings.json",
            "content.json",
            "cache.json",
            "credentials.json",
        ]
        .into_iter()
        .filter(|file| !self.values(file, folder).is_empty())
        .collect()
    }
}

fn titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

fn select_title(launcher: &Launcher, title: &str) {
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    launcher.select(index);
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    launcher.back();
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "{command} opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// Saves one value of each kind with the Greeting command of the `copy`th
/// installed package: the formal style (settings), the last greeting
/// (cache), a note (content) and a sign-in token (credentials).
fn save_everything_in(launcher: &Launcher, copy: usize) {
    for (item, answer) in [
        ("Use a formal greeting", "Saved the formal greeting"),
        ("Greet me", "Good day to you"),
        ("Save a note", "Saved a note"),
        ("Sign in", "Signed in on this computer"),
    ] {
        assert_eq!(
            run_in_copy(launcher, copy, item),
            Status::Result(answer.into())
        );
    }
}

fn save_everything(launcher: &Launcher) {
    save_everything_in(launcher, 0);
    assert_eq!(kept(launcher), Status::Result(EVERYTHING_KEPT.into()));
}

/// What the Greeting command says Pane keeps for it.
fn kept(launcher: &Launcher) -> Status {
    run(launcher, "Greeting", "Show what Pane keeps")
}

/// From root search, runs `item` of the Greeting command of the `copy`th
/// installed package (root lists each package's Greeting in install order).
fn run_in_copy(launcher: &Launcher, copy: usize, item: &str) -> Status {
    launcher.back();
    launcher.back();
    let greeting = titles(launcher)
        .iter()
        .enumerate()
        .filter(|(_, title)| *title == "Greeting")
        .map(|(index, _)| index)
        .nth(copy)
        .unwrap_or_else(|| panic!("no Greeting {copy} in {:?}", titles(launcher)));
    launcher.select(greeting);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    launcher.back();
    launcher.back();
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// From root search, asks to uninstall the package titled `title` in the
/// extension manager, on row `row` among its "Uninstall" rows.
fn ask_to_uninstall(launcher: &Launcher, title: &str, row: usize) {
    manage(launcher);
    let label = format!("Uninstall {title}");
    let index = titles(launcher)
        .iter()
        .enumerate()
        .filter(|(_, row)| **row == label)
        .map(|(index, _)| index)
        .nth(row)
        .unwrap_or_else(|| panic!("no row {label:?} in {:?}", titles(launcher)));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Confirm { .. }));
}

/// Uninstalls the only package titled `title`, choosing the row `choice`.
fn uninstall(launcher: &Launcher, title: &str, choice: &str) -> Status {
    ask_to_uninstall(launcher, title, 0);
    select_title(launcher, choice);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// Every file under `dir` with its contents.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.insert(path.clone(), fs::read(&path).unwrap());
        }
    }
    found
}

fn uninstalling_and_keeping_saved_data_restores_it_on_reinstall(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let identity = PackageIdentity::local(&folder).unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    let managed = launcher.packages()[0].location.clone();

    manage(&launcher);
    assert_eq!(
        titles(&launcher),
        [
            "Settings sample",
            "Reload Settings sample",
            "Clear cache of Settings sample",
            "Uninstall Settings sample",
            "Hotkey for Greeting",
        ]
    );
    ask_to_uninstall(&launcher, "Settings sample", 0);
    let view = launcher.view();
    assert_eq!(view.title, "Uninstall Settings sample?");
    assert_eq!(
        view.details(),
        [
            format!("From {identity}"),
            "Pane removes its installed copy, its cache and its credentials on this computer, \
             and the extension does not run. Deleting a credential does not sign you out of an \
             online service."
                .to_string(),
            "Saved data: 1 setting and 1 content record".to_string(),
            format!(
                "Its source folder {} and files it saved elsewhere are not touched.",
                // As the identity names it: resolved by the operating system
                // (macOS reports `/private/var/...` for a temporary
                // `/var/...` folder, Windows the long form of `RUNNER~1`).
                identity.local_folder().unwrap().display()
            ),
        ]
    );
    assert_eq!(titles(&launcher), [KEEP_ROW, DELETE_ROW, "Cancel"]);
    select_title(&launcher, KEEP_ROW);
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.status,
        Status::Result("Uninstalled Settings sample; its settings and content are kept".into())
    );
    assert!(view.rows.is_empty(), "{:?}", view.rows);

    // Gone from root search, with its managed copy, cache and credential.
    launcher.back();
    assert!(!titles(&launcher).contains(&"Greeting".to_string()));
    assert!(launcher.packages().is_empty());
    assert!(!managed.exists(), "{} was removed", managed.display());
    assert_eq!(dirs.kinds_kept(&folder), ["settings.json", "content.json"]);
    let retained = [RetainedData {
        identity: identity.clone(),
        title: "Settings sample".into(),
    }];
    assert_eq!(launcher.retained_data(), retained);

    // Pane restarted still has it uninstalled, with the data retained.
    let restarted = dirs.launcher();
    assert!(restarted.packages().is_empty());
    assert_eq!(restarted.retained_data(), retained);

    // Installing the same source again finds its settings and content.
    block_on(restarted.install_package(&folder));
    assert_eq!(
        restarted.view().status,
        Status::Result("Installed Settings sample".into())
    );
    assert_eq!(kept(&restarted), Status::Result(SAVED_DATA_KEPT.into()));
    assert!(restarted.retained_data().is_empty());
}

fn uninstalling_and_deleting_saved_data_removes_every_kind(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);

    assert_eq!(
        uninstall(&launcher, "Settings sample", DELETE_ROW),
        Status::Result("Uninstalled Settings sample and deleted its saved data".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert!(launcher.retained_data().is_empty());
    assert!(
        fs::read_dir(dirs.packages_dir().join("packages"))
            .unwrap()
            .next()
            .is_none()
    );

    block_on(launcher.install_package(&folder));
    assert_eq!(kept(&launcher), Status::Result(NOTHING_KEPT.into()));
}

fn cancelling_keeps_the_package_installed(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);

    ask_to_uninstall(&launcher, "Settings sample", 0);
    select_title(&launcher, "Cancel");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(view.status, Status::Idle);
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.clone()),
        Some("Uninstall Settings sample".into())
    );

    ask_to_uninstall(&launcher, "Settings sample", 0);
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    assert_eq!(launcher.packages().len(), 1);
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

fn uninstalling_one_copy_keeps_the_other_identity_and_external_files(fixture: &Fixture) {
    let dirs = Dirs::new();
    let published = settings_package(fixture, &dirs.source("published"), "Greeter");
    let development = settings_package(fixture, &dirs.source("development"), "Greeter");
    fs::write(dirs.source("notes.txt"), "the user's own document").unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&published));
    block_on(launcher.install_package(&development));
    save_everything_in(&launcher, 0);
    save_everything_in(&launcher, 1);
    let sources = files(dirs.sources.path());

    // The second "Uninstall Greeter" row is the development copy's.
    ask_to_uninstall(&launcher, "Greeter", 1);
    assert_eq!(
        launcher.view().details()[0],
        format!("From {}", PackageIdentity::local(&development).unwrap())
    );
    select_title(&launcher, DELETE_ROW);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Uninstalled Greeter and deleted its saved data".into())
    );

    let installed: Vec<PackageIdentity> = launcher
        .packages()
        .into_iter()
        .map(|package| package.identity)
        .collect();
    assert_eq!(installed, [PackageIdentity::local(&published).unwrap()]);
    assert_eq!(
        run_in_copy(&launcher, 0, "Show what Pane keeps"),
        Status::Result(EVERYTHING_KEPT.into())
    );
    assert!(dirs.kinds_kept(&development).is_empty());
    // The source folders, the development copy's included, and the user's
    // document are untouched.
    assert_eq!(files(dirs.sources.path()), sources);
}

fn kept_data_is_not_given_to_another_source_with_the_same_title(fixture: &Fixture) {
    let dirs = Dirs::new();
    let first = settings_package(fixture, &dirs.source("first"), "Greeter");
    let second = settings_package(fixture, &dirs.source("second"), "Greeter");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&first));
    save_everything(&launcher);
    assert!(matches!(
        uninstall(&launcher, "Greeter", KEEP_ROW),
        Status::Result(_)
    ));

    block_on(launcher.install_package(&second));
    assert_eq!(kept(&launcher), Status::Result(NOTHING_KEPT.into()));
    // The first source's data is still its own, and still retained.
    assert_eq!(dirs.kinds_kept(&first), ["settings.json", "content.json"]);
    assert_eq!(
        launcher.retained_data(),
        [RetainedData {
            identity: PackageIdentity::local(&first).unwrap(),
            title: "Greeter".into()
        }]
    );
}

fn a_disabled_broken_package_uninstalls_without_running_it(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.set_enabled(&identity, false));
    let package = launcher.packages().remove(0);
    drop(launcher);
    // The managed copy's component no longer loads.
    fs::write(package.location.join(fixture.component), b"not a component").unwrap();

    // Without any runtime, no guest can run.
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let restarted = Launcher::with_packages(unavailable, vec![], dirs.packages_dir());
    assert_eq!(
        uninstall(&restarted, "Settings sample", KEEP_ROW),
        Status::Result("Uninstalled Settings sample; its settings and content are kept".into())
    );
    assert!(!package.location.exists());
    assert_eq!(dirs.kinds_kept(&folder), ["settings.json", "content.json"]);
}

fn an_open_command_closes_and_its_instance_stops(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let runtime = Runtime::start().unwrap();
    let launcher = Launcher::with_packages(Ok(runtime.clone()), vec![], dirs.packages_dir());
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    run(&launcher, "Greeting", "Show what Pane keeps");
    assert_eq!(launcher.view().screen, Screen::Command);
    assert_eq!(block_on(runtime.running()).len(), 1);

    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.uninstall(&identity, SavedData::Keep));
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Uninstalled Settings sample; its settings and content are kept".into())
    );
    assert!(!titles(&launcher).contains(&"Greeting".to_string()));
    assert!(block_on(runtime.running()).is_empty());
}

fn a_registry_that_cannot_be_written_leaves_it_installed(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    // `installed.json` cannot be replaced by a file while a folder is there.
    let registry = dirs.packages_dir().join("installed.json");
    let text = fs::read(&registry).unwrap();
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    fs::write(registry.join("blocker"), "").unwrap();

    match uninstall(&launcher, "Settings sample", DELETE_ROW) {
        Status::Error(message) => {
            assert!(
                message.starts_with("Could not uninstall Settings sample: "),
                "{message}"
            );
            assert!(
                message.ends_with("It is still installed and nothing was deleted."),
                "{message}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(launcher.packages().len(), 1);
    assert_eq!(
        dirs.kinds_kept(&folder),
        [
            "settings.json",
            "content.json",
            "cache.json",
            "credentials.json"
        ]
    );

    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, text).unwrap();
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

fn data_that_cannot_be_deleted_is_explained_and_kept_on_record(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    drop(launcher);
    let cache = dirs.packages_dir().join("cache.json");
    fs::write(&cache, "not json").unwrap();

    let launcher = dirs.launcher();
    match uninstall(&launcher, "Settings sample", DELETE_ROW) {
        Status::Error(message) => {
            assert!(
                message.starts_with(
                    "Uninstalled Settings sample, but could not delete its cache: Cannot read "
                ),
                "{message}"
            );
            assert!(message.contains(&cache.display().to_string()), "{message}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(launcher.packages().is_empty());
    // The other kinds are deleted; the unreadable file is left as it was, and
    // the data it may hold stays on record.
    assert!(dirs.values("settings.json", &folder).is_empty());
    assert!(dirs.values("credentials.json", &folder).is_empty());
    assert_eq!(fs::read_to_string(&cache).unwrap(), "not json");
    assert_eq!(
        launcher.retained_data(),
        [RetainedData {
            identity: PackageIdentity::local(&folder).unwrap(),
            title: "Settings sample".into()
        }]
    );
}

/// A managed folder Pane cannot remove, as Windows refuses for a file in
/// use: here, the packages folder is made read-only.
#[cfg(unix)]
fn a_managed_copy_that_cannot_be_removed_is_explained_and_removed_at_the_next_start(
    fixture: &Fixture,
) {
    use std::os::unix::fs::PermissionsExt;
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let location = launcher.packages()[0].location.clone();
    let packages = dirs.packages_dir().join("packages");
    fs::set_permissions(&packages, fs::Permissions::from_mode(0o555)).unwrap();

    let status = uninstall(&launcher, "Settings sample", KEEP_ROW);
    fs::set_permissions(&packages, fs::Permissions::from_mode(0o755)).unwrap();
    match status {
        Status::Error(message) => {
            assert!(
                message.starts_with(&format!(
                    "Uninstalled Settings sample, but its installed copy in {} could not be \
                     removed yet (",
                    location.display()
                )),
                "{message}"
            );
            assert!(
                message.ends_with("); Pane removes it when it next starts."),
                "{message}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(launcher.packages().is_empty());
    assert!(location.exists());

    let restarted = dirs.launcher();
    assert!(restarted.packages().is_empty());
    assert!(!location.exists(), "{} was removed", location.display());
}

#[cfg(not(unix))]
fn a_managed_copy_that_cannot_be_removed_is_explained_and_removed_at_the_next_start(
    _fixture: &Fixture,
) {
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
    uninstalling_and_keeping_saved_data_restores_it_on_reinstall,
    uninstalling_and_deleting_saved_data_removes_every_kind,
    cancelling_keeps_the_package_installed,
    uninstalling_one_copy_keeps_the_other_identity_and_external_files,
    kept_data_is_not_given_to_another_source_with_the_same_title,
    a_disabled_broken_package_uninstalls_without_running_it,
    an_open_command_closes_and_its_instance_stops,
    a_registry_that_cannot_be_written_leaves_it_installed,
    data_that_cannot_be_deleted_is_explained_and_kept_on_record,
    a_managed_copy_that_cannot_be_removed_is_explained_and_removed_at_the_next_start,
);
