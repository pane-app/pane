//! Installing Pane and setting its default extensions up through the
//! launcher's public interface (#53, #278): a first setup that fetches
//! each default extension's pinned commit from its own repository —
//! served, as Pane's Git tests serve theirs, over Git's smart HTTP
//! protocol from 127.0.0.1 (`support/repo_server.rs`; nothing reaches
//! the network or a real Git host) — and installs it as a package from
//! a folder is, into a managed copy with the default extension's own
//! identity and its Git source recorded, with retries and the rows that
//! try a failed one again, while the core (root search, the install
//! rows, Manage extensions) stays usable.
//!
//! The packages are the ones `cargo xtask guests` assembles under
//! `target/guests/packages`: the icons sample (`guests/sample-icons`)
//! and the helper sample (`guests/sample-helper`) with its real helper
//! program `pane-echo` built for this system, so the helper is run as an
//! end user runs it: a prebuilt program from the repository, with no
//! Node, Rust, npm, Git or compiler involved. The default extensions'
//! own packages live in their repositories (#285), which these tests
//! cannot read; the samples stand in for them, pinned as Pane pins the
//! defaults.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::defaults::DefaultExtension;
use pane_core::{IconSource, Launcher, PackageIdentity, Runtime, Status, Target};
use serde_json::Value;
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/repo_server.rs"]
mod repo_server;

use defaults::{from_sample, made, package_files, pinned, version_of};
use repo_server::{Mode, Server};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

/// The default set the tests set up: the icons sample, whose package
/// ships a tile its rows draw, and the prebuilt-helper sample with it, a
/// repository carrying a native helper. Pane's own build no longer sets
/// the samples up (#162); the core still sets up whatever default set it
/// is given, helpers included.
const DEFAULTS: [(&str, &str); 2] = [
    ("sample-icons", "Icons sample"),
    ("helper-sample", "Helper sample"),
];

/// The helper sample's files: its assembled package, with its `pane.json`
/// naming this system's helper target — the repository a test serves
/// carries the helper built for the system it runs on.
fn helper_files() -> Vec<(String, Vec<u8>)> {
    let mut files = package_files("sample-helper");
    let target = Target::current().expect("Pane names this system's target");
    let file = format!("helpers/{}/pane-echo{}", target.id(), target.exe_suffix());
    assert!(
        files.iter().any(|(path, _)| *path == file),
        "the helper sample ships no {file}; run `cargo xtask guests`"
    );
    let contents = files
        .iter_mut()
        .find(|(path, _)| path == "pane.json")
        .map(|(_, contents)| contents)
        .expect("the helper sample has a pane.json");
    let text = String::from_utf8(contents.clone()).unwrap();
    let echo = text
        .find("\"targets\"")
        .expect("the sample declares targets");
    let end = echo + text[echo..].find('}').unwrap() + 1;
    *contents = format!(
        "{}\"targets\": {{ \"{}\": \"{file}\" }}{}",
        &text[..echo],
        target.id(),
        &text[end..]
    )
    .into_bytes();
    files
}

/// Pane's data location, compiled code cache, runtime and the server the
/// default extensions' repositories are served from, for one test.
struct Dirs {
    data: TempDir,
    /// Where the repositories' work trees live, which the server reads
    /// them from.
    repos: TempDir,
    /// Kept, not read: it holds the compiled code cache alive.
    _cache: TempDir,
    runtime: Runtime,
    server: Server,
    /// The pins of the default extensions whose repositories are served.
    pins: Vec<DefaultExtension>,
}

