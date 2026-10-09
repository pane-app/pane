//! File search over Pane's file index (#126, #175), through the launcher's
//! public interface: the real Files default extension, the real index and
//! the system's own change source, over a fixture folder standing for the
//! home folder (named by the test) and a cache folder of the test's own,
//! with a recording opener and system so that nothing opens or shows. The
//! index is waited on deterministically (`Launcher::wait_for_file_index`).
//! What the index finds and how it ranks, its scope's rules, root search's
//! Files section and its row searching every file, opening and the program
//! rule, disabling and uninstalling, restarting over the same cache folder
//! and a second Pane on it; and what the File search page (#176) reads and
//! changes: the status, Rebuild index, turning off Search Files, every
//! control applied without a restart, a folder taken out for churn included
//! again, and a folder granted to Files under #29 kept in what is indexed; and
//! the ignore rules kept between batches of changes (#186): a `.gitignore`
//! line added and removed, a repository made and deleted, its own
//! `.git/info/exclude` changed, the user's patterns changed, each holding
//! in later batches and at Enter. The coordinator's catch-up, live changes
//! and fallbacks driven through the change source's seam are its unit tests
//! (`pane_core::file_index::indexer`), and the actions on each row, in
//! every language, `file_actions.rs`. The packages are the ones
//! `cargo xtask guests` assembles in `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::file_index::{
    CaughtUpBy, INDEX_DIR, IndexState, IndexerConfig, ProblemKind, SearchOptions, UserRules,
    WalkOptions,
};
use pane_core::{Launcher, LinkOpener, PackageIdentity, Runtime, Status, WindowPresence};
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

const LIMIT: Duration = Duration::from_secs(30);

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

/// The fixture home folder, Pane's data and cache folders, and the fakes
/// Pane acts through; they outlive restarts.
struct Home {
    dir: TempDir,
    home: PathBuf,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
}

impl Home {
    /// A home folder holding:
    ///
    /// ```text
    /// Documents/plan.txt, planning notes.md, my plan b.txt
    /// Documents/Invoices 2026/march.pdf
    /// Documents/Résumé.pdf
    /// Downloads/setup.exe, run.bat, Shortcut.lnk
    /// .config/hidden plan.txt          (hidden)
    /// node_modules/plan module.js      (left out by default)
    /// Projects/app/.git/, .gitignore (build/), build/plan output.txt
    /// Caches/CACHEDIR.TAG, plan cached.txt
    /// ```
    fn new() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        for (file, text) in [
            ("Documents/plan.txt", "plan"),
            ("Documents/planning notes.md", "notes"),
            ("Documents/my plan b.txt", "b"),
            ("Documents/Invoices 2026/march.pdf", "pdf"),
            ("Documents/Résumé.pdf", "cv"),
            ("Downloads/setup.exe", "MZ"),
            ("Downloads/run.bat", "@echo off"),
            ("Downloads/Shortcut.lnk", "L"),
            (".config/hidden plan.txt", "hidden"),
            ("node_modules/plan module.js", "js"),
            ("Projects/app/.gitignore", "build/\n"),
            ("Projects/app/build/plan output.txt", "out"),
            (
                "Caches/CACHEDIR.TAG",
                "Signature: 8a477f597d28d172789f06886806bc55",
            ),
            ("Caches/plan cached.txt", "cached"),
        ] {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fs::create_dir_all(home.join("Projects/app/.git")).unwrap();
        Home {
            dir,
            home,
            opener: FakeOpener::default(),
            system: Arc::new(RecordingSystem::default()),
        }
    }

    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    fn index_dir(&self) -> PathBuf {
        self.cache().join(INDEX_DIR)
    }

