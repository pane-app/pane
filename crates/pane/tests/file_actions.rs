//! Search Files and the file actions Pane performs itself (#150) in the
//! launcher's window, with real key events, the Files default extension,
//! a recording handler of files and a recording system: typing in the
//! command's own field lists the files Pane's file index found (#175, over
//! a fixture folder standing for the home folder); on a document,
//! Enter opens it and Ctrl+Enter reveals it, each closing the window; on a
//! program, Enter reveals it (nothing runs it) and Ctrl+Enter opens the
//! Actions panel at Open With…. A path typed into root search ending in a
//! separator lists the folder it names (#204): Tab completes the query to
//! a selected folder of the entries, Shift+Tab removes the last path
//! component, and Enter opens an entry, showing a program rather than
//! running it. The core's rules (every action, the other languages, root
//! search's file results) are `pane-core`'s `file_actions.rs` and
//! `typed_folders.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{AppContext, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use pane::LauncherWindow;
use pane_core::file_index::{IndexerConfig, WalkOptions};
use pane_core::{Launcher, LauncherView, LinkOpener, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;
// The recording system pane-core's tests use.
#[path = "../../pane-core/tests/support/system.rs"]
mod recording;

use recording::{Done, RecordingSystem};
use settle::{published, settle};

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

/// What Pane keeps for one test, and the fakes it acts through.
struct World {
    _data: TempDir,
    _temp: TempDir,
    /// The folder standing for the home folder, which the file index
    /// covers, named plainly inside `_temp` (whose own name starts with a
    /// dot, which the index leaves out as hidden).
    folder: PathBuf,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
}

impl World {
    /// Pane with Files installed and its file index of a folder holding
    /// `plan.txt`, `run plan.bat` and `notes/todo.md` settled.
    fn launcher(cx: &mut TestAppContext) -> (World, Launcher) {
        let package =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/files");
        assert!(
            package.exists(),
            "{} is missing; run `cargo xtask guests`",
            package.display()
        );
        cx.executor().allow_parking();
        let data = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("Home");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("plan.txt"), "plan").unwrap();
        fs::write(folder.join("run plan.bat"), "@echo off").unwrap();
        fs::create_dir(folder.join("notes")).unwrap();
        fs::write(folder.join("notes/todo.md"), "todo").unwrap();
        let runtime = Runtime::start().unwrap();
        let system = Arc::new(RecordingSystem::default());
        runtime.set_applications(system.clone());
        let opener = FakeOpener::default();
        let index = IndexerConfig {
            first_walk_delay: Duration::ZERO,
            walk: WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            ..IndexerConfig::native(&data.path().join("cache"), folder.clone(), Vec::new())
        };
        let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
            .with_link_opener(Arc::new(opener.clone()))
            .with_system(system.clone())
            .with_file_index(index);
        block_on(launcher.install_package(&package));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        assert!(
            launcher.wait_for_file_index(Duration::from_secs(60)),
            "{:?}",
            launcher.file_index_status()
        );
        launcher.show_root_search();
        let world = World {
            _data: data,
            _temp: temp,
            folder,
            opener,
            system,
        };
        (world, launcher)
    }
}

/// Opens the window over `launcher`, then Search Files in it as a user
/// does, and types `query` in its field.
fn search_files<'a>(
    cx: &'a mut TestAppContext,
    launcher: Launcher,
    query: &str,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("search files");
    // The query's list is published once its providers answered or its
    // budget ended (#201).
    let view = published(&window, cx);
    assert_eq!(
        view.rows.first().map(|row| row.title.as_str()),
        Some("Search Files")
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        },
        "{:?}",
        view.status
    );
    cx.simulate_input(query);
    // The files found, once the search answered: each titled with its
    // name and its folder below the home folder.
    until(&window, cx, |view| {
        view.status != Status::Running
            && view.rows.iter().any(|row| {
                row.subtitle
                    .as_deref()
                    .is_some_and(|subtitle| subtitle.starts_with('~'))
            })
    });
    settle(&window, cx);
    (window, cx)
}

/// Runs the window until `done` holds of the launcher's view.
fn until(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    done: impl Fn(&LauncherView) -> bool,
) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if done(&view) {
            return view;
        }
        assert!(Instant::now() < deadline, "timed out: {view:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until the launcher no longer runs an action: a hidden
/// window draws nothing, so this does not wait for a frame.
fn done(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    until(window, cx, |view| view.status != Status::Running)
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// What the HUD shows, if it shows.
fn hud(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| window.hud())
}

/// What showing a file in the file manager says on this system.
fn showed(name: &str) -> String {
    let manager = if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    };
    format!("Showed {name} in {manager}")
}

fn selected_title(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    view.rows[view.selected.expect("a row is selected")]
        .title
        .clone()
}

