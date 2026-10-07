//! File search, the Files default extension, through the launcher's public
//! interface: the folder the user grants it through Pane's own row (the
//! host's grant, checked and recorded in Pane's data), the files root search
//! finds in it, shown by the host's own names, opening one after the host
//! checks it again (a recording fake stands in for the system's handler, so
//! nothing opens; a program or script is revealed instead, by a recording
//! system), the bounded scan policy on controlled folder fixtures,
//! and the listing: made once per visit of root search on the package's
//! worker, never holding up other results, and stopped when root search is
//! left, the grant changes or the extension is disabled (with a fake folder
//! lister the test holds up). The packages are the ones `cargo xtask guests`
//! assembles in `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::applications::{Application, Applications};
use pane_core::files::{self, FolderListing, Folders, Limits, Listed};
use pane_core::{Launcher, LinkOpener, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use rows::titles;
use system::{Done, RecordingSystem};

/// Activates the selected row and answers what the HUD then said, if one
/// showed: Pane's own file actions close the window and say what they did
/// in one, as the standard actions do.
fn activated(launcher: &Launcher) -> Option<String> {
    let window = RecordingWindow::attach(launcher);
    block_on(launcher.activate_selected());
    window.huds().pop().map(|hud| hud.title)
}

/// The Files default extension's command, which searches as the user types
/// and computes root search's file results.
const SEARCH_FILES: &str = "Search Files";

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
    fn files(&self) -> Vec<PathBuf> {
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

/// The same file as Pane reported it and as the test made it.
fn same_file(reported: &Path, made: &Path) -> bool {
    fs::canonicalize(reported).unwrap() == fs::canonicalize(made).unwrap()
}

/// Pane's data location for one test, which outlives restarts.
struct Pane {
    data: TempDir,
    sources: TempDir,
    opener: FakeOpener,
    /// Reveals what Enter does not open: a program or script.
    system: Arc<RecordingSystem>,
    folders: Option<Arc<dyn Folders>>,
    applications: Option<Arc<dyn Applications>>,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            data: tempfile::tempdir().unwrap(),
            sources: tempfile::tempdir().unwrap(),
            opener: FakeOpener::default(),
            system: Arc::new(RecordingSystem::default()),
            folders: None,
            applications: None,
        }
    }

    /// Pane whose granted folders are listed through `folders`.
    fn with_folders(folders: Arc<dyn Folders>) -> Pane {
        Pane {
            folders: Some(folders),
            ..Pane::new()
        }
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// Starts Pane on this data location, as after a restart.
    fn start(&self) -> (Launcher, Runtime) {
        let runtime = Runtime::start().unwrap();
        if let Some(folders) = &self.folders {
            runtime.set_folders(folders.clone());
        }
        if let Some(applications) = &self.applications {
            runtime.set_applications(applications.clone());
        }
        let launcher = Launcher::with_packages(Ok(runtime.clone()), vec![], self.extensions())
            .with_link_opener(Arc::new(self.opener.clone()))
            .with_system(self.system.clone());
        (launcher, runtime)
    }

    /// Starts Pane with the package in `target/guests/packages/<package>`
    /// installed.
    fn with(&self, package: &str) -> (Launcher, Runtime) {
        let (launcher, runtime) = self.start();
        install(&launcher, &built(&format!("packages/{package}")));
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

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

fn identity(launcher: &Launcher, title: &str) -> PackageIdentity {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.title() == title)
        .expect("the package is installed")
        .identity
}

/// Opens the command titled `title` from root search: a command's list,
/// or the field of one that searches as the user types (Search Files).
fn open_command(launcher: &Launcher, title: &str) {
    launcher.back();
    launcher.back();
    search(launcher, title);
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("{title} is not listed: {:?}", titles(launcher)));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(
        matches!(
            launcher.view().screen,
            Screen::Command | Screen::CommandSearch { .. }
        ),
        "{:?}",
        launcher.view().screen
    );
}

/// Grants the package of the command titled `title` the folder `folder`
/// through Pane's "Choose folder…" row, as the window does with the folder
/// the picker returned, and returns to root search with the status.
fn grant(launcher: &Launcher, title: &str, folder: &Path) -> Status {
    open_command(launcher, title);
    assert_eq!(titles(launcher)[0], "Choose folder…");
    launcher.select(0);
    let identity = launcher
        .folder_to_choose()
        .expect("the row asks for a folder");
    block_on(launcher.activate_selected());
    block_on(launcher.grant_folder(&identity, folder));
    let status = launcher.view().status;
    launcher.back();
    status
}

/// A folder of controlled fixtures, with a space and non-ASCII letters in
/// its own name and in a file's:
///
/// ```text
/// Pane files — ñ/
///   Résumé plan ü.txt
///   files index.txt
///   .hidden plan.txt
///   .git/plan.txt
///   notes/plan.md
///   notes/todo.txt
/// ```
struct Fixture {
    // Keeps the folder alive; only the Unix-only link tests read it.
    #[cfg_attr(not(unix), allow(dead_code))]
    dir: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Pane files — ñ");
        for (file, text) in [
            ("Résumé plan ü.txt", "résumé"),
            ("files index.txt", "index"),
            (".hidden plan.txt", "hidden"),
            (".git/plan.txt", "git"),
            ("notes/plan.md", "plan"),
            ("notes/todo.txt", "todo"),
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        Fixture { dir, root }
    }

    fn file(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

fn never() -> bool {
    false
}

fn relatives(listing: &FolderListing) -> Vec<&str> {
    listing
        .files
        .iter()
        .map(|file| file.relative.as_str())
        .collect()
}

#[test]
fn a_listing_is_breadth_first_in_name_order_without_hidden_entries() {
    let fixture = Fixture::new();
    let listing = files::walk(&fixture.root, &Limits::default(), &never).unwrap();
    assert_eq!(
        relatives(&listing),
        [
            "Résumé plan ü.txt",
            "files index.txt",
            "notes/plan.md",
            "notes/todo.txt"
        ]
    );
    assert!(!listing.truncated);
    for file in &listing.files {
        assert!(same_file(&file.path, &fixture.file(&file.relative)));
    }
}

#[cfg(unix)]
#[test]
fn links_are_neither_listed_nor_followed() {
    let fixture = Fixture::new();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("outside.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), fixture.root.join("linked folder")).unwrap();
    std::os::unix::fs::symlink(
        fixture.file("notes/todo.txt"),
        fixture.root.join("linked todo.txt"),
    )
    .unwrap();
    let listing = files::walk(&fixture.root, &Limits::default(), &never).unwrap();
    assert!(
        !relatives(&listing)
            .iter()
            .any(|relative| relative.contains("linked") || relative.contains("outside")),
        "{:?}",
        relatives(&listing)
    );
}

#[cfg(windows)]
#[test]
fn hidden_attributes_and_junctions_are_skipped_on_windows() {
    use std::process::Command;
    let fixture = Fixture::new();
    let hidden = fixture.root.join("attribute plan.txt");
    fs::write(&hidden, "hidden by its attribute").unwrap();
    let status = Command::new("attrib")
        .arg("+h")
        .arg(&hidden)
        .status()
        .unwrap();
    assert!(status.success());
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("outside.txt"), "outside").unwrap();
    // A junction needs no privilege, unlike a symbolic link.
    let status = Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(fixture.root.join("junction"))
        .arg(elsewhere.path())
        .status()
        .unwrap();
    assert!(status.success());
    let listing = files::walk(&fixture.root, &Limits::default(), &never).unwrap();
    assert!(
        !relatives(&listing)
            .iter()
            .any(|relative| { relative.contains("attribute") || relative.contains("junction") }),
        "{:?}",
        relatives(&listing)
    );
}

#[test]
fn a_listing_stops_at_its_limits_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    for level in 0..4 {
        fs::write(deep.join(format!("level {level}.txt")), "").unwrap();
        deep = deep.join(format!("sub{level}"));
        fs::create_dir(&deep).unwrap();
    }
    let everything = files::walk(dir.path(), &Limits::default(), &never).unwrap();
    assert_eq!(everything.files.len(), 4);
    assert!(!everything.truncated);

    let shallow = Limits {
        depth: 2,
        ..Limits::default()
    };
    let listing = files::walk(dir.path(), &shallow, &never).unwrap();
    assert_eq!(
        relatives(&listing),
        ["level 0.txt", "sub0/level 1.txt", "sub0/sub1/level 2.txt"]
    );
    assert!(listing.truncated, "a folder deeper than the limit was left");

    let few = Limits {
        files: 2,
        ..Limits::default()
    };
    let listing = files::walk(dir.path(), &few, &never).unwrap();
    assert_eq!(listing.files.len(), 2);
    assert!(listing.truncated);
}

#[test]
fn a_big_folder_is_read_only_up_to_the_entry_limit() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..50 {
        fs::write(dir.path().join(format!("file {n:02}.txt")), "").unwrap();
    }
    // Entries are counted as the folder is read, not after it is read whole.
    let counted = std::sync::atomic::AtomicUsize::new(0);
    let counting = || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    };
    let limits = Limits {
        entries: 10,
        ..Limits::default()
    };
    let listing = files::walk(dir.path(), &limits, &counting).unwrap();
    assert_eq!(listing.files.len(), 10);
    assert!(listing.truncated);
    assert_eq!(counted.load(std::sync::atomic::Ordering::SeqCst), 11);

    // Cancelled, it stops at the first entry.
    assert!(files::walk(dir.path(), &Limits::default(), &|| true).is_err());
}

