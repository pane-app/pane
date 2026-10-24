//! Pane's own application updates (#54, ADR 0050), through the launcher's
//! public interface: Pane checks its releases on GitHub for a newer
//! version of itself — at start, on a schedule, when the launcher is shown
//! after a stale check, and when the user asks — downloads and stages what
//! it finds by itself, quietly, and restarts into it only by the user's
//! choice: Restart to Update Pane, first of all in a blank root search,
//! or the toast's Restart.
//!
//! The checks and downloads run against a stand-in for GitHub on
//! 127.0.0.1 (`support/releases.rs`; nothing reaches the network or Pane's
//! published releases), which lists the releases as GitHub's API does —
//! each carrying a Pane package (a zip, as the Windows package is) and its
//! `.sha256` — with an `ETag`, answering `304` to a request that names the
//! listing as it stands, and serves their downloads behind a redirect, as
//! GitHub does. Where the program is installed stands in a folder of the
//! test's own with a program file in it, as `%LOCALAPPDATA%\Pane` holds
//! `pane.exe` on Windows; the swap an install performs is the same
//! platform-independent code the Windows smoke runs for real. The
//! schedule follows the launcher's clock, which the tests move. The new
//! version a restart starts is a stand-in too (`Relauncher`): the program
//! files hold text, so it reads the version from them, and it reports
//! that it started, stays silent, exits or cannot start, as a test asks;
//! the smokes start real programs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::ManualClock;
use pane_core::restart::{Relaunch, Started};
use pane_core::{
    ApplicationUpdate, Launcher, ReleaseSource, Runtime, Status, Target, ToastSlot, WindowPresence,
};
use tempfile::TempDir;

#[path = "support/releases.rs"]
mod releases;

use releases::Releases;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/repo_server.rs"]
mod repo_server;

use defaults::from_sample;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, titles};

#[path = "support/feedback.rs"]
mod feedback;

use feedback::{RecordingWindow, toast_title};

/// The root row that restarts Pane into the update it staged.
const RESTART: &str = "Restart to Update Pane";

/// How long the stand-in for a new version has to report that it started.
const STARTED_WITHIN: Duration = Duration::from_secs(1);

/// What the stand-in for the new version does once started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Starts {
    /// Reports that it started, as a new Pane does first thing.
    Reports,
    /// Runs, and never reports.
    Silent,
    /// Exits at once, without reporting.
    Exits,
    /// Cannot be started at all.
    Refuses,
}

/// The stand-in for the new version a restart checks and starts: the
/// program files hold text ("the 99.0.0 program"), whose version it reads
/// as `--version` would answer; what it does once started is the test's.
struct Relauncher {
    /// Where the new version reports that it started: Pane's cache folder.
    folder: PathBuf,
    starts: Mutex<Starts>,
    /// The arguments each start was given.
    started: Mutex<Vec<Vec<String>>>,
    /// Whether the restart ended the last program it started.
    ended: Arc<AtomicBool>,
}

impl Relauncher {
    fn new(folder: &Path) -> Arc<Relauncher> {
        Arc::new(Relauncher {
            folder: folder.to_path_buf(),
            starts: Mutex::new(Starts::Reports),
            started: Mutex::default(),
            ended: Arc::default(),
        })
    }

    fn will(&self, starts: Starts) {
        *self.starts.lock().unwrap() = starts;
    }

    fn started(&self) -> Vec<Vec<String>> {
        self.started.lock().unwrap().clone()
    }
}

impl Relaunch for Relauncher {
    fn version(&self, program: &Path) -> Result<String, String> {
        let text =
            String::from_utf8_lossy(&fs::read(program).map_err(|e| e.to_string())?).into_owned();
        let (_, after) = text
            .split_once("the ")
            .ok_or_else(|| "it is no program".to_owned())?;
        Ok(after.split(' ').next().unwrap_or_default().to_owned())
    }

    fn start(&self, _program: &Path, arguments: &[String]) -> Result<Box<dyn Started>, String> {
        self.started.lock().unwrap().push(arguments.to_vec());
        let starts = *self.starts.lock().unwrap();
        if starts == Starts::Refuses {
            return Err("it could not be started: access is denied".into());
        }
        if starts == Starts::Reports {
            let waited = arguments[3].parse().unwrap();
            pane_core::restart::report_started(&self.folder, waited).unwrap();
        }
        self.ended.store(false, Ordering::SeqCst);
        Ok(Box::new(Child {
            exited: starts == Starts::Exits,
            ended: self.ended.clone(),
        }))
    }
}

/// A program the stand-in started.
struct Child {
    exited: bool,
    ended: Arc<AtomicBool>,
}

impl Started for Child {
    fn exited(&mut self) -> bool {
        self.exited || self.ended.load(Ordering::SeqCst)
    }

    fn end(&mut self) {
        self.ended.store(true, Ordering::SeqCst);
    }
}

/// Asserts that `launcher` restarted into `version`: the new version
/// reported that it started, and Pane waits to quit.
fn assert_restarted(launcher: &Launcher, version: &str) {
    assert_eq!(
        launcher.view().status,
        Status::Progress(format!("Restarting into Pane {version}…"))
    );
    assert_eq!(
        launcher.application_update(),
        ApplicationUpdate::Restarting {
            version: version.into()
        }
    );
    assert!(!titles(launcher).contains(&RESTART.to_owned()));
}

