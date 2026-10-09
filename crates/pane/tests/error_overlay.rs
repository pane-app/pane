//! The error overlay of a developed package's failing command in the
//! native window (#214), on GPUI's test platform, with real key events:
//! the settings sample's "Crash" (Rust) and the TypeScript sample's "Fail"
//! (which throws) show the overlay with the message and the stack trace
//! over the command's list; Escape returns to the command; Open Logs opens
//! the package's Logs screen; Copy puts the message and trace on the
//! clipboard; Retry runs the command again; and a package not being
//! developed keeps the status line, with no overlay. The build is a
//! stand-in that copies the guest named in the folder's `source.txt`;
//! `pane-core`'s `error_overlay.rs` covers the rest through the launcher.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, until};

/// One language's settings sample: its assembled package under
/// `target/guests/packages` and the guest its stand-in build copies.
struct Sample {
    package: &'static str,
    guest: &'static str,
    title: &'static str,
}

const RUST: Sample = Sample {
    package: "sample-settings",
    guest: "sample_settings",
    title: "Settings sample",
};
const TYPESCRIPT: Sample = Sample {
    package: "sample-settings-ts",
    guest: "sample_settings_ts",
    title: "TypeScript settings sample",
};

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

/// The source map the sample `name`'s development build keeps beside its
/// component, when it has one.
fn map(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm.map"));
    path.is_file().then_some(path)
}

/// Builds the package by staging the guest its folder's `source.txt`
/// names, with the map beside it, or fails when that starts with "error".
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
        path.file_name()
            .is_some_and(|name| name == "sample_settings.wasm" || name == "sample_settings_ts.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        let source = source.trim();
        if source.starts_with("error") {
            job.line(source);
            return BuildOutcome::Failed("copy build failed".into());
        }
        let component = job.staging().join(format!("{source}.wasm"));
        fs::copy(guest(source), &component).unwrap();
        // The map beside the component, as the JavaScript and TypeScript
        // builds keep one (#214).
        if let Some(map) = map(source) {
            fs::copy(map, job.staging().join(format!("{source}.wasm.map"))).unwrap();
        }
        BuildOutcome::Built
    }
}

/// Records the files the launcher is asked to open.
#[derive(Default)]
struct Opened(Mutex<Vec<PathBuf>>);

