//! The entries of a folder the user typed into root search (#204), through
//! the launcher's public interface, with the real Files default extension
//! and the Rust, JavaScript and TypeScript files samples, which give the
//! same answers: a query that is a path ending in a separator lists that
//! folder's direct entries as file results, folders first and each in name
//! order, `~` resolved to the home folder of the file index, a missing
//! folder listing nothing, and Enter on an entry opening a document and
//! showing a program in the file manager, never running it, through the
//! recording fakes. The Files extension's own copy of the checks adds the
//! rows declared for the path beside the entries, the 500-entry bound with
//! a row saying so when the folder holds more, and Tab's completion of the
//! query and Shift+Tab's removal of a path component as the launcher reads
//! them; the keys themselves are the window's
//! (`crates/pane/tests/file_actions.rs`). The packages are the ones
//! `cargo xtask guests` assembles in `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::file_index::{IndexerConfig, WalkOptions};
use pane_core::{Launcher, LinkOpener, Runtime, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use rows::{select_title, titles};
use settle::wait_for_merges;
use system::{Done, RecordingSystem};

/// Activates the selected row and answers what the HUD then said, if one
/// showed: Pane's own file actions close the window and say what they did
/// in one, as the standard actions do.
fn activated(launcher: &Launcher) -> Option<String> {
    let window = RecordingWindow::attach(launcher);
    block_on(launcher.activate_selected());
    window.huds().pop().map(|hud| hud.title)
}

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A package that answers a typed folder's entries, in one language.
struct Package {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
}

const FILES: Package = Package { package: "files" };
const RUST: Package = Package {
    package: "sample-files",
};
const JAVASCRIPT: Package = Package {
    package: "sample-files-js",
};
const TYPESCRIPT: Package = Package {
    package: "sample-files-ts",
};

/// A handler that records the files it is asked to open.
#[derive(Clone, Default)]
struct FakeOpener {
    files: Arc<Mutex<Vec<PathBuf>>>,
}

impl FakeOpener {
    fn take(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.files.lock().unwrap())
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

fn same_file(reported: &Path, made: &Path) -> bool {
    fs::canonicalize(reported).unwrap() == fs::canonicalize(made).unwrap()
}

/// The entries the fixture's typed folder holds, as a query lists them.
const ENTRIES: [&str; 4] = ["sub", "Zed notes ü.md", "alpha.txt", "run plan.bat"];

/// A home folder of controlled fixtures, which the file index covers (so
/// `~` resolves to it):
///
/// ```text
/// Pane home/
///   plan.txt
///   Browse — ñ/
///     Zed notes ü.md
///     alpha.txt
///     .hidden.txt
///     run plan.bat
///     sub/
///       plan.md
/// ```
struct Folder {
    /// Keeps the folders alive.
    _dir: TempDir,
    home: PathBuf,
    browse: PathBuf,
}

impl Folder {
    fn new() -> Folder {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("Pane home");
        for (file, text) in [
            ("plan.txt", "plan"),
            ("Browse — ñ/Zed notes ü.md", "zed"),
            ("Browse — ñ/alpha.txt", "alpha"),
            ("Browse — ñ/.hidden.txt", "hidden"),
            ("Browse — ñ/run plan.bat", "@echo off"),
            ("Browse — ñ/sub/plan.md", "plan"),
        ] {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let browse = home.join("Browse — ñ");
        Folder {
            _dir: dir,
            home,
            browse,
        }
    }

    /// A file of the typed folder, by its name.
    fn file(&self, name: &str) -> PathBuf {
        self.browse.join(name)
    }
}

/// The query that lists `folder`'s entries: the folder's path, as the user
/// typed it, ending in a separator.
fn typed(folder: &Path) -> String {
    let separator = if cfg!(windows) { "\\" } else { "/" };
    format!("{}{separator}", folder.display())
}

/// The file manager's name on this system.
fn manager() -> &'static str {
    if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    }
}

/// Pane with one files package installed, its file index over the fixture
/// home folder settled, and the fakes it acts through.
struct Pane {
    /// Keeps the fixture and Pane's data alive.
    _data: TempDir,
    launcher: Launcher,
    _runtime: Runtime,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
    folder: Folder,
}

impl Pane {
    fn new(fixture: &Package) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let system = Arc::new(RecordingSystem::default());
        runtime.set_applications(system.clone());
        let opener = FakeOpener::default();
        let folder = Folder::new();
        let index = IndexerConfig {
            first_walk_delay: Duration::ZERO,
            walk: WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            ..IndexerConfig::native(&data.path().join("cache"), folder.home.clone(), Vec::new())
        };
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
                .with_link_opener(Arc::new(opener.clone()))
                .with_system(system.clone())
                .with_file_index(index);
        block_on(launcher.install_package(&built(&format!("packages/{}", fixture.package))));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        assert!(
            launcher.wait_for_file_index(Duration::from_secs(30)),
            "{:?}",
            launcher.file_index_status()
        );
        Pane {
            _data: data,
            launcher,
            _runtime: runtime,
            opener,
            system,
            folder,
        }
    }

    fn search(&self, query: &str) {
        self.launcher.show_root_search();
        block_on(self.launcher.set_query(query));
        // The JavaScript and TypeScript guests answer past the 200 ms
        // budget while their engine starts: the answer merges into the
        // published list within 16 ms of the search's end (#201), which
        // the entries read below wait out.
        wait_for_merges(&self.launcher);
    }

    /// The entries the typed folder's query listed, by the rows' titles,
    /// whatever else the package lists beside them.
    fn entries(&self) -> Vec<String> {
        titles(&self.launcher)
            .into_iter()
            .filter(|title| ENTRIES.contains(&title.as_str()))
            .collect()
    }

    /// The index of the first row of the "Files" section.
    fn files_section(&self) -> usize {
        self.launcher
            .presentation()
            .sections
            .into_iter()
            .find(|section| section.label == "Files")
            .map(|section| section.first)
            .unwrap_or_else(|| panic!("no Files section: {:?}", titles(&self.launcher)))
    }
}

