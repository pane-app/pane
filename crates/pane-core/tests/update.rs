//! Automatic updates of installed npm and Git packages through the
//! launcher's public interface: the npm ones from a local registry on
//! 127.0.0.1 that each test fills (`support/npm_registry.rs`), the Git
//! ones from a repository each test makes with the `git` program and
//! serves over Git's smart HTTP protocol from 127.0.0.1
//! (`support/repo_server.rs`) — nothing here reaches the network, the
//! real npm registry or a real Git host. The npm package is the assembled
//! JavaScript settings sample
//! (`target/guests/packages/sample-settings-js`) packed as an npm
//! package: its Greeting command saves settings and has "Save after
//! waiting", which waits ten seconds, so that a command still running can
//! stand in the update's way. The Git package is the assembled Git sample
//! (`target/guests/git/greeter`), whose tracked `release` branch moves to
//! a newer commit.
//!
//! What is checked: a newer version updates the npm package by itself
//! once no command of it runs, keeping its identity, its settings and its
//! disabled state, ending the old code's generation, and a tracked
//! branch that has moved updates the Git package the same way, keeping
//! its identity and its tracked reference, while a pinned revision never
//! moves; a command that is running finishes first, the update waiting
//! until the screen the user is on closes; a pinned, disabled, or
//! turned-off package is never replaced, and neither is an installed
//! local folder's copy; the global and per-extension controls work
//! through Manage extensions, a Git package's row among them; an
//! incompatible version, a dependency that cannot be installed, an
//! unreachable registry and a tracked branch that has moved to a
//! source-only revision explain and leave the installed copy alone; an
//! action or an opening asked in the moment the replacement is being
//! applied is refused rather than started and stopped by it; a new
//! version that fails to start is not rolled back; and the check repeats
//! on its cadence, and at Pane's start.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock as _, ManualClock, SystemClock};
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/npm_registry.rs"]
mod npm_registry;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/unreachable.rs"]
mod unreachable;

use feedback::shown;
use npm_registry::{Registry, pack};
use repo_server::{Repo, Server, greeter_files};

#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use guests::{guest_file as guest, guests};
use rows::{select_title, titles};

/// The npm name of the package every test here installs.
const NAME: &str = "@pane-tests/settings";

/// The tarball of the settings sample at `version`, with the API version
/// `api` and the dependency declarations `dependencies` (JSON, already
/// quoted) in its `pane.json`.
fn settings_files(version: &str, api: &str, dependencies: &str) -> Vec<(&'static str, Vec<u8>)> {
    settings_files_of(version, api, dependencies, sample_component())
}

/// The tarball of the sample at `version`, with `component` in place of
/// its built one, so a test can publish a version whose code behaves
/// differently: one that fails to start, or one padded out so that
/// replacing its managed copy takes a while.
fn settings_files_of(
    version: &str,
    api: &str,
    dependencies: &str,
    component: Vec<u8>,
) -> Vec<(&'static str, Vec<u8>)> {
    let manifest = format!(
        r#"{{ "manifestVersion": 1, "title": "Settings from npm", "version": "{version}",
             "apiVersion": "{api}",
             "commands": [{{ "id": "greeting", "title": "Greeting",
                             "component": "sample_settings_js.wasm" }}]{dependencies} }}"#
    );
    let package = format!(r#"{{ "name": "{NAME}", "version": "{version}" }}"#);
    vec![
        ("package.json", package.into_bytes()),
        ("pane.json", manifest.into_bytes()),
        ("sample_settings_js.wasm", component),
    ]
}

/// The settings sample's built component.
fn sample_component() -> Vec<u8> {
    fs::read(guest("packages/sample-settings-js/sample_settings_js.wasm")).unwrap()
}

/// The sample's component with a custom section of `pad` zero bytes
/// appended: still the same code to Pane's checks (a custom section is
/// ignored), but a file slow enough to copy that a test can ask things of
/// the package while its replacement is being written.
fn padded_component(pad: usize) -> Vec<u8> {
    /// `n` as unsigned LEB128, as a section's lengths are written.
    fn leb128(mut n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }
    // A custom section: id 0, its length, the name's length, the name, then
    // the payload.
    let name = b"padding".to_vec();
    let mut body = leb128(name.len());
    body.extend_from_slice(&name);
    body.extend_from_slice(&vec![0u8; pad]);
    let mut component = sample_component();
    component.push(0);
    component.extend_from_slice(&leb128(body.len()));
    component.extend_from_slice(&body);
    component
}