    fn file(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    /// The file index over the fixture home, its first walk starting at
    /// once unless `deferred`.
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

    /// Starts Pane on the data folder `data` (under the fixture's folder),
    /// as after a restart.
    fn start_in(&self, data: &str, deferred: bool) -> (Launcher, Runtime) {
        let runtime = Runtime::start().unwrap();
        runtime.set_applications(self.system.clone());
        let launcher = Launcher::with_packages(
            Ok(runtime.clone()),
            vec![],
            self.dir.path().join(data).join("extensions"),
        )
        .with_link_opener(Arc::new(self.opener.clone()))
        .with_system(self.system.clone())
        .with_file_index(self.config(deferred));
        (launcher, runtime)
    }

    fn start(&self) -> (Launcher, Runtime) {
        self.start_in("data", false)
    }

    /// Starts Pane with Files installed and its index settled.
    fn with_files(&self) -> (Launcher, Runtime) {
        let (launcher, runtime) = self.start();
        install(&launcher, &built("packages/files"));
        settle(&launcher);
        (launcher, runtime)
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
}

fn settle(launcher: &Launcher) {
    assert!(
        launcher.wait_for_file_index(LIMIT),
        "{:?}",
        launcher.file_index_status()
    );
}

fn search(launcher: &Launcher, query: &str) {
    launcher.show_root_search();
    block_on(launcher.set_query(query));
}

/// Waits past the second the index was walked in, where Linux's catch-up
/// (a reconciling walk comparing folders' modified times, kept in whole
/// seconds) is to see a change: elsewhere the system's records see it.
fn folder_times_move_on() {
    if cfg!(target_os = "linux") {
        std::thread::sleep(Duration::from_millis(1100));
    }
}

fn files_identity(launcher: &Launcher) -> PackageIdentity {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.title() == "Files")
        .expect("Files is installed")
        .identity
}

/// The titles of root search's rows under "Files".
fn file_rows(launcher: &Launcher) -> Vec<String> {
    let presentation = launcher.presentation();
    let rows = titles(launcher);
    let Some(files) = presentation
        .sections
        .iter()
        .position(|section| section.label == "Files")
    else {
        return Vec::new();
    };
    let first = presentation.sections[files].first;
    let end = presentation
        .sections
        .get(files + 1)
        .map_or(rows.len(), |next| next.first);
    rows[first..end].to_vec()
}

#[test]
fn typing_a_files_name_lists_it_under_files_and_enter_opens_it() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    search(&launcher, "plan");
    let rows = file_rows(&launcher);
    // The exact name first, at most five files, then the row searching
    // them all.
    assert_eq!(rows[0], "plan.txt", "{rows:?}");
    assert!(rows.len() <= 6, "{rows:?}");
    assert_eq!(rows.last().unwrap(), "Search Files for “plan”");
    // Hidden, ignored, node_modules and cache-tagged entries are absent.
    for absent in [
        "hidden plan.txt",
        "plan module.js",
        "plan output.txt",
        "plan cached.txt",
    ] {
        assert!(!rows.iter().any(|row| row == absent), "{rows:?}");
    }
    let view = launcher.view();
    let plan = view
        .rows
        .iter()
        .find(|row| row.title == "plan.txt")
        .unwrap();
    assert_eq!(plan.subtitle.as_deref(), Some("~/Documents"));

    select_title(&launcher, "plan.txt");
    assert_eq!(launcher.selected_action().label, "Open");
    let window = RecordingWindow::attach(&launcher);
    block_on(launcher.activate_selected());
    let opened = home.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &home.file("Documents/plan.txt")));
    assert_eq!(
        window.huds().pop().map(|hud| hud.title).as_deref(),
        Some("Opened plan.txt")
    );
}

#[test]
fn a_file_row_appears_from_the_first_character_and_none_for_a_blank_query() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    search(&launcher, "m");
    assert!(!file_rows(&launcher).is_empty());
    search(&launcher, "");
    assert!(file_rows(&launcher).is_empty());
}

#[test]
fn case_accents_and_folder_words_find_files() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    search(&launcher, "RESUME");
    assert_eq!(file_rows(&launcher)[0], "Résumé.pdf");
    search(&launcher, "invoices march");
    assert_eq!(file_rows(&launcher)[0], "march.pdf");
    // A folder is found as well, and reads Folder.
    search(&launcher, "invoices");
    assert_eq!(file_rows(&launcher)[0], "Invoices 2026");
    let folder = titles(&launcher)
        .iter()
        .position(|row| row == "Invoices 2026")
        .unwrap();
    assert_eq!(
        launcher.presentation().rows[folder].kind,
        Some(pane_core::RowKind::Folder)
    );
}

