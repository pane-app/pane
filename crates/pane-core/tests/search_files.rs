//! Search Files like Raycast's (#177), through the launcher's public
//! interface: Pane's registered Files default extension, installed from
//! its pinned commit in a repository made from the Rust files sample's
//! component and served over Git's smart HTTP protocol from 127.0.0.1
//! (`support/repo_server.rs`, `support/defaults.rs`), over the real file
//! index of a fixture folder standing for the home folder, with a recording
//! opener and system so that nothing opens or shows. Search Files opens with
//! no folder to choose on "Recently Used" (the most recently modified
//! entries), ranks what is typed by the index's matching, keeps only the
//! type the dropdown chooses, loads more rows as the list scrolls and keeps
//! the query it was opened with from root search; the selected file's
//! detail has its Metadata and previews an image; its actions are Pane's,
//! Copy Name among them, and Enter on a program shows it without running
//! it; the index's state is said while it is built or stopped. A copy of
//! the Files package installed from a folder keeps its own search. The
//! window's side is `crates/pane/tests/window.rs`'s `search_files_split`.
//!
//! The Files extension's own sources live in their repository (#285),
//! which the tests cannot read; the repository the pin names here is
//! made to the default's package shape over the same sample's component
//! (`sample_files.wasm`, which implements the same contract), with the
//! manifest the default's own holds: its command's id `files`, which the
//! host keys the Search Files view on, and `"fileIndex": true`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use futures::executor::block_on;
use pane_core::file_index::{Category, IndexState, IndexerConfig, WalkOptions};
use pane_core::search_files::{FileType, PAGE, RECENTLY_USED};
use pane_core::{DefaultExtension, Launcher, LinkOpener, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/system.rs"]
mod system;

use rows::{select_title, titles};
use system::{Done, RecordingSystem};

const LIMIT: Duration = Duration::from_secs(30);

/// The id of Files' Search Files row in root search, as Pane's default
/// extension.
const COMMAND: &str = "default:files#files";

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

/// The files of the package the Files pin names: the Rust files sample's
/// component under the default's own manifest (its command's id `files`,
/// which the host keys the Search Files view on, `"fileIndex": true`),
/// as the default's repository holds its release revision.
fn package_files() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = defaults::package_files("sample-files")
        .into_iter()
        .filter(|(path, _)| path != "pane.json")
        .collect();
    files.push(("pane.json".into(), manifest().into_bytes()));
    files
}

/// The manifest of the Files package: the default's own shape, naming
/// the sample's component.
fn manifest() -> String {
    r#"{
  "manifestVersion": 1,
  "title": "Files",
  "version": "0.1.0",
  "apiVersion": "0.1",
  "commands": [
    {
      "id": "files",
      "title": "Search Files",
      "subtitle": "Finds the files of your home folder: recently used, by name and by type, with a preview",
      "component": "sample_files.wasm",
      "search": true,
      "rootResults": true
    }
  ],
  "fileIndex": true
}"#
        .to_owned()
}

/// `path`'s last modified time set `days` days before now.
fn modified_days_ago(path: &Path, days: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400 + 60))
        .unwrap();
}

/// The fixture home folder, Pane's data and cache folders, the server
/// serving Files' repository, and the fakes Pane acts through.
struct Home {
    dir: TempDir,
    home: PathBuf,
    /// Kept, not read: the repository's work tree, which the server serves
    /// as long as this lives.
    _repos: TempDir,
    /// Kept, not read: the server the repository is served from, which
    /// stops when this is dropped.
    _server: repo_server::Server,
    /// The pin that names the served repository.
    pin: DefaultExtension,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
}