struct Dirs {
    data: TempDir,
    runtime: Runtime,
    registry: Registry,
    /// Where each test's Git repositories are made, served by `server`.
    repos: TempDir,
    server: Server,
    clock: Arc<ManualClock>,
}

impl Dirs {
    fn new() -> Dirs {
        let clock = ManualClock::at(SystemClock.now());
        Dirs {
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
            registry: Registry::start(),
            repos: tempfile::tempdir().unwrap(),
            server: Server::start(),
            clock: clock.clone(),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder downloading from the local registry,
    /// whose checks the test's clock moves along: advancing it past a
    /// second brings the first check, and past a day the next one.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_npm_registry(NpmRegistry::local(self.registry.url()).unwrap())
            .with_clock(self.clock.clone())
    }

    /// A launcher on the system's clock, whose first check comes by
    /// itself about a second after it starts, as Pane's does at its start.
    fn launcher_by_the_system_clock(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_npm_registry(NpmRegistry::local(self.registry.url()).unwrap())
    }

    /// Publishes the sample at `version`, tagged latest, asking for the
    /// API version `api` (the version this Pane provides is 0.1).
    fn publish(&self, version: &str, api: &str) {
        self.publish_with(version, api, "");
    }

    /// Publishes the sample at `version` with the dependency declarations
    /// `dependencies` (JSON, already quoted) in its `pane.json`.
    fn publish_with(&self, version: &str, api: &str, dependencies: &str) {
        self.registry.publish(
            NAME,
            version,
            pack(&settings_files(version, api, dependencies)),
        );
    }

    /// Publishes the sample at `version` with `component` in place of its
    /// built one, tagged latest.
    fn publish_component(&self, version: &str, component: Vec<u8>) {
        self.registry.publish(
            NAME,
            version,
            pack(&settings_files_of(version, "0.1", "", component)),
        );
    }

    /// Installs the sample at `version`, unpinned.
    fn install(&self, launcher: &Launcher, version: &str) {
        self.publish(version, "0.1");
        block_on(launcher.install_npm(NAME));
        assert_eq!(
            launcher.view().status,
            Status::Result("Installed Settings from npm".into())
        );
    }

    /// The record of the sample in `installed.json`.
    fn record(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: serde_json::Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["npm"] == NAME)
            .cloned()
            .unwrap_or_else(|| panic!("no record of {NAME} in {registry:#}"))
    }

