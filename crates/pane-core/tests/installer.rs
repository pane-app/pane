//! Installing Pane and acquiring its default extensions through the
//! launcher's public interface (#53): a first setup that downloads the
//! calculator and the prebuilt-helper sample from an artifact source on
//! 127.0.0.1 (`support/artifacts.rs`; nothing reaches the network or
//! Pane's published downloads) and installs them as packages from folders
//! are, into managed copies with the default extensions' own identity,
//! with progress, retries and compatible cache reuse, while the core
//! (root search, the install rows, Manage extensions) stays usable.
//!
//! The packages are the ones `cargo xtask guests` assembles under
//! `target/guests/packages`: the real calculator (`guests/calculator`) and
//! the helper sample (`guests/sample-helper`) with its real helper
//! program `pane-echo` built for this system, so the helper is run as an
//! end user runs it: a prebuilt program from the payload, with no Node,
//! Rust, npm, Git or compiler involved.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::defaults::ArtifactSource;
use pane_core::{DefaultExtension, IconSource, Launcher, PackageIdentity, Runtime, Status, Target};
use serde_json::Value;
use tempfile::TempDir;

#[path = "support/artifacts.rs"]
mod artifacts;

use artifacts::Artifacts;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guests;
use rows::{select_title, titles};

/// The default set the tests acquire: the calculator, and the
/// prebuilt-helper sample with it, a payload carrying a native helper.
/// Pane's own build no longer acquires the sample (#162); the core still
/// acquires whatever default set it is given, helpers included.
fn defaults() -> Vec<DefaultExtension> {
    vec![
        DefaultExtension {
            id: "calculator".into(),
            title: "Calculator".into(),
        },
        DefaultExtension {
            id: "helper-sample".into(),
            title: "Helper sample".into(),
        },
    ]
}

/// The files of the assembled package `package` under
/// `target/guests/packages`, by their path in the package.
fn package_files(package: &str) -> Vec<(String, Vec<u8>)> {
    let folder = guests().join("packages").join(package);
    assert!(
        folder.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    read_files(&folder, "")
}

fn read_files(folder: &Path, prefix: &str) -> Vec<(String, Vec<u8>)> {
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
            files.extend(read_files(&path, &in_package));
        } else {
            files.push((in_package, fs::read(&path).unwrap()));
        }
    }
    files
}

/// The version a package's `pane.json` declares.
fn version_of(files: &[(String, Vec<u8>)]) -> String {
    let manifest = files
        .iter()
        .find(|(path, _)| path == "pane.json")
        .map(|(_, contents)| contents.clone())
        .expect("the package has a pane.json");
    let manifest: Value = serde_json::from_slice(&manifest).unwrap();
    manifest["version"].as_str().unwrap().to_owned()
}

/// The helper sample's payload: its assembled package, with its
/// `pane.json` naming this system's helper target — the artifact this
/// build serves carries the helper built for the system it runs on.
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

/// Pane's data location, compiled code cache, runtime and artifact source
/// for one test.
struct Dirs {
    data: TempDir,
    /// Kept, not read: it holds the compiled code cache alive.
    _cache: TempDir,
    runtime: Runtime,
    artifacts: Artifacts,
}

impl Dirs {
    fn new() -> Dirs {
        let cache = tempfile::tempdir().unwrap();
        Dirs {
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start_with_cache(cache.path().to_path_buf()).unwrap(),
            artifacts: Artifacts::start(),
            _cache: cache,
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder acquiring its default extensions
    /// from the local artifact source; a new one is a restart of Pane.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_defaults(
                ArtifactSource::local(self.artifacts.url()).unwrap(),
                defaults(),
            )
    }