impl Dirs {
    fn new() -> Dirs {
        let cache = tempfile::tempdir().unwrap();
        Dirs {
            data: tempfile::tempdir().unwrap(),
            repos: tempfile::tempdir().unwrap(),
            runtime: Runtime::start_with_cache(cache.path().to_path_buf()).unwrap(),
            server: Server::start(),
            pins: Vec::new(),
            _cache: cache,
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// The pin of the default extension `id` of the served set.
    fn pin(&self, id: &str) -> DefaultExtension {
        self.pins
            .iter()
            .find(|pin| pin.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("no pin of {id}"))
    }

    /// Makes and serves the default set's repositories, each from its
    /// sample's assembled package and tagged as its manifest's version,
    /// and keeps the pins that name them.
    fn publish(&mut self) {
        let mut pins = Vec::new();
        for (id, title) in DEFAULTS {
            let files = if id == "helper-sample" {
                helper_files()
            } else {
                package_files(id)
            };
            let tag = format!("v{}", version_of(&files));
            pins.push(pinned(
                &self.server,
                self.repos.path(),
                id,
                title,
                &tag,
                &files,
            ));
        }
        self.pins = pins;
    }

    /// A launcher on this data folder setting its default extensions up
    /// from the pins of the served repositories; a new one is a restart
    /// of Pane.
    fn launcher(&self) -> Launcher {
        self.launcher_with(self.pins.clone())
    }

    /// A launcher on this data folder setting the default extensions
    /// `extensions` up: the whole served set, or another one (a build
    /// whose default set shrank, as #162's did).
    fn launcher_with(&self, extensions: Vec<DefaultExtension>) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_defaults(extensions)
    }

    /// The record of the default extension `id` in `installed.json`.
    fn record(&self, id: &str) -> Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["default"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no record of {id} in {registry:#}"))
    }

    /// How many times the repository of the default extension `id` was
    /// connected to: its `info/refs` requests, one per acquisition
    /// attempt (a Git acquire connects, lists the references and fetches
    /// the pack).
    fn fetches(&self, id: &str) -> usize {
        let wanted = format!("GET /{id}.git/info/refs");
        self.server
            .requests()
            .iter()
            .filter(|request| request.starts_with(&wanted))
            .count()
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

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// The installed packages' titles, in installed order.
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

/// The files of `folder`, by their path under it (as a package holds
/// them, `/`-separated), in alphabetical order.
fn read_files(folder: &Path, prefix: &str) -> Vec<String> {
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
            name
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            files.extend(read_files(&path, &in_package));
        } else {
            files.push(in_package);
        }
    }
    files
}

#[test]
fn a_first_setup_installs_the_pinned_commits_and_the_sample_answers() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    assert_eq!(
        launcher.view().status,
        Status::Result("Set up Pane's default extensions".into())
    );
    // Both default extensions are installed, from their pinned commits.
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    let identity = PackageIdentity::default_extension("sample-icons");
    assert_eq!(identity.key(), "default:sample-icons");
    let package = &launcher.packages()[0];
    assert_eq!(package.identity, identity);
    // The record keeps the default identity, the version the manifest
    // declares, and the Git source the revision came from: the
    // repository, the pin's release tag, its commit and that it is
    // pinned — what a later release's updater reads (#269).
    let record = dirs.record("sample-icons");
    assert_eq!(record["default"], "sample-icons");
    assert_eq!(record["defaultVersion"], "0.1.0");
    let pin = dirs.pin("sample-icons");
    assert_eq!(record["gitUrl"], pin.repository.as_str());
    assert_eq!(record["gitRef"], format!("refs/tags/{}", pin.tag));
    assert_eq!(record["gitCommit"], pin.commit.as_str());
    assert_eq!(record["pinned"], true);
    assert_eq!(record.get("local"), None);
    assert_eq!(record.get("npm"), None);
    assert_eq!(record.get("git"), None, "the identity is the default's");
    // The managed copy holds everything the package ships (#163) — the
    // manifest, the component, its tile icon and the images its rows
    // name — installed as a package from a folder is, and the sample
    // shows that tile from its managed copy.
    assert_eq!(
        read_files(&package.location, ""),
        [
            "assets/logo.png",
            "assets/logo@dark.png",
            "assets/logo@light.png",
            "assets/moon.svg",
            "assets/photo.png",
            "assets/sun.svg",
            "command.svg",
            "icon.png",
            "pane.json",
            "sample_icons.wasm",
        ]
    );
    let icon = launcher.icon_of(&identity.key()).expect("the icon");
    let IconSource::Image { light, dark } = &icon.source else {
        panic!("the sample's icon is not its tile: {icon:?}");
    };
    assert!(light.starts_with(&package.location), "{light:?}");
    assert_eq!(light.file_name().unwrap(), "icon.png");
    assert_eq!(dark, light);
    // Each repository was fetched from once; nothing is left in the
    // downloads folder.
    assert_eq!(dirs.fetches("sample-icons"), 1);
    assert_eq!(dirs.fetches("helper-sample"), 1);
    dirs.wait_for_no_downloads();

    // The sample answers from root search: its command is listed, opens
    // and runs an item.
    assert_eq!(
        run(&launcher, "Icons", "Built-in icon"),
        Status::Result("Chose Built-in icon".into())
    );

    // After a restart the default extensions are listed from their managed
    // copies, with nothing fetched again.
    let asked = dirs.server.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    assert_eq!(dirs.server.requests().len(), asked);
    assert_eq!(
        run(&launcher, "Icons", "Packaged image"),
        Status::Result("Chose Packaged image".into())
    );
}

#[test]
fn acquiring_says_what_is_being_set_up_and_leaves_the_core_usable() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // Every answer comes 500 ms later, so each fetch is in flight long
    // enough to watch the status line while it runs.
    dirs.server.set_mode(Mode::Slow(Duration::from_millis(500)));
    let launcher = dirs.launcher();

