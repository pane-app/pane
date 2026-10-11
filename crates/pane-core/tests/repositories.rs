//! Installing extension packages from Git repositories through the
//! launcher's public interface. Each test makes its repositories with the
//! `git` program and serves them over Git's smart HTTP protocol from
//! 127.0.0.1 (`support/repo_server.rs`): nothing here reaches the network.
//! The Git sample `cargo xtask guests` assembles (`target/guests/git/greeter`,
//! from `guests/git/greeter`) is the package: a Rust command and a `greet`
//! operation, whose answers name the Git repository.
//!
//! The controlled repository: `main` holds the sample's source only (a
//! source-only revision, which Pane explains and does not install); the
//! branch `release` adds the built component under `dist/`, and its commit
//! is tagged `v0.1.0` (a release revision).
//!
//! What is checked: the preview before anything is installed; installing
//! and running its command; tracked branches and pinned tags and commits;
//! the repository (in any of its equivalent forms) as the identity, so that
//! a second install is refused and choosing it again offers Update, while a
//! local copy of the same code is another package; Git dependencies of a
//! local package; and every way a revision is refused, from a missing
//! repository to a tree holding a link.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock as _, ManualClock, SystemClock};
use pane_core::{ChoiceOutcome, Launcher, PackageIdentity, Runtime, SavedData, Screen, Status};
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/unreachable.rs"]
mod unreachable;

use repo_server::{
    Mode, Repo, Server, collection_files, extension_collection_files, greeter_files,
};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::{guest_file as guest, guests};
use rows::{select_title, titles};

/// The controlled repository's commits.
struct Greeter {
    repo: Repo,
    /// The address it is served at, `http://127.0.0.1:<port>/greeter.git`.
    url: String,
    /// `main`: the source only.
    source: String,
    /// `release`, tagged `v0.1.0`: with the built component.
    release: String,
}

/// The controlled collection's commits: the Git sample as one extension,
/// `clock`, of the repository served as `tools`.
struct Tools {
    /// The repository itself, for the tests that configure it or read its
    /// object ids (the blobs a partial fetch was to ask for, #311).
    repo: Repo,
    /// The address it is served at, `http://127.0.0.1:<port>/tools.git`.
    url: String,
    /// `main`: the extension's source only.
    source: String,
    /// `release`, tagged `v0.1.0`: with the built component.
    release: String,
}

