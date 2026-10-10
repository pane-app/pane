//! Installing a Pane application update by the user's choice (#54),
//! through the launcher's public interface: Pane checks the artifact
//! source for a newer version of itself when it starts and notifies the
//! user, who chooses whether to install it — nothing is downloaded,
//! installed or restarted automatically, ever
//! ([decision](https://github.com/pane-app/pane/issues/1): US76).
//!
//! The check and the install run against an artifact source on 127.0.0.1
//! (`support/artifacts.rs`; nothing reaches the network or Pane's
//! published downloads), which serves the index with an `application`
//! entry naming a Pane package — a zip, as the Windows package is — and
//! the zip itself. Where the program is installed stands in a folder of
//! the test's own with a program file in it, as `%LOCALAPPDATA%\Pane`
//! holds `pane.exe` on Windows; the swap an install performs is the same
//! platform-independent code the Windows smoke runs for real.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::defaults::ArtifactSource;
use pane_core::{ApplicationUpdate, Launcher, Runtime, Status, Target};
use tempfile::TempDir;

#[path = "support/artifacts.rs"]
mod artifacts;

use artifacts::Artifacts;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/repo_server.rs"]
mod repo_server;

use defaults::from_sample;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, titles};

/// Pane's install folder, data folder, compiled code cache, artifact
/// source and the server the default extension's repository is served
/// from, for one test. The install folder holds the program and, under
/// `data`, Pane's own data — as `%LOCALAPPDATA%\Pane` does on Windows, so
/// an update's swap works around the data it must not touch.
struct Dirs {
    install: TempDir,
    /// Where the default extension's repository work tree lives, which
    /// the server reads it from.
    repos: TempDir,
    /// Kept, not read: it holds the compiled code cache alive.
    _cache: TempDir,
    runtime: Runtime,
    artifacts: Artifacts,
    server: repo_server::Server,
}

impl Dirs {
    fn new() -> Dirs {
        let cache = tempfile::tempdir().unwrap();
        Dirs {
            install: tempfile::tempdir().unwrap(),
            repos: tempfile::tempdir().unwrap(),
            runtime: Runtime::start_with_cache(cache.path().to_path_buf()).unwrap(),
            artifacts: Artifacts::start(),
            server: repo_server::Server::start(),
            _cache: cache,
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.install.path().join("data").join("extensions")
    }

    /// Where the program is installed: the folder's program file, as
    /// `%LOCALAPPDATA%\Pane` holds `pane.exe` on Windows.
    fn program(&self) -> PathBuf {
        self.install.path().join("pane")
    }

    /// Writes the program Pane runs from, with the given bytes.
    fn running(&self, program: &[u8]) {
        fs::write(self.program(), program).unwrap();
    }

    /// A launcher on this data folder, running the version `version` from
    /// the program file above, checking the local artifact source; a new
    /// one is a restart of Pane.
    fn launcher(&self, version: &str) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_application_update(
                version,
                ArtifactSource::local(self.artifacts.url()).unwrap(),
                self.program(),
            )
    }

    /// Publishes an application package `version` for this system, holding
    /// `program` as the program an install replaces this Pane's with.
    fn publish_update(&self, version: &str, program: &[u8]) {
        let name = self
            .program()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the program file is named")
            .to_owned();
        let zip = artifacts::pack_zip(&[
            (name.as_str(), program.to_vec()),
            ("README.txt", b"the readme".to_vec()),
        ]);
        let target = Target::current()
            .expect("Pane names this system's target")
            .id()
            .to_owned();
        self.artifacts.publish_application(version, &target, &zip);
    }

    /// Publishes an application package `version` for this system, packed
    /// as the Linux package is — a gzipped tarball holding `pane/` — with
    /// `program` as the program an install replaces this Pane's with. The
    /// suite runs on every system, so the Linux package's format is
    /// installed everywhere the tests run, as the zip the Windows package
    /// is.
    fn publish_update_tgz(&self, version: &str, program: &[u8]) {
        let name = self
            .program()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the program file is named")
            .to_owned();
        let tarball = artifacts::pack_tgz(&[
            (name.as_str(), program.to_vec()),
            ("README.txt", b"the readme".to_vec()),
        ]);
        let target = Target::current()
            .expect("Pane names this system's target")
            .id()
            .to_owned();
        self.artifacts
            .publish_application_tgz(version, &target, &tarball);
    }