/// The root row that checks for a Pane update at once.
const CHECK_ROW: &str = "Check for Pane Updates";

/// When the tests' clocks start: a time of the past, in milliseconds since
/// the Unix epoch.
const START: u64 = 1_800_000_000_000;

/// Pane's install folder, data folder, cache folder (where the update
/// record is kept), compiled code cache, stand-in for Pane's releases and
/// the server the default extension's repository is served from, for one
/// test. The install folder holds the program and, under `data`, Pane's
/// own data — as `%LOCALAPPDATA%\Pane` does on Windows, so an update's
/// swap works around the data it must not touch.
struct Dirs {
    install: TempDir,
    /// Pane's cache folder, holding the update record.
    records: TempDir,
    /// Where the default extension's repository work tree lives, which
    /// the server reads it from.
    repos: TempDir,
    /// Kept, not read: it holds the compiled code cache alive.
    _cache: TempDir,
    runtime: Runtime,
    releases: Releases,
    server: repo_server::Server,
    /// The stand-in for the new version a restart starts.
    relaunch: Arc<Relauncher>,
}

impl Dirs {
    fn new() -> Dirs {
        let cache = tempfile::tempdir().unwrap();
        let records = tempfile::tempdir().unwrap();
        Dirs {
            install: tempfile::tempdir().unwrap(),
            relaunch: Relauncher::new(records.path()),
            records,
            repos: tempfile::tempdir().unwrap(),
            runtime: Runtime::start_with_cache(cache.path().to_path_buf()).unwrap(),
            releases: Releases::start(),
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
    /// the program file above, checking the local releases and keeping its
    /// update record in the cache folder; a new one is a restart of Pane.
    fn launcher(&self, version: &str) -> Launcher {
        self.launcher_from(
            Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir()),
            version,
        )
    }

    /// As [`Dirs::launcher`], keeping time by `clock`.
    fn launcher_at(&self, version: &str, clock: &Arc<ManualClock>) -> Launcher {
        self.launcher_from(
            Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
                .with_clock(clock.clone()),
            version,
        )
    }

    fn launcher_from(&self, launcher: Launcher, version: &str) -> Launcher {
        self.restarting(launcher.with_application_update(
            version,
            ReleaseSource::local(self.releases.url()).unwrap(),
            self.program(),
        ))
    }

    /// `launcher`, keeping its update record in the cache folder and
    /// restarting into an update through the stand-in.
    fn restarting(&self, launcher: Launcher) -> Launcher {
        launcher
            .with_application_update_cache(self.records.path())
            .with_relaunch(self.relaunch.clone(), STARTED_WITHIN)
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
        let zip = releases::pack_zip(&[
            (name.as_str(), program.to_vec()),
            ("README.txt", b"the readme".to_vec()),
        ]);
        self.releases.publish_application(version, &target(), &zip);
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
        let tarball = releases::pack_tgz(&[
            (name.as_str(), program.to_vec()),
            ("README.txt", b"the readme".to_vec()),
        ]);
        self.releases
            .publish_application_tgz(version, &target(), &tarball);
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

/// This system's target, as a release names its package.
fn target() -> String {
    Target::current()
        .expect("Pane names this system's target")
        .id()
        .to_owned()
}

/// The package a release of `version` carries for this system.
fn package_file(version: &str) -> String {
    format!("pane-{version}-{}.zip", target())
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

/// Waits until `done`, failing with `what` after a while.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// The update the launcher holds staged, ready to restart into.
fn ready(version: &str) -> ApplicationUpdate {
    ApplicationUpdate::Ready {
        version: version.into(),
        failure: None,
    }
}

/// Runs root search's Check for Pane Updates, as the user does.
fn check_now(launcher: &Launcher) {
    block_on(launcher.set_query(""));
    select_title(launcher, CHECK_ROW);
    block_on(launcher.activate_selected());
}

#[test]
fn a_newer_version_is_downloaded_by_itself_and_restarted_into_by_choice() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");

    // A check Pane makes by itself finds the newer version and downloads
    // it at once, without any choice — quietly: nothing on the status
    // line while it does.
    block_on(launcher.check_application_update());
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    // The release server's log: the listing, then the package's `.sha256`
    // and the package, each through the redirect GitHub's downloads answer
    // with — asked for before the user chose anything.
    let file = package_file("99.0.0");
    let requests = dirs.releases.requests();
    assert!(requests[0].starts_with(releases::LISTING), "{requests:?}");
    assert_eq!(
        requests[1..],
        [
            format!("/pane-app/pane/releases/download/v99.0.0/{file}.sha256"),
            format!("/assets/{file}.sha256"),
            format!("/pane-app/pane/releases/download/v99.0.0/{file}"),
            format!("/assets/{file}"),
        ]
    );
    // It is staged beside the program, which is still the one running,
    // with its marker written last.
    assert_eq!(
        dirs.installed(),
        [
            "pane",
            "update/.pane-staged.json",
            "update/README.txt",
            "update/pane"
        ]
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    // Root search lists Restart to Update Pane first of all, above every
    // other row, selected, under a section of its own and drawn with the
    // accent.
    let rows = launcher.view().rows;
    assert_eq!(rows[0].title, RESTART, "{rows:#?}");
    assert_eq!(
        rows[0].subtitle.as_deref(),
        Some("Pane 99.0.0 is ready. Your extensions and settings are kept")
    );
    let (view, listing) = launcher.presented_list();
    assert_eq!(view.selected, Some(0));
    assert_eq!(listing.lead, 1);
    assert_eq!(listing.sections[0].label, "Update");
    assert_eq!(listing.sections[0].first, 0);
    assert_eq!(listing.sections[1].label, "Commands");
    assert_eq!(listing.sections[1].first, 1);
    assert!(launcher.present_row(0).highlighted);
    assert!(!launcher.present_row(1).highlighted);
    let window = RecordingWindow::attach(&launcher);

    // The user presses Enter: the staged program takes the old one's name
    // and place, the old one is renamed out of its way and kept, the new
    // one answers with its version and is started, told what it replaces
    // and which process to wait for, and once it reported that it started
    // the window is asked to quit Pane. Nothing is downloaded again.
    block_on(launcher.activate_selected());
    assert_restarted(&launcher, "99.0.0");
    assert_eq!(
        dirs.relaunch.started(),
        [vec![
            "--updated-from".to_owned(),
            "0.1.0".to_owned(),
            "--wait-pid".to_owned(),
            std::process::id().to_string(),
        ]]
    );
    assert_eq!(window.quits(), 1);
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(
        fs::read(dirs.install.path().join("pane.old")).unwrap(),
        b"the 0.1.0 program"
    );
    assert_eq!(dirs.installed(), ["pane", "pane.old"]);
    assert_eq!(dirs.releases.listed(), 1);
    assert_eq!(dirs.releases.downloaded(".zip"), 1);
    assert_eq!(dirs.releases.downloaded(".zip.sha256"), 1);

    // The new version starts: it keeps the old program until it has
    // settled, says it was updated when it is first shown, and its check
    // finds nothing newer and says nothing.
    drop(launcher);
    let launcher = dirs.launcher("99.0.0").with_updated_from("0.1.0");
    assert_eq!(dirs.installed(), ["pane", "pane.old"]);
    launcher.set_window_presence(WindowPresence::Shown);
    assert_eq!(
        toast_title(&launcher).as_deref(),
        Some("Updated to Pane 99.0.0")
    );
    block_on(launcher.check_application_update());
    assert!(!titles(&launcher).contains(&RESTART.to_owned()));
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    // Its window, hotkeys and tray are up: it has settled, and the next
    // start of it removes the old program.
    launcher.application_update_settled();
    drop(launcher);
    let _launcher = dirs.launcher("99.0.0");
    assert_eq!(dirs.installed(), ["pane"]);
}

#[test]
fn the_linux_packages_tarball_is_staged_and_installed() {
    let dirs = Dirs::new();
    // The Linux package: the release carries a `.tar.gz`, as `cargo xtask
    // package-linux` packs it, and staging unpacks it with the same
    // strictness as the zip the Windows package is.
    dirs.publish_update_tgz("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    select_title(&launcher, RESTART);

    block_on(launcher.activate_selected());

    assert_restarted(&launcher, "99.0.0");
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(dirs.installed(), ["pane", "pane.old"]);
    assert_eq!(dirs.releases.downloaded(".tar.gz"), 1);

    // A start of the new version after it settled removes what the update
    // left.
    drop(launcher);
    dirs.launcher("99.0.0").application_update_settled();
    let launcher = dirs.launcher("99.0.0");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.installed(), ["pane"]);
    assert_eq!(launcher.view().status, Status::Idle);
}

/// An AppImage's bytes, as `appimagetool` begins one: an ELF header with
/// the AppImage type 2 mark at byte 8, then `body`.
#[cfg(target_os = "linux")]
fn appimage(body: &[u8]) -> Vec<u8> {
    let mut bytes = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00".to_vec();
    bytes.extend_from_slice(body);
    bytes
}

/// A Pane running from an AppImage (#464, ADR 0049) stages and installs
/// the release's AppImage in that file's place, not the program the
/// tarball beside it holds. Only Linux has AppImages, so only its target
/// names one in a release.
#[cfg(target_os = "linux")]
#[test]
fn an_appimage_update_replaces_the_appimage() {
    let dirs = Dirs::new();
    let arch = target().strip_prefix("linux-").unwrap().to_owned();
    dirs.publish_update_tgz("99.0.0", b"the 99.0.0 program");
    let file = format!("Pane-99.0.0-{arch}.AppImage");
    dirs.releases
        .add_asset("99.0.0", &file, &appimage(b"the 99.0.0 AppImage"));
    let path = dirs
        .install
        .path()
        .join(format!("Pane-0.1.0-{arch}.AppImage"));
    fs::write(&path, appimage(b"the 0.1.0 AppImage")).unwrap();
    let launcher = dirs.restarting(
        Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
            .with_appimage_update(
                "0.1.0",
                ReleaseSource::local(dirs.releases.url()).unwrap(),
                path.clone(),
            ),
    );
    block_on(launcher.check_application_update());
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    select_title(&launcher, RESTART);

    block_on(launcher.activate_selected());

    assert_restarted(&launcher, "99.0.0");
    // The AppImage file keeps its path, which the menu's and the autostart
    // entries name, and holds the new AppImage, which was started; the old
    // one is beside it until the new version settled, and nothing else is
    // left.
    assert_eq!(fs::read(&path).unwrap(), appimage(b"the 99.0.0 AppImage"));
    let old = format!("Pane-0.1.0-{arch}.AppImage.old");
    assert_eq!(
        fs::read(dirs.install.path().join(&old)).unwrap(),
        appimage(b"the 0.1.0 AppImage")
    );
    assert_eq!(
        dirs.installed(),
        [format!("Pane-0.1.0-{arch}.AppImage"), old]
    );
    // The AppImage and its `.sha256` were downloaded, the tarball never.
    assert_eq!(dirs.releases.downloaded(".AppImage"), 1);
    assert_eq!(dirs.releases.downloaded(".AppImage.sha256"), 1);
    assert_eq!(dirs.releases.downloaded(".tar.gz"), 0);
}

#[test]
fn a_staged_update_survives_a_restart_and_a_stale_one_is_removed() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(launcher.application_update(), ready("99.0.0"));

    // Pane restarts before the user chose: the staged update is ready at
    // once, its row listed, with no check and no download.
    drop(launcher);
    let launcher = dirs.launcher("0.1.0");
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert!(titles(&launcher).contains(&RESTART.to_owned()));
    assert_eq!(dirs.releases.listed(), 1);
    assert_eq!(dirs.releases.downloaded(".zip"), 1);

    // A start that already runs the staged version (the setup installed
    // it, say) removes it: it is stale.
    drop(launcher);
    let launcher = dirs.launcher("99.0.0");
    assert_eq!(dirs.installed(), ["pane"]);
    assert_eq!(launcher.application_update(), ApplicationUpdate::Unchecked);
    assert!(!titles(&launcher).contains(&RESTART.to_owned()));

    // So is a staging folder whose marker was never written: a download
    // that did not end.
    drop(launcher);
    fs::create_dir_all(dirs.install.path().join("update")).unwrap();
    fs::write(dirs.install.path().join("update/pane"), b"half a program").unwrap();
    let _launcher = dirs.launcher("0.1.0");
    assert_eq!(dirs.installed(), ["pane"]);
}

#[test]
fn a_check_is_made_at_start_and_then_by_the_schedule() {
    let dirs = Dirs::new();
    dirs.running(b"the 0.1.0 program");
    let clock = ManualClock::at(START);
    let launcher = dirs.launcher_at("0.1.0", &clock);

    // The schedule checks at once: no check was ever made.
    launcher.start_application_update_checks();
    wait_until("the check at start", || {
        launcher.application_update() == ApplicationUpdate::Current
    });
    assert_eq!(dirs.releases.listed(), 1);

    // A newer version is published. Before the interval has passed, the
    // schedule checks nothing.
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    clock.advance(Duration::from_secs(3 * 3600));
    thread::sleep(Duration::from_millis(300));
    assert_eq!(dirs.releases.listed(), 1);

    // Four hours and the random delay after the last check (at most
    // fifteen minutes), it checks, and downloads what it finds, quietly.
    clock.advance(Duration::from_secs(90 * 60));
    wait_until("the scheduled check found the update", || {
        launcher.application_update() == ready("99.0.0")
    });
    assert_eq!(dirs.releases.listed(), 2);
    assert_eq!(dirs.releases.downloaded(".zip"), 1);
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(titles(&launcher).contains(&RESTART.to_owned()));
}

#[test]
fn showing_the_launcher_after_a_stale_check_checks_again() {
    let dirs = Dirs::new();
    dirs.running(b"the 0.1.0 program");
    let clock = ManualClock::at(START);
    let launcher = dirs.launcher_at("0.1.0", &clock);
    block_on(launcher.check_application_update());
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    dirs.publish_update("99.0.0", b"the 99.0.0 program");

    // Shown again soon after the check: nothing is checked.
    launcher.set_window_presence(WindowPresence::Hidden);
    launcher.set_window_presence(WindowPresence::Shown);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(dirs.releases.listed(), 1);

    // Shown again more than four hours after it: a check is made, and the
    // update it finds is downloaded.
    launcher.set_window_presence(WindowPresence::Hidden);
    clock.advance(Duration::from_secs(4 * 3600 + 1));
    launcher.set_window_presence(WindowPresence::Shown);
    wait_until("the check when shown found the update", || {
        launcher.application_update() == ready("99.0.0")
    });
    assert_eq!(dirs.releases.listed(), 2);
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn an_unchanged_listing_is_answered_from_the_record() {
    let dirs = Dirs::new();
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.releases.not_modified(), 0);

    // The next check names the listing it read: GitHub answers `304`, and
    // the answer is the record's.
    block_on(launcher.check_application_update());
    assert_eq!(dirs.releases.listed(), 2);
    assert_eq!(dirs.releases.not_modified(), 1);
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);

    // A new release changes the listing: it is read again, and the update
    // downloaded.
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.releases.not_modified(), 1);
    assert_eq!(launcher.application_update(), ready("99.0.0"));

    // Unchanged again, even after a restart: the record names the update
    // already staged, so nothing is downloaded again.
    drop(launcher);
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(dirs.releases.not_modified(), 2);
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert_eq!(dirs.releases.downloaded(".zip"), 1);
}

#[test]
fn the_record_keeps_a_restart_from_checking_before_a_check_is_due() {
    let dirs = Dirs::new();
    dirs.running(b"the 0.1.0 program");
    let clock = ManualClock::at(START);
    let launcher = dirs.launcher_at("0.1.0", &clock);
    launcher.start_application_update_checks();
    wait_until("the check at start", || dirs.releases.listed() == 1);
    wait_until("the check ended", || {
        launcher.application_update() == ApplicationUpdate::Current
    });

    // Pane restarts an hour later: the record says when it last checked,
    // so the new launcher asks nothing.
    drop(launcher);
    clock.advance(Duration::from_secs(3600));
    let launcher = dirs.launcher_at("0.1.0", &clock);
    assert_eq!(launcher.application_update_checks().checked, Some(START));
    launcher.start_application_update_checks();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(dirs.releases.listed(), 1);

    // Once a check is due, it checks.
    clock.advance(Duration::from_secs(4 * 3600));
    wait_until("the check due after the restart", || {
        dirs.releases.listed() == 2
    });
    wait_until("the check ended", || {
        launcher.application_update() == ApplicationUpdate::Current
    });
}

#[test]
fn check_for_pane_updates_answers_when_pane_is_up_to_date() {
    let dirs = Dirs::new();
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The row is listed, found by its own name and by the one Raycast
    // gives the same command.
    block_on(launcher.set_query("check for updates"));
    assert!(titles(&launcher).contains(&CHECK_ROW.to_owned()));

    check_now(&launcher);

    // The user asked, so up to date is an answer, not a silence.
    assert_eq!(
        launcher.view().status,
        Status::Result("Pane is up to date".into())
    );
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    // A Pane that wires no updater lists no such row.
    let plain = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir());
    assert!(!titles(&plain).contains(&CHECK_ROW.to_owned()));
}