struct Dirs {
    sources: TempDir,
    data: TempDir,
    repos: TempDir,
    runtime: Runtime,
    server: Server,
    /// Frozen, so the launcher's background updater never checks on its
    /// own. A launcher of these tests is on the system clock otherwise,
    /// and its first check — a second after the launcher was built, while
    /// the test is fetching from and moving the same test repositories —
    /// can race the test's own update and preview (run 36829978244's
    /// Windows leg: the user-chosen Update was refused with "Greeter
    /// from Git is updating", and a preview fetch failed on the server
    /// at the same time).
    clock: Arc<ManualClock>,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            repos: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
            server: Server::start(),
            clock: ManualClock::at(SystemClock.now()),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder; a new one is a restart of Pane.
    /// It runs on the frozen clock, so nothing happens in the background
    /// that the test did not ask for.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_clock(self.clock.clone())
    }

    /// A new repository served as `name`.
    fn repo(&self, name: &str) -> (Repo, String) {
        let repo = Repo::init(&self.repos.path().join(name), self.server.home());
        let url = self.server.serve(name, &repo);
        (repo, url)
    }

    /// The controlled repository of the Git sample, served as `greeter`.
    fn greeter(&self) -> Greeter {
        let (repo, url) = self.repo("greeter");
        let source = repo.commit(&greeter_files(&guests(), false), "Greeter 0.1.0 source");
        repo.git(&["switch", "--quiet", "-c", "release"]);
        let release = repo.commit(&greeter_files(&guests(), true), "Release 0.1.0");
        repo.tag("v0.1.0");
        repo.git(&["switch", "--quiet", "main"]);
        Greeter {
            repo,
            url,
            source,
            release,
        }
    }

    /// The controlled collection, served as `tools`: the Git sample as its
    /// extension `clock` (ADR 0044), `main` holding the extension's source
    /// only and the branch `release`, tagged `v0.1.0`, adding its built
    /// component.
    fn collection(&self) -> Tools {
        let (repo, url) = self.repo("tools");
        let source = repo.commit(
            &collection_files(&guests(), INDEX, false),
            "Clock 0.1.0 source",
        );
        repo.git(&["switch", "--quiet", "-c", "release"]);
        let release = repo.commit(&collection_files(&guests(), INDEX, true), "Release 0.1.0");
        repo.tag("v0.1.0");
        repo.git(&["switch", "--quiet", "main"]);
        Tools {
            repo,
            url,
            source,
            release,
        }
    }

    /// The controlled collection of several extensions, served as `tools`
    /// (#308): `extensions` names each by its id and by whether that one
    /// ships its built component on the branch `release`, tagged `v0.1.0`
    /// — `main` holds every extension's source only — each manifest
    /// saying its own title, description, version and (the first one) an
    /// icon, as the choice's rows read them.
    fn several(&self, extensions: &[(&'static str, bool)]) -> Tools {
        let (repo, url) = self.repo("tools");
        let index = index_of(extensions);
        let source_only: Vec<(&'static str, bool)> =
            extensions.iter().map(|(id, _)| (*id, false)).collect();
        let mut source_files = extension_collection_files(&guests(), &source_only);
        source_files.push(("pane-collection.json", index.as_bytes().to_vec()));
        // The collection's own README: a file of the repository no
        // extension of it needs, which a partial fetch never asks for
        // (#311) and no managed copy ever holds.
        source_files.push(("README.md", b"The collection".to_vec()));
        let source = repo.commit(&source_files, "Collection 0.1.0 source");
        repo.git(&["switch", "--quiet", "-c", "release"]);
        let mut release_files = extension_collection_files(&guests(), extensions);
        release_files.push(("pane-collection.json", index.as_bytes().to_vec()));
        release_files.push(("README.md", b"The collection".to_vec()));
        let release = repo.commit(&release_files, "Release 0.1.0");
        repo.tag("v0.1.0");
        repo.git(&["switch", "--quiet", "main"]);
        Tools {
            repo,
            url,
            source,
            release,
        }
    }

    /// The controlled collection, served as `tools`, whose release branch
    /// holds a second release, `v0.2.0`, that renamed its extension
    /// `clock` to `time` (#310, ADR 0044): that revision's index lists
    /// `time` at `extensions/time` with `clock` mapped to it, so an
    /// address — or a dependency's source — naming the old id resolves
    /// to the current extension, which installs under its current id.
    /// `main` and the release `v0.1.0` are as [`Dirs::collection`]'s, the
    /// two releases standing side by side as an evolving collection's do.
    fn renamed(&self) -> Tools {
        let (repo, url) = self.repo("tools");
        let source = repo.commit(
            &collection_files(&guests(), INDEX, false),
            "Clock 0.1.0 source",
        );
        repo.git(&["switch", "--quiet", "-c", "release"]);
        let release = repo.commit(&collection_files(&guests(), INDEX, true), "Release 0.1.0");
        repo.tag("v0.1.0");
        repo.commit(&renamed_collection_files(&guests()), "Rename clock to time");
        repo.tag("v0.2.0");
        repo.git(&["switch", "--quiet", "main"]);
        Tools {
            repo,
            url,
            source,
            release,
        }
    }

    /// The identity of the repository served as `name`.
    fn identity(&self, name: &str) -> String {
        let host = self.server.url().trim_start_matches("http://");
        format!("git:{host}{name}")
    }

    /// The record of the package from the repository served as `name`.
    fn record(&self, name: &str) -> Value {
        self.record_of(&self.identity(name)["git:".len()..])
    }

    /// The record of the extension `id` of the collection served as `name`.
    fn collection_record(&self, name: &str, id: &str) -> Value {
        self.record_of(&format!("{}#{id}", &self.identity(name)["git:".len()..]))
    }

    /// The record of the package whose Git source is `git`, as
    /// `installed.json` writes it.
    fn record_of(&self, git: &str) -> Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["git"] == git)
            .cloned()
            .unwrap_or_else(|| panic!("no record of {git} in {registry:#}"))
    }

    /// The record of the local package at `folder`, as `installed.json`
    /// writes it (the folder as Pane resolves it: macOS reports
    /// `/private/var/...` for `/var/...`).
    fn local_record(&self, folder: &Path) -> Value {
        let local = PackageIdentity::local(folder)
            .unwrap()
            .local_folder()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["local"] == local.as_str())
            .cloned()
            .unwrap_or_else(|| panic!("no record of {local} in {registry:#}"))
    }

    /// Writes package "Caller" in source folder `caller`, calling `greet`
    /// through `dependencies` (JSON array contents).
    fn caller(&self, dependencies: &str) -> PathBuf {
        let folder = self.sources.path().join("caller");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("sample_dependencies.wasm"),
            folder.join("caller.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{ "manifestVersion": 1, "title": "Caller", "apiVersion": "0.1",
                 "commands": [{{ "id": "greet", "title": "Greet through dependencies",
                                 "component": "caller.wasm" }}],
                 "dependencies": [{dependencies}] }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Waits until nothing downloaded is left in Pane's downloads folder,
    /// which is emptied in the background once an install ends.
    fn wait_for_no_downloads(&self) {
        let downloads = self.packages_dir().join("downloads");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left: Vec<_> = fs::read_dir(&downloads)
                .map(|entries| entries.map(|e| e.unwrap().file_name()).collect())
                .unwrap_or_default();
            if left.is_empty() {
                return;
            }
            assert!(Instant::now() < deadline, "downloads left: {left:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn details(launcher: &Launcher) -> Vec<String> {
    launcher.view().details().to_vec()
}

/// The index of the controlled collection (ADR 0044): one extension,
/// `clock`, at `extensions/clock`.
const INDEX: &str = r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" } ] }"#;

/// The index of a collection offering the extensions `extensions` names,
/// each at `extensions/<id>`.
fn index_of(extensions: &[(&'static str, bool)]) -> String {
    let entries: Vec<String> = extensions
        .iter()
        .map(|(id, _)| format!(r#"{{ "id": "{id}", "path": "extensions/{id}" }}"#))
        .collect();
    format!(r#"{{ "extensions": [{}] }}"#, entries.join(", "))
}

/// The line the choice's details end with (#308): how the list is used.
const HOW: &str = "Tick the extensions to install: each one installs on its own, with its own \
                   preview and record";

/// Writes a local collection at `folder` from the files `files`.
fn write_collection(folder: &Path, files: Vec<(&'static str, Vec<u8>)>) {
    for (path, contents) in files {
        let path = folder.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

/// The files of a collection whose index renamed its extension `clock`
/// to `time` (ADR 0044, #310), as [`Dirs::renamed`] commits them: the
/// index listing `time` at `extensions/time` with `clock` mapped to it,
/// and the Git sample's package — titled "Clock from Git", as `clock`'s
/// was: a renamed id keeps the extension — under that folder, with its
/// built component. A local collection that renamed one is written from
/// the same files ([`write_collection`]).
fn renamed_collection_files(guests: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let sample = guests.join("git/greeter");
    let read = |file: &str| {
        fs::read(sample.join(file)).unwrap_or_else(|error| {
            panic!("{}: {error}; run `cargo xtask guests`", sample.display())
        })
    };
    let manifest = String::from_utf8(read("pane.json"))
        .unwrap()
        .replace("Greeter from Git", "Clock from Git");
    let index = r#"{ "extensions": [ { "id": "time", "path": "extensions/time" } ],
        "renamed": { "clock": "time" } }"#;
    vec![
        ("pane-collection.json", index.as_bytes().to_vec()),
        ("extensions/time/pane.json", manifest.into_bytes()),
        (
            "extensions/time/dist/git_greeter.wasm",
            read("dist/git_greeter.wasm"),
        ),
    ]
}

fn has(details: &[String], line: &str) -> bool {
    details.iter().any(|detail| detail == line)
}

fn installed(launcher: &Launcher) -> Vec<String> {
    launcher.packages().iter().map(|p| p.title()).collect()
}

/// Opens the command titled `command` from root search and runs its item
/// titled `item`, returning what it showed: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn error_of(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(text) => text,
        other => panic!("not an error: {other:?}"),
    }
}

fn short(commit: &str) -> &str {
    &commit[..12]
}

const HELLO: &str = "Hello from the Git repository";

#[test]
fn a_release_tag_is_previewed_installed_and_its_command_runs() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();
    let asked = format!("{}@v0.1.0", greeter.url);

    block_on(launcher.preview_git(&asked));

    assert_eq!(launcher.view().title, "Greeter from Git");
    let details = details(&launcher);
    let expected = [
        format!("Source: Git repository {}", &dirs.identity("greeter")[4..]),
        "Version: 0.1.0".into(),
        "Revision: tag v0.1.0, which you named: installing pins it to that revision".into(),
        format!(
            "Fetched: commit {} “Release 0.1.0”, served at {}; each object checked against its id",
            greeter.release, greeter.url
        ),
        "Pane builds nothing and runs no repository hooks, scripts or submodules".into(),
        "Commands: Greeter from Git".into(),
        "Operations: greet (version 1)".into(),
    ];
    for line in &expected {
        assert!(has(&details, line), "{line:?} not in {details:#?}");
    }
    assert_eq!(titles(&launcher), ["Install"]);
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from Git".into())
    );
    let package = &launcher.packages()[0];
    assert_eq!(package.identity.key(), dirs.identity("greeter"));
    let record = dirs.record("greeter");
    assert_eq!(record["gitUrl"], greeter.url.as_str());
    assert_eq!(record["gitRef"], "refs/tags/v0.1.0");
    assert_eq!(record["gitCommit"], greeter.release.as_str());
    assert_eq!(record["pinned"], true);
    assert_eq!(
        run(&launcher, "Greeter from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    // Only the manifest and its component are kept: not the source.
    dirs.wait_for_no_downloads();
    let mut files: Vec<String> = Vec::new();
    for entry in walk(&package.location) {
        files.push(
            entry
                .strip_prefix(&package.location)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    files.sort();
    assert_eq!(files, ["dist/git_greeter.wasm", "pane.json"]);

    // After a restart it is listed from its managed copy, with nothing
    // fetched again, the server gone.
    let requests = dirs.server.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    assert_eq!(installed(&launcher), ["Greeter from Git"]);
    assert_eq!(
        run(&launcher, "Greeter from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    assert_eq!(dirs.server.requests().len(), requests);

    // It has no source folder: no Reload or Develop rows.
    launcher.back();
    select_title(&launcher, "Manage Extensions");
    block_on(launcher.activate_selected());
    let titles = titles(&launcher);
    assert!(
        titles.contains(&"Uninstall Greeter from Git".to_owned()),
        "{titles:?}"
    );
    assert!(
        !titles
            .iter()
            .any(|t| t.starts_with("Reload ") || t.starts_with("Develop ")),
        "{titles:?}"
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files.extend(walk(&entry.path()));
        } else {
            files.push(entry.path());
        }
    }
    files
}

#[test]
fn a_source_only_revision_is_explained_and_nothing_is_installed() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();

    // The default branch holds the source only.
    block_on(launcher.preview_git(&greeter.url));
    let error = error_of(&launcher);
    assert_eq!(
        error,
        format!(
            "The default branch, main (commit {}) of the Git repository {} holds only the \
             source of \"Greeter from Git\": its built component dist/git_greeter.wasm is not in \
             it. Pane does not build packages from Git or run anything in a repository; install \
             a release revision whose commit includes the built components (its author's \
             release tag or branch), or build it yourself and install the folder",
            short(&greeter.source),
            &dirs.identity("greeter")[4..]
        )
    );
    assert!(titles(&launcher).is_empty());
    assert_eq!(
        launcher.view().title,
        format!("Cannot install {}", &dirs.identity("greeter")[4..])
    );
    dirs.wait_for_no_downloads();
    // Nor as an explicit install.
    block_on(launcher.install_git(&greeter.url));
    assert!(error_of(&launcher).contains("holds only the source"));
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn a_branch_is_tracked_and_a_tag_or_commit_is_pinned() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@release", greeter.url)));
    assert!(has(
        &details(&launcher),
        "Revision: branch release, tracked: an update fetches that branch again"
    ));
    block_on(launcher.activate_selected());
    let record = dirs.record("greeter");
    assert_eq!(record["gitRef"], "refs/heads/release");
    assert_eq!(record.get("pinned"), None);

    // The branch moves on; choosing the repository again, without a
    // reference, fetches the branch it tracks.
    greeter.repo.git(&["switch", "--quiet", "release"]);
    let moved = greeter
        .repo
        .commit(&[("NOTES.md", b"moved".to_vec())], "Release 0.1.1");
    greeter.repo.git(&["switch", "--quiet", "main"]);
    block_on(launcher.preview_git(&greeter.url));
    let details = self::details(&launcher);
    assert!(
        has(
            &details,
            &format!(
                "Installed: branch release (commit {}) of this repository",
                short(&greeter.release)
            )
        ),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Update"]);
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some(
            format!(
                "Replace the installed copy with branch release (commit {}), tracked",
                short(&moved)
            )
            .as_str()
        )
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Greeter from Git to 0.1.0".into())
    );
    assert_eq!(dirs.record("greeter")["gitCommit"], moved.as_str());

    // A commit named by its id pins it; without a reference it is kept.
    block_on(launcher.preview_git(&format!("{}@{}", greeter.url, greeter.release)));
    assert!(has(
        &self::details(&launcher),
        &format!(
            "Revision: commit {}, which you named: installing pins it to that revision",
            short(&greeter.release)
        )
    ));
    block_on(launcher.activate_selected());
    let record = dirs.record("greeter");
    assert_eq!(
        (
            &record["gitCommit"],
            &record["pinned"],
            record.get("gitRef")
        ),
        (&json!(greeter.release), &json!(true), None)
    );
    block_on(launcher.preview_git(&greeter.url));
    assert!(has(
        &self::details(&launcher),
        &format!(
            "Revision: commit {}, which it is pinned to: name another branch, tag or commit to \
             change it",
            short(&greeter.release)
        )
    ));

    // A tag, as `refs/tags/…` too.
    block_on(launcher.preview_git(&format!("{}@refs/tags/v0.1.0", greeter.url)));
    block_on(launcher.activate_selected());
    let record = dirs.record("greeter");
    assert_eq!(
        (&record["gitRef"], &record["pinned"]),
        (&json!("refs/tags/v0.1.0"), &json!(true))
    );
    assert_eq!(installed(&launcher), ["Greeter from Git"]);
}

/// The preview's caution about a commit named by its id that no branch or
/// tag of `repository` points to.
fn unadvertised(repository: &str, commit: &str) -> String {
    format!(
        "Caution: no branch or tag of {repository} points to commit {}. A host that shares \
         storage between forks, as GitHub does, can serve a fork's or a pull request's commit at \
         this address, so its id alone does not show that this repository made it",
        short(commit)
    )
}

impl Greeter {
    /// Makes a commit on top of the release that only a pull request's
    /// reference holds, as a fork's commit is served from the repository it
    /// was proposed to; its id.
    fn proposed(&self) -> String {
        self.repo
            .git(&["switch", "--quiet", "-c", "proposed", "release"]);
        let proposed = self
            .repo
            .commit(&[("NOTES.md", b"proposed".to_vec())], "Proposed");
        self.repo.git(&["switch", "--quiet", "main"]);
        self.repo
            .git(&["update-ref", "refs/pull/1/head", &proposed]);
        self.repo.git(&["branch", "--quiet", "-D", "proposed"]);
        proposed
    }
}

#[test]
fn a_commit_no_branch_or_tag_points_to_is_previewed_with_a_caution() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let repository = &dirs.identity("greeter")[4..];
    let proposed = greeter.proposed();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@{proposed}", greeter.url)));
    let details = details(&launcher);
    assert!(
        has(&details, &unadvertised(repository, &proposed)),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Install"]);

    // A commit a branch or a tag points to (here both) has no caution
    // about its commit (the sample has no icon, which is cautioned about
    // apart, #139).
    let unadvertised_caution = |line: &String| line.starts_with("Caution: no branch or tag");
    block_on(launcher.preview_git(&format!("{}@{}", greeter.url, greeter.release)));
    let details = self::details(&launcher);
    assert!(!details.iter().any(unadvertised_caution), "{details:#?}");
    // Nor does a tag or a branch, whose commit the server itself named.
    block_on(launcher.preview_git(&format!("{}@v0.1.0", greeter.url)));
    let details = self::details(&launcher);
    assert!(!details.iter().any(unadvertised_caution), "{details:#?}");
}

#[test]
fn a_commit_is_cautioned_about_when_the_reference_listing_is_too_long_to_read() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let repository = &dirs.identity("greeter")[4..];
    dirs.server.set_mode(Mode::LongListing);
    let launcher = dirs.launcher();

    // The listing only tells whether a branch or tag points to the commit:
    // one too long to read is taken as saying none does.
    block_on(launcher.preview_git(&format!("{}@{}", greeter.url, greeter.release)));
    let details = details(&launcher);
    assert!(
        has(&details, &unadvertised(repository, &greeter.release)),
        "{details:#?} {:?}",
        launcher.view().status
    );
    assert_eq!(titles(&launcher), ["Install"]);

    // A tag cannot be resolved without it.
    let error = refusal(&launcher, &format!("{}@v0.1.0", greeter.url));
    assert!(
        error.starts_with(&format!("Could not list the references of {repository}: ")),
        "{error}"
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn equivalent_addresses_are_one_package_and_a_local_copy_is_another() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();
    block_on(launcher.install_git(&format!("{}@v0.1.0", greeter.url)));
    assert_eq!(installed(&launcher), ["Greeter from Git"]);

    // Without `.git`, with `git:` before it, with a trailing `/`: the same
    // repository, whatever the reference.
    let without = greeter.url.trim_end_matches(".git").to_owned();
    block_on(launcher.install_git(&format!("git:{without}/@release")));
    assert_eq!(
        error_of(&launcher),
        format!(
            "Already installed from Git repository {}; use Update to replace the installed copy",
            &dirs.identity("greeter")[4..]
        )
    );
    block_on(launcher.preview_git(&format!("{without}@v0.1.0")));
    assert_eq!(titles(&launcher), ["Update"]);

    // The same code from a folder is another package, installed beside it.
    let folder = dirs.sources.path().join("greeter");
    for (path, contents) in greeter_files(&guests(), true) {
        let path = folder.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    block_on(launcher.install_package(&folder));
    assert_eq!(
        installed(&launcher),
        ["Greeter from Git", "Greeter from Git"]
    );
    let identities: Vec<String> = launcher
        .packages()
        .iter()
        .map(|p| p.identity.key())
        .collect();
    assert_eq!(identities[0], dirs.identity("greeter"));
    assert!(identities[1].starts_with("local:"), "{identities:?}");
    // Uninstalling the local copy leaves the Git one, which still runs.
    let local = launcher.packages()[1].identity.clone();
    block_on(launcher.uninstall(&local, SavedData::Delete));
    assert_eq!(installed(&launcher), ["Greeter from Git"]);
    assert_eq!(
        run(&launcher, "Greeter from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
}

#[test]
fn on_a_host_that_ignores_case_another_spelling_is_the_same_package() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    // This server stands for github.com, which serves a repository at any
    // case of its path: it serves the repository under both spellings.
    let host = dirs
        .server
        .url()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    pane_core::git::ignore_case_on_host_for_tests(host);
    let upper = dirs.server.serve("Greeter", &greeter.repo);
    let launcher = dirs.launcher();

    block_on(launcher.install_git(&format!("{upper}@v0.1.0")));
    assert_eq!(installed(&launcher), ["Greeter from Git"]);
    assert_eq!(
        launcher.packages()[0].identity.key(),
        dirs.identity("greeter")
    );
    // Fetched as written.
    assert_eq!(dirs.record("greeter")["gitUrl"], upper.as_str());

    // The other spelling is the installed package.
    block_on(launcher.install_git(&format!("{}@release", greeter.url)));
    assert_eq!(
        error_of(&launcher),
        format!(
            "Already installed from Git repository {}; use Update to replace the installed copy",
            &dirs.identity("greeter")[4..]
        )
    );
    let shouted = dirs.server.serve("GREETER", &greeter.repo);
    block_on(launcher.preview_git(&format!("{shouted}@v0.1.0")));
    assert_eq!(
        titles(&launcher),
        ["Update"],
        "{:?}",
        launcher.view().status
    );

    // A dependency spelled in another case resolves to the installed copy.
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "git:{}@v0.1.0",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#,
        greeter.url.replace("/greeter.git", "/Greeter")
    ));
    let requests = dirs.server.requests().len();
    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    assert!(
        has(&details, "Requires: Greeter from Git, already installed"),
        "{details:#?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Greeter from Git", "Caller"]);
    assert_eq!(
        dirs.server.requests().len(),
        requests,
        "nothing fetched again"
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
}

#[test]
fn a_local_package_requiring_a_git_package_installs_it_and_calls_it_by_id() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let source = format!("git:{}@v0.1.0", greeter.url);
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "{source}",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    assert!(
        has(
            &details,
            &format!("Requires: Greeter from Git, installed with it from {source}")
        ),
        "{details:#?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Greeter from Git", "Caller"]);
    assert_eq!(dirs.record("greeter")["pinned"], true);
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn a_dependency_on_a_commit_no_branch_or_tag_points_to_is_previewed_with_a_caution() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let repository = &dirs.identity("greeter")[4..];
    let proposed = greeter.proposed();
    let source = format!("git:{}@{proposed}", greeter.url);
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "{source}",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    let requires = format!("Requires: Greeter from Git, installed with it from {source}");
    let caution =
        unadvertised(repository, &proposed).replacen("Caution:", "Caution (Greeter from Git):", 1);
    let at = details.iter().position(|line| *line == requires);
    assert!(
        at.is_some_and(|at| details.get(at + 1) == Some(&caution)),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Install"]);

    // A dependency on a commit a tag points to has none.
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "git:{}@{}",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#,
        greeter.url, greeter.release
    ));
    block_on(launcher.preview_package(&folder));
    let details = self::details(&launcher);
    assert!(
        !details.iter().any(|line| line.starts_with("Caution")),
        "{details:#?}"
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn a_dependency_naming_another_revision_than_the_installed_one_is_a_conflict() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    // The branch moves on past the tag.
    greeter.repo.git(&["switch", "--quiet", "release"]);
    let moved = greeter
        .repo
        .commit(&[("NOTES.md", b"moved".to_vec())], "Release 0.1.1");
    greeter.repo.git(&["switch", "--quiet", "main"]);
    let launcher = dirs.launcher();
    block_on(launcher.install_git(&format!("{}@release", greeter.url)));

    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "git:{}@v0.1.0",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#,
        greeter.url
    ));
    block_on(launcher.preview_package(&folder));
    let repository = &dirs.identity("greeter")[4..];
    assert_eq!(
        error_of(&launcher),
        format!(
            "Nothing was installed: Caller requires Greeter from Git at v0.1.0, and branch \
             release (commit {}) is installed; Pane does not replace the installed copy while \
             installing another extension: update it to v0.1.0 (Git repository \
             {repository}@v0.1.0) if Caller needs that revision",
            short(&moved)
        )
    );
    assert!(titles(&launcher).is_empty());

    // Naming the branch the installed copy tracks is no conflict.
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "git:{}@release",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#,
        greeter.url
    ));
    block_on(launcher.preview_package(&folder));
    assert_eq!(titles(&launcher), ["Install"]);
}

#[test]
fn a_git_package_naming_a_local_folder_is_refused() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("caller");
    let manifest = r#"{ "manifestVersion": 1, "title": "Caller", "apiVersion": "0.1",
        "commands": [{ "id": "greet", "title": "Greet", "component": "caller.wasm" }],
        "dependencies": [{ "id": "helper", "source": "local:../helper",
                           "operations": [{ "id": "greet", "version": 1 }] }] }"#;
    repo.commit(
        &[
            ("pane.json", manifest.as_bytes().to_vec()),
            (
                "caller.wasm",
                fs::read(guest("sample_dependencies.wasm")).unwrap(),
            ),
        ],
        "Caller",
    );
    let launcher = dirs.launcher();
    block_on(launcher.preview_git(&url));
    assert_eq!(
        error_of(&launcher),
        "Nothing was installed: Caller comes from Git but names the local folder \
         `local:../helper` as its dependency `helper`; a package published to npm or Git can \
         depend only on packages from npm or Git"
    );
}

#[test]
fn a_tree_holding_a_link_or_a_submodule_is_refused() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();
    greeter.repo.git(&["switch", "--quiet", "release"]);
    greeter.repo.add_entry("120000", "/etc/passwd", "dist/link");
    greeter.repo.git(&["commit", "--quiet", "-m", "A link"]);
    greeter.repo.git(&["tag", "with-link"]);
    greeter
        .repo
        .git(&["rm", "--quiet", "--cached", "dist/link"]);
    greeter
        .repo
        .add_entry("160000", &greeter.source, "vendor/other");
    greeter
        .repo
        .git(&["commit", "--quiet", "-m", "A submodule"]);
    greeter.repo.git(&["tag", "with-submodule"]);
    greeter.repo.git(&["switch", "--quiet", "main"]);

    block_on(launcher.preview_git(&format!("{}@with-link", greeter.url)));
    let error = error_of(&launcher);
    assert!(
        error.ends_with(
            "cannot be installed safely: its tree contains `dist/link`, a symbolic link; Pane \
             takes only files and folders every system can write"
        ),
        "{error}"
    );
    block_on(launcher.preview_git(&format!("{}@with-submodule", greeter.url)));
    let error = error_of(&launcher);
    assert!(
        error.ends_with(
            "its tree contains `vendor/other`, a submodule, which Pane does not fetch; Pane \
             takes only files and folders every system can write"
        ),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

/// Every file and folder under `dir` whose name starts with `escaped`.
fn escaped_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with("escaped") {
            found.push(entry.path());
        }
        if entry.file_type().unwrap().is_dir() {
            found.extend(escaped_under(&entry.path()));
        }
    }
    found
}