    /// The record of the local package installed from `folder` in
    /// `installed.json`, by its resolved folder.
    fn local_record(&self, folder: &Path) -> serde_json::Value {
        let identity = PackageIdentity::local(folder).unwrap();
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: serde_json::Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["local"] == identity.local_folder().unwrap().to_str().unwrap())
            .cloned()
            .unwrap_or_else(|| panic!("no local record in {registry:#}"))
    }

    /// The version the sample is installed at, from its record.
    fn installed_version(&self) -> String {
        self.record()["npmVersion"].as_str().unwrap().to_owned()
    }

    /// What the Greeting command saved in its settings, as the whole
    /// settings file's text.
    fn settings(&self) -> String {
        fs::read_to_string(self.packages_dir().join("settings.json")).unwrap_or_default()
    }

    /// What "Save after waiting" has saved: "started", "finished" or
    /// nothing.
    fn slow_save(&self) -> Option<&str> {
        ["finished", "started"].into_iter().find(|progress| {
            self.settings()
                .contains(&format!("\"slow-save\": \"{progress}\""))
        })
    }

    /// Asks the launcher for a check: the clock moves past when the next
    /// one is due. Waits until the check completed and whatever it staged
    /// was applied or deferred.
    fn check(&self, launcher: &Launcher) {
        self.clock.advance(Duration::from_secs(2));
        assert!(
            launcher.wait_for_updates(Duration::from_secs(30)),
            "the updater did not settle"
        );
    }

    /// The Git repository served as `name`, as an installed record's
    /// `git` field names it: its host and path, without the `git:` an
    /// identity key adds.
    fn git_identity(&self, name: &str) -> String {
        format!(
            "{}{}",
            self.server.url().trim_start_matches("http://"),
            name
        )
    }

    /// A new Git repository served as `name`, the controlled repository
    /// of the Git sample: its source alone on `main`, its built component
    /// on the branch `release`, tagged `v0.1.0`.
    fn git_repository(&self, name: &str) -> GitGreeter {
        let repo = Repo::init(&self.repos.path().join(name), self.server.home());
        let url = self.server.serve(name, &repo);
        repo.commit(&greeter_files(&guests(), false), "Greeter 0.1.0 source");
        repo.git(&["switch", "--quiet", "-c", "release"]);
        let release = repo.commit(&greeter_files(&guests(), true), "Release 0.1.0");
        repo.tag("v0.1.0");
        repo.git(&["switch", "--quiet", "main"]);
        GitGreeter { repo, url, release }
    }

    /// The record of the package installed from the Git repository served
    /// as `name`, in `installed.json`.
    fn git_record(&self, name: &str) -> serde_json::Value {
        let git = self.git_identity(name);
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: serde_json::Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["git"] == git.as_str())
            .cloned()
            .unwrap_or_else(|| panic!("no record of {git} in {registry:#}"))
    }

    /// The commit the package installed from the Git repository served as
    /// `name` is at, from its record.
    fn git_commit(&self, name: &str) -> String {
        self.git_record(name)["gitCommit"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Waits until the downloads folder is empty, as an ended install or
    /// update leaves it.
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

/// The controlled Git repository of the Git sample, as
/// [`Dirs::git_repository`] makes it: `main` holding the source only,
/// `release` its built component (tagged `v0.1.0`), so that a tracked
/// branch can move while Pane is not looking.
struct GitGreeter {
    repo: Repo,
    /// The address it is served at, `http://127.0.0.1:<port>/<name>.git`.
    url: String,
    /// `release`, tagged `v0.1.0`: with the built component.
    release: String,
}

impl GitGreeter {
    /// Moves the release branch to a new release of the sample at
    /// `version`, a commit that changes only the manifest's version: what
    /// a tracked branch moving looks like to the updater. `main` and the
    /// tag stay where they were, pointing at the old release.
    fn move_release(&self, version: &str) -> String {
        self.repo.git(&["switch", "--quiet", "release"]);
        let mut files = greeter_files(&guests(), true);
        for (path, contents) in &mut files {
            if *path == "pane.json" {
                let manifest = String::from_utf8(std::mem::take(contents)).unwrap();
                *contents = manifest
                    .replace(
                        "\"version\": \"0.1.0\"",
                        &format!("\"version\": \"{version}\""),
                    )
                    .into_bytes();
            }
        }
        let moved = self.repo.commit(&files, &format!("Release {version}"));
        self.repo.git(&["switch", "--quiet", "main"]);
        moved
    }

    /// Moves the release branch to a revision holding the source only,
    /// without the built component: an unrunnable revision the updater
    /// must refuse, as a preview would.
    fn move_release_to_source(&self) -> String {
        self.repo.git(&["switch", "--quiet", "release"]);
        self.repo.git(&["rm", "--quiet", "-r", "dist"]);
        let moved = self.repo.commit(&[], "Source only");
        self.repo.git(&["switch", "--quiet", "main"]);
        moved
    }
}

fn activate(launcher: &Launcher, title: &str) {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// Runs the item titled `item` of the Greeting command, returning what it
/// showed (its toast, or the status line); the command's screen stays
/// open, as it does for a user.
fn run(launcher: &Launcher, item: &str) -> Status {
    open_greeting(launcher);
    activate(launcher, item);
    shown(launcher)
}

/// Opens the Greeting command from root search.
fn open_greeting(launcher: &Launcher) {
    to_root(launcher);
    activate(launcher, "Greeting");
    assert_eq!(launcher.view().screen, Screen::Command);
}

fn to_root(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

/// Opens Manage extensions from root search.
fn manage(launcher: &Launcher) {
    to_root(launcher);
    activate(launcher, "Manage extensions…");
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

fn error_of(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(text) => text,
        other => panic!("not an error: {other:?}"),
    }
}

/// Waits until `what` holds, asserting it did within `limit`; `what`'s
/// name says what it was in the panic.
fn wait_until(what: &str, limit: Duration, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !check() {
        assert!(
            Instant::now() < deadline,
            "{what} did not happen within {limit:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The component of the Greeting command of the installed copy at
/// `version`, by the folder it is in.
fn component_of(launcher: &Launcher) -> PathBuf {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == PackageIdentity::npm(NAME))
        .expect("installed")
        .commands()[0]
        .component
        .clone()
}

/// Writes a local package folder titled "Local settings", whose Greeting
/// command runs the settings sample's component: an installed local-source
/// copy of the same code, as a user's own folder is.
fn local_package(sources: &Path) -> PathBuf {
    let folder = sources.join("settings");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Local settings", "version": "0.1.0",
             "apiVersion": "0.1",
             "commands": [{ "id": "greeting", "title": "Local greeting",
                             "component": "sample_settings_js.wasm" }] }"#,
    )
    .unwrap();
    fs::copy(
        guest("packages/sample-settings-js/sample_settings_js.wasm"),
        folder.join("sample_settings_js.wasm"),
    )
    .unwrap();
    folder
}

#[test]
fn a_newer_version_updates_the_package_by_itself_keeping_its_data() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    // A setting saved through the old code: an update keeps it.
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
    assert!(dirs.settings().contains("casual"));
    // Leave the command: its instance stays alive for its generation, and
    // the update ends that.
    to_root(&launcher);
    let old = component_of(&launcher);
    assert!(block_on(dirs.runtime.running()).contains(&old));

    dirs.publish("0.2.0", "0.1");
    dirs.check(&launcher);

    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Settings from npm to 0.2.0".into())
    );
    assert_eq!(dirs.installed_version(), "0.2.0");
    assert_eq!(dirs.record().get("pinned"), None);
    // The old code's generation ended: its instance is gone.
    assert!(!block_on(dirs.runtime.running()).contains(&old));
    // The new code runs, and the saved setting is still there.
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
    assert!(dirs.settings().contains("casual"));
}

#[test]
fn a_command_that_is_running_finishes_before_the_update_replaces_it() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    // Published before the command starts: packing the tarball is the
    // test's own work, and the save below runs for a fixed ten seconds,
    // which the check that finds this must fit inside.
    dirs.publish("0.2.0", "0.1");

    // "Save after waiting", run on another thread: its call is pending,
    // with the command's screen open.
    open_greeting(&launcher);
    select_title(&launcher, "Save after waiting");
    let saving = launcher.activate_selected();
    let thread = thread::spawn(move || {
        block_on(saving);
    });
    wait_until("the slow save started", Duration::from_secs(10), || {
        dirs.slow_save() == Some("started")
    });
    dirs.check(&launcher);

    // The update is staged and deferred: the installed copy is unchanged,
    // the user was not interrupted and the command is still running.
    assert_eq!(dirs.installed_version(), "0.1.0");
    assert_ne!(
        launcher.view().status,
        Status::Result("Updated Settings from npm to 0.2.0".into())
    );
    assert_eq!(dirs.slow_save(), Some("started"));

    // The running command finishes: nothing replaced it mid-command.
    thread.join().unwrap();
    assert_eq!(dirs.slow_save(), Some("finished"));
    // The screen the answer is on is still the user's: the update keeps
    // waiting while it is open.
    assert_eq!(dirs.installed_version(), "0.1.0");

    // Leaving the command is the boundary: the update applies, and says
    // so where the user is.
    to_root(&launcher);
    wait_until(
        "the deferred update applied",
        Duration::from_secs(30),
        || launcher.view().status == Status::Result("Updated Settings from npm to 0.2.0".into()),
    );
    assert_eq!(dirs.installed_version(), "0.2.0");
    // What the command saved is kept.
    assert!(dirs.settings().contains("finished"));
}

#[test]
fn a_pinned_package_is_never_updated_automatically() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.publish("0.1.0", "0.1");
    block_on(launcher.install_npm(&format!("{NAME}@0.1.0")));
    assert_eq!(dirs.record()["pinned"], serde_json::json!(true));
    let asked = dirs.registry.requests().len();

    dirs.publish("0.2.0", "0.1");
    dirs.check(&launcher);

    // Not even asked about: a pinned version is not a candidate. The
    // status line still says what happened before, not that anything was
    // updated.
    assert_eq!(dirs.installed_version(), "0.1.0");
    assert_eq!(dirs.registry.requests().len(), asked);
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Settings from npm".into())
    );
}