#[test]
fn the_state_the_about_page_reads_is_the_rows_own_state() {
    let dirs = Dirs::new();
    // No updater wired (no release source given): honestly nothing to
    // check, which the Settings About page explains instead of promising
    // a release or a feed.
    let plain = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir());
    assert_eq!(plain.application_update(), ApplicationUpdate::Unconfigured);
    // Wired, and no check has finished yet.
    let launcher = dirs.launcher("0.1.0");
    assert_eq!(launcher.application_update(), ApplicationUpdate::Unchecked);
    assert_eq!(launcher.application_update_checks().checked, None);
    // A check the user asked for answers even with nothing to offer, and
    // the state says which of the two it is: new enough, not unchecked.
    block_on(launcher.check_application_update_again());
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    assert!(launcher.application_update_checks().checked.is_some());
    assert_eq!(
        launcher.view().status,
        Status::Result("Pane is up to date".into())
    );
    // While a check runs the state says so; once it has answered, what it
    // found. The listing answers slowly, so the running check is seen.
    dirs.releases
        .stall(releases::LISTING, 1, Duration::from_millis(400));
    let checking = launcher.clone();
    let running = thread::spawn(move || block_on(checking.check_application_update_again()));
    wait_until("the check never showed as running", || {
        launcher.application_update() == ApplicationUpdate::Checking
    });
    running.join().unwrap();
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
}

