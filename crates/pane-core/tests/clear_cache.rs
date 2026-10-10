//! Clearing an installed package's cache through the launcher's public
//! interface: Manage extensions asks first, then removes only the data the
//! package keeps as cache, never its settings, content or credentials, without
//! running the package; and local credentials kept readable only by the user.
//! Every check runs against the settings sample in Rust,
//! JavaScript and TypeScript, real guests from `cargo xtask guests`, which keeps
//! one value of each kind of data.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::{CallError, Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

const MANAGE_ROW: &str = "Manage Extensions";

/// What "Show what Pane keeps" answers once every kind of data is saved.
const EVERYTHING_KEPT: &str =
    "Style: formal · Note: Water the plants · Signed in: yes · Cached greeting: Good day to you";
/// What clearing the cache adds while an instance of the package runs.
const WHILE_RUNNING: &str = ". A running instance may write it again until it stops.";
/// What it answers once the cache is cleared.
const CACHE_CLEARED: &str =
    "Style: formal · Note: Water the plants · Signed in: yes · Cached greeting: none";

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

/// Saves one value of each kind with the Greeting command: the formal style
/// (settings), the last greeting (cache), a note (content) and a sign-in
/// token (credentials).
fn save_everything(launcher: &Launcher) {
    for (item, answer) in [
        ("Use a formal greeting", "Saved the formal greeting"),
        ("Greet me", "Good day to you"),
        ("Save a note", "Saved a note"),
        ("Sign in", "Signed in on this computer"),
    ] {
        assert_eq!(
            run(launcher, "Greeting", item),
            Status::Result(answer.into())
        );
    }
    assert_eq!(kept(launcher), Status::Result(EVERYTHING_KEPT.into()));
}

/// What the Greeting command says Pane keeps for it.
fn kept(launcher: &Launcher) -> Status {
    run(launcher, "Greeting", "Show what Pane keeps")
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    launcher.back();
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// From root search, asks to clear the cache of the package titled `title`
/// in the extension manager, on row `row` among its "Clear cache" rows.
fn ask_to_clear(launcher: &Launcher, title: &str, row: usize) {
    manage(launcher);
    let label = format!("Clear cache of {title}");
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

/// Clears the cache of the only package titled `title` and confirms it.
fn clear_cache(launcher: &Launcher, title: &str) -> Status {
    ask_to_clear(launcher, title, 0);
    select_title(launcher, "Clear cache");
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

fn clearing_the_cache_keeps_settings_content_and_credentials(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);

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
            "Update extensions automatically"
        ]
    );
    ask_to_clear(&launcher, "Settings sample", 0);
    let view = launcher.view();
    assert_eq!(view.title, "Clear the cache of Settings sample?");
    assert_eq!(
        view.details(),
        [
            format!("From {}", PackageIdentity::local(&folder).unwrap()),
            "Pane deletes the data this extension keeps as its cache. Its settings, content and \
             credentials are kept, and the extension does not run."
                .to_string(),
        ]
    );
    assert_eq!(titles(&launcher), ["Clear cache", "Cancel"]);
    select_title(&launcher, "Clear cache");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.status,
        Status::Result(format!(
            "Cleared the cache of Settings sample{WHILE_RUNNING}"
        ))
    );

    assert_eq!(kept(&launcher), Status::Result(CACHE_CLEARED.into()));
    // Pane restarted keeps the same.
    assert_eq!(kept(&dirs.launcher()), Status::Result(CACHE_CLEARED.into()));
}

fn cancelling_keeps_the_cache(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    save_everything(&launcher);

    ask_to_clear(&launcher, "Settings sample", 0);
    select_title(&launcher, "Cancel");
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(launcher.view().status, Status::Idle);

    ask_to_clear(&launcher, "Settings sample", 0);
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));

    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

