//! What Pane marks as its own, an official extension (#280, ADR 0045),
//! through the launcher's public interface as Settings reads it: a
//! default extension, and a package whose recorded Git source is a
//! repository in the pane-app organization on GitHub — installed by Pane
//! at first setup or by the user by hand alike. An extension from any
//! other source, another Git host, npm or a folder, is not official.
//!
//! The default extension is the Rust sample, installed from its pinned
//! commit in a repository made with the `git` program and served over
//! Git's smart HTTP protocol from 127.0.0.1 (`support/repo_server.rs`,
//! `support/defaults.rs`) — a stand-in for a default's own repository,
//! which lives outside this one (#285) — the npm sample from a local registry
//! (`support/npm_registry.rs`), and the Git sample from a repository each
//! test makes with the `git` program and serves over Git's smart HTTP
//! protocol (`support/repo_server.rs`): nothing reaches the network. A
//! hand install from a repository under `pane-app` is what its record in
//! `installed.json` says, so the package the test installs from this
//! computer's server is given that record and read back, as a restart of
//! Pane reads what such an install wrote.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::clipboard::{Clock as _, ManualClock, SystemClock};
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{DefaultExtension, Launcher, PackageIdentity, Runtime};
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;

use defaults::from_sample;

#[path = "support/guests.rs"]
mod guests;

use guests::guests;

#[path = "support/npm_registry.rs"]
mod npm_registry;

use npm_registry::{Registry, pack};

#[path = "support/repo_server.rs"]
mod repo_server;

use repo_server::{Repo, Server, greeter_files};

/// The npm sample's name, as the registry the tests run serves it.
const GREETER: &str = "@pane-samples/greeter";

/// The world of one test: Pane's data location, the runtime the tests
/// share, the servers nothing outside this computer reaches — the
/// repository the default extension is acquired from, the npm registry the
/// sample installs from, the Git server its repository is served from —
/// and a frozen clock, so the launcher's background updater never checks
/// on its own while the test is fetching from and moving the same
/// servers (see `repositories.rs` for the race it avoids).
struct Dirs {
    sources: TempDir,
    data: TempDir,
    repos: TempDir,
    runtime: Runtime,
    registry: Registry,
    server: Server,
    clock: Arc<ManualClock>,
    /// The pin that names the default extension's served repository.
    sample: DefaultExtension,
}

impl Dirs {
    fn new() -> Dirs {
        let server = Server::start();
        let repos = tempfile::tempdir().unwrap();
        // The default extension's repository, made from the Rust sample's
        // assembled package `cargo xtask guests` leaves under
        // `target/guests/packages` and served as a Pane release pins it
        // (`support/defaults.rs`): the default the tests acquire. The
        // default extensions' own repositories live outside this one
        // (#285), so a sample's package stands in for one.
        let sample = from_sample(
            &server,
            repos.path(),
            "sample-rust",
            "Rust sample",
            "sample-rust",
        );
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            repos,
            runtime: Runtime::start().unwrap(),
            registry: Registry::start(),
            server,
            clock: ManualClock::at(SystemClock.now()),
            sample,
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder; a new one is a restart of Pane. It
    /// acquires the sample as a default extension from its pinned
    /// repository, installs npm packages from the local registry, and runs
    /// on the frozen clock, so nothing happens in the background that the
    /// test did not ask for.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_defaults(vec![self.sample.clone()])
            .with_npm_registry(NpmRegistry::local(self.registry.url()).unwrap())
            .with_clock(self.clock.clone())
    }

    /// A new repository served as `name`.
    fn repo(&self, name: &str) -> (Repo, String) {
        let repo = Repo::init(&self.repos.path().join(name), self.server.home());
        let url = self.server.serve(name, &repo);
        (repo, url)
    }

    /// The Greeter sample as a folder package in the sources: the same
    /// files the repository's release revision holds, but a local copy,
    /// another package and not an official one.
    fn greeter_folder(&self) -> PathBuf {
        let folder = self.sources.path().join("greeter");
        fs::create_dir_all(&folder).unwrap();
        for (path, contents) in greeter_files(&guests(), true) {
            let file = folder.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, contents).unwrap();
        }
        folder
    }
}