#[test]
fn an_opted_out_package_is_not_updated_until_the_user_turns_updates_back_on() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");

    // The row in Manage extensions, and what it says before and after.
    manage(&launcher);
    activate(&launcher, "Update Settings from npm automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of Settings from npm are off".into())
    );
    assert!(titles(&launcher).contains(&"Update Settings from npm automatically".to_owned()));

    dirs.publish("0.2.0", "0.1");
    let asked = dirs.registry.requests().len();
    dirs.check(&launcher);
    assert_eq!(dirs.installed_version(), "0.1.0");
    assert_eq!(dirs.registry.requests().len(), asked);

    // Turning it back on checks at once, and the update applies.
    activate(&launcher, "Update Settings from npm automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of Settings from npm are on".into())
    );
    wait_until("the update applied", Duration::from_secs(30), || {
        dirs.installed_version() == "0.2.0"
    });
}

#[test]
fn turning_updates_off_everywhere_stops_them_all() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");

    manage(&launcher);
    activate(&launcher, "Update extensions automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of extensions are off".into())
    );

    dirs.publish("0.2.0", "0.1");
    let asked = dirs.registry.requests().len();
    dirs.check(&launcher);
    assert_eq!(dirs.installed_version(), "0.1.0");
    assert_eq!(dirs.registry.requests().len(), asked);

    // Back on: the update applies.
    activate(&launcher, "Update extensions automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of extensions are on".into())
    );
    wait_until("the update applied", Duration::from_secs(30), || {
        dirs.installed_version() == "0.2.0"
    });
}