#[cfg(unix)]
#[test]
fn a_subfolder_that_cannot_be_read_makes_the_listing_partial() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let notes = fixture.root.join("notes");
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&notes).is_ok();
    let listing = files::walk(&fixture.root, &Limits::default(), &never);
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        // Running as root: permissions do not stop it.
        return;
    }
    let listing = listing.unwrap();
    assert_eq!(
        relatives(&listing),
        ["Résumé plan ü.txt", "files index.txt"]
    );
    assert!(listing.truncated);
}

#[test]
fn a_grant_is_refused_for_the_root_home_hidden_files_and_missing_folders() {
    let fixture = Fixture::new();
    let refused = |folder: &Path| files::check_grant(folder).unwrap_err();
    let root = fixture.root.ancestors().last().unwrap().to_path_buf();
    assert!(refused(&root).contains("whole disk"), "{}", refused(&root));
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|home| home.is_dir());
    if let Some(home) = home {
        assert!(refused(&home).contains("home folder"), "{}", refused(&home));
    }
    let hidden = fixture.root.join(".git");
    assert!(
        refused(&hidden).contains("hidden folder"),
        "{}",
        refused(&hidden)
    );
    let file = fixture.file("notes/todo.txt");
    assert!(refused(&file).ends_with("is a file, not a folder"));
    assert!(refused(&fixture.root.join("missing")).ends_with("does not exist"));
    assert!(refused(Path::new("notes")).contains("is not a full path"));
    if cfg!(windows) {
        for network in [r"\\server\share", r"\\?\UNC\server\share", "//server/share"] {
            assert!(
                refused(Path::new(network)).contains("network location"),
                "{network}"
            );
        }
    }
    let granted = files::check_grant(&fixture.root).unwrap();
    assert!(same_file(&granted, &fixture.root));
}

