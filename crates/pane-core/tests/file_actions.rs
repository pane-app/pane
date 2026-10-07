//! Search Files and the file actions Pane performs itself (#150), through
//! the launcher's public interface, with the real Files default extension
//! and the JavaScript and TypeScript files samples, which give the same
//! answers: the command owns the launcher's search field and lists the
//! granted folder's files as the user types, a newer text stopping the
//! search before it; a document's actions are Open (Enter), Reveal in
//! Explorer (Ctrl+Enter), Open With…, Copy Path, Copy File and Move to
//! Recycle Bin (destructive, confirmed), each closing the window; for a
//! program or script, Enter reveals it, Ctrl+Enter is Open With… and only
//! Run runs it, in Search Files and in root search's file results alike.
//! Recording fakes stand in for the system's handler of files (the link
//! opener), the system (reveal, open with an application, the clipboard,
//! the Recycle Bin and the installed applications) and the window, so
//! nothing opens, moves or shows. The packages are the ones
//! `cargo xtask guests` assembles in `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::files::{FolderListing, Folders, Limits, Listed};
use pane_core::system::Clip;
use pane_core::{
    ConfirmAnswer, Launcher, LinkOpener, PackageIdentity, Runtime, Screen, Status, SubmenuState,
    WindowPresence,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use rows::{select_title, titles};
use system::{Done, RecordingSystem};

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

/// A package that searches the files of a granted folder, in one language.
struct Package {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its command's title.
    command: &'static str,
}

const RUST: Package = Package {
    package: "files",
    title: "Files",
    command: "Search Files",
};
const JAVASCRIPT: Package = Package {
    package: "sample-files-js",
    title: "JavaScript files sample",
    command: "Find files (JavaScript)",
};
const TYPESCRIPT: Package = Package {
    package: "sample-files-ts",
    title: "TypeScript files sample",
    command: "Find files (TypeScript)",
};

/// What revealing a file is called on this system.
fn reveal() -> String {
    let manager = if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    };
    format!("Reveal in {manager}")
}

/// What moving a file to the trash is called on this system.
fn trash() -> String {
    let trash = if cfg!(target_os = "windows") {
        "Recycle Bin"
    } else {
        "Trash"
    };
    format!("Move to {trash}")
}

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

/// Whether `reported` is the file the test made at `made`.
fn same_file(reported: &Path, made: &Path) -> bool {
    fs::canonicalize(reported).unwrap() == fs::canonicalize(made).unwrap()
}

/// A folder of controlled fixtures:
///
/// ```text
/// Pane files/
///   Résumé plan ü.txt
///   notes/plan.md
///   notes/todo.txt
///   notes/run plan.bat
///   notes/plan script        (executable, on macOS and Linux)
/// ```
struct Folder {
    _dir: TempDir,
    root: PathBuf,
}