#[test]
fn a_tree_entry_naming_another_folder_writes_nothing_outside_its_download() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("hostile");
    let manifest: &[u8] = br#"{ "manifestVersion": 1, "title": "Hostile", "apiVersion": "0.1",
        "commands": [] }"#;
    // Names `git` refuses to put in a tree, made directly: each would be
    // written above the download (`..`), anywhere (an absolute path replaces
    // the folder it is joined to) or in a folder of the tree's choosing.
    let absolute = dirs.sources.path().join("escaped-absolute");
    let absolute = absolute.to_string_lossy().into_owned();
    let hostile: Vec<(&str, String)> = vec![
        ("100644", "../escaped-up".into()),
        ("100644", "../../escaped-two-up".into()),
        ("100644", "../../../escaped-three-up".into()),
        ("100644", absolute.clone()),
        ("40000", "../escaped-folder".into()),
        ("100644", "dist/escaped-nested".into()),
        ("100644", "..\\escaped-backslash".into()),
        ("100644", "sub/.git".into()),
    ];
    let launcher = dirs.launcher();
    for (i, (mode, name)) in hostile.iter().enumerate() {
        let tag = format!("hostile-{i}");
        repo.commit_raw_tree(
            &[
                ("100644", b"pane.json", manifest),
                (mode, name.as_bytes(), b"written by a hostile tree"),
            ],
            &tag,
        );
        let error = refusal(&launcher, &format!("{url}@{tag}"));
        assert!(
            error.contains(&format!(
                "cannot be installed safely: its tree contains `{name}`"
            )),
            "{name}: {error}"
        );
    }
    // Nothing was written anywhere the test can see: not in the data
    // folder, not beside it, not where the absolute name pointed.
    for dir in [
        dirs.data.path(),
        dirs.sources.path(),
        dirs.repos.path(),
        dirs.data.path().parent().unwrap(),
    ] {
        assert_eq!(
            escaped_under(dir),
            Vec::<PathBuf>::new(),
            "{}",
            dir.display()
        );
    }
    assert!(!Path::new(&absolute).exists());
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn nothing_in_the_repository_runs_and_its_files_are_taken_as_committed() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("greeter");
    // Attributes asking for a filter and line-ending conversion, a hook
    // and an install script: Pane uses no Git configuration or program, so
    // none applies, and the component is exactly the committed bytes.
    let marker = dirs.sources.path().join("ran");
    let script = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    let mut files = greeter_files(&guests(), true);
    files.push((
        ".gitattributes",
        b"*.wasm filter=evil\n*.md text eol=crlf\n".to_vec(),
    ));
    files.push((".githooks/post-checkout", script.clone().into_bytes()));
    files.push((
        "package.json",
        br#"{ "scripts": { "postinstall": "touch ran" } }"#.to_vec(),
    ));
    repo.commit(&files, "Release with extras");
    repo.git(&[
        "config",
        "filter.evil.smudge",
        &format!("sh -c 'touch {}'", marker.display()),
    ]);
    repo.git(&["config", "core.hooksPath", ".githooks"]);
    let launcher = dirs.launcher();

    block_on(launcher.install_git(&url));
    assert_eq!(
        installed(&launcher),
        ["Greeter from Git"],
        "{:?}",
        launcher.view().status
    );
    let copy = &launcher.packages()[0].location;
    assert_eq!(
        fs::read(copy.join("dist/git_greeter.wasm")).unwrap(),
        fs::read(guest("git/greeter/dist/git_greeter.wasm")).unwrap()
    );
    assert_eq!(
        run(&launcher, "Greeter from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    assert!(!marker.exists());
}

/// Why previewing `asked` is refused.
fn refusal(launcher: &Launcher, asked: &str) -> String {
    block_on(launcher.preview_git(asked));
    assert!(titles(launcher).is_empty());
    error_of(launcher)
}

#[test]
fn a_missing_repository_reference_or_commit_is_explained() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    greeter.repo.git(&["branch", "twice"]);
    greeter.repo.git(&["tag", "twice"]);
    let launcher = dirs.launcher();
    let repository = &dirs.identity("greeter")[4..];

    assert_eq!(
        refusal(&launcher, &format!("{}nobody.git", dirs.server.url())),
        format!(
            "There is no Git repository at {}nobody.git",
            dirs.server.url()
        )
    );
    assert_eq!(
        refusal(&launcher, &format!("{}@nope", greeter.url)),
        format!("The Git repository {repository} has no branch or tag named nope")
    );
    assert_eq!(
        refusal(&launcher, &format!("{}@twice", greeter.url)),
        format!(
            "The Git repository {repository} has both a branch and a tag named twice; name the \
             one to install as {repository}@refs/heads/twice or {repository}@refs/tags/twice"
        )
    );
    let missing = "0123456789abcdef0123456789abcdef01234567";
    let error = refusal(&launcher, &format!("{}@{missing}", greeter.url));
    assert!(
        error.starts_with(&format!("Could not fetch commit {missing} of {repository}")),
        "{error}"
    );
    // An address Pane does not fetch is refused before anything is asked.
    let requests = dirs.server.requests().len();
    let host = dirs.server.url().trim_start_matches("http://");
    assert!(refusal(&launcher, &format!("ftp://{host}greeter")).contains("does not use `ftp://`"));
    assert!(
        refusal(&launcher, "http://localhost:1/greeter").contains("only over HTTPS"),
        "a name is not a loopback address"
    );
    assert_eq!(dirs.server.requests().len(), requests);
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn servers_that_redirect_ask_to_sign_in_or_speak_an_older_protocol_are_explained() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();
    let asked = format!("{}@v0.1.0", greeter.url);
    let repository = &dirs.identity("greeter")[4..];

    dirs.server.set_mode(Mode::Redirect);
    assert_eq!(
        refusal(&launcher, &asked),
        format!(
            "{}/info/refs?service=git-upload-pack answered 301, sending Pane elsewhere: Pane \
             follows no redirect, so name the repository by the address it moved to",
            greeter.url
        )
    );
    dirs.server.set_mode(Mode::SignIn);
    assert_eq!(
        refusal(&launcher, &asked),
        format!(
            "The Git repository {repository} asks to sign in (its server answered 401): Pane \
             sends no credentials, so it installs only from public repositories"
        )
    );
    dirs.server.set_mode(Mode::VersionZero);
    let error = refusal(&launcher, &asked);
    assert!(
        error.starts_with(&format!(
            "The Git repository {repository} is not served with Git's protocol version 2"
        )),
        "{error}"
    );
    // A server announcing the service first, as some do, is understood.
    dirs.server.set_mode(Mode::ServiceLine);
    block_on(launcher.preview_git(&asked));
    assert_eq!(titles(&launcher), ["Install"]);
}