#[test]
fn programs_and_scripts_are_told_apart_from_documents() {
    let dir = tempfile::tempdir().unwrap();
    let runs = |name: &str| {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        files::runs_as_program(&path, &fs::metadata(&path).unwrap())
    };
    for program in [
        "setup.EXE",
        "run.bat",
        "Start.lnk",
        "tool.ps1",
        "site.url",
        "open.command",
        "Launch.desktop",
        "Viewer.app/Contents/MacOS/viewer",
    ] {
        assert!(runs(program), "{program}");
    }
    for document in ["plan.txt", "report.pdf", "notes.md", "app.txt"] {
        assert!(!runs(document), "{document}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.path().join("script");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(files::runs_as_program(
            &script,
            &fs::metadata(&script).unwrap()
        ));
    }
}

#[test]
fn a_granted_folder_is_searched_and_a_found_file_opened() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");

    // No folder granted yet: nothing is found, and nothing fails.
    search(&launcher, "plan");
    assert_eq!(titles(&launcher), Vec::<String>::new());

    let status = grant(&launcher, SEARCH_FILES, &fixture.root);
    assert_eq!(
        status,
        Status::Result("Files may now list “Pane files — ñ”".into())
    );

    search(&launcher, "plan");
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Résumé plan ü.txt", "plan.md"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("File in Pane files — ñ")
    );
    assert_eq!(
        view.rows[1].subtitle.as_deref(),
        Some("File in Pane files — ñ/notes")
    );
    assert_eq!(view.selected, Some(0));
    assert_eq!(
        activated(&launcher).as_deref(),
        Some("Opened Résumé plan ü.txt")
    );
    let opened = pane.opener.files();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &fixture.file("Résumé plan ü.txt")));
    assert_eq!(launcher.view().query(), Some("plan"), "root search stays");

    // Words may be in the folders below the granted one; case is ignored.
    search(&launcher, "NOTES todo");
    assert_eq!(titles(&launcher), ["todo.txt"]);
    search(&launcher, "hidden");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