impl Folder {
    fn new() -> Folder {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Pane files");
        for (file, text) in [
            ("Résumé plan ü.txt", "résumé"),
            ("notes/plan.md", "plan"),
            ("notes/todo.txt", "todo"),
            ("notes/run plan.bat", "@echo off"),
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = root.join("notes/plan script");
            fs::write(&script, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Folder { _dir: dir, root }
    }

    fn file(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

/// Pane with one files package installed and a folder granted to it, and
/// the fakes it acts through.
struct Pane {
    _data: TempDir,
    launcher: Launcher,
    _runtime: Runtime,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
    window: Arc<RecordingWindow>,
    folder: Folder,
    fixture: &'static Package,
}

impl Pane {
    /// Pane with `fixture`'s package installed and the fixture folder
    /// granted to it, its granted folders listed through `folders` when
    /// given.
    fn new(fixture: &'static Package, folders: Option<Arc<dyn Folders>>) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let system = Arc::new(RecordingSystem::default());
        runtime.set_applications(system.clone());
        if let Some(folders) = folders {
            runtime.set_folders(folders);
        }
        let opener = FakeOpener::default();
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
                .with_link_opener(Arc::new(opener.clone()))
                .with_system(system.clone());
        let window = RecordingWindow::attach(&launcher);
        block_on(launcher.install_package(&built(&format!("packages/{}", fixture.package))));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        let folder = Folder::new();
        let identity = launcher
            .packages()
            .into_iter()
            .find(|package| package.title() == fixture.title)
            .expect("the package is installed")
            .identity;
        block_on(launcher.grant_folder(&identity, &folder.root));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        Pane {
            _data: data,
            launcher,
            _runtime: runtime,
            opener,
            system,
            window,
            folder,
            fixture,
        }
    }

    /// Opens the command from root search, its search field empty. Chosen
    /// from the blank root search, so that no query asks the package's root
    /// results (and lists its folder) first.
    fn open(&self) {
        let launcher = &self.launcher;
        launcher.show_root_search();
        select_title(launcher, self.fixture.command);
        block_on(launcher.activate_selected());
        assert_eq!(
            launcher.view().screen,
            Screen::CommandSearch {
                query: String::new()
            },
            "{:?}",
            launcher.view().status
        );
    }

    /// Types `query` into the field on screen and waits for its answer.
    fn search(&self, query: &str) {
        block_on(self.launcher.set_query(query));
    }

    /// Selects the row `title`, with the window shown, as the user sees it.
    fn select(&self, title: &str) {
        self.launcher.set_window_presence(WindowPresence::Shown);
        select_title(&self.launcher, title);
        self.window.take();
    }

    /// The titles of the selected row's actions.
    fn actions(&self) -> Vec<String> {
        self.launcher
            .item_actions()
            .expect("the selected row has actions")
            .actions
            .into_iter()
            .map(|action| action.title)
            .collect()
    }

    /// The index of the selected row's action `title`.
    fn action(&self, title: &str) -> usize {
        self.actions()
            .iter()
            .position(|action| action == title)
            .unwrap_or_else(|| panic!("no action {title:?} in {:?}", self.actions()))
    }

    /// The selected row's id: the target the Actions panel holds.
    fn target(&self) -> String {
        self.launcher
            .item_actions()
            .expect("the selected row has actions")
            .target
    }

    /// Runs the selected row's action `title` from the Actions panel.
    fn run(&self, title: &str) {
        let index = self.action(title);
        block_on(self.launcher.run_item_action(&self.target(), index));
    }

    /// Whether the window was asked to hide since the row was selected.
    fn closed(&self) -> bool {
        self.window.hides() > 0
    }
}

/// The documents of the fixture folder `query` finds, and their own
/// folders as Pane names them.
fn listed(pane: &Pane) -> Vec<(String, Option<String>)> {
    pane.launcher
        .view()
        .rows
        .into_iter()
        .map(|row| (row.title, row.subtitle))
        .collect()
}

fn search_files_lists_the_files_in_the_launchers_own_field(fixture: &'static Package) {
    let pane = Pane::new(fixture, None);
    pane.open();
    // Its own list: Pane's folder rows, then the command's.
    assert_eq!(titles(&pane.launcher)[0], "Choose folder…");

    pane.search("résumé");
    let view = pane.launcher.view();
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: "résumé".into()
        }
    );
    assert_eq!(
        listed(&pane),
        [(
            "Résumé plan ü.txt".to_owned(),
            Some("File in Pane files".to_owned())
        )]
    );
    pane.search("todo");
    assert_eq!(
        listed(&pane),
        [(
            "todo.txt".to_owned(),
            Some("File in Pane files/notes".to_owned())
        )]
    );
    // Cleared, the command's own list is back.
    pane.search("");
    assert_eq!(titles(&pane.launcher)[0], "Choose folder…");
}

fn a_documents_actions_act_through_the_system_and_close_the_window(fixture: &'static Package) {
    let pane = Pane::new(fixture, None);
    pane.open();
    pane.search("todo");
    pane.select("todo.txt");
    let todo = pane.folder.file("notes/todo.txt");
    assert_eq!(
        pane.actions(),
        [
            "Open".to_owned(),
            reveal(),
            "Open With…".to_owned(),
            "Copy Path".to_owned(),
            "Copy File".to_owned(),
            trash(),
        ]
    );
    let actions = pane.launcher.item_actions().unwrap();
    assert!(actions.actions[2].submenu, "Open With… opens a submenu");
    assert!(actions.actions[5].destructive);
    assert_eq!(pane.launcher.selected_action().label, "Open");

    // Enter opens it with the system's handler, and the window closes.
    block_on(pane.launcher.activate_selected());
    let opened = pane.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &todo));
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Opened todo.txt".into())
    );
    assert!(pane.closed());

    // Ctrl+Enter reveals it.
    pane.select("todo.txt");
    block_on(pane.launcher.run_selected_action(1));
    match pane.system.take().as_slice() {
        [Done::Revealed(path)] => assert!(same_file(path, &todo)),
        other => panic!("{other:?}"),
    }
    assert!(pane.closed());

    // Copy Path and Copy File copy, close the window and say so.
    pane.select("todo.txt");
    pane.run("Copy Path");
    match pane.system.take().as_slice() {
        [
            Done::Copied {
                clip: Clip::Text(path),
                concealed: false,
            },
        ] => assert!(same_file(Path::new(path), &todo)),
        other => panic!("{other:?}"),
    }
    let huds: Vec<String> = pane
        .window
        .huds()
        .into_iter()
        .map(|hud| hud.title)
        .collect();
    assert_eq!(huds, ["Copied to Clipboard"]);
    assert!(pane.closed());
    pane.select("todo.txt");
    pane.run("Copy File");
    match pane.system.take().as_slice() {
        [
            Done::Copied {
                clip: Clip::File(path),
                concealed: false,
            },
        ] => assert!(same_file(path, &todo)),
        other => panic!("{other:?}"),
    }

    // Open With… lists the installed applications by name; the one chosen
    // opens the file.
    pane.select("todo.txt");
    let target = pane.target();
    block_on(
        pane.launcher
            .open_submenu(&target, pane.action("Open With…")),
    );
    let submenu = pane.launcher.submenu().expect("the submenu is open");
    assert_eq!(submenu.title, "Open With");
    let SubmenuState::Listed(entries) = &submenu.state else {
        panic!("{:?}", submenu.state);
    };
    let names: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(names, ["code editor", "Notepad", "Zed"]);
    block_on(pane.launcher.run_submenu_entry(&target, 1));
    match pane.system.take().as_slice() {
        [
            Done::Opened {
                target,
                application: Some(application),
            },
        ] => {
            assert!(same_file(Path::new(target), &todo));
            assert_eq!(application, "app:Notepad");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Opened todo.txt with Notepad".into())
    );
    assert!(pane.closed());
    assert!(pane.opener.take().is_empty());
}

