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
//! and TypeScript): the default extensions Quicklinks and Files, whose
//! own commands were declared for the typed query, left this repository
//! with their sources (#285), so what they did with the address or path
//! they were sent is their own repositories' to test.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::file_index::IndexerConfig;
use pane_core::{CommandMatches, CommandWhen, Launcher, Manifest, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
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
    fn start(&self) -> Launcher {
        let runtime = Runtime::start().unwrap();
        let config = IndexerConfig::native(
            &self.data.path().join("cache"),
            self.home.clone(),
            Vec::new(),
        );
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
            .with_file_index(config)
    }

    /// Starts Pane and installs the assembled package `name` under
    /// `target/guests/packages`, leaving root search showing.
    fn with(&self, name: &str) -> Launcher {
        let launcher = self.start();
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
        let launcher = pane.with(self.package);
        (pane, launcher)
    }
}

fn a_url_like_query_lists_only_the_command_declared_for_it(fixture: &Fixture) {
    let (_pane, launcher) = fixture.pane();

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
    let (_pane, launcher) = fixture.pane();

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
        let launcher = pane.start();
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