#[test]
fn file_rows_come_after_commands_found_by_title() {
    let home = Home::new();
    fs::write(home.file("Documents/search notes.txt"), "x").unwrap();
    let (launcher, _runtime) = home.with_files();
    search(&launcher, "search");
    let rows = titles(&launcher);
    let command = rows.iter().position(|row| row == "Search Files").unwrap();
    let file = rows
        .iter()
        .position(|row| row == "search notes.txt")
        .unwrap();
    assert!(command < file, "{rows:?}");
}

#[test]
fn enter_on_a_program_shows_it_and_never_runs_it() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    for (query, name) in [
        ("setup", "setup.exe"),
        ("run", "run.bat"),
        ("shortcut", "Shortcut.lnk"),
    ] {
        search(&launcher, query);
        select_title(&launcher, name);
        let reveal = launcher.selected_action().label;
        assert!(reveal.starts_with("Show in "), "{reveal}");
        launcher.set_window_presence(WindowPresence::Shown);
        block_on(launcher.activate_selected());
        match home.system.take().as_slice() {
            [Done::Revealed(path)] => {
                assert!(same_file(path, &home.file(&format!("Downloads/{name}"))))
            }
            other => panic!("{name}: {other:?}"),
        }
        assert!(home.opener.take().is_empty(), "{name} was not run");
    }
}

#[test]
fn an_entry_replaced_since_it_was_found_is_explained_not_opened() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    search(&launcher, "planning");
    select_title(&launcher, "planning notes.md");
    fs::remove_file(home.file("Documents/planning notes.md")).unwrap();
    fs::create_dir(home.file("Documents/planning notes.md")).unwrap();
    block_on(launcher.activate_selected());
    assert!(home.opener.take().is_empty());
    assert_eq!(
        launcher.view().status,
        Status::Error("Could not open planning notes.md: it is now a folder".into())
    );
}

#[test]
fn the_first_walk_waits_until_the_launcher_is_shown() {
    let home = Home::new();
    let (launcher, _runtime) = home.start_in("data", true);
    install(&launcher, &built("packages/files"));
    let deadline = std::time::Instant::now() + LIMIT;
    while !launcher.file_index_status().waiting {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    search(&launcher, "plan");
    assert!(file_rows(&launcher).is_empty(), "nothing is walked yet");
    launcher.set_window_presence(WindowPresence::Shown);
    settle(&launcher);
    search(&launcher, "plan");
    assert_eq!(file_rows(&launcher)[0], "plan.txt");
}

#[test]
fn disabling_files_stops_indexing_and_uninstalling_deletes_the_index() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    assert_eq!(launcher.file_index_status().state, IndexState::Current);
    let files = files_identity(&launcher);

    block_on(launcher.set_enabled(&files, false));
    assert_eq!(launcher.file_index_status().state, IndexState::Off);
    assert!(home.index_dir().exists(), "kept on disk while disabled");
    // Made while Files is disabled: found once it is enabled again.
    folder_times_move_on();
    fs::write(home.file("Documents/while disabled.txt"), "x").unwrap();
    block_on(launcher.set_enabled(&files, true));
    settle(&launcher);
    search(&launcher, "while disabled");
    let deadline = std::time::Instant::now() + LIMIT;
    while file_rows(&launcher).first().map(String::as_str) != Some("while disabled.txt") {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            file_rows(&launcher)
        );
        std::thread::sleep(Duration::from_millis(50));
        settle(&launcher);
        search(&launcher, "while disabled");
    }

    block_on(launcher.uninstall(&files, pane_core::SavedData::Keep));
    assert!(!home.index_dir().exists(), "uninstalling deletes the index");
}