/// Runs the selected row's Move to Recycle Bin on a thread of its own,
/// answers its confirmation with `answer`, and waits for it to end.
fn trash_selected(pane: &Pane, answer: ConfirmAnswer) {
    let pending = pane
        .launcher
        .run_item_action(&pane.target(), pane.action(&trash()));
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        block_on(pending);
        let _ = done.send(());
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let asked = loop {
        if let Some(asked) = pane.launcher.confirmation() {
            break asked;
        }
        assert!(Instant::now() < deadline, "no confirmation was asked");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(asked.destructive);
    assert_eq!(asked.primary, trash());
    assert!(!asked.rememberable, "Pane asks each time");
    pane.launcher.answer_confirmation(asked.id, answer, false);
    finished
        .recv_timeout(Duration::from_secs(10))
        .expect("the action ended");
}

fn move_to_recycle_bin_is_confirmed_first(fixture: &'static Package) {
    let pane = Pane::new(fixture, None);
    pane.open();
    pane.search("todo");

    // Dismissed: nothing is moved, and the window stays.
    pane.select("todo.txt");
    trash_selected(&pane, ConfirmAnswer::Dismissed);
    assert!(pane.system.take().is_empty());
    assert!(!pane.closed());

    // Confirmed: it is moved, the window closes and a HUD says so.
    pane.select("todo.txt");
    trash_selected(&pane, ConfirmAnswer::Confirmed);
    match pane.system.take().as_slice() {
        [Done::Trashed(paths)] => {
            assert_eq!(paths.len(), 1);
            assert!(same_file(&paths[0], &pane.folder.file("notes/todo.txt")));
        }
        other => panic!("{other:?}"),
    }
    let huds: Vec<String> = pane
        .window
        .huds()
        .into_iter()
        .map(|hud| hud.title)
        .collect();
    assert_eq!(huds, [trash().replace("Move to", "Moved to")]);
    assert!(pane.closed());
}

/// Checks that the selected row, the program or script named `name` at
/// `path`, is revealed by Enter, offers Open With… on Ctrl+Enter and runs
/// only through Run.
fn a_program_runs_only_through_run(pane: &Pane, name: &str, path: &Path) {
    pane.select(name);
    assert_eq!(
        pane.actions(),
        [
            reveal(),
            "Open With…".to_owned(),
            "Run".to_owned(),
            "Copy Path".to_owned(),
            "Copy File".to_owned(),
            trash(),
        ]
    );
    assert_eq!(pane.launcher.selected_action().label, reveal());

    // Enter reveals it; nothing runs it.
    block_on(pane.launcher.activate_selected());
    match pane.system.take().as_slice() {
        [Done::Revealed(revealed)] => assert!(same_file(revealed, path)),
        other => panic!("{other:?}"),
    }
    assert!(pane.opener.take().is_empty(), "Enter ran nothing");
    assert!(pane.closed());

    // Ctrl+Enter is Open With…, a submenu the window opens: the launcher
    // runs nothing.
    pane.select(name);
    assert!(pane.launcher.item_actions().unwrap().actions[1].submenu);
    block_on(pane.launcher.run_selected_action(1));
    assert!(pane.system.take().is_empty());
    assert!(pane.opener.take().is_empty());

    // Run runs it.
    pane.select(name);
    pane.run("Run");
    let ran = pane.opener.take();
    assert_eq!(ran.len(), 1);
    assert!(same_file(&ran[0], path));
    assert_eq!(
        pane.launcher.view().status,
        Status::Result(format!("Ran {name}"))
    );
    assert!(pane.closed());
}

fn enter_never_runs_a_program_in_search_files(fixture: &'static Package) {
    let pane = Pane::new(fixture, None);
    pane.open();
    pane.search("run plan");
    let path = pane.folder.file("notes/run plan.bat");
    a_program_runs_only_through_run(&pane, "run plan.bat", &path);
    #[cfg(unix)]
    {
        pane.open();
        pane.search("plan script");
        let script = pane.folder.file("notes/plan script");
        a_program_runs_only_through_run(&pane, "plan script", &script);
    }
}

fn root_searchs_file_results_have_the_same_actions(fixture: &'static Package) {
    let pane = Pane::new(fixture, None);
    pane.launcher.show_root_search();

    // A program: Enter reveals it.
    pane.search("run plan");
    let path = pane.folder.file("notes/run plan.bat");
    a_program_runs_only_through_run(&pane, "run plan.bat", &path);

    // A document: Enter opens it, root search keeping its query.
    pane.launcher.show_root_search();
    pane.search("todo");
    pane.select("todo.txt");
    assert_eq!(pane.actions()[..2], ["Open".to_owned(), reveal()]);
    block_on(pane.launcher.activate_selected());
    let opened = pane.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &pane.folder.file("notes/todo.txt")));
    assert_eq!(pane.launcher.view().query(), Some("todo"));
    assert!(pane.closed());
}