#[test]
fn an_unreachable_server_is_explained() {
    let closed = unreachable::ClosedPort::new();
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let url = format!("{}/greeter.git", closed.url());
    let error = refusal(&launcher, &url);
    let repository = url.trim_start_matches("http://").trim_end_matches(".git");
    assert!(
        error.starts_with(&format!("Could not reach the Git repository {repository}:")),
        "{error}"
    );
}

#[test]
fn the_form_in_root_search_asks_for_the_repository() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let launcher = dirs.launcher();

    select_title(&launcher, "Install extension from Git…");
    block_on(launcher.activate_selected());
    let form = launcher.view().form().cloned().expect("the Git form");
    assert_eq!(launcher.view().title, "Install extension from Git");
    assert_eq!(form.submit_label, "Show package");
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));

    select_title(&launcher, "Install extension from Git…");
    block_on(launcher.activate_selected());
    launcher.set_field_value("repository", &format!(" {}@v0.1.0 ", greeter.url));
    block_on(launcher.submit_form());
    assert_eq!(launcher.view().title, "Greeter from Git");
    block_on(launcher.activate_selected());
    assert_eq!(dirs.record("greeter")["pinned"], true);
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

// ------------------------------------------------------- collections (#307)

/// One extension of a collection, end to end: the preview names the id and
/// everything else a Git package's does, Install records the id beside the
/// Git source fields, only the extension's files reach its managed copy,
/// its command runs, and a second install of the same identity is refused
/// while choosing it again offers Update.
#[test]
fn an_extension_of_a_collection_is_previewed_installed_and_its_command_runs() {
    let dirs = Dirs::new();
    let tools = dirs.collection();
    let launcher = dirs.launcher();
    let asked = format!("{}#clock@v0.1.0", tools.url);

    // The default branch holds the extension's source only.
    let error = refusal(&launcher, &format!("{}#clock", tools.url));
    assert_eq!(
        error,
        format!(
            "The default branch, main (commit {}) of the Git repository {} holds only the \
             source of \"Clock from Git\", the extension `clock` of the collection: its built \
             component dist/git_greeter.wasm is not in it. Pane does not build packages from Git \
             or run anything in a repository; install a release revision whose commit includes \
             the built components (its author's release tag or branch), or build it yourself \
             and install the folder",
            short(&tools.source),
            &dirs.identity("tools")[4..]
        )
    );

    block_on(launcher.preview_git(&asked));

    assert_eq!(launcher.view().title, "Clock from Git");
    let details = details(&launcher);
    let expected = [
        format!(
            "Source: Git repository {}#clock",
            &dirs.identity("tools")[4..]
        ),
        "Extension: clock, one of the extensions its collection lists".into(),
        "Version: 0.1.0".into(),
        "Revision: tag v0.1.0, which you named: installing pins it to that revision".into(),
        format!(
            "Fetched: commit {} “Release 0.1.0”, served at {}; each object checked against its id",
            tools.release, tools.url
        ),
        "Pane builds nothing and runs no repository hooks, scripts or submodules".into(),
        "Commands: Clock from Git".into(),
        "Operations: greet (version 1)".into(),
    ];
    for line in &expected {
        assert!(has(&details, line), "{line:?} not in {details:#?}");
    }
    assert_eq!(titles(&launcher), ["Install"]);
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git".into())
    );
    let package = &launcher.packages()[0];
    assert_eq!(
        package.identity.key(),
        format!("{}#clock", dirs.identity("tools"))
    );
    assert_eq!(package.identity.extension_id(), Some("clock"));
    // The id is recorded beside the Git source fields, and the repository
    // is recorded as the plain address it was fetched from.
    let record = dirs.collection_record("tools", "clock");
    assert_eq!(
        record["git"],
        format!("{}#clock", &dirs.identity("tools")[4..]).as_str()
    );
    assert_eq!(record["gitUrl"], tools.url.as_str());
    assert_eq!(record["gitRef"], "refs/tags/v0.1.0");
    assert_eq!(record["gitCommit"], tools.release.as_str());
    assert_eq!(record["pinned"], true);
    assert_eq!(record["gitExtension"], "clock");
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    // Only the files under the extension's folder reach its managed copy:
    // neither the collection's own files nor the extension's source.
    dirs.wait_for_no_downloads();
    let mut files: Vec<String> = Vec::new();
    for entry in walk(&package.location) {
        files.push(
            entry
                .strip_prefix(&package.location)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    files.sort();
    assert_eq!(files, ["dist/git_greeter.wasm", "pane.json"]);

    // After a restart it is listed from its managed copy, with nothing
    // fetched again, the server gone.
    let requests = dirs.server.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    assert_eq!(installed(&launcher), ["Clock from Git"]);
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    assert_eq!(dirs.server.requests().len(), requests);

    // A second install of the same identity is refused, as today.
    block_on(launcher.install_git(&asked));
    assert_eq!(
        error_of(&launcher),
        format!(
            "Already installed from Git repository {}#clock; use Update to replace the \
             installed copy",
            &dirs.identity("tools")[4..]
        )
    );
    // Choosing it again, without a reference, keeps the revision the
    // extension is installed from, as a repository's own package does.
    block_on(launcher.preview_git(&format!("{}#clock", tools.url)));
    assert_eq!(titles(&launcher), ["Update"]);
}

#[test]
fn a_hash_id_on_a_one_extension_repository_is_refused_and_a_collection_opens_the_choice() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    let tools = dirs.collection();
    let launcher = dirs.launcher();

    // The greeter repository is one extension, with `pane.json` at its
    // root: `#<id>` names no extension of it.
    let error = refusal(&launcher, &format!("{}#anything@v0.1.0", greeter.url));
    assert_eq!(
        error,
        format!(
            "Tag v0.1.0 (commit {}) of the Git repository {} is not a collection: its root \
             holds pane.json, one extension, and no pane-collection.json; `#<id>` names one \
             extension of a collection",
            short(&greeter.release),
            &dirs.identity("greeter")[4..]
        )
    );
    // A repository holding neither file is explained as today.
    let (repo, neither) = dirs.repo("empty");
    repo.commit(&[("README.md", b"nothing".to_vec())], "Nothing");
    let error = refusal(&launcher, &format!("{neither}#anything"));
    assert!(
        error.contains("is not a Pane extension: it has no pane.json at the repository's root"),
        "{error}"
    );

    // The collection, without an id, opens the choice of its extensions
    // (#308) instead of being refused.
    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(titles(&launcher), ["Clock from Git", "Install"]);
    assert!(launcher.packages().is_empty());
    launcher.back();
    dirs.wait_for_no_downloads();
}