/// Declares one test per check for each language's files package, each of
/// which answers a typed folder's entries with its own bindings.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod files {
            $(#[test] fn $check() { super::$check(&super::FILES) })*
        }
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

/// The entries are listed folders first, in name order, as file results
/// under "Files".
fn the_entries_are_listed_folders_first_in_name_order(fixture: &Package) {
    let pane = Pane::new(fixture);
    pane.search(&typed(&pane.folder.browse));
    assert_eq!(pane.entries(), ENTRIES.map(String::from));
    assert_eq!(
        pane.launcher.view().rows[pane.files_section()].title,
        "sub",
        "the folder is first"
    );
}

/// `~` in the typed path resolves to the home folder of the file index.
fn a_tilde_resolves_to_the_home_folder(fixture: &Package) {
    let pane = Pane::new(fixture);
    pane.search("~/Browse — ñ/");
    assert_eq!(pane.entries(), ENTRIES.map(String::from));
}

/// A folder that does not exist lists nothing.
fn a_missing_folder_lists_nothing(fixture: &Package) {
    let pane = Pane::new(fixture);
    let missing = pane.folder.home.join("not there");
    pane.search(&typed(&missing));
    assert!(pane.entries().is_empty());
}

/// Enter opens a document of the typed folder through the system's
/// handler, and closes the window saying so.
fn enter_opens_a_document(fixture: &Package) {
    let pane = Pane::new(fixture);
    pane.search(&typed(&pane.folder.browse));
    select_title(&pane.launcher, "alpha.txt");
    assert_eq!(
        activated(&pane.launcher).as_deref(),
        Some("Opened alpha.txt")
    );
    match pane.opener.take().as_slice() {
        [opened] => assert!(same_file(opened, &pane.folder.file("alpha.txt"))),
        other => panic!("{other:?}"),
    }
    assert!(pane.system.take().is_empty());
}