/// The secondary action's chord, Ctrl+Enter, on every system.
const SECONDARY: &str = "ctrl-enter";

#[gpui::test]
fn enter_opens_a_document_and_closes_the_window(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    let (window, cx) = search_files(cx, launcher, "plan.txt");
    assert_eq!(selected_title(&window, cx), "plan.txt");
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    let opened = world.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &world.folder.join("plan.txt")));
    assert!(world.system.take().is_empty());
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx).as_deref(), Some("Opened plan.txt"));
}

#[gpui::test]
fn ctrl_enter_reveals_a_document_and_closes_the_window(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    let (window, cx) = search_files(cx, launcher, "plan.txt");
    cx.simulate_keystrokes(SECONDARY);
    done(&window, cx);
    match world.system.take().as_slice() {
        [Done::Revealed(path)] => assert!(same_file(path, &world.folder.join("plan.txt"))),
        other => panic!("{other:?}"),
    }
    assert!(world.opener.take().is_empty());
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx), Some(showed("plan.txt")));
}

#[gpui::test]
fn enter_reveals_a_program_and_runs_nothing(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    let (window, cx) = search_files(cx, launcher, "run plan");
    assert_eq!(selected_title(&window, cx), "run plan.bat");
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    match world.system.take().as_slice() {
        [Done::Revealed(path)] => {
            assert!(same_file(path, &world.folder.join("run plan.bat")))
        }
        other => panic!("{other:?}"),
    }
    assert!(world.opener.take().is_empty(), "nothing ran it");
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx), Some(showed("run plan.bat")));
}

#[gpui::test]
fn ctrl_enter_on_a_program_opens_the_panel_at_open_with(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    let (window, cx) = search_files(cx, launcher, "run plan");
    cx.simulate_keystrokes(SECONDARY);
    settle(&window, cx);
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    // The installed applications, by name, once the submenu answered.
    let deadline = Instant::now() + Duration::from_secs(60);
    while cx.debug_bounds("action-Notepad").is_none() {
        assert!(
            Instant::now() < deadline,
            "Open With… never listed the applications"
        );
        settle(&window, cx);
        std::thread::sleep(Duration::from_millis(5));
    }
    for entry in ["action-code editor", "action-Notepad", "action-Zed"] {
        assert!(cx.debug_bounds(entry).is_some(), "{entry} is listed");
    }
    assert!(world.system.take().is_empty(), "nothing was done yet");
    assert!(world.opener.take().is_empty(), "nothing ran it");
    assert!(!hidden(&window, cx));
}

/// The query that lists `folder`'s entries: the folder's path, as the user
/// typed it, ending in a separator.
fn typed_query(folder: &Path) -> String {
    let separator = if cfg!(windows) { "\\" } else { "/" };
    format!("{}{separator}", folder.display())
}

/// Types a root search query in the field and answers the view once the
/// window has drawn the rows it listed.
fn typed_root(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    query: &str,
) -> LauncherView {
    cx.simulate_input(query);
    until(window, cx, |view| !view.rows.is_empty())
}

/// Moves the selection to the row titled `title`, with the down key, and
/// answers the view once the window has drawn it.
fn select_row(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let view = until(window, cx, |view| {
        view.rows.iter().any(|row| row.title == title)
    });
    let index = view
        .rows
        .iter()
        .position(|row| row.title == title)
        .expect("the row is listed");
    for _ in 0..index {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(selected_title(window, cx), title);
    view
}

#[gpui::test]
fn tab_completes_a_typed_folder_and_enter_opens_its_entry(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Typing a path ending in a separator lists the folder it names: the
    // folder's entries, the folder first.
    typed_root(&window, cx, &typed_query(&world.folder));
    select_row(&window, cx, "notes");
    // Tab completes the query to the selected folder's path, with a
    // separator after it, which lists the folder's own entries.
    cx.simulate_keystrokes("tab");
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "todo.md")
    });
    assert_eq!(
        view.query(),
        Some(typed_query(&world.folder.join("notes")).as_str())
    );
    select_row(&window, cx, "todo.md");
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    let opened = world.opener.take();
    assert_eq!(opened.len(), 1);
    assert!(same_file(&opened[0], &world.folder.join("notes/todo.md")));
    assert!(world.system.take().is_empty());
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx).as_deref(), Some("Opened todo.md"));
}

#[gpui::test]
fn shift_tab_removes_the_last_path_component(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // The folder the query typed lists, then the folder above it again
    // once the last path component is removed: the entries of `Home` are
    // back, the folder among them.
    typed_root(&window, cx, &typed_query(&world.folder.join("notes")));
    select_row(&window, cx, "todo.md");
    cx.simulate_keystrokes("shift-tab");
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "notes")
    });
    assert_eq!(view.query(), Some(typed_query(&world.folder).as_str()));
    // Nothing was opened by the keys alone.
    assert!(world.opener.take().is_empty());
    assert!(world.system.take().is_empty());
    assert!(!hidden(&window, cx));
}