#[test]
fn an_incompatible_new_version_is_refused_with_its_explanation() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");

    // The new version needs an API this Pane does not provide.
    dirs.publish("0.2.0", "0.2");
    dirs.check(&launcher);

    let error = error_of(&launcher);
    assert!(
        error.starts_with(
            "Settings from npm was not updated: Incompatible package: it needs Pane extension \
             API 0.2, but this Pane provides 0.1."
        ),
        "{error}"
    );
    assert!(
        error.contains("It keeps running its installed code."),
        "{error}"
    );
    assert_eq!(dirs.installed_version(), "0.1.0");
    // The installed copy still runs.
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
}

#[test]
fn a_new_version_whose_dependency_cannot_be_installed_is_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");

    dirs.publish_with(
        "0.2.0",
        "0.1",
        r#", "dependencies": [{ "id": "nobody", "source": "npm:nobody",
                                  "operations": [{ "id": "nothing", "version": 1 }] }]"#,
    );
    dirs.check(&launcher);

    let error = error_of(&launcher);
    assert!(
        error.starts_with("Settings from npm was not updated to 0.2.0: "),
        "{error}"
    );
    assert!(
        error.contains("npm package nobody was not found in the registry"),
        "{error}"
    );
    assert!(
        error.contains("It keeps running its installed code."),
        "{error}"
    );
    assert_eq!(dirs.installed_version(), "0.1.0");
}

#[test]
fn an_unreachable_registry_is_explained_and_leaves_the_installed_copy_alone() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    drop(launcher);

    // A registry that refuses connections, as one that is down does.
    let closed = unreachable::ClosedPort::new();
    let url = format!("{}/", closed.url());
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_npm_registry(NpmRegistry::local(&url).unwrap())
        .with_clock(dirs.clock.clone());
    dirs.check(&launcher);

    let error = error_of(&launcher);
    assert!(
        error.starts_with(&format!(
            "Settings from npm was not checked for a newer version: Could not reach the npm \
             registry {url} for {NAME}:"
        )),
        "{error}"
    );
    assert_eq!(dirs.installed_version(), "0.1.0");
    // The installed copy still runs.
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
}

#[test]
fn the_check_repeats_on_its_cadence() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    // The first check: up to date, nothing happens. The status line still
    // says what happened before, not that anything was updated.
    dirs.check(&launcher);
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Settings from npm".into())
    );

    // A newer version is published; a day passes on the clock, and the
    // next check finds it.
    dirs.publish("0.2.0", "0.1");
    dirs.clock.advance(Duration::from_secs(24 * 3600 + 2));
    assert!(
        launcher.wait_for_updates(Duration::from_secs(30)),
        "the updater did not settle"
    );

    assert_eq!(dirs.installed_version(), "0.2.0");
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Settings from npm to 0.2.0".into())
    );
}

#[test]
fn a_disabled_package_is_not_updated() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    block_on(launcher.set_enabled(&PackageIdentity::npm(NAME), false));

    dirs.publish("0.2.0", "0.1");
    let asked = dirs.registry.requests().len();
    dirs.check(&launcher);

    // The user switched it off: Pane does not touch its code.
    assert_eq!(dirs.installed_version(), "0.1.0");
    assert_eq!(dirs.registry.requests().len(), asked);
}

#[test]
fn after_a_restart_the_first_check_updates_the_package() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    drop(launcher);

    dirs.publish("0.2.0", "0.1");
    // A new Pane, by the system's clock: its first check comes about a
    // second after it starts, by itself.
    let launcher = dirs.launcher_by_the_system_clock();
    assert!(
        launcher.wait_for_updates(Duration::from_secs(30)),
        "the updater did not settle"
    );

    assert_eq!(dirs.installed_version(), "0.2.0");
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Settings from npm to 0.2.0".into())
    );
}