fn clearing_one_copy_keeps_the_other_identity_and_external_files(fixture: &Fixture) {
    let dirs = Dirs::new();
    let published = settings_package(fixture, &dirs.source("published"), "Greeter");
    let development = settings_package(fixture, &dirs.source("development"), "Greeter");
    fs::write(dirs.source("notes.txt"), "the user's own document").unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&published));
    block_on(launcher.install_package(&development));
    // Each copy's Greeting saves its own: the copies are told apart by the
    // source their subtitles name (#197), not by position — what root
    // search learned about one ranks it first (#199), wherever that
    // leaves the other.
    for folder in [&published, &development] {
        for item in [
            "Use a formal greeting",
            "Greet me",
            "Save a note",
            "Sign in",
        ] {
            assert!(matches!(
                run_in_copy(&launcher, folder, item),
                Status::Result(_)
            ));
        }
    }
    let sources = files(dirs.sources.path());

    // The second "Clear cache of Greeter" row is the development copy's.
    ask_to_clear(&launcher, "Greeter", 1);
    assert_eq!(
        launcher.view().details()[0],
        format!("From {}", PackageIdentity::local(&development).unwrap())
    );
    select_title(&launcher, "Clear cache");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Cleared the cache of Greeter{WHILE_RUNNING}"))
    );

    let answers: Vec<Status> = [&published, &development]
        .into_iter()
        .map(|folder| run_in_copy(&launcher, folder, "Show what Pane keeps"))
        .collect();
    assert_eq!(
        answers,
        [
            Status::Result(EVERYTHING_KEPT.into()),
            Status::Result(CACHE_CLEARED.into())
        ]
    );
    // The source folders and the user's document are untouched.
    assert_eq!(files(dirs.sources.path()), sources);
}

fn a_disabled_broken_package_cache_clears_without_running_it(fixture: &Fixture) {
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
    let component = package.location.join(fixture.component);
    let working = fs::read(&component).unwrap();
    fs::write(&component, b"not a component").unwrap();

    // Without any runtime, no guest can run.
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let restarted = Launcher::with_packages(unavailable, vec![], dirs.packages_dir());
    assert_eq!(
        clear_cache(&restarted, "Settings sample"),
        Status::Result("Cleared the cache of Settings sample".into())
    );
    drop(restarted);

    fs::write(&component, working).unwrap();
    let repaired = dirs.launcher();
    block_on(repaired.set_enabled(&identity, true));
    assert_eq!(kept(&repaired), Status::Result(CACHE_CLEARED.into()));
}