#[test]
fn a_restart_catches_up_with_what_changed_while_pane_was_stopped() {
    let home = Home::new();
    {
        let (launcher, runtime) = home.with_files();
        // As quitting Pane does: the index is let go of at once, before the
        // runtime's threads have ended.
        launcher
            .file_indexer()
            .unwrap()
            .set_users(std::collections::BTreeSet::new());
        drop(launcher);
        drop(runtime);
    }
    folder_times_move_on();
    fs::write(home.file("Documents/written while stopped.txt"), "x").unwrap();
    fs::remove_file(home.file("Documents/my plan b.txt")).unwrap();
    let (launcher, _runtime) = home.start();
    settle(&launcher);
    let status = launcher.file_index_status();
    let caught_up = status.caught_up.map(|(by, _)| by);
    let expected = if cfg!(target_os = "windows") {
        // Needs the temporary folder's volume to keep a change journal, as
        // C: does (CI gives the runner's D: one).
        CaughtUpBy::Journal
    } else if cfg!(target_os = "macos") {
        CaughtUpBy::EventHistory
    } else {
        CaughtUpBy::ReconcilingWalk
    };
    assert_eq!(
        caught_up,
        Some(expected),
        "caught up without a full walk ({:?})",
        status.reason
    );
    search(&launcher, "written while stopped");
    let deadline = std::time::Instant::now() + LIMIT;
    while file_rows(&launcher).first().map(String::as_str) != Some("written while stopped.txt") {
        // macOS's history arrives through the live stream.
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
        settle(&launcher);
        search(&launcher, "written while stopped");
    }
    search(&launcher, "my plan b");
    assert!(
        !file_rows(&launcher)
            .iter()
            .any(|row| row == "my plan b.txt")
    );
}

#[test]
fn a_second_pane_on_the_same_cache_folder_says_file_search_is_in_use() {
    let home = Home::new();
    let (_first, _first_runtime) = home.with_files();
    let (second, _second_runtime) = home.start_in("other data", false);
    install(&second, &built("packages/files"));
    settle(&second);
    let status = second.file_index_status();
    assert_eq!(status.state, IndexState::Stopped);
    assert_eq!(
        status.reason.as_deref(),
        Some("Another Pane is using file search on this computer")
    );
}

#[test]
fn the_users_rules_are_recorded_and_applied() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    let rules = UserRules {
        include_hidden: true,
        use_ignore_files: false,
        ..UserRules::default()
    };
    block_on(launcher.set_file_search_rules(rules.clone())).unwrap();
    settle(&launcher);
    search(&launcher, "plan");
    let deadline = std::time::Instant::now() + LIMIT;
    loop {
        let rows = file_rows(&launcher);
        if rows.iter().any(|row| row == "hidden plan.txt") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{rows:?}");
        std::thread::sleep(Duration::from_millis(50));
        settle(&launcher);
        search(&launcher, "plan");
    }
    search(&launcher, "output");
    assert_eq!(file_rows(&launcher)[0], "plan output.txt");
    assert_eq!(launcher.file_search_rules().unwrap().1, rules);
    // Recorded in Pane's own record beside the installed packages.
    assert_eq!(
        UserRules::read(&home.dir.path().join("data").join("extensions")),
        rules
    );
}

// ------------------------------------------------ the File search page (#176)

/// The canonical path of `path`, without Windows' verbatim prefix, as Pane
/// records a granted folder.
fn plain(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path).unwrap();
    let text = resolved.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => resolved,
    }
}