#[test]
fn a_local_folder_package_is_never_updated_automatically() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    // An installed local-source copy of the sample, as a user's own folder
    // is: it has no registry to check against. An npm package is installed
    // beside it, so the check demonstrably runs.
    let sources = tempfile::tempdir().unwrap();
    let folder = local_package(sources.path());
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    let location = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity)
        .expect("the local package is installed")
        .location
        .clone();
    dirs.install(&launcher, "0.1.0");

    // What the check must leave alone: the record, the managed copy's
    // files, and the local package never being asked about (the only
    // request the check makes is the npm package's metadata).
    let record = dirs.local_record(&folder);
    let manifest = fs::read(location.join("pane.json")).unwrap();
    let component = fs::read(location.join("sample_settings_js.wasm")).unwrap();
    let asked = dirs.registry.requests().len();

    dirs.check(&launcher);

    // The check ran, asking only about the npm package, and nothing it
    // found changed the local copy.
    assert_eq!(dirs.registry.requests().len(), asked + 1);
    assert_eq!(
        dirs.registry.requests()[asked],
        format!("/{}", NAME.replace('/', "%2f"))
    );
    assert_eq!(dirs.local_record(&folder), record);
    assert_eq!(
        fs::read(location.join("pane.json")).unwrap(),
        manifest,
        "the local package's manifest was touched"
    );
    assert_eq!(
        fs::read(location.join("sample_settings_js.wasm")).unwrap(),
        component,
        "the local package's code was touched"
    );
    // Its command still runs, and its settings are its own.
    to_root(&launcher);
    activate(&launcher, "Local greeting");
    assert_eq!(launcher.view().screen, Screen::Command);
    activate(&launcher, "Use a casual greeting");
    assert_eq!(
        shown(&launcher),
        Status::Result("Saved the casual greeting".into())
    );
}