#[test]
fn the_grant_is_pane_s_own_record_not_the_extension_s_data() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    grant(&launcher, SEARCH_FILES, &fixture.root);

    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pane.extensions().join("folders.json")).unwrap())
            .unwrap();
    let folders = record["folders"].as_object().unwrap();
    assert_eq!(folders.len(), 1);
    let (key, folder) = folders.iter().next().unwrap();
    assert_eq!(*key, identity(&launcher, "Files").key());
    assert!(same_file(
        Path::new(folder.as_str().unwrap()),
        &fixture.root
    ));
    // The extension holds no path of it.
    let settings = pane.extensions().join("settings.json");
    if let Ok(text) = fs::read_to_string(settings) {
        assert!(!text.contains("Pane files"), "{text}");
    }

    // A refused folder changes nothing.
    let status = grant(&launcher, SEARCH_FILES, &fixture.root.join(".git"));
    assert!(
        matches!(&status, Status::Error(message)
            if message.starts_with("Pane did not grant the folder:") && message.contains("hidden")),
        "{status:?}"
    );
    search(&launcher, "todo");
    assert_eq!(titles(&launcher), ["todo.txt"]);
}

#[test]
fn stopping_to_share_the_folder_removes_the_files() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    grant(&launcher, SEARCH_FILES, &fixture.root);
    open_command(&launcher, SEARCH_FILES);
    assert_eq!(titles(&launcher)[1], "Stop sharing the folder with Files");
    launcher.select(1);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Files no longer lists a folder".into())
    );
    assert_eq!(titles(&launcher)[0], "Choose folder…");
    assert_ne!(titles(&launcher)[1], "Stop sharing the folder with Files");
    launcher.back();
    search(&launcher, "todo");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

#[test]
fn a_guest_cannot_name_a_file_or_retitle_one() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.start();
    let folder = pane.sources.path().join("faulty");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Faulty", "apiVersion": "0.1",
  "folderAccess": true,
  "commands": [{ "id": "command", "title": "Faulty answers", "component": "command.wasm",
    "rootResults": true }] }"#,
    )
    .unwrap();
    fs::copy(built("faulty.wasm"), folder.join("command.wasm")).unwrap();
    install(&launcher, &folder);
    grant(&launcher, "Faulty answers", &fixture.root);

    // A path of the guest's own is not a file Pane listed: nothing is shown.
    search(&launcher, "forged file");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    // Titles the guest gave are replaced by the files' own names.
    search(&launcher, "spoof");
    let view = launcher.view();
    assert_eq!(
        titles(&launcher),
        [
            "Résumé plan ü.txt",
            "files index.txt",
            "plan.md",
            "todo.txt"
        ]
    );
    assert_eq!(
        view.rows[2].subtitle.as_deref(),
        Some("File in Pane files — ñ/notes")
    );
    assert_eq!(
        activated(&launcher).as_deref(),
        Some("Opened Résumé plan ü.txt")
    );
}