    /// Publishes the calculator's and the helper sample's payloads, at the
    /// versions their `pane.json`s declare.
    fn publish(&self) {
        let calculator = package_files("calculator");
        self.artifacts.publish(
            "calculator",
            &version_of(&calculator),
            &borrowed(&calculator),
        );
        let helper = helper_files();
        self.artifacts
            .publish("helper-sample", &version_of(&helper), &borrowed(&helper));
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

    /// The payload files cached for the default extension `id`, and
    /// everything else in its cache folder.
    fn acquired(&self, id: &str) -> Vec<String> {
        let folder = self.packages_dir().join("acquired").join(id);
        let entries: Vec<String> = fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        let mut entries = entries;
        entries.sort();
        entries
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

fn borrowed(files: &[(String, Vec<u8>)]) -> Vec<(&str, Vec<u8>)> {
    files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.clone()))
        .collect()
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

/// The files of the managed copy at `location`, by their path in it.
fn files_of(location: &Path) -> Vec<String> {
    let mut files = read_files(location, "");
    files.sort_by(|(a, _), (b, _)| a.cmp(b));
    files.into_iter().map(|(path, _)| path).collect()
}

#[test]
fn a_first_setup_acquires_the_calculator_and_it_answers() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    assert_eq!(
        launcher.view().status,
        Status::Result("Set up Pane's default extensions".into())
    );
    // Both default extensions are installed, from Pane's own downloads.
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    let identity = PackageIdentity::default_extension("calculator");
    assert_eq!(identity.key(), "default:calculator");
    let package = &launcher.packages()[0];
    assert_eq!(package.identity, identity);
    let record = dirs.record("calculator");
    assert_eq!(record["default"], "calculator");
    assert_eq!(record["defaultVersion"], "0.5.0");
    assert_eq!(record.get("local"), None);
    assert_eq!(record.get("npm"), None);
    // The managed copy holds the manifest and the component and tile icon
    // it names (#163), installed as a package from a folder is, and the
    // calculator shows that tile from its managed copy.
    assert_eq!(
        files_of(&package.location),
        ["calculator.wasm", "icon.svg", "pane.json"]
    );
    let icon = launcher.icon_of(&identity.key()).expect("the icon");
    let IconSource::Image { light, dark } = &icon.source else {
        panic!("the calculator's icon is not its tile: {icon:?}");
    };
    assert!(light.starts_with(&package.location), "{light:?}");
    assert_eq!(light.file_name().unwrap(), "icon.svg");
    assert_eq!(dark, light);
    // The payload is cached; nothing is left in the downloads folder.
    assert_eq!(dirs.acquired("calculator").len(), 1);
    dirs.wait_for_no_downloads();

    // The calculator answers in root search: an expression lists its
    // answer first, and invoking it copies the answer.
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("6*7 = 42 · Enter copies the answer")
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Copied 42 to the clipboard".into())
    );

    // After a restart the default extensions are listed from their managed
    // copies, with nothing fetched again.
    let asked = dirs.artifacts.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    assert_eq!(dirs.artifacts.requests().len(), asked);
    search(&launcher, "12*3");
    assert_eq!(titles(&launcher), ["36"]);
}

