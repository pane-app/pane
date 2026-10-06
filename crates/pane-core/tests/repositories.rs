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
use pane_core::{Launcher, Runtime, SavedData, Screen, Status};
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/unreachable.rs"]
mod unreachable;

use repo_server::{Mode, Repo, Server, greeter_files};

#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

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

    /// The identity of the repository served as `name`.
    fn identity(&self, name: &str) -> String {
        let host = self.server.url().trim_start_matches("http://");
        format!("git:{host}{name}")
    }

    /// The record of the package from the repository served as `name`.
    fn record(&self, name: &str) -> Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        let git = self.identity(name)["git:".len()..].to_owned();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["git"] == git.as_str())
            .cloned()
            .unwrap_or_else(|| panic!("no record of {git} in {registry:#}"))
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

fn has(details: &[String], line: &str) -> bool {
    details.iter().any(|detail| detail == line)
}

fn installed(launcher: &Launcher) -> Vec<String> {
    launcher.packages().iter().map(|p| p.title()).collect()
}

/// Opens the command titled `command` from root search and runs its item
/// titled `item`, returning the status.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    launcher.view().status
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
    select_title(&launcher, "Manage extensions…");
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
