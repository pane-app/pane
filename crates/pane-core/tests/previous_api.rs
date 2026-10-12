//! What runs through the previous world's adapter (ADR 0046): a component
//! built for extension API 0.1, which this Pane still runs while the
//! window lasts, is served and called through it — every interface it
//! imports answered by the same host code, every export called with the
//! same conversions, the two paths indistinguishable in behaviour.
//!
//! The positive fixture `guests/fixtures/previous-api` is built from the
//! shared `wit/previous` tree (unlike the negative controls, which carry
//! their own WIT copies): its command answers a list whose one item's
//! action calls `show-hud` with a `toast-style`, the one shape the 0.2
//! breaking change retyped, which the adapter converts to the host's
//! drawing. It runs as a command built into Pane, as an installed package,
//! and as a default extension a Pane release pins — a released component
//! nobody can rebuild, the scenario the window exists for (#285).
//!
//! A built-in command runs with no package data, an installed package
//! with it, and a default from a repository served over Git's smart HTTP
//! protocol from 127.0.0.1 (`support/repo_server.rs`, `support/defaults.rs`):
//! nothing reaches the network or a real Git host.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::feedback::{Hud, ToastStyle};
use pane_core::{CommandRegistration, Launcher, PackageIdentity, Runtime, Screen, Status};

#[path = "support/defaults.rs"]
mod defaults;

use defaults::pinned;

#[path = "support/guests.rs"]
mod guests;

use guests::{guest, guests};

#[path = "support/repo_server.rs"]
mod repo_server;

use repo_server::Server;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, to_root};

#[path = "support/feedback.rs"]
mod window_fake;

use window_fake::RecordingWindow;

/// The command's title, in root search and on its screen.
const TITLE: &str = "Previous API";

/// The one item's title, on the command's screen.
const ITEM: &str = "Show a HUD";

/// What the item's action tells the user, in the HUD it shows.
const SAID: &str = "Hello from the previous API";

/// The HUD the fixture's action shows, over other applications.
fn hud() -> Hud {
    Hud::new(ToastStyle::Success, SAID)
}

/// Opens the command titled `TITLE` from root search and its screen.
fn open(launcher: &Launcher) {
    to_root(launcher);
    block_on(launcher.set_query(TITLE));
    select_title(launcher, TITLE);
    block_on(launcher.activate_selected());
    assert_eq!(&launcher.view().screen, &Screen::Command);
}

/// Runs the command's one item's one action, as the Actions panel
/// chooses it.
fn act(launcher: &Launcher) {
    select_title(launcher, ITEM);
    block_on(launcher.run_item_action("hud", 0));
}

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`, as the feedback tests do.
fn copy(name: &str, folder: &Path) -> PathBuf {
    let assembled = guests().join("packages").join(name);
    assert!(
        assembled.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let file = entry.unwrap().path();
        fs::copy(&file, folder.join(file.file_name().unwrap())).unwrap();
    }
    folder.to_path_buf()
}

#[test]
fn a_command_built_into_pane_runs_through_the_adapter() {
    let launcher = Launcher::new(
        Ok(Runtime::start().unwrap()),
        vec![CommandRegistration {
            id: "previous".into(),
            title: TITLE.into(),
            subtitle: None,
            component: guest("previous_api"),
            takes_query: false,
            search: false,
            keywords: Vec::new(),
            when: pane_core::CommandWhen::Always,
            matches: pane_core::CommandMatches::Title,
        }],
    );
    let window = RecordingWindow::attach(&launcher);

    // The command's screen, drawn through the previous world's bindings,
    // and its item's action, called through them: the action shows a HUD
    // — the import the breaking change retyped, served through the
    // adapter with the shape the previous world knows it by.
    open(&launcher);
    assert_eq!(launcher.view().title.as_str(), TITLE);
    act(&launcher);

    assert_eq!(window.huds(), [hud()]);
}

#[test]
fn a_package_built_for_the_previous_api_installs_and_runs() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    );
    let window = RecordingWindow::attach(&launcher);
    let folder = copy("previous-api", &sources.path().join("previous-api"));
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Previous API".into())
    );

    // The package's manifest names the version it was built for, which
    // this Pane runs: no refusal, no mark, the command's row offered like
    // any other's.
    let packages = launcher.packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(
        packages[0].identity,
        PackageIdentity::local(&folder).unwrap()
    );
    assert_eq!(launcher.extension_mark(&packages[0].identity), None);

    open(&launcher);
    act(&launcher);

    assert_eq!(window.huds(), [hud()]);
}

#[test]
fn a_default_built_for_the_previous_api_installs_and_runs_adapted() {
    let repos = tempfile::tempdir().unwrap();
    let server = Server::start();
    // The default's repository, made from the fixture's package and
    // served as a Pane release pins it: a released component nobody can
    // rebuild, kept working by the window.
    let pin = pinned(
        &server,
        repos.path(),
        "previous-api",
        "Previous API",
        "v0.1.0",
        &package_files(),
    );

    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_defaults(vec![pin]);
    let window = RecordingWindow::attach(&launcher);
    block_on(launcher.acquire_defaults());

    // Acquired by Pane at first setup, under the default's own identity,
    // and run adapted: the same answer a current-world default gives.
    let packages = launcher.packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].title(), "Previous API");
    assert_eq!(packages[0].identity.key(), "default:previous-api");

    open(&launcher);
    act(&launcher);

    assert_eq!(window.huds(), [hud()]);
}

/// The fixture's assembled package under `target/guests/packages`, by the
/// path of each file in it.
fn package_files() -> Vec<(String, Vec<u8>)> {
    let folder = guests().join("packages").join("previous-api");
    assert!(
        folder.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    files_of(&folder, "")
}

/// The files of `folder`, by their path under it, in alphabetical order.
fn files_of(folder: &Path, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().into_string().unwrap();
        let in_package = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            files.extend(files_of(&path, &in_package));
        } else {
            files.push((in_package, fs::read(&path).unwrap()));
        }
    }
    files
}