    // Acquisition runs in the background: another thread drives it, as
    // the window does.
    let acquiring = launcher.clone();
    let acquiring = thread::spawn(move || block_on(acquiring.acquire_defaults()));
    // While the pinned revisions are being fetched, the status line says
    // what is being set up — a Git fetch has no byte progress to follow —
    // and the core stays usable: root search lists its rows, including
    // the install rows.
    let deadline = Instant::now() + Duration::from_secs(10);
    let progress = loop {
        assert!(Instant::now() < deadline, "no progress of the sample");
        if let Status::Progress(text) = launcher.view().status
            && text.starts_with("Acquiring the Icons sample")
        {
            break text;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(progress, "Acquiring the Icons sample…");
    assert!(
        titles(&launcher).contains(&"Install extension from folder…".to_owned()),
        "{:?}",
        titles(&launcher)
    );
    assert!(
        titles(&launcher).contains(&"Install extension from npm…".to_owned()),
        "{:?}",
        titles(&launcher)
    );
    assert!(
        titles(&launcher).contains(&"Install extension from Git…".to_owned()),
        "{:?}",
        titles(&launcher)
    );
    // Typing in root search still works while the fetches are in flight,
    // and Pane's install row matches "calc" fuzzily (#193).
    search(&launcher, "calc");
    assert_eq!(titles(&launcher), ["Install extension from folder…"]);
    search(&launcher, "zzz");
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    acquiring.join().unwrap();

    assert_eq!(
        launcher.view().status,
        Status::Result("Set up Pane's default extensions".into())
    );
    // Once the default extensions are installed, managing them is offered
    // (the query typed meanwhile is cleared first).
    search(&launcher, "");
    assert!(titles(&launcher).contains(&"Manage Extensions".to_owned()));
    search(&launcher, "icons");
    assert_eq!(titles(&launcher), ["Icons", "Icons (package icon)"]);
}

#[test]
fn an_interrupted_fetch_is_tried_again_and_set_up() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The sample's first fetch is interrupted partway: the connection
    // ends in the middle of the pack it is sending.
    dirs.server.drop_once();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    // The interrupted fetch was tried again and set up.
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    search(&launcher, "icons");
    assert_eq!(titles(&launcher), ["Icons", "Icons (package icon)"]);
    // The sample's repository was fetched from twice: the interrupted
    // fetch and the one that recovered it. The helper sample's was asked
    // for once.
    assert_eq!(dirs.fetches("sample-icons"), 2);
    assert_eq!(dirs.fetches("helper-sample"), 1);
    // Nothing half-written is left: the downloads folder is emptied in
    // the background once the installs end.
    dirs.wait_for_no_downloads();
}

/// A default extension a build no longer sets up stays what it became:
/// an installed package (#162, the helper sample leaving Pane's default
/// set). A Pane whose default set is the icons sample alone, started over
/// data that set the helper sample up before, keeps it installed and
/// listed, fetches nothing for it, and lets the user uninstall it, after
/// which no first setup brings it back.
#[test]
fn a_default_extension_that_left_the_default_set_stays_until_uninstalled() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    drop(launcher);