/// Enter shows a program of the typed folder in the file manager and never
/// runs it.
fn enter_shows_a_program_and_runs_nothing(fixture: &Package) {
    let pane = Pane::new(fixture);
    pane.search(&typed(&pane.folder.browse));
    select_title(&pane.launcher, "run plan.bat");
    assert_eq!(
        activated(&pane.launcher).as_deref(),
        Some(format!("Showed run plan.bat in {}", manager()).as_str())
    );
    assert!(pane.opener.take().is_empty(), "nothing ran it");
    assert!(
        pane.system
            .take()
            .iter()
            .all(|done| matches!(done, Done::Revealed(_)))
    );
}

contract!(
    the_entries_are_listed_folders_first_in_name_order,
    a_tilde_resolves_to_the_home_folder,
    a_missing_folder_lists_nothing,
    enter_opens_a_document,
    enter_shows_a_program_and_runs_nothing,
);

/// The Files extension declares commands for the typed path beside the
/// entries: they are listed under "Addresses", above the entries.
#[test]
fn the_rows_declared_for_the_path_are_listed_above_the_entries() {
    let pane = Pane::new(&FILES);
    pane.search(&typed(&pane.folder.browse));
    let sections: Vec<(String, usize)> = pane
        .launcher
        .presentation()
        .sections
        .into_iter()
        .map(|section| (section.label, section.first))
        .collect();
    assert!(
        sections.contains(&("Addresses".to_owned(), 0)),
        "{sections:?}"
    );
    assert!(sections.contains(&("Files".to_owned(), 2)), "{sections:?}");
}

/// A folder with more entries than Pane lists is listed partially: the
/// first 500, and a row saying so at the end of them.
#[test]
fn a_folder_with_more_than_500_entries_is_listed_partially() {
    let pane = Pane::new(&FILES);
    let many = pane.folder.home.join("many");
    fs::create_dir(&many).unwrap();
    for number in 0..=500 {
        fs::write(many.join(format!("file {number:04}.txt")), "x").unwrap();
    }
    pane.search(&typed(&many));
    let titles = titles(&pane.launcher);
    assert!(titles.contains(&"file 0499.txt".to_owned()));
    assert!(
        !titles.contains(&"file 0500.txt".to_owned()),
        "the 501st entry is not listed"
    );
    assert_eq!(
        pane.launcher
            .view()
            .rows
            .iter()
            .next_back()
            .map(|row| row.title.as_str()),
        Some("…and more entries")
    );
    assert_eq!(
        pane.launcher.view().rows.len(),
        500 + 3,
        "the two rows for the path, 500 entries and the notice"
    );
}

/// Tab completes the query to a folder of the entries, and Shift+Tab
/// removes the query's last path component: the keys' own behaviour is the
/// window's, this reads what the launcher tells it to type.
#[test]
fn tab_completes_the_query_to_a_folder_and_shift_tab_removes_a_component() {
    let pane = Pane::new(&FILES);
    pane.search(&typed(&pane.folder.browse));
    // The folder row completes the query to its path, with a separator
    // after it, which lists the folder's own entries; a row that is not
    // one of them does not.
    select_title(&pane.launcher, "sub");
    assert_eq!(
        pane.launcher.typed_folder_completion(),
        Some(typed(&pane.folder.file("sub")))
    );
    pane.launcher.select(0);
    assert_eq!(pane.launcher.typed_folder_completion(), None);
    // Removing the last component of the folder's query lists the folder
    // above it, the one first typed.
    pane.search(&typed(&pane.folder.file("sub")));
    assert_eq!(
        pane.launcher.typed_path_parent(),
        Some(typed(&pane.folder.browse))
    );
    // A query that is not a path has no component to remove.
    pane.search("plan");
    assert_eq!(pane.launcher.typed_path_parent(), None);
}