/// Searches root search for `query` until the titles under "Files" satisfy
/// `done`, letting the index settle between tries.
fn eventually(launcher: &Launcher, query: &str, done: impl Fn(&[String]) -> bool) {
    let deadline = std::time::Instant::now() + LIMIT;
    loop {
        settle(launcher);
        search(launcher, query);
        let rows = file_rows(launcher);
        if done(&rows) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{query}: {rows:?} ({:?})",
            launcher.file_index_status()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn lists(name: &'static str) -> impl Fn(&[String]) -> bool {
    move |rows| rows.iter().any(|row| row == name)
}

fn lacks(name: &'static str) -> impl Fn(&[String]) -> bool {
    move |rows| !rows.iter().any(|row| row == name)
}

/// Changes the user's rules as the File search page does, through the
/// launcher, and checks they are recorded.
fn change_rules(home: &Home, launcher: &Launcher, edit: impl FnOnce(&mut UserRules)) {
    let mut rules = launcher.file_search_rules().unwrap().1;
    edit(&mut rules);
    block_on(launcher.set_file_search_rules(rules.clone())).unwrap();
    assert_eq!(launcher.file_search_rules().unwrap().1, rules);
    assert_eq!(
        UserRules::read(&home.dir.path().join("data").join("extensions")),
        rules
    );
}

#[test]
fn the_status_says_what_is_indexed_and_how_and_when_it_last_caught_up() {
    let home = Home::new();
    let before = std::time::SystemTime::now();
    let (launcher, _runtime) = home.with_files();
    let status = launcher.file_index_status();
    assert_eq!(status.state, IndexState::Current);
    assert!(status.entries >= 10, "{status:?}");
    let (how, when) = status.caught_up.expect("caught up");
    assert_eq!(how, CaughtUpBy::FullWalk);
    assert!(when >= before && when <= std::time::SystemTime::now());
    assert_eq!(how.describe(), "by indexing every folder");
    assert!(launcher.file_search_problems().is_empty());
    assert_eq!(
        launcher.file_search_packages(),
        [("Files".to_owned(), None)]
    );
    let (effective, rules) = launcher.file_search_rules().unwrap();
    assert_eq!(effective.roots, std::slice::from_ref(&home.home));
    assert_eq!(effective.home.as_deref(), Some(home.home.as_path()));
    assert_eq!(rules, UserRules::default());
}

#[test]
fn rebuilding_the_index_builds_it_again_from_every_folder() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    block_on(launcher.rebuild_file_index()).unwrap();
    settle(&launcher);
    let status = launcher.file_index_status();
    assert_eq!(status.state, IndexState::Current);
    assert_eq!(
        status.caught_up.map(|(by, _)| by),
        Some(CaughtUpBy::FullWalk)
    );
    assert!(home.index_dir().exists());
    eventually(&launcher, "plan", lists("plan.txt"));
}

#[test]
fn turning_off_search_files_stops_the_index_as_disabling_files_does() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    let commands: Vec<String> = launcher
        .packages()
        .into_iter()
        .find(|package| package.title() == "Files")
        .unwrap()
        .listed_commands()
        .into_iter()
        .map(|command| command.registration.id)
        .collect();
    assert!(
        commands.len() > 1,
        "Files has the typed-path commands (#195)"
    );
    // The index stops once every command is turned off, as the package's
    // own switch does; one left on keeps it running.
    for command in &commands {
        block_on(launcher.set_command_enabled(command, false)).unwrap();
    }
    let status = launcher.file_index_status();
    assert_eq!(status.state, IndexState::Off);
    assert_eq!(
        status.reason.as_deref(),
        Some("no enabled extension uses file search")
    );
    assert_eq!(
        launcher.file_search_packages(),
        [(
            "Files".to_owned(),
            Some("its commands are turned off".to_owned())
        )]
    );
    assert!(home.index_dir().exists(), "kept on disk while off");

    for command in &commands {
        block_on(launcher.set_command_enabled(command, true)).unwrap();
    }
    settle(&launcher);
    assert_eq!(launcher.file_index_status().state, IndexState::Current);
    eventually(&launcher, "plan", lists("plan.txt"));

    let files = files_identity(&launcher);
    block_on(launcher.set_enabled(&files, false));
    assert_eq!(
        launcher.file_search_packages(),
        [("Files".to_owned(), Some("it is turned off".to_owned()))]
    );
}

