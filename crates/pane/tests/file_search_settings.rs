//! The File Search page in Settings (#176), on GPUI's test platform, over
//! the Rust files sample (which uses the file index as the Files default
//! extension does) and Pane's file index of a fixture
//! folder standing for the home folder: the page says how the index is
//! doing (its state, its entries, when and how it last caught up) and
//! Rebuild Index builds it again; its switches, its pattern field and its
//! folder pickers change what is indexed without a restart; while the
//! files sample is off or missing it says file search is off and why; each
//! safety valve —
//! churn quarantine (with Include Again), the ceiling of entries, the
//! free-space floor and a folder that does not answer — is listed with its
//! reason and remedy; and the sidebar's search finds the page's controls.
//! The core's side of each control and valve is `pane-core`'s
//! `file_index.rs` and the coordinator's unit tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::file_index::{
    CaughtUpBy, IndexState, IndexerConfig, ProblemKind, UserRules, WalkOptions,
};
use pane_core::{Launcher, Runtime, Status};
use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/setup.rs"]
mod setup;

use a11y::a11y;
use setup::settings_shortcut;

const LIMIT: Duration = Duration::from_secs(60);

/// What one test keeps: Pane's data folder, and the folder standing for
/// the home folder (named plainly inside a temporary folder whose own name
/// starts with a dot, which the index leaves out as hidden).
struct World {
    data: TempDir,
    _temp: TempDir,
    home: PathBuf,
}

impl World {
    /// A home folder holding `Documents/plan.txt`, `Documents/notes.md`,
    /// `Music/song.mp3` and `.config/hidden plan.txt`.
    fn new() -> World {
        let data = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("Home");
        for (file, text) in [
            ("Documents/plan.txt", "plan"),
            ("Documents/notes.md", "notes"),
            ("Music/song.mp3", "mp3"),
            (".config/hidden plan.txt", "hidden"),
        ] {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        World {
            data,
            _temp: temp,
            home,
        }
    }

    /// Pane's own records' folder, where `file-search.json` is kept.
    fn records(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// Pane over the fixture home with its file index configured as `edit`
    /// says, the files sample installed unless `files` is false, and the
    /// index settled.
    fn launcher(&self, files: bool, edit: impl FnOnce(&mut IndexerConfig)) -> Launcher {
        let runtime = Runtime::start().unwrap();
        let mut config = IndexerConfig {
            first_walk_delay: Duration::ZERO,
            walk: WalkOptions {
                background: false,
                ..WalkOptions::default()
            },
            ..IndexerConfig::native(
                &self.data.path().join("cache"),
                self.home.clone(),
                Vec::new(),
            )
        };
        edit(&mut config);
        let launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.records()).with_file_index(config);
        if files {
            let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/guests/packages/sample-files");
            assert!(
                package.exists(),
                "{} is missing; run `cargo xtask guests`",
                package.display()
            );
            block_on(launcher.install_package(&package));
            assert!(
                matches!(launcher.view().status, Status::Result(_)),
                "{:?}",
                launcher.view().status
            );
            launcher.back();
        }
        assert!(
            launcher.wait_for_file_index(LIMIT),
            "{:?}",
            launcher.file_index_status()
        );
        launcher
    }
}

/// Opens the launcher window over `launcher`, then Settings on its File
/// Search page, tall enough that the whole page is in reach.
fn open_page(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = cx
        .cx
        .update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SettingsWindow>())
                .next()
        })
        .expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    settings_cx.simulate_resize(gpui::size(px(860.), px(1800.)));
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-File Search");
    assert!(
        settings_cx.debug_bounds("file-search").is_some(),
        "the File Search page is drawn"
    );
    (settings, settings_cx)
}