/// The query root search holds now.
fn query(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| {
        window.launcher().view().query().map(str::to_owned)
    })
}

/// A Tab pressed while the typed folder's entries are still being listed
/// is held for them (#203): the completion then runs on the published
/// list's selected row, as a pressed Tab does (#204). Shift+Tab held the
/// same way removes the last path component once the list is published.
#[gpui::test]
fn tab_and_shift_tab_held_for_the_typed_folders_entries_still_browse(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));

    // Typing a path ending in a separator lists the folder it names — but
    // not at once: the Files provider answers while the field already
    // shows the path, so a Tab pressed right after typing is held.
    cx.simulate_input(&typed_query(&world.folder));
    cx.simulate_keystrokes("tab");
    assert_eq!(
        query(&window, cx).as_deref(),
        Some(typed_query(&world.folder).as_str()),
        "the completion has not run yet"
    );

    // The entries published, the held Tab is applied to the selection at
    // that moment (#203): the address row the listing puts first, on
    // which Tab completes nothing (#204) — the folder's rows are what it
    // completes. The query stands, and the key opened nothing.
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "notes")
    });
    assert_eq!(view.query(), Some(typed_query(&world.folder).as_str()));
    assert_eq!(view.selected, Some(0), "the address row is first");
    assert!(world.opener.take().is_empty(), "the key opened nothing");

    // The folder's row picked with the pointer — the held Tab took the
    // keyboard off the field — and the field clicked back: a pressed
    // Tab then completes the query to that row's path, which lists that
    // folder's own entries. The row's element follows its data by a
    // frame — `until` above waits only for the data — so it is polled
    // for as the Actions panel's entries are below. The pointer's first
    // event in the window only records where it is, so a second move
    // onto the row is what selects it (see window.rs's pointer tests).
    let deadline = Instant::now() + Duration::from_secs(60);
    while cx.debug_bounds("row-notes").is_none() {
        assert!(Instant::now() < deadline, "the folder's row never drew");
        settle(&window, cx);
        std::thread::sleep(Duration::from_millis(5));
    }
    let row = cx.debug_bounds("row-notes").expect("the folder's row");
    cx.simulate_mouse_move(row.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_mouse_move(
        row.center() + gpui::point(gpui::px(1.), gpui::px(0.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    let field = cx.debug_bounds("search").expect("the query field");
    // The query's input is the search header, the container's top row:
    // the container's middle is the list's, whose rows a click there
    // would pick instead of the field.
    cx.simulate_click(
        gpui::point(field.center().x, field.top() + gpui::px(32.)),
        Modifiers::none(),
    );
    cx.simulate_keystrokes("tab");
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "todo.md")
    });
    assert_eq!(
        view.query(),
        Some(typed_query(&world.folder.join("notes")).as_str())
    );

    // Shift+Tab pressed under the query typed on from there: it is held
    // too, and removes the last path component once the list is
    // published, leaving the folder's entries again — the query steps
    // back at once, the rows for it only once the listing answers, so
    // both are waited for.
    cx.simulate_input("more");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(
        query(&window, cx).as_deref(),
        Some(typed_query(&world.folder.join("notes").join("more")).as_str()),
        "the path component is not removed yet"
    );
    let view = until(&window, cx, |view| {
        view.query() == Some(typed_query(&world.folder.join("notes")).as_str())
            && view.rows.iter().any(|row| row.title == "todo.md")
    });
    assert!(view.rows.iter().any(|row| row.title == "todo.md"));

    // Nothing was opened by the keys alone.
    assert!(world.opener.take().is_empty());
    assert!(world.system.take().is_empty());
    assert!(!hidden(&window, cx));
}

#[gpui::test]
fn enter_on_a_typed_folder_entry_shows_a_program_and_runs_nothing(cx: &mut TestAppContext) {
    let (world, launcher) = World::launcher(cx);
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    typed_root(&window, cx, &typed_query(&world.folder));
    select_row(&window, cx, "run plan.bat");
    cx.simulate_keystrokes("enter");
    done(&window, cx);
    match world.system.take().as_slice() {
        [Done::Revealed(path)] => {
            assert!(same_file(path, &world.folder.join("run plan.bat")))
        }
        other => panic!("{other:?}"),
    }
    assert!(world.opener.take().is_empty(), "nothing ran it");
    assert!(hidden(&window, cx), "the launcher closed");
    assert_eq!(hud(&window, cx), Some(showed("run plan.bat")));
}