#[test]
fn a_file_is_checked_again_when_it_is_opened() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    fs::write(fixture.file("notes/run plan.bat"), "@echo off").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = fixture.file("notes/plan script");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    grant(&launcher, SEARCH_FILES, &fixture.root);
    let open = |query: &str, title: &str| {
        search(&launcher, query);
        let index = titles(&launcher)
            .iter()
            .position(|row| row == title)
            .unwrap_or_else(|| panic!("{title} not found: {:?}", titles(&launcher)));
        launcher.select(index);
        activated(&launcher)
    };

    // Programs and scripts are found, but Enter shows them in the file
    // manager rather than open them (only their Run action runs them:
    // `file_actions.rs`), and says so in a HUD.
    let manager = if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    };
    assert_eq!(
        open("run plan", "run plan.bat"),
        Some(format!("Showed run plan.bat in {manager}"))
    );
    assert!(matches!(pane.system.take().as_slice(), [Done::Revealed(_)]));
    #[cfg(unix)]
    {
        assert_eq!(
            open("plan script", "plan script"),
            Some(format!("Showed plan script in {manager}"))
        );
        assert!(matches!(pane.system.take().as_slice(), [Done::Revealed(_)]));
    }

    // Removed since it was found.
    search(&launcher, "todo");
    fs::remove_file(fixture.file("notes/todo.txt")).unwrap();
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Error("Could not open todo.txt: it no longer exists".into())
    );

    // Replaced by a link to a file outside the folder since it was found.
    #[cfg(unix)]
    {
        let outside = fixture.dir.path().join("outside.txt");
        fs::write(&outside, "outside").unwrap();
        search(&launcher, "index");
        fs::remove_file(fixture.file("files index.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.file("files index.txt")).unwrap();
        block_on(launcher.activate_selected());
        assert_eq!(
            launcher.view().status,
            Status::Error(
                "Could not open files index.txt: it is now a link; Pane opens only files in \
                 the granted folder"
                    .into()
            )
        );
        // A folder above it replaced by a link outside the grant.
        search(&launcher, "plan.md");
        let notes = fixture.file("notes");
        fs::rename(&notes, fixture.dir.path().join("moved notes")).unwrap();
        std::os::unix::fs::symlink(fixture.dir.path().join("moved notes"), &notes).unwrap();
        block_on(launcher.activate_selected());
        assert_eq!(
            launcher.view().status,
            Status::Error(
                "Could not open plan.md: it is no longer inside the granted folder".into()
            )
        );
    }
    assert!(pane.opener.files().is_empty(), "{:?}", pane.opener.files());
}

#[test]
fn a_folder_gone_since_it_was_granted_is_explained_in_root_search() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    grant(&launcher, SEARCH_FILES, &fixture.root);
    fs::remove_dir_all(&fixture.root).unwrap();

    search(&launcher, "plan");
    let view = launcher.view();
    assert_eq!(titles(&launcher), [SEARCH_FILES]);
    let subtitle = view.rows[0].subtitle.clone().unwrap();
    assert!(
        subtitle.starts_with("Could not answer:") && subtitle.ends_with("does not exist"),
        "{subtitle}"
    );
}

#[test]
fn files_are_listed_after_the_results_found_by_title() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    grant(&launcher, SEARCH_FILES, &fixture.root);
    search(&launcher, "files");
    assert_eq!(titles(&launcher), [SEARCH_FILES, "files index.txt"]);
    assert_eq!(launcher.view().selected, Some(0));
}

#[test]
fn the_grant_is_kept_across_a_restart_disabling_hides_it_and_uninstalling_forgets_it() {
    let fixture = Fixture::new();
    let pane = Pane::new();
    let (launcher, _runtime) = pane.with("files");
    grant(&launcher, SEARCH_FILES, &fixture.root);
    drop(launcher);

    let (launcher, _runtime) = pane.start();
    search(&launcher, "todo");
    assert_eq!(titles(&launcher), ["todo.txt"]);

    let files = identity(&launcher, "Files");
    block_on(launcher.set_enabled(&files, false));
    search(&launcher, "tod");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    block_on(launcher.set_enabled(&files, true));
    search(&launcher, "todo");
    assert_eq!(titles(&launcher), ["todo.txt"]);

    block_on(launcher.uninstall(&files, pane_core::SavedData::Keep));
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pane.extensions().join("folders.json")).unwrap())
            .unwrap();
    assert!(record["folders"].as_object().unwrap().is_empty());
}