#[test]
fn a_download_shows_its_progress_to_the_about_page_alone() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The package arrives slowly, in two pieces, so its bytes are seen
    // arriving one at a time.
    dirs.releases.stall(
        &releases::asset_path(&package_file("99.0.0")),
        8,
        Duration::from_millis(400),
    );

    let checking = launcher.clone();
    let checking = thread::spawn(move || block_on(checking.check_application_update()));
    // While the package is still arriving, the state says how far it has
    // come, and the core stays usable: root search lists its rows, with
    // no row for the update yet, and nothing on the status line.
    let mut progress = None;
    wait_until("no progress of the package", || {
        let update = launcher.application_update();
        if matches!(update, ApplicationUpdate::Downloading { bytes, .. } if bytes > 0) {
            progress = update.progress();
            true
        } else {
            false
        }
    });
    let progress = progress.unwrap();
    assert!(progress.contains("% of "), "{progress}");
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(titles(&launcher).contains(&"Install extension from folder…".to_owned()));
    assert!(!titles(&launcher).contains(&RESTART.to_owned()));
    // Typing in root search still works while the package arrives.
    block_on(launcher.set_query("nothing matches this"));
    assert_eq!(titles(&launcher), Vec::<String>::new());
    checking.join().unwrap();

    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn a_damaged_download_fails_quietly_and_a_check_downloads_it_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    // The package the release names is not what arrives: its bytes were
    // damaged, so they do not match the sha256 its `.sha256` gives.
    dirs.releases.corrupt_application();
    let launcher = dirs.launcher("0.1.0");

    block_on(launcher.check_application_update());

    // Nothing is said, no row is listed, and nothing is staged; the About
    // page reads why, and the record keeps it, for the log and the next
    // start.
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(!titles(&launcher).contains(&RESTART.to_owned()));
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Offered { failure: Some(why), .. }
                if why.contains("does not match the sha256")
        ),
        "{:?}",
        launcher.application_update()
    );
    let (_, why) = launcher.application_update_checks().failure.unwrap();
    assert!(why.starts_with("Could not download Pane 99.0.0: "), "{why}");
    assert_eq!(dirs.top_level(), ["pane"]);
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");

    // The source works again; the check the user asks for downloads it,
    // with its progress and its answer on the status line.
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    check_now(&launcher);
    assert_eq!(
        toast_title(&launcher).as_deref(),
        Some("Pane 99.0.0 is ready")
    );
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert_eq!(launcher.application_update_checks().failure, None);
    assert_eq!(dirs.releases.downloaded(".zip"), 2);
}