/// Clicks the element whose debug selector is `selector`, as its user
/// would: the pointer moving onto it first, the page scrolled to it if it
/// is below the window's edge.
fn click(cx: &mut VisualTestContext, selector: &str) {
    let viewport = cx.update(|window, _| window.viewport_size());
    let selector = selector.to_owned();
    for _ in 0..10 {
        let bounds = cx
            .debug_bounds(Box::leak(selector.clone().into_boxed_str()))
            .unwrap_or_else(|| panic!("{selector} is not drawn"));
        if bounds.bottom() <= viewport.height {
            break;
        }
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: gpui::point(bounds.center().x, viewport.height / 2.),
            delta: gpui::ScrollDelta::Pixels(gpui::point(
                px(0.),
                viewport.height - bounds.bottom() - px(24.),
            )),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
    }
    let bounds = cx
        .debug_bounds(Box::leak(selector.clone().into_boxed_str()))
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_mouse_move(bounds.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

/// Whether the element named `selector` is drawn.
fn drawn(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .is_some()
}

/// Runs the window until `done` holds of the launcher, the index settling
/// meanwhile; then lets the page's watcher redraw it.
fn until(cx: &mut VisualTestContext, launcher: &Launcher, done: impl Fn(&Launcher) -> bool) {
    let deadline = Instant::now() + LIMIT;
    loop {
        cx.run_until_parked();
        if launcher.wait_for_file_index(Duration::from_millis(100)) && done(launcher) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{:?} {:?}",
            launcher.file_index_status(),
            launcher.file_search_rules()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // The page's watcher redraws it once its tick comes.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
}

/// The names of the entries the index finds for `query`, as the files
/// sample would be told.
fn found(launcher: &Launcher, query: &str) -> Vec<String> {
    launcher.show_root_search();
    block_on(launcher.set_query(query));
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

#[gpui::test]
fn the_page_says_how_the_index_is_doing_and_rebuild_builds_it_again(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher.clone());
    let tree = a11y(&mut sc);
    assert!(tree.contains("Up to date"), "{tree}");
    assert!(tree.contains("files and folders indexed"), "{tree}");
    assert!(
        tree.contains("Last caught up by indexing every folder, just now"),
        "{tree}"
    );
    assert!(
        drawn(&mut sc, "file-search-root-Home"),
        "the home folder is listed"
    );
    for switch in [
        "file-search-hidden",
        "file-search-ignore-files",
        "file-search-default-exclusions",
        "file-search-other-volumes",
    ] {
        assert!(drawn(&mut sc, switch), "{switch} is drawn");
    }
    assert!(
        !drawn(&mut sc, "file-search-section-Problems"),
        "nothing to attend to"
    );

    // Rebuild Index deletes the index and builds it again.
    fs::write(world.home.join("Documents/after.txt"), "x").unwrap();
    // The index was current, by a full walk, before the rebuild too: the
    // rebuild is done once a full walk is recorded again, later.
    let before = launcher.file_index_status().caught_up;
    click(&mut sc, "file-search-rebuild");
    until(&mut sc, &launcher, |launcher| {
        let status = launcher.file_index_status();
        status.state == IndexState::Current
            && status.caught_up.map(|(by, _)| by) == Some(CaughtUpBy::FullWalk)
            && status.caught_up != before
            && found(launcher, "after")
                .iter()
                .any(|row| row == "after.txt")
    });
    assert!(a11y(&mut sc).contains("Up to date"));
}

#[gpui::test]
fn the_switches_change_what_is_indexed_without_a_restart(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher.clone());
    assert!(
        !found(&launcher, "hidden plan")
            .iter()
            .any(|row| row == "hidden plan.txt")
    );

    click(&mut sc, "file-search-hidden-row");
    until(&mut sc, &launcher, |launcher| {
        launcher.file_search_rules().unwrap().1.include_hidden
    });
    let deadline = Instant::now() + LIMIT;
    while !found(&launcher, "hidden plan")
        .iter()
        .any(|row| row == "hidden plan.txt")
    {
        assert!(Instant::now() < deadline, "the hidden file was never found");
        launcher.wait_for_file_index(Duration::from_millis(200));
    }
    // Recorded in Pane's own record.
    assert!(UserRules::read(&world.records()).include_hidden);

    click(&mut sc, "file-search-hidden-row");
    until(&mut sc, &launcher, |launcher| {
        !launcher.file_search_rules().unwrap().1.include_hidden
    });
    assert!(!UserRules::read(&world.records()).include_hidden);
}

#[gpui::test]
fn a_pattern_typed_is_excluded_and_its_remove_brings_it_back(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher.clone());

    click(&mut sc, "file-search-pattern-field");
    sc.simulate_input("*.md");
    sc.run_until_parked();
    click(&mut sc, "file-search-add-pattern");
    until(&mut sc, &launcher, |launcher| {
        launcher.file_search_rules().unwrap().1.excluded_patterns == ["*.md"]
    });
    assert!(
        drawn(&mut sc, "file-search-pattern-*.md"),
        "the pattern is listed"
    );
    assert!(
        !found(&launcher, "notes")
            .iter()
            .any(|row| row == "notes.md")
    );

    click(&mut sc, "file-search-remove-pattern-*.md");
    until(&mut sc, &launcher, |launcher| {
        launcher
            .file_search_rules()
            .unwrap()
            .1
            .excluded_patterns
            .is_empty()
    });
    assert!(!drawn(&mut sc, "file-search-pattern-*.md"));
}

#[gpui::test]
fn a_long_list_of_exclusions_draws_only_the_rows_near_the_pages_view(cx: &mut TestAppContext) {
    // Three hundred patterns: the page draws the ones near its view and a
    // stand-in for each other, as the Shortcuts page does (#165).
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_, mut rules) = launcher.file_search_rules().unwrap();
    rules.excluded_patterns = (0..300).map(|at| format!("*.p{at:03}")).collect();
    block_on(launcher.set_file_search_rules(rules)).unwrap();
    let (_settings, mut sc) = open_page(cx, launcher.clone());
    sc.update(|window, cx| window.simulate_next_frame(cx));
    sc.run_until_parked();
    assert!(
        drawn(&mut sc, "file-search-pattern-*.p000"),
        "the first is drawn"
    );
    assert!(
        !drawn(&mut sc, "file-search-pattern-*.p299"),
        "the last, far below the view, is a stand-in"
    );

    // Scrolled to the end, the last is drawn and the first no longer.
    let viewport = sc.update(|window, _| window.viewport_size());
    for _ in 0..4 {
        sc.simulate_event(gpui::ScrollWheelEvent {
            position: gpui::point(viewport.width / 2., viewport.height / 2.),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-40_000.))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        sc.update(|window, cx| window.simulate_next_frame(cx));
        sc.run_until_parked();
    }
    assert!(
        drawn(&mut sc, "file-search-pattern-*.p299"),
        "the last is drawn"
    );
    assert!(!drawn(&mut sc, "file-search-pattern-*.p000"));
}

