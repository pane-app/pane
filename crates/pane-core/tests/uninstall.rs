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

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage extensions…";
const KEEP_ROW: &str = "Uninstall and keep saved data";
const DELETE_ROW: &str = "Uninstall and delete saved data";
const DELETE_RETAINED_ROW: &str = "Delete retained data";

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

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    launcher.back();
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "{command} opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
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
/// installed package (root lists each package's Greeting in install order),
/// returning the outcome: its toast, or the status line.
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

/// From root search, asks to delete the retained data titled `title` in the
/// extension manager, on row `row` among its "Delete retained data" rows.
fn ask_to_delete_retained(launcher: &Launcher, title: &str, row: usize) {
    manage(launcher);
    let label = format!("Delete retained data of {title}");
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

/// Deletes the only retained data titled `title`, confirming it.
fn delete_retained(launcher: &Launcher, title: &str) -> Status {
    ask_to_delete_retained(launcher, title, 0);
    select_title(launcher, DELETE_RETAINED_ROW);
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
            "Alias for Greeting",
            "Develop Settings sample",
            "Update extensions automatically",
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
    // It is listed only as retained data now.
    assert_eq!(
        titles(&launcher),
        [
            "Delete retained data of Settings sample",
            "Update extensions automatically"
        ]
    );

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
    let text = fs::read(&cache).unwrap();
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

    // Once the file is repaired, deleting the retained data removes the
    // cache the uninstall could not delete.
    fs::write(&cache, text).unwrap();
    assert_eq!(dirs.kinds_kept(&folder), ["cache.json"]);
    assert_eq!(
        delete_retained(&launcher, "Settings sample"),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert!(launcher.retained_data().is_empty());
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

fn retained_data_is_listed_and_deleted_without_the_extension(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    fs::write(dirs.source("notes.txt"), "the user's own document").unwrap();
    let identity = PackageIdentity::local(&folder).unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    assert!(matches!(
        uninstall(&launcher, "Settings sample", KEEP_ROW),
        Status::Result(_)
    ));
    drop(launcher);
    let sources = files(dirs.sources.path());

    // After a restart, and without any runtime so that no guest can run, the
    // retained data is listed with its title, source and what is kept.
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let restarted = Launcher::with_packages(unavailable, vec![], dirs.packages_dir());
    manage(&restarted);
    let view = restarted.view();
    // The retained data's row, then the global automatic-update row that
    // ends the list.
    assert_eq!(view.rows.len(), 2, "{:?}", view.rows);
    assert_eq!(
        view.rows[0].title,
        "Delete retained data of Settings sample"
    );
    assert_eq!(
        view.rows[0].subtitle,
        Some(format!(
            "Not installed · keeps 1 setting and 1 content record · {identity}"
        ))
    );
    ask_to_delete_retained(&restarted, "Settings sample", 0);
    let view = restarted.view();
    assert_eq!(view.title, "Delete the retained data of Settings sample?");
    assert_eq!(
        view.details(),
        [
            format!("From {identity}"),
            "Settings sample is not installed. Pane deletes the data it keeps for this source \
             itself; the extension is not downloaded and does not run."
                .to_string(),
            "Retained data: 1 setting and 1 content record".to_string(),
            format!(
                "Its source folder {}, files it saved elsewhere and other extensions' data are \
                 not touched.",
                identity.local_folder().unwrap().display()
            ),
        ]
    );
    assert_eq!(titles(&restarted), ["Cancel", DELETE_RETAINED_ROW]);
    // Cancel is selected, so Enter keeps the data.
    assert_eq!(view.selected, Some(0));
    select_title(&restarted, DELETE_RETAINED_ROW);
    block_on(restarted.activate_selected());
    let view = restarted.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.status,
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    // Only the global automatic-update row, which ends the list, is left.
    assert_eq!(titles(&restarted), ["Update extensions automatically"]);
    assert!(restarted.retained_data().is_empty());
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert_eq!(files(dirs.sources.path()), sources);
    drop(restarted);

    // Still gone after a restart; installing the same source again starts
    // with nothing.
    let launcher = dirs.launcher();
    assert!(launcher.retained_data().is_empty());
    block_on(launcher.install_package(&folder));
    assert_eq!(kept(&launcher), Status::Result(NOTHING_KEPT.into()));

    // An installed package's data is not retained data.
    save_everything(&launcher);
    block_on(launcher.delete_retained_data(&identity));
    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Settings sample is installed: its data is deleted by uninstalling it".into()
        )
    );
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

fn cancelling_keeps_the_retained_data(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    uninstall(&launcher, "Settings sample", KEEP_ROW);

    ask_to_delete_retained(&launcher, "Settings sample", 0);
    select_title(&launcher, "Cancel");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(view.status, Status::Idle);
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.clone()),
        Some("Delete retained data of Settings sample".into())
    );
    ask_to_delete_retained(&launcher, "Settings sample", 0);
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    assert_eq!(launcher.retained_data().len(), 1);
    assert_eq!(dirs.kinds_kept(&folder), ["settings.json", "content.json"]);
}