#[test]
fn a_swap_that_cannot_be_made_keeps_the_update_and_enter_tries_again() {
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

    // The user chooses the update. The row leaves the list while the
    // restart runs, so it cannot be started twice, and the selection moves
    // to another row meanwhile.
    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());

    // The failure is explained, and nothing changed: the program is still
    // the one running, and the update is still staged. The row is back
    // and selected, so the next Enter is the retry.
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not update Pane to 99.0.0: "),
        "{error}"
    );
    assert!(
        error.contains("the previous version's program could not be replaced"),
        "{error}"
    );
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Ready {
                failure: Some(_),
                ..
            }
        ),
        "{:?}",
        launcher.application_update()
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    assert!(dirs.install.path().join("update").join("pane").is_file());
    let view = launcher.view();
    let selected = view.selected.map(|index| view.rows[index].title.clone());
    assert_eq!(selected.as_deref(), Some(RESTART));

    // The name is free again; Enter alone restarts, with no new download.
    fs::remove_dir_all(&blocked).unwrap();
    block_on(launcher.activate_selected());
    assert_restarted(&launcher, "99.0.0");
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(dirs.releases.downloaded(".zip"), 1);
}

#[test]
fn a_staged_program_that_changed_is_downloaded_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    // Something wrote over the staged program after it was checked.
    fs::write(
        dirs.install.path().join("update").join("pane"),
        b"something else",
    )
    .unwrap();

    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());

    // Not installed: explained, and downloaded again by itself, so the row
    // is back.
    let error = error_of(&launcher);
    assert!(error.contains("downloads it again"), "{error}");
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert_eq!(dirs.releases.downloaded(".zip"), 2);
    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
}