/// A folder lister the test holds up: each listing waits until the test
/// lets it finish, or until it is cancelled, and finds the files `names`.
struct HeldFolders {
    names: Vec<&'static str>,
    state: Mutex<(usize, bool)>,
    changed: Condvar,
}

impl HeldFolders {
    fn new(names: Vec<&'static str>) -> Arc<HeldFolders> {
        Arc::new(HeldFolders {
            names,
            state: Mutex::new((0, false)),
            changed: Condvar::new(),
        })
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }

    fn wait_until_started(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.state.lock().unwrap();
        while state.0 == 0 {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("the folder was never listed");
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    fn started(&self) -> usize {
        self.state.lock().unwrap().0
    }
}

impl Folders for HeldFolders {
    fn list(
        &self,
        folder: &Path,
        _limits: &Limits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FolderListing, String> {
        let mut state = self.state.lock().unwrap();
        state.0 += 1;
        self.changed.notify_all();
        while !state.1 {
            if cancelled() {
                return Err("cancelled".into());
            }
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap()
                .0;
        }
        Ok(FolderListing {
            files: self
                .names
                .iter()
                .map(|name| Listed {
                    path: folder.join(name),
                    relative: (*name).into(),
                })
                .collect(),
            truncated: false,
        })
    }
}

/// Types `query` on a thread of its own; the receiver hears once its
/// answer is shown (or it was stopped).
fn search_in_background(launcher: &Launcher, query: &str) -> mpsc::Receiver<()> {
    let searching = launcher.set_query(query);
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        block_on(searching);
        let _ = done.send(());
    });
    finished
}

fn a_newer_text_stops_the_search_before_it(fixture: &'static Package) {
    let folders = HeldFolders::new(vec!["report late.txt", "report current.txt"]);
    let pane = Pane::new(fixture, Some(folders.clone()));
    pane.open();

    // The first text waits for the folder's listing; the newer one stops
    // it, and only the newer text's files are listed once it is done.
    let first = search_in_background(&pane.launcher, "late");
    folders.wait_until_started();
    let second = search_in_background(&pane.launcher, "current");
    first
        .recv_timeout(Duration::from_secs(10))
        .expect("the stale search ended at once");
    folders.release();
    second
        .recv_timeout(Duration::from_secs(10))
        .expect("the search finished");
    assert_eq!(titles(&pane.launcher), ["report current.txt"]);
    assert_eq!(folders.started(), 1, "one listing for the command");
    // Later texts filter the kept listing.
    pane.search("late");
    assert_eq!(titles(&pane.launcher), ["report late.txt"]);
    assert_eq!(folders.started(), 1);
}

/// Declares one test per check for each language's files package.
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
    search_files_lists_the_files_in_the_launchers_own_field,
    a_documents_actions_act_through_the_system_and_close_the_window,
    move_to_recycle_bin_is_confirmed_first,
    enter_never_runs_a_program_in_search_files,
    root_searchs_file_results_have_the_same_actions,
    a_newer_text_stops_the_search_before_it,
);

#[test]
fn the_package_keeps_its_identity_across_the_rework() {
    // The Files command keeps its id, so a hotkey, alias or pin given it
    // before the rework still finds it.
    let pane = Pane::new(&RUST, None);
    let identity: PackageIdentity = pane.launcher.packages()[0].identity.clone();
    let command = format!("{}#files", identity.key());
    pane.launcher.show_root_search();
    pane.search("search files");
    assert!(
        pane.launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id == command && row.title == "Search Files"),
        "{:?}",
        pane.launcher.view().rows
    );
}