#[gpui::test]
fn folders_are_added_and_excluded_through_the_systems_picker(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher.clone());

    // A folder outside the home folder, added.
    let drive = world._temp.path().join("Drive");
    fs::create_dir_all(&drive).unwrap();
    fs::write(drive.join("far plan.txt"), "x").unwrap();
    click(&mut sc, "file-search-add-root");
    assert!(sc.did_prompt_for_paths(), "a folder picker opened");
    let chosen = drive.clone();
    sc.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![chosen])
    });
    until(&mut sc, &launcher, |launcher| {
        launcher.file_search_rules().unwrap().1.added_roots == [drive.clone()]
    });
    assert!(drawn(&mut sc, "file-search-root-Drive"));
    let deadline = Instant::now() + LIMIT;
    while !found(&launcher, "far plan")
        .iter()
        .any(|row| row == "far plan.txt")
    {
        assert!(
            Instant::now() < deadline,
            "the added folder was never indexed"
        );
        launcher.wait_for_file_index(Duration::from_millis(200));
    }
    click(&mut sc, "file-search-remove-root-Drive");
    until(&mut sc, &launcher, |launcher| {
        launcher
            .file_search_rules()
            .unwrap()
            .1
            .added_roots
            .is_empty()
    });

    // A folder excluded; cancelling the picker changes nothing.
    click(&mut sc, "file-search-exclude-folder");
    sc.simulate_path_prompt_response(|_| None);
    sc.run_until_parked();
    assert!(
        launcher
            .file_search_rules()
            .unwrap()
            .1
            .excluded_folders
            .is_empty()
    );
    let music = world.home.join("Music");
    click(&mut sc, "file-search-exclude-folder");
    let chosen = music.clone();
    sc.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    until(&mut sc, &launcher, |launcher| {
        launcher.file_search_rules().unwrap().1.excluded_folders == [music.clone()]
    });
    assert!(drawn(&mut sc, "file-search-excluded-Music"));
    assert!(!found(&launcher, "song").iter().any(|row| row == "song.mp3"));
    click(&mut sc, "file-search-remove-excluded-Music");
    until(&mut sc, &launcher, |launcher| {
        launcher
            .file_search_rules()
            .unwrap()
            .1
            .excluded_folders
            .is_empty()
    });
}