#[test]
fn an_interrupted_download_is_tried_again() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The first download of the package is interrupted partway: the
    // connection closes after its first bytes.
    dirs.releases
        .drop_after(&releases::asset_path(&package_file("99.0.0")), 16, 1);

    block_on(launcher.check_application_update());

    // The interrupted download was tried again and staged.
    assert_eq!(launcher.application_update(), ready("99.0.0"));
    assert_eq!(dirs.releases.downloaded(".zip"), 2);
}

#[test]
fn an_unreachable_source_fails_quietly_and_the_check_the_user_asks_for_explains() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    assert_eq!(dirs.top_level(), ["pane"]);
    // Nothing answers where the releases are.
    drop(dirs.releases);

    block_on(launcher.check_application_update());

    // A check Pane made by itself says nothing and lists nothing new: the
    // About page reads the failure, and the record keeps it.
    assert_eq!(launcher.view().status, Status::Idle);
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Failed(why) if why.starts_with("Pane's releases at")
        ),
        "the failure is the state: {:?}",
        launcher.application_update()
    );
    let (_, why) = launcher.application_update_checks().failure.unwrap();
    assert!(why.starts_with("Could not check for an update: "), "{why}");
    assert_eq!(
        titles(&launcher),
        [
            // Pane's own rows, ranked with the rest by title (#199).
            CHECK_ROW,
            // The authoring rows (ADR 0047, #222).
            "Create Extension…",
            "Import Extension…",
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Settings…"
        ]
    );
    // The check the user asks for explains why it could not be made.
    check_now(&launcher);
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not check for a Pane update: "),
        "{error}"
    );
    assert!(
        error.contains("Pane's releases at http://127.0.0.1:")
            && error.contains("Pane tried 3 times"),
        "{error}"
    );
}

#[test]
fn a_check_the_source_answers_with_an_error_is_tried_again_by_the_user() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The source answers, but not with its listing.
    dirs.releases
        .fail_status(releases::LISTING, 404, usize::MAX);

    block_on(launcher.check_application_update());
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Failed(why) if why.contains("answered 404")
        ),
        "{:?}",
        launcher.application_update()
    );
    assert_eq!(launcher.view().status, Status::Idle);

    // The source works again; the check the user asks for finds the
    // update and downloads it.
    dirs.releases.stop_failing(releases::LISTING);
    check_now(&launcher);
    assert_eq!(
        toast_title(&launcher).as_deref(),
        Some("Pane 99.0.0 is ready")
    );
    assert!(titles(&launcher).contains(&RESTART.to_owned()));
}

#[test]
fn a_used_up_rate_limit_is_waited_out() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let clock = ManualClock::at(START);
    let launcher = dirs.launcher_at("0.1.0", &clock);
    // GitHub answers as it does once this address read its releases as
    // often as an hour allows.
    dirs.releases.rate_limit(releases::LISTING, 1);

    block_on(launcher.check_application_update());

    // Asked once, not three times, since trying again at once cannot help;
    // why is kept, with when the limit lifts.
    assert_eq!(dirs.releases.listed(), 1);
    let why = match launcher.application_update() {
        ApplicationUpdate::Failed(why) => why,
        other => panic!("not a failure: {other:?}"),
    };
    assert!(
        why.contains("GitHub limits how often Pane's releases can be read"),
        "{why}"
    );
    assert!(why.contains("try again in about 10 minutes"), "{why}");
    assert!(!why.contains("Pane tried"), "{why}");

    // Until it lifts, even the check the user asks for asks nothing, and
    // says when it can.
    clock.advance(Duration::from_secs(5 * 60));
    check_now(&launcher);
    let error = error_of(&launcher);
    assert!(error.contains("try again in about 5 minutes"), "{error}");
    assert_eq!(dirs.releases.listed(), 1);

    // Once it lifts, the check finds the update.
    clock.advance(Duration::from_secs(6 * 60));
    check_now(&launcher);
    assert_eq!(
        toast_title(&launcher).as_deref(),
        Some("Pane 99.0.0 is ready")
    );
    assert_eq!(dirs.releases.listed(), 2);
}