impl Home {
    /// A home folder holding, newest first:
    ///
    /// ```text
    /// Pictures/shot.png         (today)
    /// Documents/plan.txt        (1 day ago)
    /// Documents/report.pdf      (2 days ago)
    /// Music/song.mp3            (3 days ago)
    /// Videos/clip.mp4           (4 days ago)
    /// Downloads/setup.exe       (5 days ago)
    /// Downloads/photos.zip      (6 days ago)
    /// Downloads/data.bin        (7 days ago)
    /// ```
    ///
    /// and the folders they are in.
    fn new() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        for (days, file) in [
            "Pictures/shot.png",
            "Documents/plan.txt",
            "Documents/report.pdf",
            "Music/song.mp3",
            "Videos/clip.mp4",
            "Downloads/setup.exe",
            "Downloads/photos.zip",
            "Downloads/data.bin",
        ]
        .into_iter()
        .enumerate()
        {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![b'x'; 100 + days]).unwrap();
            modified_days_ago(&path, days as u64);
        }
        let server = repo_server::Server::start();
        let repos = tempfile::tempdir().unwrap();
        let files = package_files();
        let tag = format!("v{}", defaults::version_of(&files));
        let pin = defaults::pinned(&server, repos.path(), "files", "Files", &tag, &files);
        Home {
            dir,
            home,
            _repos: repos,
            _server: server,
            pin,
            opener: FakeOpener::default(),
            system: Arc::new(RecordingSystem::default()),
        }
    }

    fn file(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    /// The file index over the fixture home, its first walk at once
    /// unless `deferred`.
    fn config(&self, deferred: bool) -> IndexerConfig {
        IndexerConfig {
            first_walk_delay: if deferred {
                Duration::from_secs(3600)
            } else {
                Duration::ZERO
            },
            walk: WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            ..IndexerConfig::native(&self.cache(), self.home.clone(), Vec::new())
        }
    }

    /// Pane with Files set up as its default extension, and its index
    /// settled unless `deferred`.
    fn start_with(&self, deferred: bool) -> Launcher {
        let runtime = Runtime::start().unwrap();
        runtime.set_applications(self.system.clone());
        let launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.dir.path().join("data/extensions"))
                .with_defaults(vec![self.pin.clone()])
                .with_link_opener(Arc::new(self.opener.clone()))
                .with_system(self.system.clone())
                .with_file_index(self.config(deferred));
        block_on(launcher.acquire_defaults());
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        if !deferred {
            assert!(
                launcher.wait_for_file_index(LIMIT),
                "{:?}",
                launcher.file_index_status()
            );
        }
        launcher
    }

    fn start(&self) -> Launcher {
        self.start_with(false)
    }
}

/// Opens Search Files from root search, as a user does.
fn open(launcher: &Launcher) {
    launcher.show_root_search();
    block_on(launcher.set_query("search files"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id == COMMAND)
        .unwrap_or_else(|| panic!("no row {COMMAND} in {:?}", launcher.view().rows));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::CommandSearch { .. }),
        "{:?}",
        launcher.view().status
    );
}

/// The rows listed: each one's title and folder.
fn listed(launcher: &Launcher) -> Vec<(String, Option<String>)> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| (row.title, row.subtitle))
        .collect()
}

/// The files (not folders) listed, by title, in order.
fn files_listed(launcher: &Launcher) -> Vec<String> {
    titles(launcher)
        .into_iter()
        .filter(|title| title.contains('.'))
        .collect()
}

#[test]
fn search_files_opens_on_recently_used_with_no_folder_to_choose() {
    let home = Home::new();
    let launcher = home.start();
    // Root search projects nothing.
    assert!(launcher.search_files_view().is_none());

    open(&launcher);
    let view = launcher.search_files_view().expect("Search Files is shown");
    assert_eq!(view.query, "");
    assert_eq!(view.filter, FileType::All);
    assert_eq!(view.section, Some(RECENTLY_USED));
    assert!(!view.loading);
    assert_eq!(view.status.state, IndexState::Current);
    assert_eq!(view.note(), None);
    // No folder to choose, nothing explained: the files, newest first.
    let rows = titles(&launcher);
    for absent in ["Choose folder…", "What is searched"] {
        assert!(!rows.iter().any(|row| row == absent), "{rows:?}");
    }
    assert_eq!(
        files_listed(&launcher),
        [
            "shot.png",
            "plan.txt",
            "report.pdf",
            "song.mp3",
            "clip.mp4",
            "setup.exe",
            "photos.zip",
            "data.bin"
        ]
    );
    let shot = listed(&launcher)
        .into_iter()
        .find(|(title, _)| title == "shot.png")
        .unwrap();
    assert_eq!(shot.1.as_deref(), Some("~/Pictures"));
    assert_eq!(launcher.selected(), Some(0));
    // Each row has the system's icon of its file (its outline until the
    // system's is loaded); there is none past the rows.
    let rows = titles(&launcher).len();
    for row in 0..rows {
        assert!(launcher.search_files_icon(row).is_some(), "row {row}");
    }
    assert!(launcher.search_files_icon(rows).is_none());
}