    /// The files in the install folder, sorted.
    fn installed(&self) -> Vec<String> {
        read_names(self.install.path(), "")
    }

    /// Makes and serves a default extension's repository — the Rust
    /// sample's assembled package, tagged as its manifest's version — and
    /// returns the pin that names it, as a Pane release pins its default
    /// extensions: a stand-in, the default extensions' own repositories
    /// living outside this one (#285).
    fn sample(&self) -> pane_core::DefaultExtension {
        from_sample(
            &self.server,
            self.repos.path(),
            "sample-rust",
            "Rust sample",
            "sample-rust",
        )
    }

    /// The names at the top of the install folder, sorted.
    fn top_level(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.install.path())
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn read_names(folder: &Path, prefix: &str) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return names;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let shown = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.path().is_dir() {
            names.extend(read_names(&entry.path(), &shown));
        } else {
            names.push(shown);
        }
    }
    names.sort();
    names
}

/// The error the status line shows, as text.
fn error_of(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(text) => text,
        other => panic!("not an error: {other:?}"),
    }
}

#[test]
fn choosing_to_install_downloads_and_swaps_the_program() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    select_title(&launcher, "Update Pane to 99.0.0");

    block_on(launcher.activate_selected());

    // The offer is installed: the new program took the old one's name and
    // place, the old one is renamed out of its way, and nothing else is
    // in the install folder. The status line says what happened: the new
    // version is used the next time Pane starts, which Pane never does by
    // itself.
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    // What the About page reads once the install is done: the installed
    // version, used the next time Pane starts.
    assert_eq!(
        launcher.application_update(),
        ApplicationUpdate::Installed {
            version: "99.0.0".into(),
        }
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(
        fs::read(dirs.install.path().join("pane.old")).unwrap(),
        b"the 0.1.0 program"
    );
    assert_eq!(dirs.installed(), ["pane", "pane.old"]);
    // The row is gone: the offer was installed. Nothing is listed for a
    // Pane whose update is installed.
    assert!(!titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));
    // The package was downloaded once, and the index read once for the
    // check and once for nothing else.
    assert_eq!(
        dirs.artifacts
            .requests()
            .iter()
            .filter(|path| path.ends_with(".zip"))
            .count(),
        1
    );

    // A Pane starting with the new program removes what the update left:
    // the old program goes, and the offer stays gone, because this Pane
    // runs the version the index names.
    drop(launcher);
    let launcher = dirs.launcher("99.0.0");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.installed(), ["pane"]);
    assert!(!titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));
    // The check at Pane's start stays quiet when there is nothing to
    // tell: no row, no word on the status line.
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn choosing_to_install_the_linux_packages_tarball_swaps_the_program() {
    let dirs = Dirs::new();
    // The Linux package: the index names a `.tar.gz`, as `cargo xtask
    // package-linux` packs it, and the install unpacks it with the same
    // strictness as the zip the Windows package is.
    dirs.publish_update_tgz("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    select_title(&launcher, "Update Pane to 99.0.0");

    block_on(launcher.activate_selected());

    // The offer is installed from the tarball exactly as from the zip: the
    // new program in place, the old one renamed out of its way, the
    // staging folder gone, and the tarball downloaded once.
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(
        fs::read(dirs.install.path().join("pane.old")).unwrap(),
        b"the 0.1.0 program"
    );
    assert_eq!(dirs.installed(), ["pane", "pane.old"]);
    assert_eq!(
        dirs.artifacts
            .requests()
            .iter()
            .filter(|path| path.ends_with(".tar.gz"))
            .count(),
        1
    );

    // A Pane starting with the new program removes what the update left.
    drop(launcher);
    let launcher = dirs.launcher("99.0.0");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.installed(), ["pane"]);
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn a_newer_version_is_notified_and_nothing_is_downloaded() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");

    // The check runs at Pane's start, in the background; the notification
    // is the status line and a row in root search.
    block_on(launcher.check_application_update());

    assert_eq!(
        launcher.view().status,
        Status::Result("Pane 99.0.0 is available".into())
    );
    // The same state, as the Settings About page reads it: the offer the
    // row lists, with no install attempted against it.
    assert_eq!(
        launcher.application_update(),
        ApplicationUpdate::Offered {
            version: "99.0.0".into(),
            failure: None,
        }
    );
    let rows = launcher.view().rows;
    let update = rows
        .iter()
        .find(|row| row.title == "Update Pane to 99.0.0")
        .unwrap_or_else(|| panic!("no update row in {rows:#?}"));
    assert_eq!(
        update.subtitle.as_deref(),
        Some(
            "Your extensions and settings are kept; the new version is used the next time Pane starts"
        )
    );
    // The check read the index and nothing else: no package was
    // downloaded, installed or restarted. Taking no action keeps it that
    // way.
    assert_eq!(
        dirs.artifacts
            .requests()
            .iter()
            .filter(|path| path.ends_with(".zip"))
            .count(),
        0,
        "a package was downloaded without the user choosing it"
    );
    assert_eq!(dirs.installed(), ["pane"]);
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");

    // Declining leaves everything as it was: still no package asked for,
    // the program still the one running, and the offer still listed.
    drop(launcher);
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert!(titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));
    assert_eq!(
        dirs.artifacts
            .requests()
            .iter()
            .filter(|path| path.ends_with(".zip"))
            .count(),
        0
    );
    assert_eq!(dirs.installed(), ["pane"]);
}