#[test]
fn a_release_without_its_checksum_is_not_downloaded() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.releases.remove_checksums();
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");

    block_on(launcher.check_application_update());

    // A package with no `.sha256` beside it cannot be checked, so it is
    // neither downloaded nor offered; the check says why.
    let file = package_file("99.0.0");
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Failed(why)
                if why.contains(&format!("no `{file}.sha256` to check it against"))
        ),
        "{:?}",
        launcher.application_update()
    );
    assert!(!titles(&launcher).contains(&RESTART.to_owned()));
    assert_eq!(dirs.releases.downloaded(".zip"), 0);
}

#[test]
fn a_download_redirected_away_from_github_is_refused() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // The package's download redirects somewhere other than where Pane's
    // releases are served (another port of this computer standing in for
    // another host): Pane does not follow it.
    let file = package_file("99.0.0");
    let download = format!("/pane-app/pane/releases/download/v99.0.0/{file}");
    dirs.releases
        .redirect(&download, &format!("http://127.0.0.1:9/{file}"), 1);

    block_on(launcher.check_application_update());

    // Refused, asked once (a redirect elsewhere is not tried again),
    // nothing downloaded from where it pointed, and nothing changed.
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Offered { failure: Some(why), .. }
                if why.contains("to 127.0.0.1:9, which is not one of GitHub's own hosts")
        ),
        "{:?}",
        launcher.application_update()
    );
    assert_eq!(
        dirs.releases
            .requests()
            .iter()
            .filter(|path| **path == download)
            .count(),
        1
    );
    assert_eq!(dirs.releases.downloaded(&file), 0);
    assert_eq!(dirs.top_level(), ["pane"]);

    // Without the redirect elsewhere, the next check downloads it.
    check_now(&launcher);
    assert_eq!(launcher.application_update(), ready("99.0.0"));
}

#[test]
fn a_listing_pane_cannot_take_is_noted_and_the_defaults_are_set_up() {
    let dirs = Dirs::new();
    // The sample's repository is served, so the default extension is
    // set up from it whatever Pane's releases say of Pane's own update.
    let sample = dirs.sample();
    dirs.running(b"the 0.1.0 program");
    // The only newer release carries another system's package.
    dirs.releases
        .publish_application("99.0.0", "some-other-system", b"whatever");
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(vec![sample])
        .with_application_update(
            "0.1.0",
            ReleaseSource::local(dirs.releases.url()).unwrap(),
            dirs.program(),
        );

    block_on(launcher.check_application_update());
    // A package for another system is not this Pane's business: the
    // release stays silent, offering nothing, exactly as an equal or older
    // version does.
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    assert_eq!(launcher.view().status, Status::Idle);
    block_on(launcher.acquire_defaults());

    // The default extension is still set up from its repository: one part
    // of Pane that cannot take a release never stops the other.
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
    // A listing that is not a list of releases (an error GitHub gives as
    // JSON, say) is a broken source.
    dirs.releases
        .serve_listing(r#"{ "message": "Not Found" }"#.to_owned());
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Failed(why) if why.contains("gave a listing that cannot be read")
        ),
        "{:?}",
        launcher.application_update()
    );
    // A release that names no newer version offers nothing and says
    // nothing: this Pane is new enough.
    dirs.releases.derived_listing();
    dirs.publish_update("0.0.1", b"the 0.0.1 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(launcher.application_update(), ApplicationUpdate::Current);
    assert!(!titles(&launcher).contains(&"Update Pane to 0.0.1".to_owned()));
}

#[test]
fn a_restart_keeps_pane_s_data() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    // The sample is set up first: a default extension, installed in
    // Pane's data folder under the install folder, exactly as the Windows
    // install keeps it.
    let sample = dirs.sample();
    let launcher = dirs.restarting(
        Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
            .with_defaults(vec![sample])
            .with_application_update(
                "0.1.0",
                ReleaseSource::local(dirs.releases.url()).unwrap(),
                dirs.program(),
            ),
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

    // The user restarts into it; its data — the installed extension and its
    // record — is untouched by the swap, which changes only the program.
    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());
    assert_restarted(&launcher, "99.0.0");
    assert_eq!(data, read_all(&dirs.install.path().join("data")));
    assert_eq!(dirs.top_level(), ["data", "pane", "pane.old"]);

    // A Pane starting with the new program still has its extensions: the
    // sample is installed, its record where it was, and once it settled,
    // nothing of the update is left in the install folder.
    drop(launcher);
    dirs.launcher("99.0.0").application_update_settled();
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