#[test]
fn typing_ranks_by_the_index_and_the_dropdown_filters_by_type() {
    let home = Home::new();
    let launcher = home.start();
    open(&launcher);

    block_on(launcher.set_query("plan"));
    let view = launcher.search_files_view().unwrap();
    assert_eq!(view.query, "plan");
    assert_eq!(view.section, None, "Recently Used is the blank query's");
    assert_eq!(titles(&launcher)[0], "plan.txt");

    // Cleared: Recently Used again, the dropdown's type kept.
    block_on(launcher.set_query(""));
    for (kind, only) in [
        (FileType::Of(Category::Images), vec!["shot.png"]),
        (FileType::Of(Category::Documents), vec!["report.pdf"]),
        (FileType::Of(Category::Text), vec!["plan.txt"]),
        (FileType::Of(Category::Audio), vec!["song.mp3"]),
        (FileType::Of(Category::Video), vec!["clip.mp4"]),
        (FileType::Of(Category::Applications), vec!["setup.exe"]),
        (FileType::Of(Category::Archives), vec!["photos.zip"]),
        (FileType::Of(Category::Other), vec!["data.bin"]),
    ] {
        block_on(launcher.set_file_type(kind));
        assert_eq!(launcher.search_files_view().unwrap().filter, kind);
        assert_eq!(titles(&launcher), only, "{kind:?}");
    }
    // Folders alone.
    block_on(launcher.set_file_type(FileType::Folder));
    let folders = titles(&launcher);
    for folder in ["Pictures", "Documents", "Downloads"] {
        assert!(folders.iter().any(|row| row == folder), "{folders:?}");
    }
    assert!(folders.iter().all(|row| !row.contains('.')), "{folders:?}");
    // A type and a query together.
    block_on(launcher.set_file_type(FileType::Of(Category::Text)));
    block_on(launcher.set_query("report"));
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    assert_eq!(
        launcher.search_files_view().unwrap().empty_note(),
        "No files found. Try another search or type."
    );
    block_on(launcher.set_file_type(FileType::All));
    assert_eq!(titles(&launcher)[0], "report.pdf");
}

#[test]
fn the_detail_shows_the_metadata_and_previews_an_image() {
    let home = Home::new();
    let launcher = home.start();
    open(&launcher);

    select_title(&launcher, "shot.png");
    let index = launcher.selected().unwrap();
    let details = launcher.search_files_details(index).unwrap();
    assert_eq!(details.name, "shot.png");
    assert_eq!(details.place, "~/Pictures");
    assert_eq!(details.kind, "PNG Image");
    assert_eq!(details.size, Some(100));
    assert!(details.modified.is_some());
    assert!(details.preview, "an image is previewed");
    assert!(same_file(&details.path, &home.file("Pictures/shot.png")));
    if cfg!(any(windows, target_os = "macos")) {
        assert!(
            details.created.is_some(),
            "the system says when it was made"
        );
    }

    select_title(&launcher, "plan.txt");
    let details = launcher
        .search_files_details(launcher.selected().unwrap())
        .unwrap();
    assert_eq!(details.kind, "TXT Text");
    assert!(!details.preview, "a text file shows its icon");

    select_title(&launcher, "Documents");
    let details = launcher
        .search_files_details(launcher.selected().unwrap())
        .unwrap();
    assert_eq!((details.kind.as_str(), details.size), ("Folder", None));
}

#[test]
fn more_rows_load_as_the_list_scrolls() {
    let home = Home::new();
    let pages = home.file("Pages");
    fs::create_dir_all(&pages).unwrap();
    let count = PAGE * 2 + 20;
    for page in 0..count {
        fs::write(pages.join(format!("page {page:03}.txt")), "x").unwrap();
    }
    let launcher = home.start();
    open(&launcher);
    block_on(launcher.set_query("page"));
    assert_eq!(titles(&launcher).len(), PAGE);
    assert!(launcher.search_files_view().unwrap().more);

    launcher.select(10);
    block_on(launcher.load_more_files());
    assert_eq!(titles(&launcher).len(), PAGE * 2);
    assert_eq!(launcher.selected(), Some(10), "the selection stays");
    block_on(launcher.load_more_files());
    let all = titles(&launcher);
    let pages_listed = all
        .iter()
        .filter(|title| title.starts_with("page "))
        .count();
    assert_eq!(pages_listed, count, "{all:?}");
    assert!(!launcher.search_files_view().unwrap().more);
    // Nothing more is asked once the index has no more.
    block_on(launcher.load_more_files());
    assert_eq!(titles(&launcher), all);
}

#[test]
fn opening_from_root_searchs_row_keeps_the_query() {
    let home = Home::new();
    let launcher = home.start();
    launcher.show_root_search();
    block_on(launcher.set_query("plan"));
    select_title(&launcher, "Search Files for “plan”");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::CommandSearch {
            query: "plan".into()
        }
    );
    assert_eq!(launcher.search_files_view().unwrap().query, "plan");
    assert_eq!(titles(&launcher)[0], "plan.txt");
}