#[gpui::test]
fn while_files_is_off_the_page_says_file_search_is_off_and_why(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let files = launcher
        .packages()
        .into_iter()
        .find(|package| package.title() == "Rust files sample")
        .unwrap()
        .identity;
    block_on(launcher.set_enabled(&files, false));
    let (_settings, mut sc) = open_page(cx, launcher.clone());
    assert!(drawn(&mut sc, "file-search-off"));
    let tree = a11y(&mut sc);
    assert!(
        tree.contains("File search is off: no enabled extension uses file search"),
        "{tree}"
    );
    assert!(
        tree.contains(
            "Rust files sample uses file search, but it is turned off: turn it on under Extensions"
        ),
        "{tree}"
    );
    // The rules can still be changed; they apply when it runs again.
    assert!(drawn(&mut sc, "file-search-hidden"));
}

#[gpui::test]
fn without_an_extension_that_uses_it_the_page_says_to_install_one(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(false, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher);
    assert!(drawn(&mut sc, "file-search-off"));
    assert!(a11y(&mut sc).contains("Off"));
}

#[gpui::test]
fn a_folder_taken_out_for_churn_is_listed_and_included_again(cx: &mut TestAppContext) {
    let world = World::new();
    let documents = world.home.join("Documents");
    fs::create_dir_all(world.records()).unwrap();
    UserRules {
        quarantined: vec![documents.clone()],
        ..UserRules::default()
    }
    .write(&world.records())
    .unwrap();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher.clone());
    assert!(drawn(&mut sc, "file-search-section-Problems"));
    assert!(drawn(&mut sc, "file-search-problem-Churned-Documents"));
    assert!(
        a11y(&mut sc).contains("so Pane took it out of the index"),
        "the reason is given"
    );
    click(&mut sc, "file-search-include-again-Documents");
    until(&mut sc, &launcher, |launcher| {
        launcher.file_search_problems().is_empty()
    });
    assert!(!drawn(&mut sc, "file-search-problem-Churned-Documents"));
    assert!(found(&launcher, "plan").iter().any(|row| row == "plan.txt"));
}

#[gpui::test]
fn a_walk_stopped_at_the_ceiling_is_listed(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |config| config.walk.max_entries = 2);
    assert!(
        launcher
            .file_search_problems()
            .iter()
            .any(|problem| problem.kind == ProblemKind::Ceiling)
    );
    let (_settings, mut sc) = open_page(cx, launcher);
    assert!(drawn(&mut sc, "file-search-problem-Ceiling"));
    assert!(a11y(&mut sc).contains("Indexing stopped at 2 entries"));
}

#[gpui::test]
fn a_stop_for_free_space_is_listed(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |config| {
        config.valves.free_space = Arc::new(|_: &Path| Some(0));
    });
    assert!(launcher.file_index_status().low_space);
    let (_settings, mut sc) = open_page(cx, launcher);
    assert!(drawn(&mut sc, "file-search-problem-LowSpace"));
    let tree = a11y(&mut sc);
    assert!(
        tree.contains("Less than 1 GB is free on the disk that holds Pane's cache"),
        "{tree}"
    );
    assert!(tree.contains("Stopped: Pane stopped indexing"), "{tree}");
}

#[gpui::test]
fn a_folder_that_did_not_answer_is_listed(cx: &mut TestAppContext) {
    let world = World::new();
    // Every folder is given no time at all to answer.
    let launcher = world.launcher(true, |config| {
        config.walk.hung_after = Some(Duration::from_nanos(1));
    });
    assert!(launcher.file_index_status().hung > 0);
    let (_settings, mut sc) = open_page(cx, launcher);
    assert!(drawn(&mut sc, "file-search-problem-Hung-Home"));
    assert!(a11y(&mut sc).contains("so Pane skipped it for this walk"));
}

#[gpui::test]
fn the_sidebar_search_finds_the_page_and_its_controls(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(true, |_| {});
    let (_settings, mut sc) = open_page(cx, launcher);
    // Leave the page, then find it again by its controls.
    click(&mut sc, "section-General");
    sc.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    });
    sc.simulate_input("rebuild");
    sc.run_until_parked();
    assert!(drawn(&mut sc, "settings-search-result-Rebuild Index"));
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(drawn(&mut sc, "file-search"), "the jump opened the page");

    click(&mut sc, "section-General");
    sc.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    });
    sc.simulate_input("file search");
    sc.run_until_parked();
    assert!(drawn(&mut sc, "settings-search-result-File Search"));
}