#[test]
fn every_control_of_the_page_changes_what_is_indexed_without_a_restart() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();

    // A folder added, then removed.
    let drive = home.dir.path().join("Second drive");
    fs::create_dir_all(&drive).unwrap();
    fs::write(drive.join("far plan.txt"), "x").unwrap();
    change_rules(&home, &launcher, |rules| {
        rules.added_roots.push(drive.clone())
    });
    assert_eq!(
        launcher.file_search_rules().unwrap().0.roots,
        [home.home.clone(), drive.clone()]
    );
    eventually(&launcher, "far plan", lists("far plan.txt"));
    change_rules(&home, &launcher, |rules| rules.added_roots.clear());
    eventually(&launcher, "far plan", lacks("far plan.txt"));

    // A folder excluded, then no longer.
    let invoices = home.file("Documents/Invoices 2026");
    change_rules(&home, &launcher, |rules| {
        rules.excluded_folders.push(invoices.clone())
    });
    eventually(&launcher, "march", lacks("march.pdf"));
    eventually(&launcher, "plan", lists("plan.txt"));
    change_rules(&home, &launcher, |rules| rules.excluded_folders.clear());
    eventually(&launcher, "march", lists("march.pdf"));

    // A pattern excluded, then no longer.
    change_rules(&home, &launcher, |rules| {
        rules.excluded_patterns.push("*.md".into())
    });
    eventually(&launcher, "planning", lacks("planning notes.md"));
    change_rules(&home, &launcher, |rules| rules.excluded_patterns.clear());
    eventually(&launcher, "planning", lists("planning notes.md"));

    // The switches.
    change_rules(&home, &launcher, |rules| rules.include_hidden = true);
    eventually(&launcher, "hidden plan", lists("hidden plan.txt"));
    change_rules(&home, &launcher, |rules| rules.include_hidden = false);
    eventually(&launcher, "hidden plan", lacks("hidden plan.txt"));
    change_rules(&home, &launcher, |rules| rules.use_ignore_files = false);
    eventually(&launcher, "plan output", lists("plan output.txt"));
    change_rules(&home, &launcher, |rules| rules.default_exclusions = false);
    eventually(&launcher, "plan module", lists("plan module.js"));
    // Cache-tagged folders stay out whatever the switches say.
    eventually(&launcher, "plan cached", lacks("plan cached.txt"));
    change_rules(&home, &launcher, |rules| rules.include_other_volumes = true);
    assert!(
        launcher
            .file_search_rules()
            .unwrap()
            .0
            .include_other_volumes
    );
    settle(&launcher);
    assert_eq!(launcher.file_index_status().state, IndexState::Current);
}

#[test]
fn a_folder_taken_out_for_churn_is_listed_and_included_again() {
    let home = Home::new();
    let documents = home.file("Documents");
    let record = home.dir.path().join("data").join("extensions");
    fs::create_dir_all(&record).unwrap();
    UserRules {
        quarantined: vec![documents.clone()],
        ..UserRules::default()
    }
    .write(&record)
    .unwrap();
    let (launcher, _runtime) = home.with_files();
    eventually(&launcher, "plan", lacks("plan.txt"));
    let problems = launcher.file_search_problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].kind, ProblemKind::Churned);
    assert_eq!(problems[0].folder.as_deref(), Some(documents.as_path()));
    // Changing another rule keeps it out.
    change_rules(&home, &launcher, |rules| rules.include_hidden = true);
    assert_eq!(
        launcher.file_search_rules().unwrap().1.quarantined,
        std::slice::from_ref(&documents)
    );

    block_on(launcher.include_in_file_search(documents.clone())).unwrap();
    eventually(&launcher, "plan", lists("plan.txt"));
    assert!(launcher.file_search_problems().is_empty());
    assert!(UserRules::read(&record).quarantined.is_empty());
}

