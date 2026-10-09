//! Typed web addresses and paths in root search (#195): what the query is,
//! understood once per change of it — a URL-like query (an absolute URL
//! with a scheme, or a bare domain with `https://` inferred) or a path-like
//! one (a drive letter and a separator, `\\`, `~` resolved to the home
//! folder, `/`, or `file://`) — and the commands that declare `when` and
//! `matches` in their `pane.json`, listed only for such a query, without
//! title matching, below the results and above the files, and sent the
//! parsed address or resolved path as their launch record's fallback text.
//! Through the launcher's public interface with the real sample packages
//! `cargo xtask guests` assembles (`sample-matches` in Rust, JavaScript
//! and TypeScript), and the default extensions Quicklinks and Files, whose
//! own commands are declared for them, with the recording system for
//! opening and revealing (`support/system.rs`) and the recording link
//! opener of the files suite.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::file_index::IndexerConfig;
use pane_core::system::System;
use pane_core::{
    CommandMatches, CommandWhen, Launcher, LinkOpener, Manifest, Runtime, Screen, Status,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/system.rs"]
mod recording;
#[path = "support/rows.rs"]
mod rows;

use feedback::{RecordingWindow, shown};
use recording::{Done, RecordingSystem};
use rows::{select_title, titles};

/// Pane's data location, its sources, and the home folder `~` resolves to:
/// the one its file index is configured with, in these tests a folder of
/// the test's own (the index itself never runs: no package here uses it,
/// so it only names the home folder).
struct Pane {
    data: TempDir,
    sources: TempDir,
    /// The home folder's own directory, kept for as long as the Pane.
    _home: TempDir,
    home: PathBuf,
}

/// The fakes a test hands Pane: what its commands ask of the system, and
/// the link opener that opens the files it is asked to open. `None` for
/// the real ones (which these tests never reach).
struct Fakes {
    system: Option<Arc<RecordingSystem>>,
    links: Option<Arc<dyn LinkOpener>>,
}

impl Pane {
    fn new() -> Pane {
        let home = tempfile::tempdir().unwrap();
        let home_path = home.path().to_path_buf();
        Pane {
            data: tempfile::tempdir().unwrap(),
            sources: tempfile::tempdir().unwrap(),
            _home: home,
            home: home_path,
        }
    }

    /// Starts Pane on this data location, with its home folder.
    fn start(&self, fakes: Fakes) -> Launcher {
        let runtime = Runtime::start().unwrap();
        let system = fakes.system.map(|system| {
            runtime.set_applications(system.clone());
            let recording: Arc<dyn System> = system;
            recording
        });
        let config = IndexerConfig::native(
            &self.data.path().join("cache"),
            self.home.clone(),
            Vec::new(),
        );
        let mut launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
                .with_file_index(config);
        if let Some(system) = system {
            launcher = launcher.with_system(system);
        }
        if let Some(links) = fakes.links {
            launcher = launcher.with_link_opener(links);
        }
        launcher
    }

    /// Starts Pane and installs the assembled package `name` under
    /// `target/guests/packages`, leaving root search showing.
    fn with(&self, name: &str, fakes: Fakes) -> Launcher {
        let launcher = self.start(fakes);
        let folder = package(name, &self.sources.path().join(name));
        block_on(launcher.install_package(&folder));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        assert!(matches!(launcher.view().screen, Screen::Root { .. }));
        launcher
    }
}

struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-matches",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-matches-js",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-matches-ts",
};

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn package(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Types `query` and invokes the row titled `title`; what the user reads.
fn send(launcher: &Launcher, query: &str, title: &str) -> Status {
    search(launcher, query);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// The section labels over root search's rows, with the index of each
/// section's first row.
fn sections(launcher: &Launcher) -> Vec<(String, usize)> {
    launcher
        .presentation()
        .sections
        .into_iter()
        .map(|section| (section.label, section.first))
        .collect()
}

impl Fixture {
    fn pane(&self) -> (Pane, Launcher) {
        let pane = Pane::new();
        let launcher = pane.with(
            self.package,
            Fakes {
                system: None,
                links: None,
            },
        );
        (pane, launcher)
    }
}

fn a_url_like_query_lists_only_the_command_declared_for_it(fixture: &Fixture) {
    let (_, launcher) = fixture.pane();

    search(&launcher, "https://github.com/pane-app/pane");
    assert_eq!(titles(&launcher), ["Hear an Address"]);
    assert_eq!(
        sections(&launcher),
        [("Addresses".into(), 0)],
        "below the results, above the files: {:?}",
        sections(&launcher)
    );
    assert_eq!(
        send(
            &launcher,
            "https://github.com/pane-app/pane",
            "Hear an Address"
        ),
        Status::Result("Heard “https://github.com/pane-app/pane”".into()),
        "the parsed address is the fallback text"
    );

    // A bare domain is one, with `https://` inferred; what is not an
    // address or a path is words, and title matching never lists the
    // command.
    assert_eq!(
        send(&launcher, "github.com", "Hear an Address"),
        Status::Result("Heard “https://github.com”".into())
    );
    search(&launcher, "1.5");
    assert_eq!(titles(&launcher), Vec::<String>::new(), "not an address");
    search(&launcher, "address");
    assert_eq!(
        titles(&launcher),
        Vec::<String>::new(),
        "never matched by its title"
    );
}

fn windows_and_unix_paths_are_paths(fixture: &Fixture) {
    let (_, launcher) = fixture.pane();

    let path = if cfg!(windows) {
        r"C:\Windows".to_owned()
    } else {
        "/etc/hosts".to_owned()
    };
    assert_eq!(
        send(&launcher, &path, "Hear a Path"),
        Status::Result(format!("Heard “{path}”"))
    );
    // A `file://` URL names the path after it, an empty authority and
    // optionally a drive letter included.
    let (url, path) = if cfg!(windows) {
        ("file:///C:/Windows".to_owned(), "C:/Windows".to_owned())
    } else {
        ("file:///etc/hosts".to_owned(), "/etc/hosts".to_owned())
    };
    assert_eq!(
        send(&launcher, &url, "Hear a Path"),
        Status::Result(format!("Heard “{path}”"))
    );
}

fn a_tilde_resolves_to_the_home_folder(fixture: &Fixture) {
    let (pane, launcher) = fixture.pane();

    assert_eq!(
        send(&launcher, "~", "Hear a Path"),
        Status::Result(format!("Heard “{}”", pane.home.to_string_lossy()))
    );
    assert_eq!(
        send(&launcher, "~/Documents", "Hear a Path"),
        Status::Result(format!(
            "Heard “{}”",
            pane.home.join("Documents").to_string_lossy()
        ))
    );
}

fn when_is_honoured(fixture: &Fixture) {
    let (_, launcher) = fixture.pane();

    // With a blank query, only the command declared for one is listed.
    search(&launcher, "");
    let listed = titles(&launcher);
    assert!(listed.contains(&"Blank Only".to_owned()), "{listed:?}");
    assert!(!listed.contains(&"Searching Only".to_owned()), "{listed:?}");
    assert!(
        !listed.contains(&"Hear an Address".to_owned()),
        "{listed:?}"
    );
    assert!(!listed.contains(&"Hear a Path".to_owned()), "{listed:?}");

    // While searching, only the command declared for that is: not the one
    // a query would find by its title.
    search(&launcher, "blank only");
    assert_eq!(
        titles(&launcher),
        Vec::<String>::new(),
        "Blank Only is not listed while something is typed"
    );
    search(&launcher, "searching only");
    assert_eq!(titles(&launcher), ["Searching Only"]);
}

impl Fixture {
    /// Installs this sample with its `pane.json` changed by `change`: the
    /// pane, its launcher and the status line.
    fn install_changed(
        &self,
        change: impl FnOnce(&mut serde_json::Value),
    ) -> (Pane, Launcher, Status) {
        let pane = Pane::new();
        let folder = package(self.package, &pane.sources.path().join("changed"));
        let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        change(&mut manifest);
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        let launcher = pane.start(Fakes {
            system: None,
            links: None,
        });
        block_on(launcher.install_package(&folder));
        let status = launcher.view().status;
        (pane, launcher, status)
    }
}

fn the_when_and_matches_are_read_from_the_manifest(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = package(fixture.package, &pane.sources.path().join("read"));
    let manifest = Manifest::read(&folder).unwrap();
    let read: Vec<(&str, CommandWhen, CommandMatches)> = manifest
        .commands
        .iter()
        .map(|command| (command.id.as_str(), command.when, command.matches))
        .collect();
    assert_eq!(
        read,
        [
            ("url", CommandWhen::Searching, CommandMatches::Url),
            ("path", CommandWhen::Searching, CommandMatches::FilePath),
            ("blank", CommandWhen::Blank, CommandMatches::Title),
            ("searching", CommandWhen::Searching, CommandMatches::Title),
        ]
    );
}

fn an_unknown_when_is_refused_at_install_with_the_reason(fixture: &Fixture) {
    let (_pane, launcher, status) = fixture.install_changed(|manifest| {
        manifest["commands"][0]["when"] = "sometimes".into();
    });
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(
        error.contains(
            "command `url` has the when \"sometimes\"; a command's `when` is \"always\" \
             (the default), \"blank\" (only while nothing is typed) or \"searching\" \
             (only while something is)"
        ),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
}

fn an_unknown_matches_is_refused_at_install_with_the_reason(fixture: &Fixture) {
    let (_pane, launcher, status) = fixture.install_changed(|manifest| {
        manifest["commands"][1]["matches"] = "address".into();
    });
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(
        error.contains(
            "command `path` has the matches \"address\"; a command's `matches` is \
             \"title\" (the default), \"url\" or \"file-path\""
        ),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
}

/// Declares one test per check for each sample.
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
    a_url_like_query_lists_only_the_command_declared_for_it,
    windows_and_unix_paths_are_paths,
    a_tilde_resolves_to_the_home_folder,
    when_is_honoured,
    the_when_and_matches_are_read_from_the_manifest,
    an_unknown_when_is_refused_at_install_with_the_reason,
    an_unknown_matches_is_refused_at_install_with_the_reason,
);

/// The recording link opener of the files suite: records the files it is
/// asked to open, opens no link.
#[derive(Clone, Default)]
struct FakeOpener {
    files: Arc<std::sync::Mutex<Vec<PathBuf>>>,
}

impl FakeOpener {
    /// The files it was asked to open, in order.
    fn opened(&self) -> Vec<PathBuf> {
        self.files.lock().unwrap().clone()
    }
}

impl LinkOpener for FakeOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        panic!("no link is opened here: {url}")
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        self.files.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

/// Pane with the Files default extension installed over a home folder
/// holding `notes/plan.md` and `notes/run plan.bat`, and the fakes its
/// rows act through.
struct FilesPane {
    pane: Pane,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
}

impl FilesPane {
    /// The path of the file `below` its home folder, as it is typed.
    fn file(&self, below: &str) -> String {
        self.pane.home.join(below).to_string_lossy().into_owned()
    }

    /// What opening and revealing did so far, taking the records.
    fn acted(&self) -> (Vec<PathBuf>, Vec<Done>) {
        (self.opener.opened(), self.system.take())
    }
}

fn files_pane() -> (FilesPane, Launcher) {
    let pane = Pane::new();
    let notes = pane.home.join("notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("plan.md"), "plan").unwrap();
    fs::write(notes.join("run plan.bat"), "@echo off").unwrap();
    let system = Arc::new(RecordingSystem::default());
    let opener = FakeOpener::default();
    let links: Arc<dyn LinkOpener> = Arc::new(opener.clone());
    let launcher = pane.with(
        "files",
        Fakes {
            system: Some(system.clone()),
            links: Some(links),
        },
    );
    (
        FilesPane {
            pane,
            opener,
            system,
        },
        launcher,
    )
}

#[test]
fn a_typed_path_offers_files_opening_and_revealing_it() {
    let (files, launcher) = files_pane();

    search(&launcher, &files.file("notes/plan.md"));
    assert_eq!(titles(&launcher), ["Open", "Reveal in File Explorer"]);
    assert_eq!(
        sections(&launcher),
        [("Addresses".into(), 0)],
        "below the results, above the files"
    );
    select_title(&launcher, "Open");
    block_on(launcher.activate_selected());
    let (opened, done) = files.acted();
    assert_eq!(opened, [PathBuf::from(files.file("notes/plan.md"))]);
    assert!(done.is_empty(), "nothing is revealed: {done:?}");

    select_title(&launcher, "Reveal in File Explorer");
    block_on(launcher.activate_selected());
    let (opened, done) = files.acted();
    assert!(opened.is_empty(), "nothing is opened: {opened:?}");
    assert_eq!(
        done,
        [Done::Revealed(PathBuf::from(files.file("notes/plan.md")))]
    );
}

#[test]
fn opening_a_typed_program_reveals_it_and_never_runs_it() {
    let (files, launcher) = files_pane();

    search(&launcher, &files.file("notes/run plan.bat"));
    assert_eq!(titles(&launcher), ["Open", "Reveal in File Explorer"]);
    select_title(&launcher, "Open");
    block_on(launcher.activate_selected());
    let (opened, done) = files.acted();
    assert!(opened.is_empty(), "a program is never opened or run");
    assert_eq!(
        done,
        [Done::Revealed(PathBuf::from(
            files.file("notes/run plan.bat")
        ))]
    );
}

#[test]
fn a_typed_address_offers_quicklinks_opening_and_saving_it() {
    let pane = Pane::new();
    let system = Arc::new(RecordingSystem::default());
    let launcher = pane.with(
        "quicklinks",
        Fakes {
            system: Some(system.clone()),
            links: None,
        },
    );
    let window = RecordingWindow::attach(&launcher);

    // A typed address lists the two commands declared for it, and only
    // them; a query that is not one lists none of them.
    search(&launcher, "https://example.com/pane");
    assert_eq!(titles(&launcher), ["Open in Browser", "Create Quicklink"]);
    assert_eq!(
        launcher
            .selected()
            .and_then(|index| launcher.view().rows.get(index).cloned())
            .map(|row| row.title),
        Some("Open in Browser".to_owned()),
        "the first address row is selected"
    );
    assert_eq!(
        sections(&launcher),
        [("Addresses".into(), 0)],
        "below the results, above the files"
    );
    search(&launcher, "quicklink");
    assert!(!titles(&launcher).contains(&"Open in Browser".to_owned()));
    assert_eq!(
        titles(&launcher)
            .iter()
            .filter(|title| title.as_str() == "Create Quicklink")
            .count(),
        1,
        "only the command matched by its title: {:?}",
        titles(&launcher)
    );

    // Enter opens the address with the system's handler, `https://`
    // inferred before a bare domain, and closes the window.
    window.take();
    system.take();
    search(&launcher, "example.com/pane");
    select_title(&launcher, "Open in Browser");
    block_on(launcher.activate_selected());
    assert_eq!(
        system.take(),
        [Done::Opened {
            target: "https://example.com/pane".into(),
            application: None,
        }]
    );
    assert_eq!(window.take(), [WindowRequest::Hide]);

    // Create Quicklink opens its form with the address filled in, and
    // saving makes a quicklink of it.
    search(&launcher, "https://example.com/pane");
    select_title(&launcher, "Create Quicklink");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    let form = view.form().expect("the form is open");
    assert_eq!(
        form.fields
            .iter()
            .find(|field| field.id == "link")
            .map(|field| field.value.clone()),
        Some("https://example.com/pane".into()),
        "the address is prefilled: {:?}",
        form.fields
    );
    launcher.set_field_value("name", "Example");
    block_on(launcher.submit_form());
    assert_eq!(shown(&launcher), Status::Result("Created “Example”".into()));
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    search(&launcher, "example");
    assert!(titles(&launcher).contains(&"Example".to_owned()));
}
