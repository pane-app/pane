//! Search Files and the file actions Pane performs itself (#150), over
//! Pane's file index (#175), through the launcher's public interface, with
//! the Rust, JavaScript and TypeScript files samples, which give the same
//! answers the Files default extension does (its own sources live in their
//! repository, #285): the command owns the
//! launcher's search field and lists what the index finds as the user
//! types; a document's actions are Open (Enter), Show in Explorer
//! (Ctrl+Enter), Open With…, Copy Path, Copy Name, Copy File and Move to Recycle Bin
//! (destructive, confirmed), each closing the window and saying what it did
//! in a HUD; for a program or script, Enter shows it in Explorer,
//! Ctrl+Enter is Open With… and only Run runs it, in Search Files and in
//! root search's file results alike. Root search lists the files after the
//! commands, with a row opening the command with the query typed.
//! Recording fakes stand in for the system's handler of files (the link
//! opener), the system (reveal, open with an application, the clipboard,
//! the Recycle Bin and the installed applications) and the window, so
//! nothing opens, moves or shows; the index is the real one, over a fixture
//! folder standing for the home folder, kept in the test's own cache
//! folder. The packages are the ones `cargo xtask guests` assembles in
//! `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::file_index::{IndexerConfig, WalkOptions};
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
#[path = "support/settle.rs"]
mod settle;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use rows::{select_title, titles};
use settle::wait_for_merges;
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

/// A package that searches Pane's file index, in one language.
struct Package {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its command's title.
    command: &'static str,
}

const RUST: Package = Package {
    package: "sample-files",
    title: "Rust files sample",
    command: "Find files (Rust)",
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

/// What showing a file in the file manager is called on this system:
/// "Show in Explorer" on Windows.
fn reveal() -> String {
    format!("Show in {}", manager())
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

/// A folder of controlled fixtures, standing for the home folder:
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

/// Pane with one files package installed, its file index over the
/// fixture folder settled, and the fakes it acts through.
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

/// The file index over `home`, kept in `cache`'s own folder, with the
/// system's own change source; its first walk starts at once.
fn index_config(cache: &Path, home: &Path) -> IndexerConfig {
    IndexerConfig {
        first_walk_delay: Duration::ZERO,
        walk: WalkOptions {
            background: false,
            ..WalkOptions::default()
        },
        ..IndexerConfig::native(cache, home.to_path_buf(), Vec::new())
    }
}

impl Pane {
    /// Pane with `fixture`'s package installed and its file index over the
    /// fixture folder settled.
    fn new(fixture: &'static Package) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let system = Arc::new(RecordingSystem::default());
        runtime.set_applications(system.clone());
        let opener = FakeOpener::default();
        let folder = Folder::new();
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
                .with_link_opener(Arc::new(opener.clone()))
                .with_system(system.clone())
                .with_file_index(index_config(&data.path().join("cache"), &folder.root));
        let window = RecordingWindow::attach(&launcher);
        block_on(launcher.install_package(&built(&format!("packages/{}", fixture.package))));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        assert!(
            launcher
                .packages()
                .iter()
                .any(|package| package.title() == fixture.title)
        );
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
        // The JavaScript and TypeScript guests answer past the 200 ms
        // budget while their engine starts, and a folder that was still
        // being listed answers again once it is: the answer merges into
        // the published list within 16 ms of the search's end (#201),
        // which the rows read below wait out.
        wait_for_merges(&self.launcher);
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

    /// What the HUDs shown since the row was selected said.
    fn huds(&self) -> Vec<String> {
        self.window
            .huds()
            .into_iter()
            .map(|hud| hud.title)
            .collect()
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
    let pane = Pane::new(fixture);
    pane.open();
    // Its own list.
    assert_eq!(titles(&pane.launcher), ["What is searched"]);

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
        [("Résumé plan ü.txt".to_owned(), Some("~".to_owned()))]
    );
    pane.search("todo");
    assert_eq!(
        listed(&pane),
        [("todo.txt".to_owned(), Some("~/notes".to_owned()))]
    );
    // Folders are found too, a name before the files only in it.
    pane.search("notes");
    assert_eq!(listed(&pane)[0], ("notes".to_owned(), Some("~".to_owned())));
    // Cleared, the command's own list is back.
    pane.search("");
    assert_eq!(titles(&pane.launcher), ["What is searched"]);
}

fn a_documents_actions_act_through_the_system_and_close_the_window(fixture: &'static Package) {
    let pane = Pane::new(fixture);
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
            "Copy Name".to_owned(),
            "Copy File".to_owned(),
            trash(),
        ]
    );
    let actions = pane.launcher.item_actions().unwrap();
    assert!(actions.actions[2].submenu, "Open With… opens a submenu");
    assert!(actions.actions[6].destructive);
    assert_eq!(pane.launcher.selected_action().label, "Open");

    // Enter opens it with the system's handler, and the window closes.
    block_on(pane.launcher.activate_selected());
    let opened = pane.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &todo));
    assert_eq!(pane.huds(), ["Opened todo.txt"]);
    assert_eq!(
        pane.launcher.view().status,
        Status::Idle,
        "not the status line"
    );
    assert!(pane.closed());

    // Ctrl+Enter shows it in the file manager.
    pane.select("todo.txt");
    block_on(pane.launcher.run_selected_action(1));
    match pane.system.take().as_slice() {
        [Done::Revealed(path)] => assert!(same_file(path, &todo)),
        other => panic!("{other:?}"),
    }
    assert_eq!(pane.huds(), [format!("Showed todo.txt in {}", manager())]);
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
    // Copy Name copies the name alone (#177).
    pane.select("todo.txt");
    pane.run("Copy Name");
    match pane.system.take().as_slice() {
        [
            Done::Copied {
                clip: Clip::Text(name),
                concealed: false,
            },
        ] => assert_eq!(name, "todo.txt"),
        other => panic!("{other:?}"),
    }
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
    assert_eq!(pane.huds(), ["Opened todo.txt with Notepad"]);
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
    let pane = Pane::new(fixture);
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
            "Copy Name".to_owned(),
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
    assert_eq!(pane.huds(), [format!("Ran {name}")]);
    assert!(pane.closed());
}