#[test]
fn acquiring_shows_progress_and_leaves_the_core_usable() {
    let dirs = Dirs::new();
    dirs.publish();
    // The calculator's payload arrives slowly, in two pieces, so its
    // bytes arrive one at a time.
    dirs.artifacts.stall(".tgz", 16, Duration::from_millis(400));
    let launcher = dirs.launcher();

    // Acquisition runs in the background: another thread drives it, as
    // the window does.
    let acquiring = launcher.clone();
    let acquiring = thread::spawn(move || block_on(acquiring.acquire_defaults()));
    // While the payload is still arriving, the status line says what is
    // being acquired, and the core stays usable: root search lists its
    // rows, including the install rows.
    let deadline = Instant::now() + Duration::from_secs(10);
    let progress = loop {
        assert!(Instant::now() < deadline, "no progress of the calculator");
        if let Status::Progress(text) = launcher.view().status
            && text.contains('%')
        {
            break text;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        progress.starts_with("Acquiring the Calculator"),
        "{progress}"
    );
    assert!(progress.contains("0%"), "{progress}");
    assert!(titles(&launcher).contains(&"Install extension from folder…".to_owned()));
    assert!(titles(&launcher).contains(&"Install extension from npm…".to_owned()));
    // Typing in root search still works while the payload arrives.
    search(&launcher, "calc");
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
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
}

#[test]
fn an_interrupted_download_is_tried_again_and_set_up() {
    let dirs = Dirs::new();
    dirs.publish();
    // The first download of the calculator's payload is interrupted
    // partway: the connection closes after its first bytes.
    dirs.artifacts.drop_after("calculator-0.5.0.tgz", 16, 1);
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    // The interrupted download was tried again and set up.
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
    // The calculator's payload was asked for twice: the interrupted
    // download and the one that recovered it. The helper sample's was
    // asked for once.
    let calculator_requests = dirs
        .artifacts
        .requests()
        .iter()
        .filter(|path| path.contains("calculator"))
        .count();
    assert_eq!(calculator_requests, 2);
    // Nothing half-written is left in the cache: the payload is there,
    // complete, and no `.part` file.
    assert_eq!(dirs.acquired("calculator").len(), 1);
    assert!(
        !dirs
            .acquired("calculator")
            .iter()
            .any(|file| file.ends_with(".part"))
    );
}

#[test]
fn a_cached_payload_is_reused_and_a_damaged_one_is_replaced() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    let downloaded = dirs.artifacts.payload_requests().len();
    assert_eq!(downloaded, 2);

    // Uninstalling the calculator and acquiring it again reuses the
    // cached payload: the index is read, but nothing is downloaded again.
    let identity = PackageIdentity::default_extension("calculator");
    block_on(launcher.uninstall(&identity, pane_core::SavedData::Delete));
    assert_eq!(
        launcher.view().status,
        Status::Result("Uninstalled Calculator and deleted its saved data".into())
    );
    let asked = dirs.artifacts.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Helper sample", "Calculator"]);
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
    // Only the index was asked for again; the payload was not downloaded.
    assert_eq!(dirs.artifacts.payload_requests().len(), downloaded);
    assert_eq!(dirs.artifacts.requests().len() - asked, 1);

    // A damaged cache entry is not reused: it does not match the
    // integrity its index gives, so the payload is downloaded again.
    let cached: Vec<PathBuf> = fs::read_dir(dirs.packages_dir().join("acquired/calculator"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(cached.len(), 1);
    fs::write(&cached[0], b"damaged").unwrap();
    let identity = PackageIdentity::default_extension("calculator");
    block_on(launcher.uninstall(&identity, pane_core::SavedData::Delete));
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        dirs.artifacts.payload_requests().len(),
        downloaded + 1,
        "the damaged payload was downloaded again"
    );
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
}

/// A default extension a build no longer acquires stays what it became:
/// an installed package (#162, the helper sample leaving Pane's default
/// set). A Pane whose default set is the calculator alone, started over
/// data that acquired the helper sample before, keeps it installed and
/// listed, downloads nothing for it, and lets the user uninstall it, after
/// which no first setup brings it back.
#[test]
fn a_default_extension_that_left_the_default_set_stays_until_uninstalled() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    drop(launcher);

    // The next build's default set no longer has the helper sample.
    let calculator_only = || {
        Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
            .with_defaults(
                ArtifactSource::local(dirs.artifacts.url()).unwrap(),
                vec![DefaultExtension {
                    id: "calculator".into(),
                    title: "Calculator".into(),
                }],
            )
    };
    let downloaded = dirs.artifacts.payload_requests().len();
    let launcher = calculator_only();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Calculator", "Helper sample"],
        "Pane removes nothing it acquired"
    );
    assert_eq!(dirs.record("helper-sample")["default"], "helper-sample");
    assert_eq!(dirs.artifacts.payload_requests().len(), downloaded);
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
    assert_eq!(installed(&launcher), ["Calculator"]);
    drop(launcher);
    let launcher = calculator_only();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Calculator"],
        "a first setup does not bring it back"
    );
    assert_eq!(dirs.artifacts.payload_requests().len(), downloaded);
}

#[test]
fn an_unreachable_source_leaves_the_core_usable_and_a_row_tries_again() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    // Nothing answers where the artifact source is.
    drop(dirs.artifacts);

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
        error.contains("Pane's downloads at http://127.0.0.1:")
            && error.contains("Pane tried 3 times"),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
    assert_eq!(
        titles(&launcher),
        [
            "Install extension from folder…",
            "Install extension from npm…",
            "Install extension from Git…",
            "Create Extension…",
            "Import Extension…",
            "Set up Calculator",
            "Set up Helper sample",
            // Pane's own row, listed after every command.
            "Settings…"
        ]
    );
    // The row tries again, and the failure is explained again: the
    // source is still unreachable, and the row stays.
    select_title(&launcher, "Set up Calculator");
    block_on(launcher.activate_selected());
    let error = error_of(&launcher);
    assert!(
        error.starts_with("Could not set up the Calculator: "),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Calculator".to_owned()));
    assert!(titles(&launcher).contains(&"Set up Helper sample".to_owned()));
}