/// How many bytes of the offered package arrive before the connection
/// closes: partway, so the download is interrupted and tried again.
const DROPPED_AFTER: usize = 16;

#[test]
fn the_state_the_about_page_reads_is_the_rows_own_state() {
    let dirs = Dirs::new();
    // No updater wired (no artifact source given): honestly nothing to
    // check, which the Settings About page explains instead of promising
    // a release or a feed.
    let plain = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir());
    assert_eq!(plain.application_update(), ApplicationUpdate::Unconfigured);
    // Wired, and no check has finished yet.
    let launcher = dirs.launcher("0.1.0");
    assert_eq!(launcher.application_update(), ApplicationUpdate::Unchecked);
    // A check the user asked for answers even with nothing to offer, and
    // the state says which of the two it is: new enough, not unchecked.
    block_on(launcher.check_application_update_again());
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    assert_eq!(
        launcher.view().status,
        Status::Result("Pane is up to date".into())
    );
    // While a check runs the state says so; once it has answered, what it
    // found. The index answers slowly, so the running check is seen.
    dirs.artifacts
        .stall("pane-defaults.json", 8, Duration::from_millis(400));
    let checking = launcher.clone();
    let running = thread::spawn(move || block_on(checking.check_application_update_again()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while launcher.application_update() != ApplicationUpdate::Checking {
        assert!(
            Instant::now() < deadline,
            "the check never showed as running"
        );
        thread::sleep(Duration::from_millis(5));
    }
    running.join().unwrap();
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
}

#[test]
fn an_install_shows_progress_and_leaves_the_core_usable() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    // The package arrives slowly, in two pieces, so its bytes are seen
    // arriving one at a time.
    let file = format!("pane-99.0.0-{}.zip", Target::current().unwrap().id());
    dirs.artifacts.stall(&file, 8, Duration::from_millis(400));

    // The install runs as the window drives it: another thread, while the
    // core stays usable.
    select_title(&launcher, "Update Pane to 99.0.0");
    let installing = launcher.clone();
    let installing = thread::spawn(move || block_on(installing.activate_selected()));
    // While the package is still arriving, the status line says what is
    // being downloaded and how far it has come, and the core stays
    // usable: root search lists its rows, and the row being installed is
    // not listed meanwhile.
    let deadline = Instant::now() + Duration::from_secs(10);
    let progress = loop {
        assert!(Instant::now() < deadline, "no progress of the package");
        if let Status::Progress(text) = launcher.view().status
            && text.contains('%')
        {
            break text;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        progress.starts_with("Downloading Pane 99.0.0"),
        "{progress}"
    );
    assert!(progress.contains("% of "), "{progress}");
    assert!(titles(&launcher).contains(&"Install extension from folder…".to_owned()));
    assert!(!titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));
    // Typing in root search still works while the package arrives.
    block_on(launcher.set_query("nothing matches this"));
    assert_eq!(titles(&launcher), Vec::<String>::new());
    installing.join().unwrap();

    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
}