#[test]
fn every_way_a_collection_index_is_refused_saying_what_is_wrong() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("tools");
    let launcher = dirs.launcher();
    // One tag per way an index cannot be taken, each holding the
    // collection with its extension's package built, so the index is the
    // only thing wrong with the revision.
    let cases = [
        (
            "unknown-field",
            r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock",
                                   "title": "Clock" } ] }"#,
            "unknown field `title`, expected `id` or `path` at line 2 column 42",
        ),
        (
            "no-extensions",
            r#"{ "renamed": {} }"#,
            "missing field `extensions`",
        ),
        (
            "duplicate-id",
            r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" },
                                 { "id": "clock", "path": "extensions/other" } ] }"#,
            "the extension id `clock` is listed twice",
        ),
        (
            "duplicate-path",
            r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" },
                                 { "id": "timer", "path": "extensions/clock" } ] }"#,
            "the path `extensions/clock` is listed twice",
        ),
        (
            "malformed-id",
            r#"{ "extensions": [ { "id": "Clock", "path": "extensions/clock" } ] }"#,
            "the extension id `Clock` must be lowercase letters, digits and `-`",
        ),
        (
            "reused-id",
            r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" } ],
                 "renamed": { "clock": "timer" } }"#,
            "the extension id `clock` is in its `renamed` map, so it is not one: an id is \
             never reused",
        ),
        (
            "climbing-path",
            r#"{ "extensions": [ { "id": "clock", "path": "../escaped" } ] }"#,
            "the path `../escaped` of `clock` which climbs out with `..`",
        ),
        (
            "backslash-path",
            r#"{ "extensions": [ { "id": "clock", "path": "extensions\\clock" } ] }"#,
            "the path `extensions\\clock` of `clock` whose name holds `/` or `\\`, which \
             would put it in another folder",
        ),
        (
            "device-name",
            r#"{ "extensions": [ { "id": "clock", "path": "con" } ] }"#,
            "the path `con` of `clock` which is a Windows device name",
        ),
    ];
    for (tag, index, why) in cases {
        let commit = repo.commit(&collection_files(&guests(), index, true), tag);
        repo.tag(tag);
        let error = refusal(&launcher, &format!("{url}#clock@{tag}"));
        assert_eq!(
            error,
            format!(
                "Tag {tag} (commit {}) of the Git repository {} is a collection whose \
                 pane-collection.json is invalid: {why}",
                short(&commit),
                &dirs.identity("tools")[4..]
            )
        );
    }
    // A path naming no package.
    let index = r#"{ "extensions": [ { "id": "clock", "path": "extensions/none" } ] }"#;
    let commit = repo.commit(&collection_files(&guests(), index, true), "no-package");
    repo.tag("no-package");
    let error = refusal(&launcher, &format!("{url}#clock@no-package"));
    assert_eq!(
        error,
        format!(
            "Tag no-package (commit {}) of the Git repository {} lists its extension \
             `clock` at `extensions/none`, which is not a package: it has no pane.json",
            short(&commit),
            &dirs.identity("tools")[4..]
        )
    );
    // A root holding both manifests.
    let mut both = collection_files(&guests(), INDEX, true);
    both.push((
        "pane.json",
        fs::read(guests().join("git/greeter/pane.json")).unwrap(),
    ));
    let commit = repo.commit(&both, "both");
    repo.tag("both");
    let error = refusal(&launcher, &format!("{url}#clock@both"));
    assert_eq!(
        error,
        format!(
            "Tag both (commit {}) of the Git repository {} holds both pane.json and \
             pane-collection.json: a repository is one extension or a collection, never both",
            short(&commit),
            &dirs.identity("tools")[4..]
        )
    );
    // An id only the `renamed` map names is not one the collection lists
    // as an extension; following it is #310's.
    let index = r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" } ],
        "renamed": { "old-clock": "clock", "retired": null } }"#;
    let commit = repo.commit(&collection_files(&guests(), index, true), "renamed");
    repo.tag("renamed");
    let error = refusal(&launcher, &format!("{url}#old-clock@renamed"));
    assert_eq!(
        error,
        format!(
            "Tag renamed (commit {}) of the Git repository {} lists no extension \
             `old-clock` in its pane-collection.json",
            short(&commit),
            &dirs.identity("tools")[4..]
        )
    );
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn a_component_outside_the_extension_s_folder_is_explained_as_source_only() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("tools");
    // The built component sits at the collection's root, not under the
    // extension's folder: only the files under that folder are the
    // extension's, so the component it names is missing from it.
    let mut files = collection_files(&guests(), INDEX, false);
    files.push((
        "dist/git_greeter.wasm",
        fs::read(guests().join("git/greeter/dist/git_greeter.wasm")).unwrap(),
    ));
    let release = repo.commit(&files, "Release 0.1.0");
    repo.tag("v0.1.0");
    let launcher = dirs.launcher();

    let error = refusal(&launcher, &format!("{url}#clock@v0.1.0"));
    assert_eq!(
        error,
        format!(
            "Tag v0.1.0 (commit {}) of the Git repository {} holds only the source of \
             \"Clock from Git\", the extension `clock` of the collection: its built component \
             dist/git_greeter.wasm is not in it. Pane does not build packages from Git or run \
             anything in a repository; install a release revision whose commit includes the \
             built components (its author's release tag or branch), or build it yourself and \
             install the folder",
            short(&release),
            &dirs.identity("tools")[4..]
        )
    );
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn the_local_folder_form_installs_one_extension_of_a_collection() {
    let dirs = Dirs::new();
    let tools = dirs.sources.path().join("tools");
    write_collection(&tools, collection_files(&guests(), INDEX, true));
    let folder = PackageIdentity::local(&tools)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();
    let launcher = dirs.launcher();

    // A collection folder picked (or named without an id) opens the
    // choice of its extensions (#308) instead of being refused.
    block_on(launcher.preview_package(&tools));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(launcher.view().title, "tools");
    assert_eq!(titles(&launcher), ["Clock from Git", "Install"]);
    assert!(has(
        &details(&launcher),
        &format!("Source: local folder {}", folder.display())
    ));
    launcher.back();

    // An id after the last `#` of the path names one extension of it.
    block_on(launcher.preview_collection(&tools, "clock"));
    assert_eq!(launcher.view().title, "Clock from Git");
    let details = details(&launcher);
    assert!(
        has(
            &details,
            &format!("Source: local folder {}#clock", folder.display())
        ),
        "{details:#?}"
    );
    assert!(
        has(
            &details,
            "Extension: clock, one of the extensions its collection lists"
        ),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Install"]);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git".into())
    );
    // Its identity is the collection folder's, with the id.
    let identity = PackageIdentity::local_extension(&tools, "clock").unwrap();
    let package = &launcher.packages()[0];
    assert_eq!(package.identity, identity);
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        registry["packages"][0]["local"],
        format!("{}#clock", folder.display()).as_str()
    );
    // Only the files under the extension's folder reach its managed copy.
    let mut files: Vec<String> = Vec::new();
    for entry in walk(&package.location) {
        files.push(
            entry
                .strip_prefix(&package.location)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    files.sort();
    assert_eq!(files, ["dist/git_greeter.wasm", "pane.json"]);
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    // Choosing it again offers Update, as for any installed package.
    block_on(launcher.preview_collection(&tools, "clock"));
    assert_eq!(titles(&launcher), ["Update"]);
    // A folder that is not a collection refuses the id; one whose root
    // holds `pane.json` says what is wrong.
    let plain = dirs.sources.path().join("plain");
    fs::create_dir_all(plain.join("dist")).unwrap();
    fs::copy(
        guest("git/greeter/dist/git_greeter.wasm"),
        plain.join("dist/git_greeter.wasm"),
    )
    .unwrap();
    fs::write(
        plain.join("pane.json"),
        fs::read_to_string(guests().join("git/greeter/pane.json"))
            .unwrap()
            .replace("Greeter from Git", "Plain"),
    )
    .unwrap();
    let resolved_plain = PackageIdentity::local(&plain)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();
    block_on(launcher.preview_collection(&plain, "clock"));
    assert_eq!(
        error_of(&launcher),
        format!(
            "The folder {} is not a collection: its root holds pane.json, one extension, and \
             no pane-collection.json; `#` names one extension of a collection",
            resolved_plain.display()
        )
    );
}

#[test]
fn a_dependency_naming_one_extension_of_a_collection_installs_it_and_is_called_by_id() {
    let dirs = Dirs::new();
    let tools = dirs.collection();
    let source = format!("git:{}#clock@v0.1.0", tools.url);
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "{source}",
              "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    assert!(
        has(
            &details,
            &format!("Requires: Clock from Git, installed with it from {source}")
        ),
        "{details:#?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Clock from Git", "Caller"]);
    let record = dirs.collection_record("tools", "clock");
    assert_eq!(record["gitExtension"], "clock");
    assert_eq!(record["pinned"], true);
    // The dependency is recorded by the identity it resolved to, with the
    // id, and a call through it reaches the extension.
    let caller = dirs.local_record(&folder);
    assert_eq!(
        caller["dependencies"][0],
        json!({ "id": "greeter", "git": format!("{}#clock", &dirs.identity("tools")[4..]) })
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn a_local_dependency_naming_one_extension_of_a_collection_installs_it() {
    let dirs = Dirs::new();
    let tools = dirs.sources.path().join("tools");
    write_collection(&tools, collection_files(&guests(), INDEX, true));
    let resolved = PackageIdentity::local(&tools)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();
    let folder = dirs.caller(
        r#"{ "id": "greeter", "source": "local:../tools#clock",
            "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    assert!(
        has(
            &details,
            "Requires: Clock from Git, installed with it from local:../tools#clock"
        ),
        "{details:#?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Clock from Git", "Caller"]);
    let caller = dirs.local_record(&folder);
    assert_eq!(
        caller["dependencies"][0],
        json!({ "id": "greeter", "local": format!("{}#clock", resolved.display()) })
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
}

// ------------------------------------------- renames and removals (#310)

/// An address naming an old id a collection renamed resolves to the
/// current extension (ADR 0044, #310): the preview shows the extension
/// under its current id, and installing installs it under that id — the
/// old id names the same extension, not a package of its own.
#[test]
fn an_old_id_a_collection_renamed_installs_the_current_extension() {
    let dirs = Dirs::new();
    let url = dirs.renamed().url;
    let launcher = dirs.launcher();
    let asked = format!("{url}#clock@refs/tags/v0.2.0");

    // The preview resolves the old id through the index's `renamed` map:
    // the extension is shown under its current id, its files read from
    // the current id's folder.
    block_on(launcher.preview_git(&asked));
    assert_eq!(launcher.view().title, "Clock from Git");
    let details = details(&launcher);
    let expected = [
        format!(
            "Source: Git repository {}#time",
            &dirs.identity("tools")[4..]
        ),
        "Extension: time, one of the extensions its collection lists".into(),
        "Revision: tag v0.2.0, which you named: installing pins it to that revision".into(),
    ];
    for line in &expected {
        assert!(has(&details, line), "{line:?} not in {details:#?}");
    }
    assert_eq!(titles(&launcher), ["Install"]);
    assert!(launcher.packages().is_empty());

    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git".into())
    );
    assert_eq!(installed(&launcher), ["Clock from Git"]);
    let package = &launcher.packages()[0];
    assert_eq!(package.identity.extension_id(), Some("time"));
    // The record names the current id beside the Git source fields; the
    // old id named the same extension, so no `clock` package exists.
    let record = dirs.collection_record("tools", "time");
    assert_eq!(record["gitExtension"], "time");
    assert_eq!(record["gitRef"], "refs/tags/v0.2.0");
    assert!(
        launcher
            .packages()
            .iter()
            .all(|package| package.identity.extension_id() != Some("clock"))
    );
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    dirs.wait_for_no_downloads();
}