    // The next build's default set no longer has the helper sample.
    let sample_only = || dirs.launcher_with(vec![dirs.pin("sample-icons")]);
    let fetched = dirs.fetches("helper-sample");
    let launcher = sample_only();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Helper sample"],
        "Pane removes nothing it set up"
    );
    assert_eq!(dirs.record("helper-sample")["default"], "helper-sample");
    assert_eq!(dirs.fetches("helper-sample"), fetched);
    search(&launcher, "helper");
    assert!(
        titles(&launcher)
            .iter()
            .any(|title| title == "Helper sample"),
        "its command is still in root search: {:?}",
        titles(&launcher)
    );

    // The user uninstalls it, as any installed package.
    let identity = PackageIdentity::default_extension("helper-sample");
    block_on(launcher.uninstall(&identity, pane_core::SavedData::Delete));
    assert_eq!(installed(&launcher), ["Icons sample"]);
    drop(launcher);
    let launcher = sample_only();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Icons sample"],
        "a first setup does not bring it back"
    );
    assert_eq!(dirs.fetches("helper-sample"), fetched);
}

/// A default extension whose data an uninstall the user chose keeps is
/// not acquired again: the user's choice, as disabling is.
#[test]
fn a_default_with_retained_data_is_not_acquired_again() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The sample's saved data: a value its command saved in an earlier
    // session, which Pane reads with the data folder at start and an
    // install again keeps.
    fs::create_dir_all(dirs.packages_dir()).unwrap();
    fs::write(
        dirs.packages_dir().join("settings.json"),
        r#"{ "version": 1, "packages": { "default:sample-icons": { "style": "formal" } } }"#,
    )
    .unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);

    // Uninstalling the sample while keeping its data.
    let identity = PackageIdentity::default_extension("sample-icons");
    block_on(launcher.uninstall(&identity, pane_core::SavedData::Keep));
    assert_eq!(
        launcher.view().status,
        Status::Result("Uninstalled Icons sample; its settings and content are kept".into())
    );
    drop(launcher);

    let fetched = dirs.fetches("sample-icons");
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Helper sample"],
        "the uninstalled default is not re-acquired"
    );
    assert_eq!(dirs.fetches("sample-icons"), fetched);
    // Its retained data is still managed, and nothing was fetched for it.
    search(&launcher, "");
    assert!(titles(&launcher).contains(&"Manage Extensions".to_owned()));
}

/// An install that acquired a default from the artifact source (an older
/// Pane) keeps it: its identity, its record and its data are unchanged,
/// and it is not re-acquired from the repository the pin names.
#[test]
fn an_install_that_acquired_a_default_from_the_artifact_source_keeps_it() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The record an older Pane wrote, which acquired the sample as a
    // default from its own downloads: the default identity and a version,
    // and no Git source. The managed copy is in place, as Pane left it.
    let copy = dirs.packages_dir().join("packages").join("1");
    fs::create_dir_all(&copy).unwrap();
    for (path, contents) in &package_files("sample-icons") {
        let file = copy.join(path);
        fs::create_dir_all(file.parent().expect("inside the copy")).unwrap();
        fs::write(&file, contents).unwrap();
    }
    let registry = r#"{
        "version": 1,
        "next": 2,
        "packages": [
            { "default": "sample-icons", "defaultVersion": "0.1.0", "dir": "1" }
        ]
    }"#;
    fs::write(dirs.packages_dir().join("installed.json"), registry).unwrap();
    // The data the old Pane's sample saved, under its identity.
    fs::write(
        dirs.packages_dir().join("settings.json"),
        r#"{ "version": 1, "packages": { "default:sample-icons": { "style": "formal" } } }"#,
    )
    .unwrap();

    // The new Pane opens it and acquires nothing over it: its own default
    // set is the sample alone, so the pin's repository is never asked
    // for anything.
    let asked = dirs.server.requests().len();
    let launcher = dirs.launcher_with(vec![dirs.pin("sample-icons")]);
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Icons sample"]);
    let record = dirs.record("sample-icons");
    assert_eq!(record["default"], "sample-icons");
    assert_eq!(record["defaultVersion"], "0.1.0");
    assert_eq!(record.get("gitUrl"), None);
    assert_eq!(record.get("gitRef"), None);
    assert_eq!(record.get("gitCommit"), None);
    assert_eq!(record.get("pinned"), None);
    assert_eq!(dirs.server.requests().len(), asked);
    // Its identity keeps the data, and the sample answers from the
    // managed copy the old Pane wrote.
    let data: Value = serde_json::from_str(
        &fs::read_to_string(dirs.packages_dir().join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["packages"]["default:sample-icons"]["style"], "formal");
    search(&launcher, "icons");
    assert_eq!(titles(&launcher), ["Icons", "Icons (package icon)"]);
}