/// The Git package's record in `installed.json`, the one whose source
/// names a repository.
fn git_record(dirs: &Dirs) -> Value {
    let registry = read_registry(dirs);
    registry["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record.get("git").is_some())
        .cloned()
        .unwrap_or_else(|| panic!("no Git record in {registry:#}"))
}

/// Reads `installed.json` back as Pane writes it.
fn read_registry(dirs: &Dirs) -> Value {
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Gives the Git package's record the repository `repository`
/// (`host/path`) fetched from `url`, the record an install of that
/// repository writes, and restarts Pane over the records: the read-back
/// is what a hand install from `repository` leaves Pane with.
fn installed_from(dirs: &Dirs, repository: &str, url: &str) -> Launcher {
    let path = dirs.packages_dir().join("installed.json");
    let mut registry = read_registry(dirs);
    let record = registry["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record.get("git").is_some())
        .expect("the Git package's record");
    record["git"] = json!(repository);
    record["gitUrl"] = json!(url);
    fs::write(&path, registry.to_string()).unwrap();
    dirs.launcher()
}

/// The titles of the installed packages that are one of Pane's own.
fn official_titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .packages()
        .into_iter()
        .filter(|package| launcher.extension_is_official(&package.identity))
        .map(|package| package.title().to_owned())
        .collect()
}

#[test]
fn a_default_extension_is_one_of_panes_own() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());

    // Acquired by Pane at first setup, with the default extension's own
    // identity: official (ADR 0045).
    let packages = launcher.packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].title(), "Rust sample");
    assert_eq!(packages[0].identity.key(), "default:sample-rust");
    assert!(launcher.extension_is_official(&packages[0].identity));
    assert_eq!(official_titles(&launcher), ["Rust sample"]);
}

#[test]
fn a_package_from_a_pane_app_repository_is_official_installed_by_hand_or_by_pane() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("greeter");
    repo.commit(&greeter_files(&guests(), false), "Greeter 0.1.0 source");
    repo.git(&["switch", "--quiet", "-c", "release"]);
    repo.commit(&greeter_files(&guests(), true), "Release 0.1.0");
    repo.tag("v0.1.0");
    repo.git(&["switch", "--quiet", "main"]);
    let launcher = dirs.launcher();

    // Installed by hand from this repository — on another host, this
    // computer's server here: not official.
    block_on(launcher.install_git(&format!("{url}@v0.1.0")));
    let packages = launcher.packages();
    assert_eq!(packages.len(), 1);
    let repository = packages[0]
        .identity
        .git_repository()
        .expect("a Git package");
    assert!(repository.starts_with("127.0.0.1:"), "{repository}");
    assert!(!launcher.extension_is_official(&packages[0].identity));
    assert_eq!(git_record(&dirs)["git"], repository);
    drop(launcher);

    // The same package given the record an install from the pane-app
    // organization writes — as Pane's own first setup writes it and as
    // the user installing that repository by hand does: official, read
    // back over a restart.
    let launcher = installed_from(
        &dirs,
        "github.com/pane-app/greeter",
        "https://github.com/pane-app/greeter.git",
    );
    let packages = launcher.packages();
    assert_eq!(
        packages[0].identity.key(),
        "git:github.com/pane-app/greeter"
    );
    assert!(launcher.extension_is_official(&packages[0].identity));
    drop(launcher);

    // Another owner on GitHub is not the pane-app organization; neither
    // is the pane-app path on another host, nor the organization's own
    // page, which names no repository.
    for (repository, url) in [
        ("github.com/vu/greeter", "https://github.com/vu/greeter.git"),
        (
            "gitlab.com/pane-app/greeter",
            "https://gitlab.com/pane-app/greeter.git",
        ),
        ("github.com/pane-app", "https://github.com/pane-app"),
    ] {
        let launcher = installed_from(&dirs, repository, url);
        let packages = launcher.packages();
        assert_eq!(packages[0].identity.key(), format!("git:{repository}"));
        assert!(
            !launcher.extension_is_official(&packages[0].identity),
            "{repository} is not the pane-app organization"
        );
        drop(launcher);
    }
}

#[test]
fn packages_from_npm_and_a_folder_are_not_official() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();

    // The npm sample, from the local registry: not official.
    let tarball = pack(&npm_registry::greeter_files(&guests(), "0.1.0"));
    dirs.registry.publish(GREETER, "0.1.0", tarball);
    block_on(launcher.install_npm(GREETER));
    let packages = launcher.packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].identity.key(), format!("npm:{GREETER}"));
    assert!(!launcher.extension_is_official(&packages[0].identity));

    // The same files as a folder package: a local copy is another
    // package, and not an official one.
    let folder = dirs.greeter_folder();
    block_on(launcher.install_package(&folder));
    let packages = launcher.packages();
    assert_eq!(packages.len(), 2);
    assert_eq!(
        packages[1].identity,
        PackageIdentity::local(&folder).unwrap()
    );
    assert!(!launcher.extension_is_official(&packages[1].identity));
    assert!(official_titles(&launcher).is_empty());
}