/// A dependency's source naming an old id a collection renamed resolves
/// to the current extension (ADR 0044, #310): the dependency installs
/// under its current id, and the dependent's resolved record names the
/// identity it got, so a call through the dependency reaches it.
#[test]
fn a_dependency_naming_an_old_id_installs_the_current_extension() {
    let dirs = Dirs::new();
    let url = dirs.renamed().url;
    let source = format!("git:{url}#clock@refs/tags/v0.2.0");
    let folder = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "{source}",
            "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let details = details(&launcher);
    assert!(
        has(
            &details,
            &format!("Requires: Clock from Git, installed with it from {source}")
        ),
        "{details:#?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Clock from Git", "Caller"]);
    let record = dirs.collection_record("tools", "time");
    assert_eq!(record["gitExtension"], "time");
    // The dependency is recorded by the identity the resolution got, with
    // the current id, and a call through it reaches the extension.
    let caller = dirs.local_record(&folder);
    assert_eq!(
        caller["dependencies"][0],
        json!({ "id": "greeter", "git": format!("{}#time", &dirs.identity("tools")[4..]) })
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
    dirs.wait_for_no_downloads();
}

/// A local dependency's source naming an old id resolves the same way
/// (ADR 0044, #310): the collection's folder is read where the source
/// names it, the `renamed` map followed to the current id.
#[test]
fn a_local_dependency_naming_an_old_id_installs_the_current_extension() {
    let dirs = Dirs::new();
    let tools = dirs.sources.path().join("tools");
    write_collection(&tools, renamed_collection_files(&guests()));
    let resolved = PackageIdentity::local(&tools)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();
    let folder = dirs.caller(
        r#"{ "id": "greeter", "source": "local:../tools#clock",
            "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Clock from Git", "Caller"]);
    let caller = dirs.local_record(&folder);
    assert_eq!(
        caller["dependencies"][0],
        json!({ "id": "greeter", "local": format!("{}#time", resolved.display()) })
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the Git repository".into())
    );
}

// ------------------------------------------------- the choice of them (#308)

/// The choice of a collection's extensions (#308): a Git address naming
/// the collection, without an id, lists its extensions — each row what
/// its own manifest says of it, none ticked — and choosing Install
/// installs the ticked ones each as its own package, in the collection's
/// order, through its own preview. Activating an extension's row opens
/// the ordinary preview of that one extension, whose Back returns to the
/// choice.
#[test]
fn a_collection_named_by_its_address_opens_the_choice_and_installs_the_ticked_ones() {
    let dirs = Dirs::new();
    let tools = dirs.several(&[("clock", true), ("timers", true), ("notes", true)]);
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));

    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(view.title, "tools");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    // What each extension's own manifest says of it, in the collection's
    // order: its title, description and version, its icon the first one's
    // own (an image it ships) and the others' first-letter tiles.
    assert_eq!(
        choice
            .extensions
            .iter()
            .map(|extension| extension.id.as_str())
            .collect::<Vec<_>>(),
        ["clock", "timers", "notes"]
    );
    assert_eq!(
        choice
            .extensions
            .iter()
            .map(|extension| extension.title.as_str())
            .collect::<Vec<_>>(),
        ["Clock from Git", "Timers from Git", "Notes from Git"]
    );
    for extension in &choice.extensions {
        assert_eq!(extension.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            extension.description.as_deref(),
            Some(format!("The {} extension of the tools collection", extension.id).as_str())
        );
    }
    assert!(matches!(
        choice.extensions[0].icon.source,
        pane_core::IconSource::Image { .. }
    ));
    assert!(matches!(
        choice.extensions[1].icon.source,
        pane_core::IconSource::Letter('T')
    ));
    // Nothing is ticked, and the Install row after the extensions says so.
    assert!(choice.ticked.iter().all(|ticked| !ticked), "{choice:?}");
    assert_eq!(
        titles(&launcher),
        [
            "Clock from Git",
            "Timers from Git",
            "Notes from Git",
            "Install"
        ]
    );
    assert_eq!(
        launcher.view().rows[3].subtitle.as_deref(),
        Some("Tick the extensions to install")
    );
    let details = details(&launcher);
    for line in [
        format!("Source: Git repository {}", &dirs.identity("tools")[4..]),
        "Revision: tag v0.1.0, which you named: installing pins it to that revision".into(),
        format!(
            "Fetched: commit {} “Release 0.1.0”, served at {}; each object checked against its id",
            tools.release, tools.url
        ),
        HOW.into(),
    ] {
        assert!(has(&details, &line), "{line:?} not in {details:#?}");
    }
    assert!(launcher.packages().is_empty());

    // Choosing Install with nothing ticked is refused, and nothing runs.
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());
    assert_eq!(
        error_of(&launcher),
        "No extensions are ticked: tick the ones to install"
    );
    assert!(launcher.packages().is_empty());

    // Ticking and unticking: what Space and the row's check mark do.
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("timers");
    launcher.toggle_choice_tick("timers");
    launcher.toggle_choice_tick("notes");
    let Screen::Choice(choice) = &launcher.view().screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, false, true]);
    // The ticked row is selected, and the Install row says how many.
    assert_eq!(launcher.view().selected, Some(2));
    assert_eq!(
        launcher.view().rows[3].subtitle.as_deref(),
        Some("Copy the 2 ticked extensions into Pane and add their commands")
    );

    // The ordinary preview of the extension under the selection, before
    // anything is installed: activating its row opens it, and Back
    // returns to the choice with the ticks kept.
    select_title(&launcher, "Timers from Git");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Package { .. }), "{view:?}");
    assert_eq!(view.title, "Timers from Git");
    let details = self::details(&launcher);
    for line in [
        format!(
            "Source: Git repository {}#timers",
            &dirs.identity("tools")[4..]
        ),
        "Extension: timers, one of the extensions its collection lists".into(),
        "Version: 0.1.0".into(),
        "Commands: Timers from Git".into(),
        "Operations: greet (version 1)".into(),
    ] {
        assert!(has(&details, &line), "{line:?} not in {details:#?}");
    }
    assert_eq!(titles(&launcher), ["Install"]);
    launcher.back();
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, false, true]);
    assert!(launcher.packages().is_empty());

    // Choosing Install runs the ticked extensions one after another, in
    // the collection's order, each as its own package.
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    // The ending lists what was installed — here all of them — and the
    // last preview the run showed stays under the title, as any
    // extension's preview shows what it uses.
    assert_eq!(
        view.status,
        Status::Result("Installed Clock from Git and Notes from Git".into())
    );
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice.outcomes,
        [
            Some(ChoiceOutcome::Installed),
            None,
            Some(ChoiceOutcome::Installed)
        ]
    );
    assert!(has(&self::details(&launcher), "Commands: Notes from Git"));
    // Each ticked extension is a package of its own, with its own record:
    // the choice itself is never recorded as a unit.
    assert_eq!(installed(&launcher), ["Clock from Git", "Notes from Git"]);
    let identity = |id: &str| format!("{}#{id}", dirs.identity("tools"));
    assert_eq!(launcher.packages()[0].identity.key(), identity("clock"));
    assert_eq!(launcher.packages()[1].identity.key(), identity("notes"));
    for id in ["clock", "notes"] {
        let record = dirs.collection_record("tools", id);
        assert_eq!(record["gitExtension"], id);
        assert_eq!(record["gitUrl"], tools.url.as_str());
        assert_eq!(record["gitRef"], "refs/tags/v0.1.0");
        assert_eq!(record["pinned"], true);
    }
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(registry["packages"].as_array().unwrap().len(), 2);
    // Their commands run.
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    // Leaving the choice drops the revision it held, and with it the
    // download folder of the one fetch the choice made.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    dirs.wait_for_no_downloads();
}