fn enter_never_runs_a_program_in_search_files(fixture: &'static Package) {
    let pane = Pane::new(fixture);
    pane.open();
    pane.search("run plan");
    let path = pane.folder.file("notes/run plan.bat");
    a_program_runs_only_through_run(&pane, "run plan.bat", &path);
    // A script known by its executable bit alone: its row, told from its
    // name, has a document's actions (the index reads no file to list
    // it), but the check before Enter acts finds the bit, so Enter shows
    // it in the file manager and runs nothing (#175, docs/files.md).
    #[cfg(unix)]
    {
        pane.open();
        pane.search("plan script");
        let script = pane.folder.file("notes/plan script");
        pane.select("plan script");
        assert_eq!(
            pane.actions(),
            [
                "Open".to_owned(),
                reveal(),
                "Open With…".to_owned(),
                "Copy Path".to_owned(),
                "Copy Name".to_owned(),
                "Copy File".to_owned(),
                trash(),
            ]
        );
        block_on(pane.launcher.activate_selected());
        match pane.system.take().as_slice() {
            [Done::Revealed(revealed)] => assert!(same_file(revealed, &script)),
            other => panic!("{other:?}"),
        }
        assert!(pane.opener.take().is_empty(), "Enter ran nothing");
        assert_eq!(
            pane.huds(),
            [format!("Showed plan script in {}", manager())]
        );
        assert!(pane.closed());
    }
}

