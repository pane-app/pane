//! The error overlay of a developed package's failing command (#214),
//! through the launcher's public interface: a crash (a trap) and a
//! JavaScript or TypeScript command's thrown error show the message and
//! the stack trace over the command's view — the thrown error's stack
//! mapped to the sources the development build's map names — with rows
//! that open the package's Logs screen, copy the message and trace, and
//! run the command again. A package not being developed keeps today's
//! status line and toast, and a no-view command's failure covers whatever
//! the launcher was showing, since it opens no view of its own. The build
//! is a stand-in that copies the guest named in the folder's `source.txt`,
//! as in `develop.rs`; `develop.rs` covers the overlay of a reload that
//! fails to start, and `source_map.rs` the mapping itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::extension_log::{LogLine, LogSource, LogStream};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::{guest, guests};
use rows::{manage, select_title, titles};

/// Builds a folder by staging the guest its `source.txt` names, with the
/// source map the guest's development build keeps beside it, or fails when
/// that starts with "error".
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
        path == Path::new("command.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        let source = source.trim();
        if source.starts_with("error") {
            job.line(source);
            return BuildOutcome::Failed("copy build failed".into());
        }
        fs::copy(guest(source), job.staging().join("command.wasm")).unwrap();
        // The map beside the component, as the JavaScript and TypeScript
        // builds keep one (#214): a reload from this build keeps it too.
        if let Some(map) = map_of(source) {
            fs::copy(map, job.staging().join("command.wasm.map")).unwrap();
        }
        BuildOutcome::Built
    }
}

/// The source map kept beside the guest `source`'s component, when its
/// build made one: the JavaScript and TypeScript samples'.
fn map_of(source: &str) -> Option<PathBuf> {
    let map = guests().join(format!("{source}.wasm.map"));
    map.is_file().then_some(map)
}

/// The manifest of a package whose one command opens a view.
const VIEW: &str = r#"{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{ "id": "open", "title": "Open Dev", "component": "command.wasm" }]
}"#;

/// The manifest of a package whose one command runs without a view, taking
/// the text typed in root search (see `no_view.rs`).
const NO_VIEW: &str = r#"{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{
    "id": "report",
    "title": "Report launch",
    "component": "command.wasm",
    "mode": "no-view",
    "takesQuery": true
  }]
}"#;

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

struct Pane {
    _sources: TempDir,
    sources: PathBuf,
    _data: TempDir,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let (changes, _) = pane_core::changes::channel();
        let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
            .with_link_opener(Arc::new(Opened::default()))
            .with_development(Arc::new(CopyBuilder), changes);
        Pane {
            sources: sources.path().to_path_buf(),
            _sources: sources,
            _data: data,
            launcher,
        }
    }

    /// Installs the package "Dev" with `manifest`, built from the guest
    /// `source`, with the map its build keeps beside its component.
    fn install(&self, manifest: &str, source: &str) -> (PathBuf, PackageIdentity) {
        let folder = self.sources.join("Dev");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::write(folder.join("source.txt"), source).unwrap();
        fs::copy(guest(source), folder.join("command.wasm")).unwrap();
        if let Some(map) = map_of(source) {
            fs::copy(map, folder.join("command.wasm.map")).unwrap();
        }
        block_on(self.launcher.install_package(&folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        (folder, PackageIdentity::local(&folder).unwrap())
    }

    /// Installs the package "Dev" and develops it.
    fn developing(&self, manifest: &str, source: &str) -> (PathBuf, PackageIdentity) {
        let (folder, identity) = self.install(manifest, source);
        block_on(self.launcher.start_developing(&identity));
        assert!(
            self.launcher.development(&identity).is_some(),
            "{:?}",
            self.launcher.view().status
        );
        (folder, identity)
    }

    /// From root search, opens "Dev" and runs its item titled `item`.
    fn run(&self, item: &str) -> Status {
        for _ in 0..3 {
            self.launcher.back();
        }
        select_title(&self.launcher, "Open Dev");
        block_on(self.launcher.activate_selected());
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    fn log(&self, identity: &PackageIdentity) -> Vec<LogLine> {
        self.launcher.extension_log(identity)
    }
}

/// The lines the extension wrote, as (stream, text).
fn written(lines: &[LogLine]) -> Vec<(LogStream, String)> {
    lines
        .iter()
        .filter_map(|line| match line.source {
            LogSource::Extension(stream) => Some((stream, line.text.clone())),
            LogSource::Pane => None,
        })
        .collect()
}

/// Runs the developed sample built from `source` and its `item`, and
/// answers the overlay it shows.
fn overlay(pane: &Pane, source: &str, item: &str) -> pane_core::LauncherView {
    let _ = pane.run(item);
    let view = pane.launcher.view();
    assert!(
        matches!(view.screen, Screen::Crash { .. }),
        "{source}: {:?}",
        view.title
    );
    view
}

#[test]
fn a_crash_of_a_developed_package_shows_the_overlay_over_its_command() {
    let pane = Pane::new();
    let (_, identity) = pane.developing(VIEW, "sample_settings");

    let view = overlay(&pane, "sample_settings", "Crash");
    assert_eq!(view.title, "Dev crashed");
    let details = view.details().to_vec();
    assert!(details[0].starts_with("The extension crashed:"));
    // The trap's text carries the wasm backtrace after its message.
    assert!(details.len() > 1, "the backtrace: {details:?}");
    assert_eq!(
        titles(&pane.launcher),
        ["Logs for Dev", "Copy the message and trace", "Run Open Dev again"]
    );

    // Back puts the command's view back as it was, and the package is not
    // paused: one crash does not pause it, and it still answers.
    pane.launcher.back();
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Command));
    assert!(titles(&pane.launcher).contains(&"Crash".to_string()));
    assert_eq!(
        pane.run("Write to the log"),
        Status::Result("Wrote to the log".into())
    );
    // The crash is in its log as Pane's own line.
    let pane_line = |line: &LogLine| {
        line.source == LogSource::Pane && line.text.starts_with("The extension crashed:")
    };
    assert!(pane.log(&identity).iter().any(pane_line));
}