#[test]
fn a_failed_download_leaves_the_old_state_and_can_be_tried_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    // The package the index names is not what arrives: its bytes were
    // damaged, so they do not match the integrity the index gives.
    dirs.artifacts.corrupt_application();

    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());

    // The failure is explained, and nothing changed: the program still
    // the one running, nothing staged, nothing renamed. The row stays,
    // ready to try again.
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not update Pane to 99.0.0: "),
        "{error}"
    );
    assert!(
        error.contains("does not match the sha512 integrity"),
        "{error}"
    );
    // The About page reads the same failure with the offer it belongs
    // to: the offer stays, ready to be chosen again.
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Offered {
                failure: Some(why),
                ..
            } if why.contains("does not match the sha512 integrity")
        ),
        "the offer keeps its failure: {:?}",
        launcher.application_update()
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    assert_eq!(dirs.top_level(), ["pane"]);
    assert!(titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));

    // The source works again; trying again installs the update.
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
}

#[test]
fn an_interrupted_download_is_tried_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    // The first download of the package is interrupted partway: the
    // connection closes after its first bytes.
    let file = format!("pane-99.0.0-{}.zip", Target::current().unwrap().id());
    dirs.artifacts.drop_after(&file, DROPPED_AFTER, 1);

    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());

    // The interrupted download was tried again and installed.
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    let downloads = dirs
        .artifacts
        .requests()
        .iter()
        .filter(|path| path.ends_with(".zip"))
        .count();
    assert_eq!(downloads, 2);
}

#[test]
fn an_unreachable_source_is_explained_and_the_row_tries_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    assert_eq!(dirs.top_level(), ["pane"]);
    // Nothing answers where the artifact source is.
    drop(dirs.artifacts);

    block_on(launcher.check_application_update());

    // The check is explained — nothing else is touched, nothing is
    // offered — and a row tries it again.
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not check for a Pane update: "),
        "{error}"
    );
    assert!(
        error.contains("Pane's downloads at http://127.0.0.1:")
            && error.contains("Pane tried 3 times"),
        "{error}"
    );
    // The About page reads the same failure, with the check to try again
    // offered there too.
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Failed(why) if why.starts_with("Pane's downloads at")
        ),
        "the failure is the state: {:?}",
        launcher.application_update()
    );
    assert_eq!(
        titles(&launcher),
        [
            "Check for a Pane update",
            // The authoring rows (ADR 0047, #222) ranked with the rest.
            "Create Extension…",
            "Import Extension…",
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            // Pane's own row, ranked with the update rows by title (#199).
            "Settings…"
        ]
    );
    // The row tries again, and the failure is explained again: the source
    // is still unreachable, and the row stays.
    select_title(&launcher, "Check for a Pane update");
    block_on(launcher.activate_selected());
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not check for a Pane update: "),
        "{error}"
    );
}

#[test]
fn a_check_the_source_answers_with_an_error_is_explained_until_it_answers() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The source answers, but not with its index.
    dirs.artifacts
        .fail_status("pane-defaults.json", 404, usize::MAX);

    block_on(launcher.check_application_update());

    let error = error_of(&launcher);
    assert!(error.contains("answered 404 for its index"), "{error}");
    assert!(titles(&launcher).contains(&"Check for a Pane update".to_owned()));

    // The source works again; the row that tries again finds the update.
    dirs.artifacts.stop_failing("pane-defaults.json");
    select_title(&launcher, "Check for a Pane update");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Pane 99.0.0 is available".into())
    );
    assert!(titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));
}