#[test]
fn a_folder_granted_to_files_outside_the_home_folder_is_kept_in_what_is_indexed() {
    let home = Home::new();
    let key = {
        let (launcher, runtime) = home.with_files();
        let key = files_identity(&launcher).key();
        launcher
            .file_indexer()
            .unwrap()
            .set_users(std::collections::BTreeSet::new());
        drop(launcher);
        drop(runtime);
        key
    };
    // Files was granted a folder under #29, outside the home folder.
    let granted = home.dir.path().join("Second drive");
    fs::create_dir_all(&granted).unwrap();
    fs::write(granted.join("far plan.txt"), "x").unwrap();
    let granted = plain(&granted);
    let record = home.dir.path().join("data").join("extensions");
    fs::write(
        record.join("folders.json"),
        serde_json::json!({ "version": 1, "folders": { key.clone(): granted } }).to_string(),
    )
    .unwrap();

    let (launcher, _runtime) = home.start();
    assert_eq!(
        launcher.file_search_rules().unwrap().1.added_roots,
        std::slice::from_ref(&granted)
    );
    assert_eq!(
        UserRules::read(&record).added_roots,
        std::slice::from_ref(&granted)
    );
    // The grant is forgotten.
    let grants: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(record.join("folders.json")).unwrap()).unwrap();
    assert!(grants["folders"].get(&key).is_none(), "{grants}");
    eventually(&launcher, "far plan", lists("far plan.txt"));
}

#[test]
fn a_folder_granted_to_files_that_the_index_covers_is_simply_forgotten() {
    let home = Home::new();
    let key = {
        let (launcher, runtime) = home.with_files();
        let key = files_identity(&launcher).key();
        launcher
            .file_indexer()
            .unwrap()
            .set_users(std::collections::BTreeSet::new());
        drop(launcher);
        drop(runtime);
        key
    };
    let granted = plain(&home.file("Documents"));
    let record = home.dir.path().join("data").join("extensions");
    fs::write(
        record.join("folders.json"),
        serde_json::json!({ "version": 1, "folders": { key.clone(): granted } }).to_string(),
    )
    .unwrap();
    let (launcher, _runtime) = home.start();
    assert!(
        launcher
            .file_search_rules()
            .unwrap()
            .1
            .added_roots
            .is_empty()
    );
    let grants: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(record.join("folders.json")).unwrap()).unwrap();
    assert!(grants["folders"].get(&key).is_none(), "{grants}");
    eventually(&launcher, "plan", lists("plan.txt"));
}

// ------------------------------ the ignore rules kept between batches (#186)