#[test]
fn a_new_version_that_does_not_report_is_ended_and_the_swap_undone() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    let window = RecordingWindow::attach(&launcher);
    // The new version starts, but never says so.
    dirs.relaunch.will(Starts::Silent);

    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());

    // It was ended, the old program has its name back, Pane keeps running
    // and says why, and the update stays staged, its row back and
    // selected, so Enter tries again.
    assert!(dirs.relaunch.ended.load(Ordering::SeqCst));
    assert_eq!(window.quits(), 0);
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Pane 99.0.0 could not start, so Pane stays at 0.1.0: ")
            && error.contains("did not report that it started"),
        "{error}"
    );
    assert!(
        matches!(
            launcher.application_update(),
            ApplicationUpdate::Ready { failure: Some(why), .. } if why == error
        ),
        "{:?}",
        launcher.application_update()
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    assert_eq!(
        dirs.installed(),
        [
            "pane",
            "update/.pane-staged.json",
            "update/README.txt",
            "update/pane"
        ]
    );
    let view = launcher.view();
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.rows[0].title, RESTART);

    // It reports this time: Pane restarts, with nothing downloaded again.
    dirs.relaunch.will(Starts::Reports);
    block_on(launcher.activate_selected());
    assert_restarted(&launcher, "99.0.0");
    assert_eq!(window.quits(), 1);
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(dirs.releases.downloaded(".zip"), 1);
}

#[test]
fn a_new_version_that_exits_cannot_start_or_is_another_is_undone() {
    for (starts, said) in [
        (Starts::Exits, "it exited as it started"),
        (Starts::Refuses, "it could not be started: access is denied"),
    ] {
        let dirs = Dirs::new();
        dirs.publish_update("99.0.0", b"the 99.0.0 program");
        dirs.running(b"the 0.1.0 program");
        let launcher = dirs.launcher("0.1.0");
        block_on(launcher.check_application_update());
        dirs.relaunch.will(starts);
        select_title(&launcher, RESTART);
        block_on(launcher.activate_selected());
        let error = error_of(&launcher);
        assert_eq!(
            error,
            format!("Pane 99.0.0 could not start, so Pane stays at 0.1.0: {said}")
        );
        assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
        assert!(!dirs.install.path().join("pane.old").exists());
        assert_eq!(launcher.view().rows[0].title, RESTART);
    }

    // A program that answers with another version than the release named
    // is not started at all.
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 98.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());
    let error = error_of(&launcher);
    assert!(
        error.ends_with(": the new program says it is Pane 98.0.0"),
        "{error}"
    );
    assert!(dirs.relaunch.started().is_empty());
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    // And one that does not run at all is not either.
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"not a program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    select_title(&launcher, RESTART);
    block_on(launcher.activate_selected());
    let error = error_of(&launcher);
    assert!(
        error.ends_with(": the new program does not run: it is no program"),
        "{error}"
    );
    assert!(dirs.relaunch.started().is_empty());
}

#[test]
fn the_toast_says_an_update_is_ready_once_and_restarts_into_it() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    // Hidden while the update is staged: nothing is said then, never as a
    // HUD over other applications.
    let window = RecordingWindow::attach(&launcher);
    launcher.set_window_presence(WindowPresence::Hidden);
    block_on(launcher.check_application_update());
    assert_eq!(launcher.toast(), None);
    assert!(window.huds().is_empty());
    // Collapsed to its search field, the launcher shows the row, and says
    // nothing either: the toast would be a HUD.
    launcher.set_window_presence(WindowPresence::Compact);
    assert_eq!(launcher.toast(), None);
    assert!(window.huds().is_empty());
    // Shown expanded, the toast says it once.
    launcher.set_window_presence(WindowPresence::Shown);
    let toast = launcher.toast().expect("the ready toast");
    assert_eq!(toast.toast.title, "Pane 99.0.0 is ready");
    assert_eq!(toast.toast.primary.as_ref().unwrap().title, "Restart");
    launcher.toast_left(toast.id, toast.revision);
    launcher.set_window_presence(WindowPresence::Hidden);
    launcher.set_window_presence(WindowPresence::Shown);
    assert_eq!(launcher.toast(), None);

    // Its Restart restarts, as the row does. (The toast is shown again for
    // the test to choose it.)
    let toast = {
        drop(launcher);
        let launcher = dirs.launcher("0.1.0");
        let window = RecordingWindow::attach(&launcher);
        launcher.set_window_presence(WindowPresence::Shown);
        let toast = launcher.toast().expect("the ready toast after a restart");
        block_on(launcher.run_toast_action(toast.id, ToastSlot::Primary));
        assert_restarted(&launcher, "99.0.0");
        assert_eq!(window.quits(), 1);
        toast
    };
    assert_eq!(toast.toast.title, "Pane 99.0.0 is ready");
}

#[test]
fn a_typed_query_finds_the_restart_among_the_results() {
    let dirs = Dirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let launcher = dirs.launcher("0.1.0");
    block_on(launcher.check_application_update());
    for query in ["restart", "update", "update pane to 99"] {
        block_on(launcher.set_query(query));
        assert!(titles(&launcher).contains(&RESTART.to_owned()), "{query}");
        // Found as any row is: no section of its own, nothing above the
        // pinned home, which a query hides anyway.
        let (_, listing) = launcher.presented_list();
        assert_eq!(listing.lead, 0, "{query}");
        assert_ne!(listing.sections[0].label, "Update", "{query}");
    }
    // Blank again, it is first of all.
    block_on(launcher.set_query(""));
    assert_eq!(titles(&launcher)[0], RESTART);
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