#[test]
fn escape_clears_the_query_and_lists_recently_used_again() {
    let home = Home::new();
    let launcher = home.start();
    open(&launcher);
    let recent = titles(&launcher);
    block_on(launcher.set_query("plan"));
    assert_ne!(titles(&launcher), recent);

    // The first Escape clears the field; Recently Used is listed again in
    // the background.
    launcher.back();
    assert_eq!(
        launcher.view().screen,
        Screen::CommandSearch {
            query: String::new()
        }
    );
    let deadline = Instant::now() + LIMIT;
    while titles(&launcher) != recent {
        assert!(Instant::now() < deadline, "{:?}", titles(&launcher));
        std::thread::sleep(Duration::from_millis(10));
    }
    // The next leaves the command.
    launcher.back();
    assert!(launcher.search_files_view().is_none());
}

#[test]
fn a_files_actions_are_panes_own_with_copy_name_and_a_program_is_shown_not_run() {
    let home = Home::new();
    let launcher = home.start();
    open(&launcher);
    let actions = |launcher: &Launcher| -> Vec<String> {
        launcher
            .item_actions()
            .expect("the selected file has actions")
            .actions
            .into_iter()
            .map(|action| action.title)
            .collect()
    };
    let manager = if cfg!(windows) {
        "Show in Explorer"
    } else if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in File Manager"
    };
    let trash = if cfg!(windows) {
        "Move to Recycle Bin"
    } else {
        "Move to Trash"
    };

    select_title(&launcher, "report.pdf");
    assert_eq!(
        actions(&launcher),
        [
            "Open",
            manager,
            "Open With…",
            "Copy Path",
            "Copy Name",
            "Copy File",
            trash
        ]
    );
    // Copy Name copies the name alone.
    let target = launcher.item_actions().unwrap().target;
    let index = actions(&launcher)
        .iter()
        .position(|title| title == "Copy Name")
        .unwrap();
    block_on(launcher.run_item_action(&target, index));
    match home.system.take().as_slice() {
        [
            Done::Copied {
                clip: pane_core::system::Clip::Text(name),
                ..
            },
        ] => assert_eq!(name, "report.pdf"),
        other => panic!("{other:?}"),
    }

    // Enter opens a document.
    open(&launcher);
    select_title(&launcher, "report.pdf");
    block_on(launcher.activate_selected());
    let opened = home.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &home.file("Documents/report.pdf")));

    // Enter on a program shows it; nothing runs it.
    open(&launcher);
    select_title(&launcher, "setup.exe");
    assert_eq!(
        actions(&launcher)[..3],
        [
            manager.to_owned(),
            "Open With…".to_owned(),
            "Run".to_owned()
        ]
    );
    assert_eq!(launcher.selected_action().label, manager);
    block_on(launcher.activate_selected());
    match home.system.take().as_slice() {
        [Done::Revealed(path)] => assert!(same_file(path, &home.file("Downloads/setup.exe"))),
        other => panic!("{other:?}"),
    }
    assert!(home.opener.take().is_empty(), "Enter ran nothing");
}

#[test]
fn the_index_being_built_is_said() {
    let home = Home::new();
    // The first walk waits for the launcher to be shown, which no test
    // window does here: the index is being built all along.
    let launcher = home.start_with(true);
    open(&launcher);
    let view = launcher.search_files_view().unwrap();
    assert_eq!(view.status.state, IndexState::Building, "{:?}", view.status);
    assert_eq!(view.note().as_deref(), Some("Indexing… (0 found so far)"));
    assert!(!view.needs_settings());
    assert!(titles(&launcher).is_empty());
    assert_eq!(view.empty_note(), "Indexing… (0 found so far)");
}

#[test]
fn a_stopped_index_says_why_and_leads_to_the_settings() {
    let home = Home::new();
    // A file where the cache folder should be: the index cannot be kept.
    fs::write(home.cache(), "not a folder").unwrap();
    let launcher = home.start_with(true);
    let deadline = Instant::now() + LIMIT;
    while launcher.file_index_status().state != IndexState::Stopped {
        assert!(
            Instant::now() < deadline,
            "{:?}",
            launcher.file_index_status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    open(&launcher);
    let view = launcher.search_files_view().unwrap();
    let note = view.note().expect("the view says why");
    assert!(note.starts_with("File search stopped: "), "{note}");
    assert!(view.needs_settings());
}

#[test]
fn a_copy_installed_from_a_folder_keeps_its_own_search() {
    let home = Home::new();
    let launcher = home.start();
    let copy = tempfile::tempdir().unwrap();
    for (path, contents) in package_files() {
        fs::write(copy.path().join(path), contents).unwrap();
    }
    block_on(launcher.install_package(copy.path()));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    let local = pane_core::PackageIdentity::local(copy.path()).unwrap();
    launcher.show_root_search();
    block_on(launcher.set_query("search files"));
    let id = format!("{}#files", local.key());
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id == id)
        .unwrap_or_else(|| panic!("no row {id} in {:?}", launcher.view().rows));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(launcher.search_files_view().is_none());
    assert_eq!(titles(&launcher), ["What is searched"]);
}