#[test]
fn the_javascript_and_typescript_samples_find_and_open_files_too() {
    let fixture = Fixture::new();
    for (package, title) in [
        ("sample-files-js", "Find files (JavaScript)"),
        ("sample-files-ts", "Find files (TypeScript)"),
    ] {
        let pane = Pane::new();
        let (launcher, _runtime) = pane.with(package);
        let status = grant(&launcher, title, &fixture.root);
        assert!(matches!(status, Status::Result(_)), "{status:?}");
        search(&launcher, "résumé");
        assert_eq!(titles(&launcher), ["Résumé plan ü.txt"], "{title}");
        assert_eq!(
            launcher.view().rows[0].subtitle.as_deref(),
            Some("File in Pane files — ñ")
        );
        assert_eq!(
            activated(&launcher).as_deref(),
            Some("Opened Résumé plan ü.txt")
        );
        let opened = pane.opener.files();
        assert_eq!(opened.len(), 1, "{title}");
        assert!(same_file(&opened[0], &fixture.file("Résumé plan ü.txt")));
    }
}

/// A folder lister the test holds up: each listing waits until the test
/// lets it finish, or until it is cancelled. Every listing finds the files
/// `names`.
struct HeldFolders {
    names: Vec<&'static str>,
    state: Mutex<Held>,
    changed: Condvar,
}

#[derive(Default)]
struct Held {
    started: usize,
    cancelled: usize,
    returned: usize,
    released: bool,
}