#[test]
fn an_unreachable_repository_leaves_the_core_usable_and_a_row_tries_again() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    // Nothing answers where the repositories are.
    drop(dirs.server);

    block_on(launcher.acquire_defaults());

    // The failure is explained (the last one the status line holds), and
    // the core stays usable: root search still lists its rows, and
    // nothing is installed. Each default extension that failed is
    // offered again as a row.
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not set up the Helper sample: "),
        "{error}"
    );
    assert!(
        error.contains("Could not reach the Git repository 127.0.0.1:")
            && error.contains("Pane tried 3 times"),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
    assert_eq!(
        titles(&launcher),
        [
            "Create Extension…",
            "Import Extension…",
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Set up Helper sample",
            "Set up Icons sample",
            // Pane's own row, ranked with the retry rows by title (#199).
            "Settings…"
        ]
    );
    // The row tries again, and the failure is explained again: the
    // repositories are still unreachable, and the row stays.
    select_title(&launcher, "Set up Icons sample");
    block_on(launcher.activate_selected());
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not set up the Icons sample: "),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Icons sample".to_owned()));
    assert!(titles(&launcher).contains(&"Set up Helper sample".to_owned()));
}

#[test]
fn a_row_tries_again_and_sets_the_extension_up() {
    let dirs = Dirs::new();
    // The helper sample's repository is made, but not served: the server
    // answers 404 for it.
    let sample = from_sample(
        &dirs.server,
        dirs.repos.path(),
        "sample-icons",
        "Icons sample",
        "sample-icons",
    );
    let files = helper_files();
    let tag = format!("v{}", version_of(&files));
    let (repo, helper) = made(
        &dirs.server,
        dirs.repos.path(),
        "helper-sample",
        "Helper sample",
        &tag,
        &files,
    );
    let launcher = dirs.launcher_with(vec![sample, helper]);
    block_on(launcher.acquire_defaults());
    let error = error_of(&launcher);
    assert!(
        error.contains("There is no Git repository at http://127.0.0.1:"),
        "{error}"
    );
    // The sample was set up; only the helper sample failed.
    assert_eq!(installed(&launcher), ["Icons sample"]);

    // The repository is there now; the row that tries again sets it up.
    dirs.server.serve("helper-sample", &repo);
    select_title(&launcher, "Set up Helper sample");
    block_on(launcher.activate_selected());

    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Set up the Helper sample".into())
    );
    // The row is gone: what it asked for is there, and the helper's
    // command is in root search.
    assert!(!titles(&launcher).contains(&"Set up Helper sample".to_owned()));
    search(&launcher, "helper");
    assert!(
        titles(&launcher)
            .iter()
            .any(|title| title == "Helper sample"),
        "{:?}",
        titles(&launcher)
    );
}

#[test]
fn an_acquired_helper_runs_from_the_managed_copy() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    // The helper sample is installed, and its helper program is this
    // system's prebuilt file, copied into the managed copy with the
    // permission Pane gives it.
    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    let package = &launcher.packages()[1];
    assert_eq!(
        package.identity,
        PackageIdentity::default_extension("helper-sample")
    );
    let target = Target::current().unwrap();
    let helper = package
        .location
        .join("helpers")
        .join(target.id())
        .join(format!("pane-echo{}", target.exe_suffix()));
    assert!(helper.is_file(), "{} is missing", helper.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&helper).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    // Running it needs no Node, Rust, npm, Git or compiler: it answers
    // with what the prebuilt program prints, for this system.
    assert_eq!(
        run(&launcher, "Helper sample", "Echo through the helper"),
        Status::Result(format!("Echoed \"hello from Pane\" on {}", target))
    );
}

/// A repository whose pinned commit is not a release revision: no
/// `pane.json` at its root (ADR 0021). It is explained and not
/// installed, with the row that tries again.
#[test]
fn a_revision_without_a_pane_json_is_explained_and_not_installed() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The sample's pin names a commit of a repository holding its
    // built component and tile but no manifest.
    let whole = package_files("sample-icons");
    let tag = format!("v{}", version_of(&whole));
    let mut files = whole;
    files.retain(|(path, _)| path != "pane.json");
    let manifestless = pinned(
        &dirs.server,
        &dirs.repos.path().join("manifestless"),
        "sample-icons",
        "Icons sample",
        &tag,
        &files,
    );
    let launcher = dirs.launcher_with(vec![manifestless, dirs.pin("helper-sample")]);

    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Helper sample"]);
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not set up the Icons sample: Tag"),
        "{error}"
    );
    assert!(
        error.contains("is not a Pane extension: it has no pane.json"),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Icons sample".to_owned()));
    dirs.wait_for_no_downloads();
}

