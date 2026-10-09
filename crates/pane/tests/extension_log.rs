//! A developed package's Logs screen in the native window (#213), on
//! GPUI's test platform, with real key events: it opens from the package's
//! development row and from its failed build's details, shows what the
//! settings sample writes and the lines that come while it shows, follows
//! them and stops following, copies a line and every line, clears the
//! lines (its log file keeps them) and opens the log file. The build is a
//! stand-in that copies the sample's guest, or fails, as `develop.rs`'s
//! does; `pane-core`'s `extension_log.rs` covers the log itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::extension_log::{LogLine, LogSource};
use pane_core::{Launcher, LinkOpener, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, until};

/// The title of the settings sample's package.
const TITLE: &str = "Settings sample";

fn guest(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// Builds the package by staging the guest its folder's `source.txt`
/// names, or fails when that starts with "error".
struct CopyBuilder;

struct CopyBuild(PathBuf);

impl Builder for CopyBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(CopyBuild(folder.to_path_buf())))
    }
}

impl Build for CopyBuild {
    fn command(&self) -> String {
        "copy build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("sample_settings.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        let source = source.trim();
        if source.starts_with("error") {
            job.line(source);
            return BuildOutcome::Failed("copy build failed".into());
        }
        fs::copy(guest(source), job.staging().join("sample_settings.wasm")).unwrap();
        BuildOutcome::Built
    }
}

/// Records the files the launcher is asked to open.
#[derive(Default)]
struct Opened(Mutex<Vec<PathBuf>>);

impl LinkOpener for Opened {
    fn open(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        self.0.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

/// The assembled settings sample, copied into `folder` as a package of
/// its own, built from the guest `source.txt` names.
fn package(folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for file in ["pane.json", "sample_settings.wasm"] {
        fs::copy(assembled.join(file), folder.join(file)).unwrap();
    }
    fs::write(folder.join("source.txt"), "sample_settings").unwrap();
    folder.to_path_buf()
}

/// Opens the launcher window over a launcher that installs packages in
/// `data`, builds them with [`CopyBuilder`] and opens files through
/// `opened`, with `folder`'s package installed, following the changes the
/// launcher reports, as the binary does.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    folder: &Path,
    opened: Arc<Opened>,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"))
        .with_link_opener(opened)
        .with_development(Arc::new(CopyBuilder), sender);
    futures::executor::block_on(launcher.install_package(folder));
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Installed {TITLE}"))
    );
    cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    })
}

/// Selects the row titled `title`.
fn select(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, title: &str) {
    cx.update_entity(window, |window, _| {
        let launcher = window.launcher();
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == title)
            .unwrap_or_else(|| panic!("no row {title}"));
        launcher.select(index);
    });
}

/// Develops the package from its row in the extension list, as Settings
/// enters the list, with Enter.
fn develop(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    enter_flow(window, cx);
    select(window, cx, &format!("Develop {TITLE}"));
    cx.simulate_keystrokes("enter");
    until(
        window,
        cx,
        |view| matches!(&view.status, Status::Result(text) if text.starts_with("Developing")),
    );
}

/// Runs the item titled `item` of the sample's command through the
/// launcher, as the command's list does.
fn run_item(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, item: &str) {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    launcher.show_root_search();
    for title in ["Greeting", item] {
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == title)
            .unwrap_or_else(|| panic!("no row {title}"));
        launcher.select(index);
        futures::executor::block_on(launcher.activate_selected());
    }
}

/// Opens the package's Logs screen from its row beside Stop developing,
/// with Enter.
fn open_logs(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    enter_flow(window, cx);
    select(window, cx, &format!("Logs for {TITLE}"));
    cx.simulate_keystrokes("enter");
    let view = until(window, cx, |view| {
        matches!(view.screen, Screen::ExtensionLog { .. })
    });
    assert_eq!(view.title, format!("Logs for {TITLE}"));
}