/// One extension of the choice that cannot be installed is explained and
/// the others continue: the ending lists what was installed and what was
/// not, with the refusal in the row of the one that was not.
#[test]
fn an_extension_of_the_choice_that_cannot_be_installed_is_explained_and_the_rest_continue() {
    let dirs = Dirs::new();
    // `clock` ships its built component; `timers` holds its source only,
    // which an install of it explains.
    let tools = dirs.several(&[("clock", true), ("timers", false)]);
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("timers");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(
        view.status,
        Status::Error("Installed Clock from Git, but not Timers from Git".into())
    );
    let refused = format!(
        "Tag v0.1.0 (commit {}) of the Git repository {} holds only the \
         source of \"Timers from Git\", the extension `timers` of the collection: its built \
         component dist/git_greeter.wasm is not in it. Pane does not build packages from Git or \
         run anything in a repository; install a release revision whose commit includes the \
         built components (its author's release tag or branch), or build it yourself and \
         install the folder",
        short(&tools.release),
        &dirs.identity("tools")[4..]
    );
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice.outcomes,
        [
            Some(ChoiceOutcome::Installed),
            Some(ChoiceOutcome::Refused(refused))
        ]
    );
    assert_eq!(installed(&launcher), ["Clock from Git"]);
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// An extension of the choice that is already installed — a duplicate
/// identity — is refused as today, explained, and the others continue.
#[test]
fn an_extension_of_the_choice_already_installed_is_refused_and_the_rest_continue() {
    let dirs = Dirs::new();
    let tools = dirs.several(&[("clock", true), ("timers", true)]);
    let launcher = dirs.launcher();

    // One extension of the collection installed already, as #307 installs
    // one by its id.
    block_on(launcher.install_git(&format!("{}#clock@v0.1.0", tools.url)));
    assert_eq!(installed(&launcher), ["Clock from Git"]);

    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("timers");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(
        view.status,
        Status::Error("Installed Timers from Git, but not Clock from Git".into())
    );
    let refused = format!(
        "Already installed from Git repository {}#clock; use Update to replace the installed \
         copy",
        &dirs.identity("tools")[4..]
    );
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice.outcomes,
        [
            Some(ChoiceOutcome::Refused(refused)),
            Some(ChoiceOutcome::Installed)
        ]
    );
    assert_eq!(installed(&launcher), ["Clock from Git", "Timers from Git"]);
    assert_eq!(
        dirs.collection_record("tools", "timers")["gitExtension"],
        "timers"
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// A local folder holding a collection opens the same choice (#308): a
/// collection can be tried before it is published, and each ticked
/// extension installs with the identity of the folder and its id.
#[test]
fn a_local_collection_folder_opens_the_choice_and_installs_the_ticked_ones() {
    let dirs = Dirs::new();
    let extensions = [("clock", true), ("timers", true)];
    let tools = dirs.sources.path().join("tools");
    let mut files = extension_collection_files(&guests(), &extensions);
    files.push(("pane-collection.json", index_of(&extensions).into_bytes()));
    write_collection(&tools, files);
    let folder = PackageIdentity::local(&tools)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&tools));
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(view.title, "tools");
    assert_eq!(
        titles(&launcher),
        ["Clock from Git", "Timers from Git", "Install"]
    );
    assert!(has(
        &details(&launcher),
        &format!("Source: local folder {}", folder.display())
    ));

    // The ordinary preview of one extension, from the choice.
    select_title(&launcher, "Timers from Git");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().title, "Timers from Git");
    assert!(has(
        &details(&launcher),
        &format!("Source: local folder {}#timers", folder.display())
    ));
    launcher.back();

    // Ticking and installing several: each its own package, with its own
    // record, nothing of the choice recorded.
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("timers");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git and Timers from Git".into())
    );
    assert_eq!(installed(&launcher), ["Clock from Git", "Timers from Git"]);
    assert_eq!(
        launcher.packages()[0].identity,
        PackageIdentity::local_extension(&tools, "clock").unwrap()
    );
    assert_eq!(
        launcher.packages()[1].identity,
        PackageIdentity::local_extension(&tools, "timers").unwrap()
    );
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(registry["packages"].as_array().unwrap().len(), 2);
    assert_eq!(
        run(&launcher, "Timers from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
}

// ---------------------------------------------- fetching in parts (#311, ADR 0044)

/// The id of the blob the file `path` names in the revision `commit` of
/// `repo`: what a partial fetch asks the server for, by id.
fn blob_of(repo: &Repo, commit: &str, path: &str) -> String {
    repo.git(&["rev-parse", &format!("{commit}:{path}")])
}

/// The `want` lines a fetch was sent, sorted: the blobs it asked for,
/// however the trees were walked to find them.
fn wants_of(fetch: &[String]) -> Vec<String> {
    let mut wanted: Vec<String> = fetch
        .iter()
        .filter(|line| line.starts_with("want "))
        .cloned()
        .collect();
    wanted.sort();
    wanted
}

/// Where the server allows the partial-clone filter, a collection is
/// fetched without its file contents and the chosen extensions' folders'
/// blobs after (#311, ADR 0044): the trees and the collection's listing —
/// the index, each extension's manifest, the icon a manifest names — to
/// show the choice, then one fetch for every extension chosen, so
/// installing from a large repository downloads only what is installed.
/// The repository's own file, and any extension's files that were not
/// chosen, are never asked for.
#[test]
fn a_filter_capable_server_serves_the_choice_and_the_chosen_files_only() {
    let dirs = Dirs::new();
    let tools = dirs.several(&[("clock", true), ("timers", true), ("notes", true)]);
    // The server allows the filter, as GitHub's does.
    tools.repo.allow_filter();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));

    // The choice reads what it does from a server without the filter: each
    // extension's own manifest says its title, description and version,
    // and the first one's own icon.
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice
            .extensions
            .iter()
            .map(|extension| extension.id.as_str())
            .collect::<Vec<_>>(),
        ["clock", "timers", "notes"]
    );
    assert_eq!(
        choice
            .extensions
            .iter()
            .map(|extension| extension.title.as_str())
            .collect::<Vec<_>>(),
        ["Clock from Git", "Timers from Git", "Notes from Git"]
    );
    for extension in &choice.extensions {
        assert_eq!(extension.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            extension.description.as_deref(),
            Some(format!("The {} extension of the tools collection", extension.id).as_str())
        );
    }
    assert!(matches!(
        choice.extensions[0].icon.source,
        pane_core::IconSource::Image { .. }
    ));
    assert!(matches!(
        choice.extensions[1].icon.source,
        pane_core::IconSource::Letter('T')
    ));
    let details = details(&launcher);
    for line in [
        format!("Source: Git repository {}", &dirs.identity("tools")[4..]),
        "Revision: tag v0.1.0, which you named: installing pins it to that revision".into(),
        format!(
            "Fetched: commit {} “Release 0.1.0”, served at {}; each object checked against its id",
            tools.release, tools.url
        ),
        HOW.into(),
    ] {
        assert!(has(&details, &line), "{line:?} not in {details:#?}");
    }
    // The listing: the revision's trees without its file contents, then the
    // index, each extension's manifest, and the icon the first one's
    // manifest names — never the repository's own README, nor any
    // extension's component or source.
    let release = &tools.release;
    let wants = |path: &str| blob_of(&tools.repo, release, path);
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 4, "{fetches:?}");
    assert_eq!(
        fetches[0],
        vec![
            format!("want {release}"),
            "deepen 1".into(),
            "filter blob:none".into(),
            "no-progress".into(),
            "done".into()
        ]
    );
    assert_eq!(
        fetches[1],
        vec![
            format!("want {}", wants("pane-collection.json")),
            "no-progress".into(),
            "done".into()
        ]
    );
    let mut manifests: Vec<String> = ["clock", "timers", "notes"]
        .iter()
        .map(|id| format!("want {}", wants(&format!("extensions/{id}/pane.json"))))
        .collect();
    manifests.sort();
    assert_eq!(wants_of(&fetches[2]), manifests);
    assert_eq!(
        fetches[3],
        vec![
            format!("want {}", wants("extensions/clock/icon.svg")),
            "no-progress".into(),
            "done".into()
        ]
    );

    // The ordinary preview of one extension, before anything is chosen: its
    // own folder's files are fetched for it, only that one's, and its
    // preview reads them as a whole revision's does.
    select_title(&launcher, "Timers from Git");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().title, "Timers from Git");
    let lines = self::details(&launcher);
    for line in [
        format!(
            "Source: Git repository {}#timers",
            &dirs.identity("tools")[4..]
        ),
        "Extension: timers, one of the extensions its collection lists".into(),
        "Version: 0.1.0".into(),
        "Commands: Timers from Git".into(),
        "Operations: greet (version 1)".into(),
    ] {
        assert!(has(&lines, &line), "{line:?} not in {lines:#?}");
    }
    launcher.back();
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 5, "{fetches:?}");
    let mut timers: Vec<String> = ["src/lib.rs", "dist/git_greeter.wasm"]
        .iter()
        .map(|file| format!("want {}", wants(&format!("extensions/timers/{file}"))))
        .collect();
    timers.sort();
    assert_eq!(wants_of(&fetches[4]), timers);

    // Ticking two and choosing Install: ONE fetch for both of them
    // together — the revision is never fetched again — and the extension
    // already previewed is not asked for again either.
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("notes");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git and Notes from Git".into())
    );
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 6, "{fetches:?}");
    let mut chosen: Vec<String> = [
        ("clock", "src/lib.rs"),
        ("clock", "dist/git_greeter.wasm"),
        ("notes", "src/lib.rs"),
        ("notes", "dist/git_greeter.wasm"),
    ]
    .iter()
    .map(|(id, file)| format!("want {}", wants(&format!("extensions/{id}/{file}"))))
    .collect();
    chosen.sort();
    // The extensions share the contents of their source and component
    // files, so one blob serves both: each id is wanted once, the pack
    // holding it once.
    chosen.dedup();
    assert_eq!(wants_of(&fetches[5]), chosen);
    assert_eq!(installed(&launcher), ["Clock from Git", "Notes from Git"]);
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    // The repository's own file was never asked for, and the revision's
    // trees were fetched once only.
    let readme = wants("README.md");
    assert!(
        fetches
            .iter()
            .flatten()
            .all(|line| !line.contains(readme.as_str())),
        "{fetches:?}"
    );
    assert_eq!(
        fetches
            .iter()
            .filter(|fetch| fetch.iter().any(|line| line == "filter blob:none"))
            .count(),
        1
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// A server that does not allow the filter — an ordinary `git upload-pack`
/// — is fetched whole, as ever: the choice and the run of its ticked
/// extensions all read the one revision, under ADR 0021's limits.
#[test]
fn a_server_that_allows_no_filter_is_fetched_whole() {
    let dirs = Dirs::new();
    let tools = dirs.several(&[("clock", true), ("timers", true)]);
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", tools.url)));
    assert_eq!(
        titles(&launcher),
        ["Clock from Git", "Timers from Git", "Install"]
    );
    let fetches = dirs.server.fetches();
    assert_eq!(
        fetches,
        vec![vec![
            format!("want {}", tools.release),
            "deepen 1".into(),
            "no-progress".into(),
            "done".into()
        ]]
    );
    launcher.toggle_choice_tick("clock");
    launcher.toggle_choice_tick("timers");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git and Timers from Git".into())
    );
    // Still the one fetch: no blob was ever asked for by its id.
    assert_eq!(dirs.server.fetches().len(), 1);
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// An install of one extension by its id, from a filter-capable server:
/// the trees, the index, and that one extension's whole folder — nothing
/// of the repository's other extensions (#311, ADR 0044).
#[test]
fn one_extension_installed_by_its_id_from_a_filter_capable_server() {
    let dirs = Dirs::new();
    let tools = dirs.several(&[("clock", true), ("timers", true), ("notes", true)]);
    tools.repo.allow_filter();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}#clock@v0.1.0", tools.url)));
    assert_eq!(launcher.view().title, "Clock from Git");
    assert_eq!(titles(&launcher), ["Install"]);
    let release = &tools.release;
    let wants = |path: &str| blob_of(&tools.repo, release, path);
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 3, "{fetches:?}");
    assert!(fetches[0].iter().any(|line| line == "filter blob:none"));
    assert_eq!(
        wants_of(&fetches[1]),
        vec![format!("want {}", wants("pane-collection.json"))]
    );
    let mut clock: Vec<String> = [
        "icon.svg",
        "pane.json",
        "src/lib.rs",
        "dist/git_greeter.wasm",
    ]
    .iter()
    .map(|file| format!("want {}", wants(&format!("extensions/clock/{file}"))))
    .collect();
    clock.sort();
    assert_eq!(wants_of(&fetches[2]), clock);
    // The repository's own file and the other extensions' were never
    // asked for.
    let readme = wants("README.md");
    assert!(
        fetches
            .iter()
            .flatten()
            .all(|line| !line.contains(readme.as_str())),
        "{fetches:?}"
    );
    let timers = wants("extensions/timers/dist/git_greeter.wasm");
    assert!(
        fetches
            .iter()
            .flatten()
            .all(|line| !line.contains(timers.as_str())),
        "{fetches:?}"
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git".into())
    );
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// An old id a collection renamed resolves on a filter-capable server as
/// on one that allows no filter (#310, #311): the partial fetch resolves
/// the id through the index's `renamed` map before it asks for any blob,
/// so the blobs it asks for are the current id's folder's, and the
/// extension previews and installs under its current id, as a whole
/// fetch of the same revision resolves it.
#[test]
fn an_old_id_a_collection_renamed_resolves_on_a_filter_capable_server() {
    let dirs = Dirs::new();
    let tools = dirs.renamed();
    tools.repo.allow_filter();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}#clock@refs/tags/v0.2.0", tools.url)));
    assert_eq!(launcher.view().title, "Clock from Git");
    let details = details(&launcher);
    assert!(has(
        &details,
        "Extension: time, one of the extensions its collection lists"
    ));
    assert_eq!(titles(&launcher), ["Install"]);
    // The rename was resolved before anything was fetched: the blobs
    // asked for are the current id's folder's, and the only ones.
    let wants = |path: &str| blob_of(&tools.repo, "v0.2.0", path);
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 3, "{fetches:?}");
    assert!(fetches[0].iter().any(|line| line == "filter blob:none"));
    assert_eq!(
        wants_of(&fetches[1]),
        vec![format!("want {}", wants("pane-collection.json"))]
    );
    let mut time: Vec<String> = ["pane.json", "dist/git_greeter.wasm"]
        .iter()
        .map(|file| format!("want {}", wants(&format!("extensions/time/{file}"))))
        .collect();
    time.sort();
    assert_eq!(wants_of(&fetches[2]), time);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Clock from Git".into())
    );
    assert_eq!(launcher.packages()[0].identity.extension_id(), Some("time"));
    assert_eq!(
        run(&launcher, "Clock from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// A one-extension repository on a filter-capable server is fetched whole,
/// as ever: the filter asks for its trees first, finds no collection's
/// index, and the whole revision is fetched as a server that allows no
/// filter is — only a collection is fetched in parts (#311).
#[test]
fn a_one_extension_repository_on_a_filter_capable_server_is_fetched_whole() {
    let dirs = Dirs::new();
    let greeter = dirs.greeter();
    greeter.repo.allow_filter();
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", greeter.url)));
    assert_eq!(launcher.view().title, "Greeter from Git");
    assert_eq!(titles(&launcher), ["Install"]);
    let fetches = dirs.server.fetches();
    assert_eq!(fetches.len(), 2, "{fetches:?}");
    // The probe: the revision's trees, without its file contents, looking
    // for a collection's index at the root.
    assert_eq!(
        fetches[0],
        vec![
            format!("want {}", greeter.release),
            "deepen 1".into(),
            "filter blob:none".into(),
            "no-progress".into(),
            "done".into()
        ]
    );
    // The whole revision, exactly as a server that allows no filter is
    // asked for it.
    assert_eq!(
        fetches[1],
        vec![
            format!("want {}", greeter.release),
            "deepen 1".into(),
            "no-progress".into(),
            "done".into()
        ]
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from Git".into())
    );
    assert_eq!(
        run(&launcher, "Greeter from Git", "Say hello"),
        Status::Result(HELLO.into())
    );
    launcher.back();
    dirs.wait_for_no_downloads();
}

/// A collection's refusals say the same from a filter-capable server as
/// they do where the whole revision was fetched: an index that cannot be
/// taken, and a root holding both manifests (#311).
#[test]
fn a_collection_s_refusals_from_a_filter_capable_server_say_the_same() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("tools");
    repo.allow_filter();
    // An index whose extension's id is not one.
    let index = r#"{ "extensions": [ { "id": "Clock", "path": "extensions/clock" } ] }"#;
    let commit = repo.commit(&collection_files(&guests(), index, true), "malformed");
    repo.tag("v0.1.0");
    let launcher = dirs.launcher();

    let error = refusal(&launcher, &format!("{url}@v0.1.0"));
    assert_eq!(
        error,
        format!(
            "Tag v0.1.0 (commit {}) of the Git repository {} is a collection whose \
             pane-collection.json is invalid: the extension id `Clock` must be lowercase \
             letters, digits and `-`",
            short(&commit),
            &dirs.identity("tools")[4..]
        )
    );
    // A root holding both manifests.
    let mut both = collection_files(&guests(), INDEX, true);
    both.push((
        "pane.json",
        fs::read(guests().join("git/greeter/pane.json")).unwrap(),
    ));
    let commit = repo.commit(&both, "both");
    repo.tag("v0.2.0");
    let error = refusal(&launcher, &format!("{url}#clock@v0.2.0"));
    assert_eq!(
        error,
        format!(
            "Tag v0.2.0 (commit {}) of the Git repository {} holds both pane.json and \
             pane-collection.json: a repository is one extension or a collection, never both",
            short(&commit),
            &dirs.identity("tools")[4..]
        )
    );
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

/// A component stored with Git LFS is refused as stored with LFS from a
/// filter-capable server too: the pointer is fetched with the extension's
/// folder's files, and the extension's read refuses it (ADR 0021, #311).
#[test]
fn a_component_stored_with_git_lfs_is_refused_from_a_filter_capable_server() {
    let dirs = Dirs::new();
    let (repo, url) = dirs.repo("tools");
    repo.allow_filter();
    // The extension's built component is a Git LFS pointer: a file the
    // repository stores as a pointer to its contents rather than the
    // contents themselves.
    let pointer = b"version https://git-lfs.github.com/spec/v1\n\
                    oid sha256:6c0e8c4ee7b1f7a2f0a3b0a2c9a68b60ff4a1f9e6a4a04d0c3b3e5f7a9b1d2c3\n\
                    size 12345\n"
        .to_vec();
    let mut files = collection_files(&guests(), INDEX, false);
    files.push(("extensions/clock/dist/git_greeter.wasm", pointer));
    let release = repo.commit(&files, "Release 0.1.0");
    repo.tag("v0.1.0");
    let launcher = dirs.launcher();

    block_on(launcher.preview_git(&format!("{}@v0.1.0", url)));
    assert_eq!(titles(&launcher), ["Clock from Git", "Install"]);
    launcher.toggle_choice_tick("clock");
    select_title(&launcher, "Install");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice.outcomes,
        [Some(ChoiceOutcome::Refused(format!(
            "Tag v0.1.0 (commit {}) of the Git repository {} stores its component \
             extensions/clock/dist/git_greeter.wasm with Git LFS, which Pane does not fetch: its \
             author must commit the built component itself in a release revision",
            short(&release),
            &dirs.identity("tools")[4..]
        )))]
    );
    assert!(launcher.packages().is_empty());
    launcher.back();
    dirs.wait_for_no_downloads();
}