fn an_unreadable_cache_is_explained_and_nothing_is_deleted(fixture: &Fixture) {
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    block_on(dirs.launcher().install_package(&folder));
    let launcher = dirs.launcher();
    save_everything(&launcher);
    // What the runs taught root search is written off the thread (#199):
    // it is waited for, so the snapshot below is of everything written.
    assert!(
        launcher.wait_for_learned_recorded(Duration::from_secs(30)),
        "the learned record was written"
    );
    drop(launcher);
    let cache = dirs.packages_dir().join("cache.json");
    fs::write(&cache, "not json").unwrap();
    let before = files(&dirs.packages_dir());

    let launcher = dirs.launcher();
    match clear_cache(&launcher, "Settings sample") {
        Status::Error(message) => {
            assert!(
                message.starts_with("Could not clear the cache of Settings sample: Cannot read "),
                "{message}"
            );
            assert!(message.contains(&cache.display().to_string()), "{message}");
            assert!(
                message.ends_with(
                    "Nothing was deleted. That file holds only extension caches: repair or \
                     delete it, then clear the cache again."
                ),
                "{message}"
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
    assert_eq!(files(&dirs.packages_dir()), before);

    // Deleting the file as advised lets clearing succeed without a restart.
    fs::remove_file(&cache).unwrap();
    assert_eq!(
        clear_cache(&launcher, "Settings sample"),
        Status::Result("Cleared the cache of Settings sample".into())
    );
    assert_eq!(kept(&launcher), Status::Result(CACHE_CLEARED.into()));
}

/// From root search, opens the Greeting command of the package installed
/// from `folder` and runs its item titled `item`, returning the outcome:
/// its toast, or the status line. Two copies of the package are told apart
/// by the source the same-name pass names in the row's subtitle (#197),
/// not by position: what root search learned about one ranks it first
/// (#199), wherever that leaves the other.
fn run_in_copy(launcher: &Launcher, folder: &Path, item: &str) -> Status {
    launcher.back();
    launcher.back();
    let source = format!("local folder {}", folder.display());
    let greeting = launcher
        .view()
        .rows
        .iter()
        .position(|row| {
            row.title == "Greeting"
                && row
                    .subtitle
                    .as_deref()
                    .is_some_and(|subtitle| subtitle.ends_with(&source))
        })
        .unwrap_or_else(|| panic!("no Greeting row of {source}"));
    launcher.select(greeting);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn clearing_keeps_a_cache_another_pane_saved_meanwhile(fixture: &Fixture) {
    let dirs = Dirs::new();
    let first = settings_package(fixture, &dirs.source("first"), "First");
    let second = settings_package(fixture, &dirs.source("second"), "Second");
    let pane = dirs.launcher();
    block_on(pane.install_package(&first));
    block_on(pane.install_package(&second));
    for item in ["Use a formal greeting", "Greet me"] {
        assert!(matches!(
            run_in_copy(&pane, &first, item),
            Status::Result(_)
        ));
    }
    // A second Pane on the same data folder caches Second's greeting.
    let other = dirs.launcher();
    for item in ["Use a formal greeting", "Greet me"] {
        assert!(matches!(
            run_in_copy(&other, &second, item),
            Status::Result(_)
        ));
    }

    assert_eq!(
        clear_cache(&pane, "First"),
        Status::Result(format!("Cleared the cache of First{WHILE_RUNNING}"))
    );

    let restarted = dirs.launcher();
    let Status::Result(kept) = run_in_copy(&restarted, &second, "Show what Pane keeps") else {
        panic!("{:?}", restarted.view().status);
    };
    assert!(kept.ends_with("Cached greeting: Good day to you"), "{kept}");
    let Status::Result(kept) = run_in_copy(&restarted, &first, "Show what Pane keeps") else {
        panic!("{:?}", restarted.view().status);
    };
    assert!(kept.ends_with("Cached greeting: none"), "{kept}");
}

/// The permission bits of `path` (Unix).
#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn local_credentials_are_readable_only_by_the_user(fixture: &Fixture) {
    use std::os::unix::fs::PermissionsExt;
    let dirs = Dirs::new();
    let folder = settings_package(fixture, &dirs.source("settings"), "Settings sample");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let credentials = dirs.packages_dir().join("credentials.json");

    save_everything(&launcher);
    assert_eq!(mode(&credentials), 0o600);

    // A file an earlier Pane left readable by others becomes private at the
    // next save.
    let permissions = fs::Permissions::from_mode(0o644);
    fs::set_permissions(&credentials, permissions).unwrap();
    assert_eq!(
        run(&launcher, "Greeting", "Sign in"),
        Status::Result("Signed in on this computer".into())
    );
    assert_eq!(mode(&credentials), 0o600);
}

#[cfg(not(unix))]
fn local_credentials_are_readable_only_by_the_user(_fixture: &Fixture) {}

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
    clearing_the_cache_keeps_settings_content_and_credentials,
    cancelling_keeps_the_cache,
    clearing_one_copy_keeps_the_other_identity_and_external_files,
    a_disabled_broken_package_cache_clears_without_running_it,
    an_unreadable_cache_is_explained_and_nothing_is_deleted,
    clearing_keeps_a_cache_another_pane_saved_meanwhile,
    local_credentials_are_readable_only_by_the_user,
);
