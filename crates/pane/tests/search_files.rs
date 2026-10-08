//! Search Files' split view in the launcher's window (#177), with real key
//! events and clicks: Pane's registered Files default extension, acquired
//! from an artifact source on 127.0.0.1, over the real file index of a
//! fixture folder standing for the home folder, with a recording handler of
//! files and a recording system. It opens with no folder to choose on
//! "Recently Used", each row with the system's icon of its file; the type
//! dropdown at the search field's right filters by kind; the detail
//! previews an image over the Metadata (Name, Where, Type, Size, Created,
//! Modified); typing searches and Escape brings Recently Used back; Enter
//! on a program shows it and runs nothing; the Actions panel offers Copy
//! Name. The core's rules are `pane-core`'s `search_files.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use futures::executor::block_on;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::defaults::ArtifactSource;
use pane_core::file_index::{Category, IndexerConfig, WalkOptions};
use pane_core::search_files::FileType;
use pane_core::{DefaultExtension, Launcher, LinkOpener, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "../../pane-core/tests/support/artifacts.rs"]
mod artifacts;
#[path = "support/settle.rs"]
mod settle;
// The recording system pane-core's tests use.
#[path = "../../pane-core/tests/support/system.rs"]
mod recording;

use artifacts::Artifacts;
use recording::{Done, RecordingSystem};
use settle::{settle, until};

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The id of Files' Search Files row in root search.
const COMMAND: &str = "default:files#files";

/// A 1×1 transparent PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

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
    data: TempDir,
    _temp: TempDir,
    /// The folder standing for the home folder, named plainly inside
    /// `_temp` (whose own name starts with a dot, which the index leaves
    /// out as hidden).
    home: PathBuf,
    artifacts: Artifacts,
    opener: FakeOpener,
    system: Arc<RecordingSystem>,
}

impl World {
    /// A home folder holding, newest first, `Pictures/shot.png`,
    /// `Documents/plan.txt` and `Downloads/setup.exe`, with Files
    /// published by the artifact source.
    fn new() -> World {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("Home");
        for (days, (file, contents)) in [
            ("Pictures/shot.png", PNG.to_vec()),
            ("Documents/plan.txt", b"plan".to_vec()),
            ("Downloads/setup.exe", b"MZ".to_vec()),
        ]
        .into_iter()
        .enumerate()
        {
            let path = home.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(days as u64 * 86_400 + 60))
                .unwrap();
        }
        let folder =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/files");
        assert!(
            folder.is_dir(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );
        let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| {
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                (name, fs::read(&path).unwrap())
            })
            .collect();
        files.sort();
        let manifest: serde_json::Value = serde_json::from_slice(
            &files
                .iter()
                .find(|(path, _)| path == "pane.json")
                .expect("the package has a pane.json")
                .1,
        )
        .unwrap();
        let borrowed: Vec<(&str, Vec<u8>)> = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.clone()))
            .collect();
        let artifacts = Artifacts::start();
        artifacts.publish("files", manifest["version"].as_str().unwrap(), &borrowed);
        World {
            data: tempfile::tempdir().unwrap(),
            _temp: temp,
            home,
            artifacts,
            opener: FakeOpener::default(),
            system: Arc::new(RecordingSystem::default()),
        }
    }

    /// Pane with Files acquired as its default extension and its index
    /// settled.
    fn launcher(&self, cx: &mut TestAppContext) -> Launcher {
        cx.executor().allow_parking();
        let runtime = Runtime::start().unwrap();
        runtime.set_applications(self.system.clone());
        let index = IndexerConfig {
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
        let launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
                .with_defaults(
                    ArtifactSource::local(self.artifacts.url()).unwrap(),
                    vec![DefaultExtension {
                        id: "files".into(),
                        title: "Files".into(),
                    }],
                )
                .with_link_opener(Arc::new(self.opener.clone()))
                .with_system(self.system.clone())
                .with_file_index(index);
        block_on(launcher.acquire_defaults());
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        assert!(
            launcher.wait_for_file_index(Duration::from_secs(60)),
            "{:?}",
            launcher.file_index_status()
        );
        launcher.show_root_search();
        launcher
    }
}