/// Writes `text` to `relative` in the fixture home, making its folders.
fn write(home: &Home, relative: &str, text: &str) {
    let path = home.file(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn a_gitignore_line_hides_what_it_matches_and_removing_it_shows_it_again() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    // Changes in the repository, each batch learning its rules.
    write(&home, "Projects/app/src/trace.draft", "draft");
    eventually(&launcher, "trace", lists("trace.draft"));
    write(&home, "Projects/app/src/main.rs", "fn main() {}");
    eventually(&launcher, "main", lists("main.rs"));

    // A line added: what it matches goes, and stays out in later batches.
    write(&home, "Projects/app/.gitignore", "build/\n*.draft\n");
    eventually(&launcher, "trace", lacks("trace.draft"));
    write(&home, "Projects/app/src/second.draft", "draft");
    write(&home, "Projects/app/src/second.rs", "");
    eventually(&launcher, "second", lists("second.rs"));
    eventually(&launcher, "second", lacks("second.draft"));
    eventually(&launcher, "main", lists("main.rs"));
    eventually(&launcher, "plan output", lacks("plan output.txt"));

    // The line removed: what it matched comes back, and later changes are
    // admitted again.
    write(&home, "Projects/app/.gitignore", "build/\n");
    eventually(&launcher, "trace", lists("trace.draft"));
    eventually(&launcher, "second", lists("second.draft"));
    write(&home, "Projects/app/src/third.draft", "draft");
    eventually(&launcher, "third", lists("third.draft"));
    eventually(&launcher, "plan output", lacks("plan output.txt"));
}

#[test]
fn a_new_repository_starts_applying_its_ignore_rules() {
    let home = Home::new();
    // Outside a repository Git reads no .gitignore.
    write(&home, "Projects/site/.gitignore", "dist/\n");
    write(&home, "Projects/site/dist/bundle.js", "js");
    write(&home, "Projects/site/index.html", "html");
    let (launcher, _runtime) = home.with_files();
    eventually(&launcher, "bundle", lists("bundle.js"));
    write(&home, "Projects/site/dist/chunk.js", "js");
    eventually(&launcher, "chunk", lists("chunk.js"));

    fs::create_dir_all(home.file("Projects/site/.git")).unwrap();
    eventually(&launcher, "bundle", lacks("bundle.js"));
    eventually(&launcher, "chunk", lacks("chunk.js"));
    eventually(&launcher, "index", lists("index.html"));
    // Later batches keep to it.
    write(&home, "Projects/site/dist/later.js", "js");
    write(&home, "Projects/site/page.html", "html");
    eventually(&launcher, "page", lists("page.html"));
    eventually(&launcher, "later", lacks("later.js"));

    // The repository deleted: its .gitignore no longer applies.
    fs::remove_dir_all(home.file("Projects/site/.git")).unwrap();
    eventually(&launcher, "bundle", lists("bundle.js"));
    eventually(&launcher, "later", lists("later.js"));
    write(&home, "Projects/site/dist/last.js", "js");
    eventually(&launcher, "last", lists("last.js"));
}

#[test]
fn a_repositorys_own_exclude_file_applies_while_pane_runs() {
    let home = Home::new();
    // `.git` is never indexed: on Linux its `info` is watched apart.
    write(&home, "Projects/tool/.git/info/exclude", "");
    write(&home, "Projects/tool/notes.draft", "draft");
    let (launcher, _runtime) = home.with_files();
    eventually(&launcher, "notes", lists("notes.draft"));

    write(&home, "Projects/tool/.git/info/exclude", "*.draft\n");
    eventually(&launcher, "notes", lacks("notes.draft"));
    write(&home, "Projects/tool/.git/info/exclude", "");
    eventually(&launcher, "notes", lists("notes.draft"));
}

#[test]
fn the_users_excluded_patterns_apply_without_a_restart_across_batches() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    write(&home, "Documents/report.bak", "old");
    eventually(&launcher, "report", lists("report.bak"));

    change_rules(&home, &launcher, |rules| {
        rules.excluded_patterns.push("*.bak".into())
    });
    eventually(&launcher, "report", lacks("report.bak"));
    write(&home, "Documents/later.bak", "old");
    write(&home, "Documents/later.txt", "new");
    eventually(&launcher, "later", lists("later.txt"));
    eventually(&launcher, "later", lacks("later.bak"));

    change_rules(&home, &launcher, |rules| rules.excluded_patterns.clear());
    eventually(&launcher, "report", lists("report.bak"));
    eventually(&launcher, "later", lists("later.bak"));
    write(&home, "Documents/last.bak", "old");
    eventually(&launcher, "last", lists("last.bak"));
}

#[test]
fn a_file_an_ignore_file_hides_since_it_was_found_is_explained_not_opened() {
    let home = Home::new();
    let (launcher, _runtime) = home.with_files();
    write(&home, "Projects/app/src/notes.draft", "draft");
    eventually(&launcher, "notes", lists("notes.draft"));
    select_title(&launcher, "notes.draft");

    write(&home, "Projects/app/.gitignore", "build/\n*.draft\n");
    // Waited for through the index itself, so that the row stays selected.
    let indexer = launcher.file_indexer().unwrap();
    let owner = files_identity(&launcher).key();
    let deadline = std::time::Instant::now() + LIMIT;
    loop {
        settle(&launcher);
        let found = indexer
            .search(&owner, "notes", SearchOptions::default())
            .unwrap();
        if !found.iter().any(|entry| entry.name == "notes.draft") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{found:?}");
        std::thread::sleep(Duration::from_millis(50));
    }

    block_on(launcher.activate_selected());
    assert!(home.opener.take().is_empty());
    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Could not open notes.draft: it is no longer in the folders file search covers".into()
        )
    );
}