impl HeldFolders {
    fn new(names: Vec<&'static str>) -> Arc<HeldFolders> {
        Arc::new(HeldFolders {
            names,
            state: Mutex::new(Held::default()),
            changed: Condvar::new(),
        })
    }

    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.changed.notify_all();
    }

    /// Waits until `done` holds of the state, for at most ten seconds.
    fn wait_for(&self, what: &str, done: impl Fn(&Held) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.state.lock().unwrap();
        while !done(&state) {
            let left = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting until {what}"));
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    fn started(&self) -> usize {
        self.state.lock().unwrap().started
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
        state.started += 1;
        self.changed.notify_all();
        while !state.released {
            if cancelled() {
                state.cancelled += 1;
                state.returned += 1;
                self.changed.notify_all();
                return Err("cancelled".into());
            }
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap()
                .0;
        }
        state.returned += 1;
        self.changed.notify_all();
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

/// Runs `launcher.set_query(query)` on a thread of its own; the receiver
/// hears once it has finished.
fn search_in_background(launcher: &Launcher, query: &str) -> mpsc::Receiver<()> {
    let searching = launcher.set_query(query);
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        block_on(searching);
        let _ = done.send(());
    });
    finished
}

fn finishes(search: &mpsc::Receiver<()>) {
    search
        .recv_timeout(Duration::from_secs(10))
        .expect("the search finished");
}

/// Waits until the launcher lists `title`, for at most ten seconds.
fn wait_for_row(launcher: &Launcher, title: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !titles(launcher).iter().any(|row| row == title) {
        assert!(
            Instant::now() < deadline,
            "{title} was not listed: {:?}",
            titles(launcher)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A folder in `pane`'s sources to grant: its files come from the fake.
fn granted_folder(pane: &Pane) -> PathBuf {
    let folder = pane.sources.path().join("Granted");
    fs::create_dir_all(&folder).unwrap();
    for name in ["report late.txt", "report current.txt"] {
        fs::write(folder.join(name), "").unwrap();
    }
    folder
}

/// Installed applications that answer at once.
struct OneApplication;

impl Applications for OneApplication {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(vec![Application {
            id: "report-writer".into(),
            name: "Report Writer".into(),
            location: "test".into(),
        }])
    }

    fn open(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn a_slow_listing_holds_up_neither_the_calculator_nor_the_applications() {
    let folders = HeldFolders::new(vec!["report 2 + 2.txt"]);
    let mut pane = Pane::with_folders(folders.clone());
    pane.applications = Some(Arc::new(OneApplication));
    let (launcher, _runtime) = pane.with("files");
    install(&launcher, &built("packages/calculator"));
    install(&launcher, &built("packages/applications"));
    let folder = granted_folder(&pane);
    let identity = identity(&launcher, "Files");
    block_on(launcher.grant_folder(&identity, &folder));
    launcher.back();

    // The Files command is asked first, and its folder is still listing.
    let pending = search_in_background(&launcher, "report");
    folders.wait_for("the folder is being listed", |held| held.started == 1);
    wait_for_row(&launcher, "Report Writer");
    let second = search_in_background(&launcher, "2 + 2");
    wait_for_row(&launcher, "4");
    finishes(&pending);
    assert_eq!(folders.started(), 1, "one listing for the visit");
    folders.release();
    finishes(&second);
    assert!(
        titles(&launcher)
            .iter()
            .any(|row| row == "report 2 + 2.txt"),
        "{:?}",
        titles(&launcher)
    );
    // Later keystrokes filter the kept listing: nothing is listed again.
    search(&launcher, "2 + 2.txt");
    assert_eq!(titles(&launcher), ["report 2 + 2.txt"]);
    assert_eq!(folders.started(), 1);
}

#[test]
fn a_new_query_waits_for_the_same_listing_and_older_answers_never_show() {
    let folders = HeldFolders::new(vec!["report late.txt", "report current.txt"]);
    let pane = Pane::with_folders(folders.clone());
    let (launcher, _runtime) = pane.with("files");
    let folder = granted_folder(&pane);
    block_on(launcher.grant_folder(&identity(&launcher, "Files"), &folder));
    launcher.back();

    let first = search_in_background(&launcher, "late");
    folders.wait_for("the folder is being listed", |held| held.started == 1);
    let second = search_in_background(&launcher, "current");
    // The first search's wait is cancelled at once.
    finishes(&first);
    folders.release();
    finishes(&second);
    assert_eq!(titles(&launcher), ["report current.txt"]);
    assert_eq!(folders.started(), 1);
}

#[test]
fn leaving_root_search_stops_the_listing_and_the_next_visit_lists_again() {
    let folders = HeldFolders::new(vec!["report late.txt"]);
    let pane = Pane::with_folders(folders.clone());
    let (launcher, runtime) = pane.with("files");
    let folder = granted_folder(&pane);
    block_on(launcher.grant_folder(&identity(&launcher, "Files"), &folder));
    launcher.back();

    let pending = search_in_background(&launcher, "report");
    folders.wait_for("the folder is being listed", |held| held.started == 1);
    block_on(launcher.preview_package(&built("packages/calculator")));
    assert!(matches!(launcher.view().screen, Screen::Package { .. }));
    folders.wait_for("the listing is cancelled", |held| held.cancelled == 1);
    finishes(&pending);
    // The extension's instance was not held by it.
    assert!(!block_on(runtime.running()).is_empty());

    folders.release();
    launcher.back();
    search(&launcher, "report");
    assert_eq!(titles(&launcher), ["report late.txt"]);
    assert_eq!(folders.started(), 2);
}

#[test]
fn disabling_files_stops_the_listing_and_nothing_arrives_afterwards() {
    let folders = HeldFolders::new(vec!["report late.txt"]);
    let pane = Pane::with_folders(folders.clone());
    let (launcher, runtime) = pane.with("files");
    let folder = granted_folder(&pane);
    let files = identity(&launcher, "Files");
    block_on(launcher.grant_folder(&files, &folder));
    launcher.back();

    let pending = search_in_background(&launcher, "report");
    folders.wait_for("the folder is being listed", |held| held.started == 1);
    block_on(launcher.set_enabled(&files, false));
    folders.wait_for("the listing is cancelled", |held| held.cancelled == 1);
    finishes(&pending);
    assert_eq!(titles(&launcher), Vec::<String>::new());
    assert!(block_on(runtime.running()).is_empty());
    // Released now, the listing already returned: nothing more arrives.
    folders.release();
    folders.wait_for("every listing returned", |held| held.returned == 1);
    assert_eq!(titles(&launcher), Vec::<String>::new());
    assert_eq!(folders.started(), 1);
}

#[test]
fn a_new_grant_stops_the_listing_of_the_old_one() {
    let folders = HeldFolders::new(vec!["report late.txt"]);
    let pane = Pane::with_folders(folders.clone());
    let (launcher, _runtime) = pane.with("files");
    let folder = granted_folder(&pane);
    let files = identity(&launcher, "Files");
    block_on(launcher.grant_folder(&files, &folder));
    launcher.back();

    let pending = search_in_background(&launcher, "report");
    folders.wait_for("the folder is being listed", |held| held.started == 1);
    let other = pane.sources.path().join("Other");
    fs::create_dir_all(&other).unwrap();
    block_on(launcher.grant_folder(&files, &other));
    folders.wait_for("the old listing is cancelled", |held| held.cancelled == 1);
    folders.release();
    drop(pending);
}