/// Opens the window over `launcher`, then Search Files in it as a user
/// does: typing in root search, then Enter.
fn open_files(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("search files");
    until(&window, cx, |view| {
        view.rows.first().is_some_and(|row| row.id == COMMAND)
    });
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::CommandSearch { .. })
            && view.rows.iter().any(|row| row.title == "shot.png")
    });
    assert!(
        cx.read_entity(&window, |window, _| window.search_files_shown()),
        "the split view shows"
    );
    (window, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Runs the window until `selector` is drawn (or, with `drawn` false, is
/// not).
fn until_drawn(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    selector: &'static str,
    drawn: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while cx.debug_bounds(selector).is_some() != drawn {
        assert!(Instant::now() < deadline, "{selector} drawn: {}", !drawn);
        settle(window, cx);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn selected_title(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    view.rows[view.selected.expect("a row is selected")]
        .title
        .clone()
}

#[gpui::test]
fn search_files_opens_on_recently_used_with_system_icons(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(cx);
    let (window, cx) = open_files(cx, launcher);
    settle(&window, cx);
    assert!(cx.debug_bounds("section-Recently Used").is_some());
    for row in [
        "files-row-shot.png",
        "files-row-plan.txt",
        "files-row-setup.exe",
    ] {
        assert!(cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    // No folder to choose, nothing explained first.
    assert!(cx.debug_bounds("row-Choose folder…").is_none());
    assert!(cx.debug_bounds("files-row-What is searched").is_none());
    // Each row draws the system's icon of its file.
    assert!(cx.debug_bounds("icon-files-icon").is_some());
    // The type dropdown at the search field's right; the footer names the
    // command.
    let field = cx.debug_bounds("files-back").expect("the back button");
    let types = cx.debug_bounds("files-type").expect("the type dropdown");
    assert!(types.origin.x > field.origin.x, "at the field's right");
    assert!(cx.debug_bounds("footer-command").is_some());
    assert_eq!(
        cx.read_entity(&window, |window, _| window.search_files_type()),
        Some(FileType::All)
    );
}

#[gpui::test]
fn the_type_dropdown_filters_by_kind(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(cx);
    let (window, cx) = open_files(cx, launcher);

    click(cx, "files-type");
    settle(&window, cx);
    for choice in [
        "files-type-all",
        "files-type-folder",
        "files-type-document",
        "files-type-image",
        "files-type-video",
        "files-type-audio",
        "files-type-archive",
        "files-type-text",
        "files-type-application",
        "files-type-other",
    ] {
        assert!(cx.debug_bounds(choice).is_some(), "{choice} is offered");
    }
    click(cx, "files-type-image");
    settle(&window, cx);
    assert_eq!(
        cx.read_entity(&window, |window, _| window.search_files_type()),
        Some(FileType::Of(Category::Images))
    );
    until_drawn(&window, cx, "files-row-plan.txt", false);
    assert!(cx.debug_bounds("files-row-shot.png").is_some());

    click(cx, "files-type");
    settle(&window, cx);
    click(cx, "files-type-folder");
    settle(&window, cx);
    until_drawn(&window, cx, "files-row-Pictures", true);
    assert!(cx.debug_bounds("files-row-shot.png").is_none());

    click(cx, "files-type");
    settle(&window, cx);
    click(cx, "files-type-all");
    settle(&window, cx);
    until_drawn(&window, cx, "files-row-plan.txt", true);
}

#[gpui::test]
fn the_detail_previews_an_image_and_shows_the_metadata(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(cx);
    let (window, cx) = open_files(cx, launcher);
    // An image previewed, over its Metadata.
    cx.read_entity(&window, |window, _| {
        let launcher = window.launcher();
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == "shot.png")
            .unwrap();
        launcher.select(index);
    });
    window.update(cx, |_, cx| cx.notify());
    settle(&window, cx);
    assert_eq!(selected_title(&window, cx), "shot.png");
    assert!(cx.debug_bounds("files-preview-image").is_some());
    assert!(cx.debug_bounds("files-information").is_some());
    for row in [
        "files-info-Name",
        "files-info-Where",
        "files-info-Type",
        "files-info-Size",
        "files-info-Modified",
    ] {
        assert!(cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    if cfg!(any(windows, target_os = "macos")) {
        assert!(cx.debug_bounds("files-info-Created").is_some());
    }

    // A text file shows its icon instead.
    click(cx, "files-row-plan.txt");
    settle(&window, cx);
    assert_eq!(selected_title(&window, cx), "plan.txt");
    assert!(cx.debug_bounds("files-preview-image").is_none());
    assert!(cx.debug_bounds("files-preview-icon").is_some());
}

#[gpui::test]
fn typing_searches_and_escape_brings_recently_used_back(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(cx);
    let (window, cx) = open_files(cx, launcher);
    cx.simulate_input("plan");
    until(&window, cx, |view| {
        view.screen
            == Screen::CommandSearch {
                query: "plan".into(),
            }
            && view.rows.first().is_some_and(|row| row.title == "plan.txt")
    });
    assert!(cx.debug_bounds("section-Recently Used").is_none());

    cx.simulate_keystrokes("escape");
    until(&window, cx, |view| {
        view.screen
            == Screen::CommandSearch {
                query: String::new(),
            }
            && view.rows.iter().any(|row| row.title == "shot.png")
    });
    until_drawn(&window, cx, "section-Recently Used", true);
    assert!(
        cx.read_entity(&window, |window, _| window.search_files_shown()),
        "still in Search Files"
    );
    cx.simulate_keystrokes("escape");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::Root { .. })
    });
    assert!(!cx.read_entity(&window, |window, _| window.search_files_shown()));
}

#[gpui::test]
fn enter_on_a_program_shows_it_and_the_panel_offers_copy_name(cx: &mut TestAppContext) {
    let world = World::new();
    let launcher = world.launcher(cx);
    let (window, cx) = open_files(cx, launcher);
    cx.simulate_input("setup");
    until(&window, cx, |view| {
        view.rows
            .first()
            .is_some_and(|row| row.title == "setup.exe")
    });
    assert_eq!(selected_title(&window, cx), "setup.exe");

    // The Actions panel lists Pane's file actions, Copy Name among them.
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    for entry in ["action-Run", "action-Copy Path", "action-Copy Name"] {
        assert!(cx.debug_bounds(entry).is_some(), "{entry} is listed");
    }
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));

    // Enter shows it in the file manager; nothing runs it.
    cx.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(60);
    let shown = loop {
        cx.run_until_parked();
        let done = world.system.take();
        if !done.is_empty() {
            break done;
        }
        assert!(Instant::now() < deadline, "nothing was shown");
        std::thread::sleep(Duration::from_millis(5));
    };
    match shown.as_slice() {
        [Done::Revealed(path)] => {
            assert!(same_file(path, &world.home.join("Downloads/setup.exe")))
        }
        other => panic!("{other:?}"),
    }
    assert!(world.opener.take().is_empty(), "nothing ran it");
}