fn root_searchs_file_results_have_the_same_actions(fixture: &'static Package) {
    let pane = Pane::new(fixture);
    pane.launcher.show_root_search();

    // A program: Enter reveals it.
    pane.search("run plan");
    let path = pane.folder.file("notes/run plan.bat");
    a_program_runs_only_through_run(&pane, "run plan.bat", &path);

    // A document: Enter opens it, root search keeping its query. The
    // files come after the commands, under "Files", with the row opening
    // the command with the query typed.
    pane.launcher.show_root_search();
    pane.search("todo");
    assert_eq!(
        titles(&pane.launcher),
        [
            "todo.txt".to_owned(),
            format!("{} for “todo”", pane.fixture.command)
        ]
    );
    let presentation = pane.launcher.presentation();
    assert_eq!(presentation.rows[0].kind, Some(pane_core::RowKind::File));
    assert!(presentation.rows[0].icon.is_some(), "the system's icon");
    assert_eq!(
        presentation
            .sections
            .iter()
            .map(|section| section.label.as_str())
            .collect::<Vec<_>>(),
        ["Files"]
    );
    pane.select("todo.txt");
    assert_eq!(pane.actions()[..2], ["Open".to_owned(), reveal()]);
    block_on(pane.launcher.activate_selected());
    let opened = pane.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &pane.folder.file("notes/todo.txt")));
    assert_eq!(pane.launcher.view().query(), Some("todo"));
    assert!(pane.closed());
}

fn search_files_opens_from_root_search_with_the_query_typed(fixture: &'static Package) {
    let pane = Pane::new(fixture);
    pane.launcher.show_root_search();
    pane.search("plan");
    let row = format!("{} for “plan”", fixture.command);
    assert!(
        titles(&pane.launcher).contains(&row),
        "{:?}",
        titles(&pane.launcher)
    );
    // At most five files, then the row searching them all.
    assert!(titles(&pane.launcher).len() <= 6);
    select_title(&pane.launcher, &row);
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.launcher.view().screen,
        Screen::CommandSearch {
            query: "plan".into()
        }
    );
    let found = titles(&pane.launcher);
    for name in ["Résumé plan ü.txt", "plan.md", "run plan.bat"] {
        assert!(found.iter().any(|title| title == name), "{found:?}");
    }
}

fn a_folder_opens_in_the_file_manager(fixture: &'static Package) {
    let pane = Pane::new(fixture);
    pane.launcher.show_root_search();
    pane.search("notes");
    pane.select("notes");
    assert_eq!(
        pane.launcher.presentation().rows[0].kind,
        Some(pane_core::RowKind::Folder)
    );
    assert_eq!(pane.actions()[..2], ["Open".to_owned(), reveal()]);
    block_on(pane.launcher.activate_selected());
    let opened = pane.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &pane.folder.file("notes")));
    assert_eq!(pane.huds(), ["Opened notes"]);
}

fn a_file_created_while_pane_runs_is_found(fixture: &'static Package) {
    let pane = Pane::new(fixture);
    let new = pane.folder.file("notes/minutes 2026.txt");
    fs::write(&new, "minutes").unwrap();
    // The system's watcher reports it; it is found within a few seconds.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(pane.launcher.wait_for_file_index(Duration::from_secs(10)));
        pane.launcher.show_root_search();
        pane.search("minutes");
        if titles(&pane.launcher).first().map(String::as_str) == Some("minutes 2026.txt") {
            break;
        }
        assert!(Instant::now() < deadline, "{:?}", titles(&pane.launcher));
        std::thread::sleep(Duration::from_millis(50));
    }
    // A stale row is explained at Enter rather than opened.
    pane.select("minutes 2026.txt");
    fs::remove_file(&new).unwrap();
    block_on(pane.launcher.activate_selected());
    assert!(pane.opener.take().is_empty());
    assert_eq!(
        pane.launcher.view().status,
        Status::Error("Could not open minutes 2026.txt: it no longer exists".into())
    );
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
    search_files_opens_from_root_search_with_the_query_typed,
    a_folder_opens_in_the_file_manager,
    a_file_created_while_pane_runs_is_found,
);

#[test]
fn the_package_keeps_its_identity_across_the_rework() {
    // The sample's command keeps its id, so a hotkey, alias or pin given it
    // before an update still finds it.
    let pane = Pane::new(&RUST);
    let identity: PackageIdentity = pane.launcher.packages()[0].identity.clone();
    let command = format!("{}#find-files", identity.key());
    pane.launcher.show_root_search();
    pane.search("find files");
    assert!(
        pane.launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id == command && row.title == "Find files (Rust)"),
        "{:?}",
        pane.launcher.view().rows
    );
}