/// The Logs screen's lines shown, its selected line and whether it
/// follows new lines, once the window is idle.
fn log_state(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> (usize, Option<usize>, bool) {
    cx.run_until_parked();
    cx.read_entity(window, |window, _| window.extension_log_shown())
        .expect("the Logs screen shows")
}

/// Runs the window until its Logs screen shows every line the launcher
/// keeps of the log, having read them by itself, and `done` holds for
/// them; answers the lines and the screen's state.
fn until_log(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&[LogLine]) -> bool,
) -> (Vec<LogLine>, (usize, Option<usize>, bool)) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (lines, shown) = cx.read_entity(window, |window, _| {
            let launcher = window.launcher();
            let Screen::ExtensionLog { identity } = launcher.screen() else {
                panic!("not on the Logs screen: {:?}", launcher.screen());
            };
            (
                launcher.extension_log(&identity),
                window.extension_log_shown(),
            )
        });
        if let Some(shown) = shown
            && shown.0 == lines.len()
            && done(&lines)
        {
            return (lines, shown);
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the screen shows {shown:?} of {} lines",
            lines.len()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether the last frame drew the element with debug selector `selector`.
fn drawn(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .is_some()
}

/// The keys of `key` with Pane's own modifier on this system.
fn chord(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// The text on the clipboard.
fn clipboard(cx: &mut VisualTestContext) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .expect("text on the clipboard")
}

#[gpui::test]
fn the_logs_screen_shows_follows_copies_and_clears_a_developed_package_s_log(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("settings"));
    let opened = Arc::new(Opened::default());
    let (window, cx) = open(cx, data.path(), &folder, opened.clone());
    let identity = PackageIdentity::local(&folder).unwrap();
    develop(&window, cx);
    run_item(&window, cx, "Write to the log");

    // Its log, from its development row: what the sample wrote after
    // Pane's line about developing it, the last selected and followed.
    open_logs(&window, cx);
    let printed = |lines: &[LogLine]| lines.iter().any(|line| line.text == "a printed error");
    let (lines, shown) = until_log(&window, cx, printed);
    assert_eq!(shown, (lines.len(), Some(lines.len() - 1), true));
    let info = lines
        .iter()
        .position(|line| line.text == "an info line")
        .unwrap();
    let developing = lines
        .iter()
        .position(|line| line.text.starts_with("Developing: "))
        .unwrap();
    assert_eq!(lines[developing].source, LogSource::Pane);
    // Pane's own lines are told apart from the extension's.
    assert!(drawn(cx, format!("log-pane-{developing}")));
    assert!(!drawn(cx, format!("log-extension-{developing}")));
    assert!(drawn(cx, format!("log-extension-{info}")));
    assert!(!drawn(cx, format!("log-pane-{info}")));
    // Each line shows its time and its level.
    let warning = lines
        .iter()
        .position(|line| line.text == "a warning line")
        .unwrap();
    assert!(drawn(cx, format!("log-time-{info}")));
    assert!(drawn(cx, format!("log-Info-{info}")));
    assert!(drawn(cx, format!("log-Warn-{warning}")));

    // Moving up stops following; Enter copies the selected line, as the
    // log file writes it.
    cx.simulate_keystrokes("home");
    for _ in 0..info {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(log_state(&window, cx), (lines.len(), Some(info), false));
    cx.simulate_keystrokes("enter");
    assert_eq!(clipboard(cx), lines[info].file_line());
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().status()),
        Status::Result("Copied the line".into())
    );

    // A save builds and reloads the package while the screen shows: Pane's
    // lines about it arrive by themselves, and the selection stays put.
    fs::write(folder.join("source.txt"), "sample_settings\n").unwrap();
    let reloaded = format!("Reloaded {TITLE}");
    let once = |lines: &[LogLine]| lines.iter().any(|line| line.text == reloaded);
    let (grown, shown) = until_log(&window, cx, once);
    assert!(grown.len() > lines.len());
    assert_eq!(shown, (grown.len(), Some(info), false));

    // End follows again: the newest line is selected, and stays the
    // selection as more lines come.
    cx.simulate_keystrokes("end");
    assert_eq!(
        log_state(&window, cx),
        (grown.len(), Some(grown.len() - 1), true)
    );
    fs::write(folder.join("source.txt"), "sample_settings").unwrap();
    let twice = |lines: &[LogLine]| lines.iter().filter(|line| line.text == reloaded).count() == 2;
    let (more, shown) = until_log(&window, cx, twice);
    assert_eq!(shown, (more.len(), Some(more.len() - 1), true));

    // Copying every line, as the log file writes them.
    cx.simulate_keystrokes(&chord("shift-c"));
    let all: Vec<String> = more.iter().map(LogLine::file_line).collect();
    assert_eq!(clipboard(cx), all.join("\n"));

    // Clearing forgets the lines Pane keeps; the log file keeps them.
    cx.simulate_keystrokes(&chord("l"));
    assert_eq!(log_state(&window, cx), (0, None, true));
    assert!(drawn(cx, "log-empty".into()));
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert!(launcher.extension_log(&identity).is_empty());
    let file = launcher.extension_log_file(&identity).unwrap();
    let kept = fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains(" info  stdout [greeting] an info line\n"),
        "{kept}"
    );

    // Its log file opens with the system's handler.
    cx.simulate_keystrokes(&chord("o"));
    let deadline = Instant::now() + Duration::from_secs(60);
    while opened.0.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the log file never opened");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*opened.0.lock().unwrap(), [file]);
}