fn deleting_one_identitys_retained_data_keeps_the_others(fixture: &Fixture) {
    let dirs = Dirs::new();
    let published = settings_package(fixture, &dirs.source("published"), "Greeter");
    let development = settings_package(fixture, &dirs.source("development"), "Greeter");
    let installed = settings_package(fixture, &dirs.source("installed"), "Greeter");
    let launcher = dirs.launcher();
    for folder in [&published, &development, &installed] {
        block_on(launcher.install_package(folder));
    }
    for copy in 0..3 {
        save_everything_in(&launcher, copy);
    }
    // Uninstall the published copy, then the development copy, keeping both.
    for _ in 0..2 {
        ask_to_uninstall(&launcher, "Greeter", 0);
        select_title(&launcher, KEEP_ROW);
        block_on(launcher.activate_selected());
    }
    assert_eq!(launcher.retained_data().len(), 2);

    manage(&launcher);
    let subtitles: Vec<Option<String>> = launcher
        .view()
        .rows
        .into_iter()
        .filter(|row| row.title == "Delete retained data of Greeter")
        .map(|row| row.subtitle)
        .collect();
    assert_eq!(
        subtitles,
        [&published, &development].map(|folder| Some(format!(
            "Not installed · keeps 1 setting and 1 content record · {}",
            PackageIdentity::local(folder).unwrap()
        )))
    );
    // The second row is the development copy's.
    ask_to_delete_retained(&launcher, "Greeter", 1);
    assert_eq!(
        launcher.view().details()[0],
        format!("From {}", PackageIdentity::local(&development).unwrap())
    );
    select_title(&launcher, DELETE_RETAINED_ROW);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Deleted the retained data of Greeter".into())
    );

    assert!(dirs.kinds_kept(&development).is_empty());
    assert_eq!(
        dirs.kinds_kept(&published),
        ["settings.json", "content.json"]
    );
    assert_eq!(
        dirs.kinds_kept(&installed),
        [
            "settings.json",
            "content.json",
            "cache.json",
            "credentials.json"
        ]
    );
    assert_eq!(
        launcher.retained_data(),
        [RetainedData {
            identity: PackageIdentity::local(&published).unwrap(),
            title: "Greeter".into()
        }]
    );
    assert_eq!(
        run_in_copy(&launcher, 0, "Show what Pane keeps"),
        Status::Result(EVERYTHING_KEPT.into())
    );
}