/// A repository whose pinned commit holds only the source of its command:
/// the built component is missing (ADR 0021: Pane never builds).
#[test]
fn a_source_only_revision_is_explained_and_not_installed() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let mut files = package_files("sample-icons");
    files.retain(|(path, _)| path != "sample_icons.wasm");
    let tag = format!("v{}", version_of(&files));
    let source_only = pinned(
        &dirs.server,
        &dirs.repos.path().join("source-only"),
        "sample-icons",
        "Icons sample",
        &tag,
        &files,
    );
    let launcher = dirs.launcher_with(vec![source_only, dirs.pin("helper-sample")]);

    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Helper sample"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Icons sample: Tag"),
        "{error}"
    );
    assert!(
        error.contains("holds only the source of \"Icons\""),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Icons sample".to_owned()));
    dirs.wait_for_no_downloads();
}

#[test]
fn an_incompatible_payload_is_explained_and_not_installed() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The sample's repository declares a platform this is not.
    let mut files = package_files("sample-icons");
    let others: Vec<String> = pane_core::Platform::ALL
        .iter()
        .filter(|platform| Some(**platform) != pane_core::Platform::current())
        .map(|platform| platform.id().to_owned())
        .collect();
    for (path, contents) in &mut files {
        if path == "pane.json" {
            let mut manifest: Value = serde_json::from_slice(contents).unwrap();
            manifest["platforms"] =
                Value::Array(others.iter().map(|id| Value::String(id.clone())).collect());
            *contents = serde_json::to_vec_pretty(&manifest).unwrap();
        }
    }
    let tag = format!("v{}", version_of(&files));
    let elsewhere = pinned(
        &dirs.server,
        &dirs.repos.path().join("elsewhere"),
        "sample-icons",
        "Icons sample",
        &tag,
        &files,
    );
    let launcher = dirs.launcher_with(vec![elsewhere, dirs.pin("helper-sample")]);

    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Helper sample"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Icons sample: Not available on"),
        "{error}"
    );
    assert!(error.contains("this package supports only"), "{error}");
}

#[test]
fn a_payload_without_its_declared_helper_file_is_explained_and_not_installed() {
    let mut dirs = Dirs::new();
    dirs.publish();
    // The helper sample's repository declares this system's helper
    // target, but its commit holds no helper file: a release that is not
    // compatible with this system.
    let mut files = helper_files();
    files.retain(|(path, _)| !path.starts_with("helpers/"));
    let tag = format!("v{}", version_of(&files));
    let helperless = pinned(
        &dirs.server,
        &dirs.repos.path().join("helperless"),
        "helper-sample",
        "Helper sample",
        &tag,
        &files,
    );
    let launcher = dirs.launcher_with(vec![dirs.pin("sample-icons"), helperless]);

    block_on(launcher.acquire_defaults());

    // The sample is set up; the helper sample is explained and not
    // installed, with the row that tries again.
    assert_eq!(installed(&launcher), ["Icons sample"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Helper sample: "),
        "{error}"
    );
    let target = Target::current().unwrap();
    assert!(
        error.contains(&format!(
            "Not ready to run: the package ships helper `echo` for {target}, but its file \
             helpers/{}/pane-echo{} is missing",
            target.id(),
            target.exe_suffix()
        )),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Helper sample".to_owned()));
    dirs.wait_for_no_downloads();
}

#[test]
fn a_disabled_default_extension_is_not_acquired_again() {
    let mut dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    // Disabling the sample is the opt-out: it stays installed, so
    // acquisition never re-acquires or re-enables it.
    let identity = PackageIdentity::default_extension("sample-icons");
    block_on(launcher.set_enabled(&identity, false));
    let asked = dirs.server.requests().len();

    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Icons sample", "Helper sample"]);
    let packages = launcher.packages();
    let sample = packages.iter().find(|p| p.identity == identity).unwrap();
    assert!(!sample.enabled);
    assert_eq!(dirs.server.requests().len(), asked);
    // Its commands contribute nothing while it is disabled.
    search(&launcher, "icons");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}