#[test]
fn an_index_pane_cannot_take_is_explained_and_the_defaults_are_set_up() {
    let dirs = Dirs::new();
    // The sample's repository is served, so the default extension is
    // set up from it whatever the artifact source's index says of Pane's
    // own update.
    let sample = dirs.sample();
    dirs.running(b"the 0.1.0 program");
    // The index describes an application package for another system.
    let mut index: serde_json::Value = serde_json::from_str(&dirs.artifacts.index()).unwrap();
    index["application"] = serde_json::json!({
        "version": "99.0.0",
        "file": "pane-99.0.0.zip",
        "integrity": artifacts::integrity(b"whatever"),
        "size": 8,
        "target": "some-other-system",
    });
    dirs.artifacts.serve_index(index.to_string());
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(vec![sample])
        .with_application_update(
            "0.1.0",
            ArtifactSource::local(dirs.artifacts.url()).unwrap(),
            dirs.program(),
        );

    block_on(launcher.check_application_update());
    // A package for another system is not this Pane's business: the entry
    // stays silent, offering nothing, exactly as an equal or older version
    // does (an entry for this system that cannot be used is the one that
    // explains itself — the unit tests cover it).
    assert_eq!(launcher.view().status, Status::Idle);
    block_on(launcher.acquire_defaults());

    // The default extension is still set up from its repository: one part
    // of Pane that cannot take an entry never stops the other. The
    // acquisition's own outcome takes the status line; the check's
    // explanation stays in its row.
    assert_eq!(
        launcher.view().status,
        Status::Result("Set up the Rust sample".into())
    );
    assert_eq!(
        launcher
            .packages()
            .iter()
            .map(|p| p.title())
            .collect::<Vec<_>>(),
        ["Rust sample"]
    );
    // An index whose format version this Pane does not read is explained
    // as a broken source, for the update check as for the defaults.
    let mut index: serde_json::Value = serde_json::from_str(&dirs.artifacts.index()).unwrap();
    index["formatVersion"] = serde_json::json!(2);
    dirs.artifacts.serve_index(index.to_string());
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    let error = error_of(&launcher);
    assert!(
        error.contains("gave an index with format version 2, this Pane reads 1"),
        "{error}"
    );
    // An entry that names no newer version offers nothing and says
    // nothing: this Pane is new enough.
    dirs.artifacts.derived_index();
    dirs.publish_update("0.0.1", b"the 0.0.1 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(!titles(&launcher).contains(&"Update Pane to 0.0.1".to_owned()));
    assert!(!titles(&launcher).contains(&"Check for a Pane update".to_owned()));
}

#[test]
fn a_replacement_that_cannot_be_made_is_explained_and_recoverable() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    // The old program's file cannot be renamed away: something else holds
    // the name it would take (a folder, here, which no rename or removal
    // can put a file in).
    let blocked = dirs.install.path().join("pane.old");
    fs::create_dir_all(&blocked).unwrap();

    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());

    // The failure is explained, and nothing changed: the program is still
    // the one running, the staging folder is gone, and the row stays.
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not update Pane to 99.0.0: "),
        "{error}"
    );
    assert!(
        error.contains("the previous version's program could not be replaced"),
        "{error}"
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    assert!(!dirs.install.path().join("update").exists());
    assert!(titles(&launcher).contains(&"Update Pane to 99.0.0".to_owned()));

    // The name is free again; trying again installs the update.
    fs::remove_dir_all(&blocked).unwrap();
    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
}

#[test]
fn an_install_keeps_pane_s_data() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    // The sample is set up first: a default extension, installed in
    // Pane's data folder under the install folder, exactly as the Windows
    // install keeps it.
    let sample = dirs.sample();
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(vec![sample])
        .with_application_update(
            "0.1.0",
            ArtifactSource::local(dirs.artifacts.url()).unwrap(),
            dirs.program(),
        );
    block_on(launcher.acquire_defaults());
    assert_eq!(
        launcher
            .packages()
            .iter()
            .map(|p| p.title())
            .collect::<Vec<_>>(),
        ["Rust sample"]
    );
    let data = read_all(&dirs.install.path().join("data"));
    block_on(launcher.check_application_update());

    // The user chooses to install; Pane keeps running; its data — the
    // installed extension and its record — is untouched by the swap,
    // which changes only the program.
    select_title(&launcher, "Update Pane to 99.0.0");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert_eq!(data, read_all(&dirs.install.path().join("data")));
    assert_eq!(dirs.top_level(), ["data", "pane", "pane.old"]);

    // A Pane starting with the new program still has its extensions: the
    // sample is installed, its record where it was, and nothing of
    // the update is left in the install folder.
    drop(launcher);
    let launcher = dirs.launcher("99.0.0");
    assert_eq!(
        launcher
            .packages()
            .iter()
            .map(|p| p.title())
            .collect::<Vec<_>>(),
        ["Rust sample"]
    );
    assert_eq!(dirs.top_level(), ["data", "pane"]);
    // The sample still answers.
    block_on(launcher.set_query("reverse 42"));
    assert_eq!(titles(&launcher), ["24"]);
}

/// Every file under `folder`, by its path and contents.
fn read_all(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name().into_string().unwrap();
            if path.is_dir() {
                files.extend(
                    read_all(&path)
                        .into_iter()
                        .map(|(nested, contents)| (format!("{name}/{nested}"), contents)),
                );
            } else {
                files.push((name, fs::read(&path).unwrap()));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}