#[test]
fn an_action_asked_while_the_update_applies_is_refused_not_stopped() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");

    // A new version whose component is padded out, so applying it — the
    // unpack, the checks and the copy of the managed folder — takes a
    // while: the claim the apply holds stays open long enough to ask
    // something of the package inside it. The pad is 192 MiB: the claim
    // is read directly (below), from before the copy is written until
    // after it lands, and that much padding keeps the claim open for
    // hundreds of milliseconds on even the fastest disk, well past the
    // test's 20 ms polling, while the stage before it stays quick (the
    // padded tarball compresses to almost nothing, so the download and
    // unpack are fast however large the pad is). The wait's failure
    // explains the state it found, because the three ways it can fail
    // look alike from the outside: the window missed because the copy
    // was written between two polls (the record then holds 0.2.0), a
    // check or stage failure (the status line then explains it), or the
    // apply deferred or never begun (both then idle at 0.1.0).
    dirs.publish_component("0.2.0", padded_component(192 * 1024 * 1024));

    // An action of the package's command, asked for but not sent yet: as
    // the deferral test holds a command running by not resolving it, this
    // holds its call back by not polling the future that makes it, so the
    // package is quiet and the update may apply. Leaving the command is
    // the boundary.
    open_greeting(&launcher);
    select_title(&launcher, "Use a casual greeting");
    let action = launcher.activate_selected();
    to_root(&launcher);

    // The check: the update is staged, and applying it claims the package
    // while the replacement is written. The claim is polled directly, from
    // the moment it is taken — but not at the sleeping cadence: an
    // M-series writes the whole 192 MiB copy from the page cache in
    // barely more than one 20 ms sleep, and run 36836762847's macOS leg
    // missed the window entirely that way (the record already held 0.2.0
    // when the wait gave up). The claim is due moments after the clock
    // moves — the stage before it takes a second or so — so the wait
    // spins with a yield while it is due, polling far faster than any
    // copy, and falls back to sleeping once ten seconds pass without it:
    // a claim that late is a slow leg's, and a slow copy is a long window
    // that sleeping polls cannot miss. The wait explains itself on
    // timeout: which of the three states above the leg is in.
    dirs.clock.advance(Duration::from_secs(2));
    let component = component_of(&launcher);
    eprintln!("polling for the claim of {}", component.display());
    {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(120);
        // A trace a second, so a failure's captured output shows the
        // timeline: when the record flipped (mid-claim) against the polls.
        // Both the looked-up claim and the raw claim map are polled, so a
        // disagreement between them shows in the trace too.
        let mut told = 0u64;
        while !launcher.package_being_updated(&component) {
            let elapsed = start.elapsed().as_secs();
            if elapsed >= told {
                told = elapsed + 1;
                let (claims, packages) = launcher.claims_now();
                eprintln!(
                    "{elapsed:>3} s: the claim is not held ({claims:?}; the packages are \
                     {packages:?}); the status is {:?}; the record has {}",
                    launcher.view().status,
                    dirs.installed_version()
                );
            }
            assert!(
                Instant::now() < deadline,
                "the update never claimed the package: the status is {:?}, the record has {}",
                launcher.view().status,
                dirs.installed_version()
            );
            if start.elapsed() < Duration::from_secs(10) {
                std::thread::yield_now();
            } else {
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    // The action asked of the updating package now, while the replacement
    // is being applied, is refused with the update's explanation rather
    // than started and then stopped by the replacement: its call is never
    // made. Opening the command is refused the same way, which is what
    // tells the moment was the apply window.
    block_on(action);
    activate(&launcher, "Greeting");
    assert_eq!(
        error_of(&launcher),
        "Settings from npm is updating; open it again once that is done"
    );

    // The update lands, and the refused action never ran.
    wait_until("the update applied", Duration::from_secs(120), || {
        dirs.installed_version() == "0.2.0"
    });
    assert!(!dirs.settings().contains("casual"));
}

#[test]
fn a_new_version_that_fails_to_start_is_not_rolled_back() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    dirs.install(&launcher, "0.1.0");
    // A setting saved through the old code: the replacement keeps it.
    assert_eq!(
        run(&launcher, "Use a casual greeting"),
        Status::Result("Saved the casual greeting".into())
    );
    to_root(&launcher);

    // 0.2.0's component is the failing-start fixture, which traps the
    // first time it is asked for its view and starts on every later one.
    dirs.publish_component("0.2.0", fs::read(guest("failing_start.wasm")).unwrap());
    dirs.check(&launcher);

    // The update applied: the record shows the NEW version — the earlier
    // code is not restored — and the saved setting is kept.
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Settings from npm to 0.2.0".into())
    );
    assert_eq!(dirs.installed_version(), "0.2.0");
    assert!(dirs.settings().contains("casual"));

    // An update starts no code itself, as an install does (a reload, which
    // does start what it replaced, would pause a failing start with
    // Retry): the new code runs from the next call, the first of which
    // fails as the component does, without rolling anything back.
    activate(&launcher, "Greeting");
    let error = error_of(&launcher);
    assert!(error.starts_with("The extension crashed: "), "{error}");
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(dirs.installed_version(), "0.2.0");
    assert!(dirs.settings().contains("casual"));
    manage(&launcher);
    assert!(!titles(&launcher).iter().any(|t| t.starts_with("Retry")));

    // The next start is the fixture's later one: the new code runs.
    to_root(&launcher);
    open_greeting(&launcher);
    assert_eq!(launcher.view().title, "Started");
    activate(&launcher, "Started on a later attempt");
    assert_eq!(shown(&launcher), Status::Result("ran started".into()));
    assert_eq!(dirs.installed_version(), "0.2.0");
}

/// What the Greeter from Git command's "Say hello" shows in its toast.
const GIT_HELLO: &str = "Hello from the Git repository";

/// Opens the Greeter from Git command from root search and runs `item`,
/// returning what it showed (its toast, or the status line); the
/// command's screen stays open, as it does for a user.
fn run_greeter(launcher: &Launcher, item: &str) -> Status {
    to_root(launcher);
    activate(launcher, "Greeter from Git");
    assert_eq!(launcher.view().screen, Screen::Command);
    activate(launcher, item);
    shown(launcher)
}