#[test]
fn a_row_tries_again_and_sets_the_extension_up() {
    let dirs = Dirs::new();
    dirs.publish();
    // The payload is not there yet: the source answers 404 for it.
    let launcher = dirs.launcher();
    dirs.artifacts
        .fail_status("calculator-0.5.0.tgz", 404, usize::MAX);
    block_on(launcher.acquire_defaults());
    let error = error_of(&launcher);
    assert!(
        error.contains("the payload `calculator-0.5.0.tgz` its index names is not there"),
        "{error}"
    );
    // The helper sample was set up; only the calculator failed.
    assert_eq!(installed(&launcher), ["Helper sample"]);

    // The source answers now; the row that tries again sets it up.
    dirs.artifacts.stop_failing("calculator-0.5.0.tgz");
    select_title(&launcher, "Set up Calculator");
    block_on(launcher.activate_selected());

    assert_eq!(installed(&launcher), ["Helper sample", "Calculator"]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Set up the Calculator".into())
    );
    // The row is gone: what it asked for is there, and the calculator
    // answers.
    assert!(!titles(&launcher).contains(&"Set up Calculator".to_owned()));
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
}

#[test]
fn an_acquired_helper_runs_from_the_managed_copy() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    // The helper sample is installed, and its helper program is this
    // system's prebuilt file, copied into the managed copy with the
    // permission Pane gives it.
    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
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

#[test]
fn a_payload_that_cannot_be_used_is_explained_and_not_installed() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    // The helper sample's payload is not the one its index describes:
    // its bytes were damaged, so they do not match its integrity.
    let calculator = package_files("calculator");
    dirs.artifacts.publish(
        "calculator",
        &version_of(&calculator),
        &borrowed(&calculator),
    );
    let helper = helper_files();
    dirs.artifacts
        .publish("helper-sample", &version_of(&helper), &borrowed(&helper));
    dirs.artifacts.corrupt("helper-sample");

    block_on(launcher.acquire_defaults());

    // The calculator is set up; the helper sample is explained and not
    // installed, with the row that tries again.
    assert_eq!(installed(&launcher), ["Calculator"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Helper sample: "),
        "{error}"
    );
    assert!(
        error.contains("does not match the sha512 integrity"),
        "{error}"
    );
    assert!(titles(&launcher).contains(&"Set up Helper sample".to_owned()));
    // Nothing it downloaded was kept.
    dirs.wait_for_no_downloads();
}

#[test]
fn an_incompatible_payload_is_explained_and_not_installed() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    // The calculator's payload declares a platform this is not.
    let mut files = package_files("calculator");
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
    dirs.artifacts
        .publish("calculator", &version_of(&files), &borrowed(&files));
    let helper = helper_files();
    dirs.artifacts
        .publish("helper-sample", &version_of(&helper), &borrowed(&helper));

    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Helper sample"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Calculator: Not available on"),
        "{error}"
    );
    assert!(error.contains("this package supports only"), "{error}");
}

#[test]
fn a_payload_without_its_declared_helper_file_is_explained_and_not_installed() {
    let dirs = Dirs::new();
    let calculator = package_files("calculator");
    dirs.artifacts.publish(
        "calculator",
        &version_of(&calculator),
        &borrowed(&calculator),
    );
    // The helper sample's payload declares this system's helper target,
    // but its tarball holds no helper file: an artifact that is not
    // compatible with this system.
    let mut helper = helper_files();
    helper.retain(|(path, _)| !path.starts_with("helpers/"));
    let version = version_of(&helper);
    dirs.artifacts
        .publish("helper-sample", &version, &borrowed(&helper));
    let target = Target::current().unwrap();
    let launcher = dirs.launcher();

    block_on(launcher.acquire_defaults());

    // The calculator is set up; the helper sample is explained and not
    // installed, with the row that tries again.
    assert_eq!(installed(&launcher), ["Calculator"]);
    let error = error_of(&launcher);
    assert!(
        error.contains("Could not set up the Helper sample: "),
        "{error}"
    );
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
    // Nothing it downloaded was kept.
    dirs.wait_for_no_downloads();
}

#[test]
fn a_disabled_default_extension_is_not_acquired_again() {
    let dirs = Dirs::new();
    dirs.publish();
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    // Disabling the calculator is the opt-out: it stays installed, so
    // acquisition never re-acquires or re-enables it.
    let identity = PackageIdentity::default_extension("calculator");
    block_on(launcher.set_enabled(&identity, false));
    let asked = dirs.artifacts.requests().len();

    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());

    assert_eq!(installed(&launcher), ["Calculator", "Helper sample"]);
    let packages = launcher.packages();
    let calculator = packages.iter().find(|p| p.identity == identity).unwrap();
    assert!(!calculator.enabled);
    assert_eq!(dirs.artifacts.requests().len(), asked);
    // Its command contributes nothing while it is disabled.
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}