impl pane_core::LinkOpener for Opened {
    fn open(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        self.0.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

/// The sample's assembled package, copied into `folder` as a package of
/// its own, built from the guest its `source.txt` names.
fn package(sample: &Sample, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(sample.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::copy(assembled.join("pane.json"), folder.join("pane.json")).unwrap();
    let component = format!("{}.wasm", sample.guest);
    fs::copy(assembled.join(&component), folder.join(&component)).unwrap();
    if let Some(map) = map(sample.guest) {
        fs::copy(map, folder.join(format!("{}.map", component))).unwrap();
    }
    fs::write(folder.join("source.txt"), sample.guest).unwrap();
    folder.to_path_buf()
}

/// Opens the launcher window over a launcher that installs packages in
/// `data`, builds them with [`CopyBuilder`] and opens files through
/// `opened`, with the sample copied into `sources` installed, following
/// the changes the launcher reports, as the binary does.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    sources: &Path,
    sample: &Sample,
    opened: Arc<Opened>,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let folder = package(sample, &sources.join("settings"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"))
        .with_link_opener(opened)
        .with_development(Arc::new(CopyBuilder), sender);
    futures::executor::block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Installed {}", sample.title))
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
fn develop(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, sample: &Sample) {
    enter_flow(window, cx);
    select(window, cx, &format!("Develop {}", sample.title));
    cx.simulate_keystrokes("enter");
    until(
        window,
        cx,
        |view| matches!(&view.status, Status::Result(text) if text.starts_with("Developing")),
    );
}

/// Opens the sample's command from root search with Enter, and waits for
/// its list.
fn open_command(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    select(window, cx, "Greeting");
    cx.simulate_keystrokes("enter");
    until(window, cx, |view| matches!(view.screen, Screen::Command));
}

/// Runs the item titled `item` of the open command with Enter.
fn run_item(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, item: &str) {
    select(window, cx, item);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// The text on the clipboard.
fn clipboard(cx: &mut VisualTestContext) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .expect("text on the clipboard")
}

/// Runs the item that crashes and waits for the overlay it shows.
fn crash(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> pane_core::LauncherView {
    run_item(window, cx, "Crash");
    until(window, cx, |view| {
        matches!(view.screen, Screen::Crash { .. })
    })
}

#[gpui::test]
fn a_developed_package_s_crash_shows_the_overlay_with_its_rows(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let opened = Arc::new(Opened::default());
    let (window, cx) = open(cx, data.path(), sources.path(), &RUST, opened);
    develop(&window, cx, &RUST);
    open_command(&window, cx);

    // The item that crashes: the overlay shows over the command's list,
    // with the message, the backtrace and its rows.
    let view = crash(&window, cx);
    assert_eq!(view.title, format!("{} crashed", RUST.title));
    let details = view.details().to_vec();
    assert!(
        details[0].starts_with("The extension crashed:"),
        "{details:?}"
    );
    assert!(details.len() > 1, "the backtrace: {details:?}");
    assert!(cx.debug_bounds("row-Logs for Settings sample").is_some());
    assert!(cx.debug_bounds("row-Copy the message and trace").is_some());
    assert!(cx.debug_bounds("row-Run Greeting again").is_some());
    assert!(
        cx.debug_bounds(Box::leak(format!("detail-{}", details[0]).into_boxed_str()))
            .is_some()
    );

    // Escape returns to the command's list as it was.
    cx.simulate_keystrokes("escape");
    until(&window, cx, |view| matches!(view.screen, Screen::Command));
}

#[gpui::test]
fn the_overlay_s_rows_copy_the_message_and_open_the_logs(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let opened = Arc::new(Opened::default());
    let (window, cx) = open(cx, data.path(), sources.path(), &RUST, opened);
    let identity = PackageIdentity::local(&sources.path().join("settings")).unwrap();
    develop(&window, cx, &RUST);
    open_command(&window, cx);

    // Copy: the message and trace, on the clipboard.
    let view = crash(&window, cx);
    select(&window, cx, "Copy the message and trace");
    cx.simulate_keystrokes("enter");
    assert_eq!(clipboard(cx), view.details().join("\n"));

    // Open Logs: the package's Logs screen, from the overlay.
    select(&window, cx, "Logs for Settings sample");
    cx.simulate_keystrokes("enter");
    let view = until(
        &window,
        cx,
        |view| matches!(&view.screen, Screen::ExtensionLog { identity: shown } if *shown == identity),
    );
    assert_eq!(view.title, format!("Logs for {}", RUST.title));
}

#[gpui::test]
fn the_overlay_s_row_runs_the_command_again(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(
        cx,
        data.path(),
        sources.path(),
        &RUST,
        Arc::new(Opened::default()),
    );
    develop(&window, cx, &RUST);
    open_command(&window, cx);

    // Retry: the command runs again, and its list comes back.
    crash(&window, cx);
    select(&window, cx, "Run Greeting again");
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| matches!(view.screen, Screen::Command));
    assert!(cx.debug_bounds("row-Crash").is_some());
}

#[gpui::test]
fn a_typescript_command_s_thrown_error_shows_the_overlay_with_its_mapped_stack(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(cx, data.path(), sources.path(), &TYPESCRIPT, Arc::default());
    develop(&window, cx, &TYPESCRIPT);
    open_command(&window, cx);

    // The item that throws: the overlay shows the message and the stack
    // the command threw, whose frames name its sources, not the bundle
    // (#214's source maps).
    run_item(&window, cx, "Fail");
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::Crash { .. })
    });
    assert_eq!(view.title, format!("{} failed", TYPESCRIPT.title));
    let details = view.details().to_vec();
    assert!(
        details[0].contains("The extension reported an error: failed on purpose"),
        "{details:?}"
    );
    assert!(
        details.iter().any(|line| line.contains("src/index.")),
        "{details:?}"
    );
    let mapped = details.iter().find(|line| line.contains("src/index."));
    let mapped = mapped.expect("a frame mapped to the source");
    assert!(
        cx.debug_bounds(Box::leak(format!("detail-{mapped}").into_boxed_str()))
            .is_some()
    );
}

#[gpui::test]
fn a_package_not_being_developed_keeps_the_status_line(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(cx, data.path(), sources.path(), &RUST, Arc::default());
    open_command(&window, cx);

    // The crash answers as it always did: the status line over the
    // command's list, and no overlay.
    run_item(&window, cx, "Crash");
    let view = until(&window, cx, |view| matches!(view.status, Status::Error(_)));
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.title);
    assert!(cx.debug_bounds("status-error").is_some());
}