#[test]
fn a_moved_tracked_branch_updates_the_package_by_itself() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let greeter = dirs.git_repository("greeter");

    // Installed from its tracked release branch; its command runs.
    block_on(launcher.install_git(&format!("{}@release", greeter.url)));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from Git".into())
    );
    assert_eq!(
        run_greeter(&launcher, "Say hello"),
        Status::Result(GIT_HELLO.into())
    );
    let record = dirs.git_record("greeter");
    assert_eq!(record["gitRef"], "refs/heads/release");
    assert_eq!(record["gitCommit"], greeter.release.as_str());
    assert_eq!(record.get("pinned"), None);

    // Leave the command, so that no screen of the package is open when
    // the branch moves.
    to_root(&launcher);

    // The branch moves to a new release while no command runs.
    let moved = greeter.move_release("0.2.0");
    dirs.check(&launcher);

    // The update applied by itself: the record keeps the identity, the
    // tracked branch and the pin, at the branch's new commit, and the
    // status line says what happened.
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Greeter from Git to 0.2.0".into())
    );
    let record = dirs.git_record("greeter");
    assert_eq!(record["git"], dirs.git_identity("greeter").as_str());
    assert_eq!(record["gitRef"], "refs/heads/release");
    assert_eq!(record["gitCommit"], moved.as_str());
    assert_eq!(record.get("pinned"), None);
    dirs.wait_for_no_downloads();

    // The new copy runs, and a check that finds the branch at its new
    // commit fetches nothing: no download appears and the record stays.
    assert_eq!(
        run_greeter(&launcher, "Say hello"),
        Status::Result(GIT_HELLO.into())
    );
    dirs.check(&launcher);
    assert_eq!(dirs.git_commit("greeter"), moved);
    dirs.wait_for_no_downloads();
}

#[test]
fn a_pinned_revision_is_never_updated_automatically() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let greeter = dirs.git_repository("greeter");

    // Installed from its release tag: pinned, whatever the branch does.
    block_on(launcher.install_git(&format!("{}@v0.1.0", greeter.url)));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from Git".into())
    );
    let record = dirs.git_record("greeter");
    assert_eq!(record["gitRef"], "refs/tags/v0.1.0");
    assert_eq!(record["pinned"], serde_json::json!(true));
    let asked = dirs.server.requests().len();

    // Both the tag and the branch move; a check runs. Not even the
    // repository's listing is asked for: a pinned revision is not a
    // candidate.
    greeter.move_release("0.2.0");
    dirs.check(&launcher);

    assert_eq!(dirs.git_commit("greeter"), greeter.release);
    assert_eq!(dirs.server.requests().len(), asked);
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from Git".into())
    );
}

#[test]
fn an_opted_out_git_package_is_not_updated_until_the_user_turns_updates_back_on() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let greeter = dirs.git_repository("greeter");
    block_on(launcher.install_git(&format!("{}@release", greeter.url)));

    // The row in Manage extensions, and what it says before and after.
    manage(&launcher);
    activate(&launcher, "Update Greeter from Git automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of Greeter from Git are off".into())
    );
    assert!(titles(&launcher).contains(&"Update Greeter from Git automatically".to_owned()));

    greeter.move_release("0.2.0");
    let asked = dirs.server.requests().len();
    dirs.check(&launcher);
    assert_eq!(dirs.git_commit("greeter"), greeter.release);
    assert_eq!(dirs.server.requests().len(), asked);

    // Turning it back on checks at once, and the update applies.
    activate(&launcher, "Update Greeter from Git automatically");
    assert_eq!(
        launcher.view().status,
        Status::Result("Automatic updates of Greeter from Git are on".into())
    );
    wait_until("the update applied", Duration::from_secs(30), || {
        dirs.git_commit("greeter") != greeter.release
    });
}

#[test]
fn a_tracked_branch_now_holding_only_the_source_is_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let greeter = dirs.git_repository("greeter");
    block_on(launcher.install_git(&format!("{}@release", greeter.url)));
    assert_eq!(
        run_greeter(&launcher, "Say hello"),
        Status::Result(GIT_HELLO.into())
    );

    // Leave the command, so that the check's explanation shows in root
    // search's status line, where a background check explains itself.
    to_root(&launcher);

    // The branch moves to a revision without the built component: not a
    // runnable release revision, refused as a preview would refuse it,
    // with the installed copy untouched and still running.
    greeter.move_release_to_source();
    dirs.check(&launcher);

    let status = launcher.view().status.clone();
    let Status::Error(explanation) = &status else {
        panic!("not an error: {status:?}")
    };
    assert!(
        explanation.starts_with("Greeter from Git was not updated: Branch release (commit ")
            && explanation.contains("holds only the source of")
            && explanation.ends_with("It keeps running its installed code."),
        "{explanation}"
    );
    assert_eq!(dirs.git_commit("greeter"), greeter.release);
    assert_eq!(
        run_greeter(&launcher, "Say hello"),
        Status::Result(GIT_HELLO.into())
    );
    dirs.wait_for_no_downloads();
}