#[test]
fn the_overlay_s_row_opens_the_package_s_logs() {
    let pane = Pane::new();
    let (_, identity) = pane.developing(VIEW, "sample_settings");
    overlay(&pane, "sample_settings", "Crash");

    select_title(&pane.launcher, "Logs for Dev");
    block_on(pane.launcher.activate_selected());
    let view = pane.launcher.view();
    assert!(matches!(&view.screen, Screen::ExtensionLog { shown } if *shown == identity));
    assert_eq!(view.title, "Logs for Dev");
}

#[test]
fn the_overlay_s_row_copies_the_message_and_trace() {
    let pane = Pane::new();
    pane.developing(VIEW, "sample_settings");
    let view = overlay(&pane, "sample_settings", "Crash");
    let expected: Vec<String> = view.details().to_vec();

    select_title(&pane.launcher, "Copy the message and trace");
    assert_eq!(pane.launcher.selected_copy(), Some(expected.join("\n")));
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Copied the message and trace".into())
    );
}

#[test]
fn the_overlay_s_row_runs_the_command_again() {
    let pane = Pane::new();
    pane.developing(VIEW, "sample_settings");
    overlay(&pane, "sample_settings", "Crash");

    select_title(&pane.launcher, "Run Open Dev again");
    block_on(pane.launcher.activate_selected());
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Command));
    assert_eq!(view.status, Status::Idle);
    // It ran again: a second crash of the same kind shows the overlay
    // again, over the command's view.
    select_title(&pane.launcher, "Crash");
    block_on(pane.launcher.activate_selected());
    assert!(matches!(pane.launcher.view().screen, Screen::Crash { .. }));
    assert_eq!(pane.launcher.view().title, "Dev crashed");
}

#[test]
fn a_thrown_javascript_or_typescript_error_shows_the_overlay_with_its_source_mapped_stack() {
    for source in ["sample_settings_js", "sample_settings_ts"] {
        let pane = Pane::new();
        let (_, identity) = pane.developing(VIEW, source);

        let view = overlay(&pane, source, "Fail");
        assert_eq!(view.title, "Dev failed", "{source}");
        let details = view.details().to_vec();
        assert!(
            details[0].contains("The extension reported an error: failed on purpose"),
            "{source}: {details:?}"
        );
        // The stack the command threw, mapped back to its sources: the
        // frames name the author's files, not the bundle.
        let frames: Vec<&String> = details[1..]
            .iter()
            .filter(|line| line.trim_start().starts_with("at "))
            .collect();
        assert!(!frames.is_empty(), "{source}: {details:?}");
        assert!(
            frames.iter().any(|frame| frame.contains("src/index.")),
            "{source}: {details:?}"
        );
        assert!(
            !frames.iter().any(|frame| frame.contains("bundle.mjs")),
            "{source}: {details:?}"
        );

        // The log's lines are mapped as they were captured: the overlay's
        // trace is the log's.
        let mapped = |(_, text): &(LogStream, String)| {
            text.trim_start().starts_with("at ") && text.contains("src/index.")
        };
        let log = written(&pane.log(&identity));
        assert!(log.iter().any(mapped), "{source}: the frames are not mapped");
    }
}

#[test]
fn a_package_not_being_developed_keeps_the_status_line_and_no_overlay() {
    let pane = Pane::new();
    pane.install(VIEW, "sample_settings");

    // The crash answers as it always did: the status line over the
    // command's view, and no overlay.
    assert!(matches!(pane.run("Crash"), Status::Error(_)));
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Command));
    assert!(matches!(view.status, Status::Error(_)));
}

#[test]
fn a_no_view_command_s_failure_shows_the_overlay_and_its_retry_runs_it_again() {
    let pane = Pane::new();
    let (_, identity) = pane.developing(NO_VIEW, "sample_no_view_ts");

    // "Report launch" as the fallback the user chose, sent "fail", which
    // it answers by throwing (see `no_view.rs`).
    manage(&pane.launcher);
    select_title(&pane.launcher, "Fallback: Report launch");
    block_on(pane.launcher.activate_selected());
    for _ in 0..3 {
        pane.launcher.back();
    }
    block_on(pane.launcher.set_query("fail"));
    pane.launcher.move_selection(1);
    block_on(pane.launcher.activate_selected());

    // A no-view command opens no view: the overlay covers root search, and
    // the stack it threw is its trace.
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Crash { .. }));
    assert_eq!(view.title, "Dev failed");
    let details = view.details().to_vec();
    assert!(
        details[0].contains("The extension reported an error: Report launch fails on request"),
        "{details:?}"
    );
    let has_frame = details[1..].iter().any(|line| line.contains("at "));
    assert!(has_frame, "the thrown stack: {details:?}");

    // The row that runs it again does: the same failure shows again, and
    // the log has the throw twice.
    select_title(&pane.launcher, "Run Report launch again");
    block_on(pane.launcher.activate_selected());
    assert!(matches!(pane.launcher.view().screen, Screen::Crash { .. }));
    let thrown = |(_, text): &(LogStream, String)| {
        text.starts_with("Error: Report launch fails on request")
    };
    let count = written(&pane.log(&identity)).iter().filter(|line| thrown(line)).count();
    assert_eq!(count, 2, "the command ran again");

    // Back returns to root search, which a no-view command leaves as it
    // found it.
    pane.launcher.back();
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
}