#[gpui::test]
fn the_logs_screen_opens_from_a_failed_build_s_details(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("settings"));
    let (window, cx) = open(cx, data.path(), &folder, Arc::default());
    develop(&window, cx);

    // A save that does not build: its details offer its log.
    fs::write(folder.join("source.txt"), "error: expected `;`").unwrap();
    until(&window, cx, |view| matches!(view.status, Status::Error(_)));
    select(&window, cx, &format!("Why {TITLE} did not build"));
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::BuildDetails { .. })
    });
    select(&window, cx, &format!("Logs for {TITLE}"));
    cx.simulate_keystrokes("enter");
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::ExtensionLog { .. })
    });
    assert_eq!(view.title, format!("Logs for {TITLE}"));

    // The failure is there, Pane's own line, in view and followed.
    let failure = format!("{TITLE} did not build");
    let has_failure = |lines: &[LogLine]| lines.iter().any(|line| line.text.starts_with(&failure));
    let (lines, shown) = until_log(&window, cx, has_failure);
    assert_eq!(shown, (lines.len(), Some(lines.len() - 1), true));
    let failed = lines
        .iter()
        .position(|line| line.text.starts_with(&failure))
        .unwrap();
    assert_eq!(lines[failed].source, LogSource::Pane);
    assert!(drawn(cx, format!("log-pane-{failed}")));
}

#[gpui::test]
fn scrolling_up_stops_following_and_scrolling_back_down_follows_again(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("settings"));
    let (window, cx) = open(cx, data.path(), &folder, Arc::default());
    develop(&window, cx);
    // More lines than the screen has room for.
    run_item(&window, cx, "Flood the log");
    // Pane notes the lines it dropped once their second is over.
    std::thread::sleep(Duration::from_millis(1_100));
    open_logs(&window, cx);
    let (lines, shown) = until_log(&window, cx, |lines| lines.len() > 100);
    assert_eq!(shown, (lines.len(), Some(lines.len() - 1), true));

    // The wheel up: the list stays where it was scrolled to, the
    // selection too, and new lines are not followed.
    let list = cx.debug_bounds("log-lines").expect("the lines are drawn");
    let wheel = |cx: &mut VisualTestContext, y: f32| {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(y))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
    };
    wheel(cx, 400.);
    assert_eq!(
        log_state(&window, cx),
        (lines.len(), Some(lines.len() - 1), false)
    );
    // A line in view, a little above the end it was scrolled up from.
    let watched: &'static str = Box::leak(format!("log-line-{}", lines.len() - 24).into());
    let scrolled = cx.debug_bounds(watched).expect("in view").top();
    fs::write(folder.join("source.txt"), "sample_settings\n").unwrap();
    let reloaded = format!("Reloaded {TITLE}");
    let once = |lines: &[LogLine]| lines.iter().any(|line| line.text == reloaded);
    let (grown, shown) = until_log(&window, cx, once);
    assert_eq!(shown, (grown.len(), Some(lines.len() - 1), false));
    let still = cx.debug_bounds(watched).expect("still in view").top();
    assert_eq!(still, scrolled, "the list stays where it was scrolled to");

    // Back down to the end: followed again, the newest line selected.
    // Lines not yet laid out when the wheel turns can leave it short of
    // the end, so it turns again, as a hand would.
    for _ in 0..5 {
        wheel(cx, -1_000_000.);
        if log_state(&window, cx).2 {
            break;
        }
    }
    assert_eq!(
        log_state(&window, cx),
        (grown.len(), Some(grown.len() - 1), true)
    );
}