fn retained_data_that_cannot_be_deleted_is_explained_and_stays_listed(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    uninstall(&launcher, "Settings sample", KEEP_ROW);
    drop(launcher);
    let content = dirs.packages_dir().join("content.json");
    let text = fs::read(&content).unwrap();
    fs::write(&content, "not json").unwrap();

    let launcher = dirs.launcher();
    ask_to_delete_retained(&launcher, "Settings sample", 0);
    let details = launcher.view().details().to_vec();
    assert!(
        details[2].starts_with(
            "Retained data: 1 setting and content records that cannot be read now (Cannot read "
        ),
        "{details:?}"
    );
    select_title(&launcher, DELETE_RETAINED_ROW);
    block_on(launcher.activate_selected());
    match launcher.view().status {
        Status::Error(message) => {
            assert!(
                message.starts_with(
                    "Could not delete all the retained data of Settings sample: could not \
                     delete its content: Cannot read "
                ),
                "{message}"
            );
            assert!(
                message.contains(&content.display().to_string()),
                "{message}"
            );
            assert!(
                message.ends_with(
                    "What could not be deleted stays listed: repair or delete that file, then \
                     delete it again."
                ),
                "{message}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert!(dirs.values("settings.json", &folder).is_empty());
    assert_eq!(fs::read_to_string(&content).unwrap(), "not json");
    assert_eq!(launcher.retained_data().len(), 1);
    assert!(titles(&launcher).contains(&"Delete retained data of Settings sample".to_string()));

    // Once the file is repaired, the list counts what it holds, and deleting
    // again finishes, without a restart.
    fs::write(&content, text).unwrap();
    manage(&launcher);
    assert_eq!(
        retained_subtitles(&launcher),
        [format!(
            "Not installed · keeps 1 content record · {}",
            PackageIdentity::local(&folder).unwrap()
        )]
    );
    assert_eq!(
        delete_retained(&launcher, "Settings sample"),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert!(dirs.launcher().retained_data().is_empty());
}

/// The subtitles of the extension list's "Delete retained data" rows.
fn retained_subtitles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .filter(|row| row.title.starts_with("Delete retained data of "))
        .filter_map(|row| row.subtitle)
        .collect()
}

/// `installed.json` that cannot be read: a folder is in its place, which
/// every system refuses to read as a file, so this fails the same way on
/// Windows, macOS and Linux.
fn a_registry_that_cannot_be_read_deletes_nothing(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    uninstall(&launcher, "Settings sample", KEEP_ROW);
    let registry = dirs.packages_dir().join("installed.json");
    let text = fs::read(&registry).unwrap();
    fs::remove_file(&registry).unwrap();
    fs::create_dir(&registry).unwrap();
    fs::write(registry.join("blocker"), "").unwrap();

    match delete_retained(&launcher, "Settings sample") {
        Status::Error(message) => {
            assert!(
                message.starts_with("Could not delete the retained data of Settings sample: "),
                "{message}"
            );
            assert!(message.ends_with(". Nothing was deleted."), "{message}");
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert_eq!(dirs.kinds_kept(&folder), ["settings.json", "content.json"]);
    assert_eq!(launcher.retained_data().len(), 1);

    fs::remove_dir_all(&registry).unwrap();
    fs::write(&registry, text).unwrap();
    assert_eq!(
        delete_retained(&launcher, "Settings sample"),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert!(dirs.launcher().retained_data().is_empty());
}

/// `installed.json` readable but not writable once the data is deleted: the
/// data folder made read-only. Unix only: on Windows a read-only folder
/// still lets files be created in it, and a folder in place of the file
/// fails when it is read, before anything is deleted (the check above).
#[cfg(unix)]
fn a_record_that_cannot_be_updated_is_explained_and_stays_listed(fixture: &Fixture) {
    use std::os::unix::fs::PermissionsExt;
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    uninstall(&launcher, "Settings sample", KEEP_ROW);
    // Its data files are emptied while Pane runs, as another Pane deleting
    // it would, so deleting writes only `installed.json`.
    for file in ["settings.json", "content.json"] {
        fs::remove_file(dirs.packages_dir().join(file)).unwrap();
    }
    let extensions = dirs.packages_dir();
    fs::set_permissions(&extensions, fs::Permissions::from_mode(0o555)).unwrap();

    let status = delete_retained(&launcher, "Settings sample");
    fs::set_permissions(&extensions, fs::Permissions::from_mode(0o755)).unwrap();
    match status {
        Status::Error(message) => {
            assert!(
                message.starts_with(
                    "Deleted the retained data of Settings sample, but could not remove it from \
                     the list: "
                ),
                "{message}"
            );
            assert!(
                message.ends_with("It stays listed, keeping nothing, until it is deleted again."),
                "{message}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    manage(&launcher);
    assert_eq!(
        retained_subtitles(&launcher),
        [format!(
            "Not installed · keeps nothing · {}",
            PackageIdentity::local(&folder).unwrap()
        )]
    );
    assert_eq!(
        delete_retained(&launcher, "Settings sample"),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.launcher().retained_data().is_empty());
}

#[cfg(not(unix))]
fn a_record_that_cannot_be_updated_is_explained_and_stays_listed(_fixture: &Fixture) {}

/// A kind whose file does not exist keeps nothing: the list and the
/// confirmation count the other kinds, and deleting succeeds.
fn a_missing_kind_file_counts_as_empty(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);
    uninstall(&launcher, "Settings sample", KEEP_ROW);
    fs::remove_file(dirs.packages_dir().join("content.json")).unwrap();

    manage(&launcher);
    let identity = PackageIdentity::local(&folder).unwrap();
    assert_eq!(
        retained_subtitles(&launcher),
        [format!("Not installed · keeps 1 setting · {identity}")]
    );
    ask_to_delete_retained(&launcher, "Settings sample", 0);
    assert_eq!(launcher.view().details()[2], "Retained data: 1 setting");
    select_title(&launcher, DELETE_RETAINED_ROW);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    assert!(!dirs.packages_dir().join("content.json").exists());
}

/// Two Panes on one data folder: the second installs the same source again,
/// and installs another package, after the first read `installed.json`. The
/// first, still listing the retained data, deletes nothing, and keeps the
/// second's install records and live data.
fn another_pane_reinstalling_the_source_keeps_its_data(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let other = settings_package(fixture, &dirs.source("other"), "Other sample");
    let first = dirs.launcher();
    block_on(first.install_package(&folder));
    save_everything(&first);
    uninstall(&first, "Settings sample", KEEP_ROW);
    assert_eq!(first.retained_data().len(), 1);

    let second = dirs.launcher();
    block_on(second.install_package(&folder));
    block_on(second.install_package(&other));
    assert_eq!(kept(&second), Status::Result(SAVED_DATA_KEPT.into()));
    drop(second);

    match delete_retained(&first, "Settings sample") {
        Status::Error(message) => assert_eq!(
            message,
            "Settings sample was installed again from the same source by another Pane using \
             this data folder, so its data is in use and nothing was deleted"
        ),
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(first.retained_data().is_empty());
    assert!(retained_subtitles(&first).is_empty());
    assert_eq!(dirs.kinds_kept(&folder), ["settings.json", "content.json"]);

    let third = dirs.launcher();
    let installed: Vec<PackageIdentity> = third
        .packages()
        .into_iter()
        .map(|package| package.identity)
        .collect();
    assert_eq!(
        installed,
        [
            PackageIdentity::local(&folder).unwrap(),
            PackageIdentity::local(&other).unwrap()
        ]
    );
    assert!(third.retained_data().is_empty());
}

/// Deleting keeps what another Pane recorded since this one read
/// `installed.json`, such as a package it installed.
fn deleting_keeps_another_panes_install_records(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let other = settings_package(fixture, &dirs.source("other"), "Other sample");
    let first = dirs.launcher();
    block_on(first.install_package(&folder));
    save_everything(&first);
    uninstall(&first, "Settings sample", KEEP_ROW);

    let second = dirs.launcher();
    block_on(second.install_package(&other));
    drop(second);

    assert_eq!(
        delete_retained(&first, "Settings sample"),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(dirs.kinds_kept(&folder).is_empty());
    let third = dirs.launcher();
    assert!(third.retained_data().is_empty());
    let installed: Vec<PackageIdentity> = third
        .packages()
        .into_iter()
        .map(|package| package.identity)
        .collect();
    assert_eq!(installed, [PackageIdentity::local(&other).unwrap()]);
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
    retained_data_is_listed_and_deleted_without_the_extension,
    cancelling_keeps_the_retained_data,
    deleting_one_identitys_retained_data_keeps_the_others,
    retained_data_that_cannot_be_deleted_is_explained_and_stays_listed,
    a_record_that_cannot_be_updated_is_explained_and_stays_listed,
    a_registry_that_cannot_be_read_deletes_nothing,
    a_missing_kind_file_counts_as_empty,
    another_pane_reinstalling_the_source_keeps_its_data,
    deleting_keeps_another_panes_install_records,
);
