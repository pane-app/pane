//! Extension logs through the launcher's public interface: what a package's
//! code prints and logs, and Pane's own messages about the package, in one
//! log. A developed package's log is kept in a file of the development
//! session and sent to whoever follows it; a package not developed keeps
//! only its most recent lines, in memory.
//!
//! The settings sample writes a line at each level ("Write to the log"),
//! floods the log ("Flood the log"), crashes and stops responding. The
//! build is a stand-in that copies the guest named in the folder's
//! `source.txt`, as in `develop.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::extension_log::{
    LINE_LIMIT, LINES_PER_SECOND, LogLevel, LogLine, LogSource, LogStream, WINDOW_LINES,
};
use pane_core::{
    Launcher, Limits, LinkOpener, OperationKind, PackageIdentity, Runtime, Screen, Status,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::{manage, select_title, titles};

/// Builds a folder by staging the guest its `source.txt` names, or fails
/// when that starts with "error".
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

struct Pane {
    _sources: TempDir,
    data: TempDir,
    sources: PathBuf,
    runtime: Runtime,
    launcher: Launcher,
    opened: Arc<Opened>,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let (changes, _) = pane_core::changes::channel();
        let opened = Arc::new(Opened::default());
        let launcher =
            Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
                .with_link_opener(opened.clone())
                .with_development(Arc::new(CopyBuilder), changes);
        Pane {
            sources: sources.path().to_path_buf(),
            _sources: sources,
            data,
            runtime,
            launcher,
            opened,
        }
    }

    /// Installs the package "Dev" built from the guest `source`.
    fn install(&self, source: &str) -> (PathBuf, PackageIdentity) {
        let folder = self.sources.join("Dev");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("pane.json"),
            r#"{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{ "id": "open", "title": "Open Dev", "component": "command.wasm" }]
}"#,
        )
        .unwrap();
        fs::write(folder.join("source.txt"), source).unwrap();
        fs::copy(guest(source), folder.join("command.wasm")).unwrap();
        block_on(self.launcher.install_package(&folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        (folder.clone(), PackageIdentity::local(&folder).unwrap())
    }

    /// Installs "Dev" and develops it.
    fn developing(&self, source: &str) -> (PathBuf, PackageIdentity) {
        let (folder, identity) = self.install(source);
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

    /// Every file under Pane's data folder named like a log of
    /// development.
    fn log_files(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut folders = vec![self.data.path().to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("extension.log"))
                {
                    found.push(path);
                }
            }
        }
        found
    }
}

const DEADLINE: Duration = Duration::from_secs(120);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The lines the extension wrote, as (stream, level, text).
fn written(lines: &[LogLine]) -> Vec<(LogStream, LogLevel, String)> {
    lines
        .iter()
        .filter_map(|line| match line.source {
            LogSource::Extension(stream) => Some((stream, line.level, line.text.clone())),
            LogSource::Pane => None,
        })
        .collect()
}

/// Pane's own lines, as (level, text).
fn panes(lines: &[LogLine]) -> Vec<(LogLevel, String)> {
    lines
        .iter()
        .filter(|line| line.source == LogSource::Pane)
        .map(|line| (line.level, line.text.clone()))
        .collect()
}

#[test]
fn a_developed_package_logs_what_it_writes_in_order_with_levels() {
    let pane = Pane::new();
    let (_, identity) = pane.developing("sample_settings");
    let followed = pane.launcher.follow_extension_log(&identity);
    assert_eq!(
        pane.run("Write to the log"),
        Status::Result("Wrote to the log".into())
    );
    use LogLevel::*;
    use LogStream::*;
    let expected = vec![
        (Stdout, Debug, "a debug line".to_string()),
        (Stdout, Info, "an info line".into()),
        (Stderr, Warn, "a warning line".into()),
        (Stderr, Error, "an error line".into()),
        (Stdout, Info, "a printed line".into()),
        (Stderr, Error, "a printed error".into()),
    ];
    let lines = pane.log(&identity);
    assert_eq!(written(&lines), expected);
    let ours: Vec<&LogLine> = lines
        .iter()
        .filter(|line| line.source != LogSource::Pane)
        .collect();
    assert!(
        ours.iter()
            .all(|line| line.command.as_deref() == Some("open") && line.generation > 0),
        "{ours:?}"
    );
    // Its follower got the same lines, after Pane's that it started
    // following during.
    let sent: Vec<LogLine> = followed.try_iter().collect();
    assert_eq!(written(&sent), expected);

    // The session's log file has every line.
    let file = pane.launcher.extension_log_file(&identity).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains(" warn  stderr [open] a warning line\n"),
        "{text}"
    );
    assert!(text.contains(" pane Developing: each save in "), "{text}");
}

#[test]
fn a_crash_logs_the_panic_and_pane_s_own_lines_follow_it() {
    let pane = Pane::new();
    let (folder, identity) = pane.developing("sample_settings");
    // A crash of a developed package shows as the error overlay (#214),
    // over the command's view; the panic line and Pane's own are in its
    // log either way.
    pane.run("Crash");
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Crash { .. }));
    assert_eq!(view.title, "Dev crashed");
    assert!(view.details()[0].starts_with("The extension crashed:"));
    pane.launcher.back();
    assert!(matches!(pane.launcher.view().screen, Screen::Command));
    let lines = pane.log(&identity);
    let panicked = written(&lines)
        .into_iter()
        .find(|(_, _, text)| text.contains("crashed on purpose"))
        .unwrap();
    assert_eq!(panicked.0, LogStream::Stderr);
    assert_eq!(panicked.1, LogLevel::Error);
    assert!(
        panicked.2.starts_with("panicked at ") && panicked.2.contains("lib.rs:"),
        "{}",
        panicked.2
    );
    // Pane's line about the crash comes after the panic.
    let at = |find: &dyn Fn(&LogLine) -> bool| lines.iter().position(find).unwrap();
    let panic = at(&|line| line.text.contains("crashed on purpose"));
    let crash = at(&|line| {
        line.source == LogSource::Pane && line.text.starts_with("The extension crashed:")
    });
    assert!(panic < crash, "{lines:#?}");

    // A save builds and reloads it; a failed build is logged too.
    fs::write(folder.join("source.txt"), "error: it does not build").unwrap();
    wait_until("the failed build", || {
        panes(&pane.log(&identity))
            .iter()
            .any(|(level, text)| *level == LogLevel::Error && text.starts_with("Dev did not build"))
    });
    fs::write(folder.join("source.txt"), "sample_settings").unwrap();
    wait_until("the reload", || {
        panes(&pane.log(&identity))
            .iter()
            .any(|(_, text)| text == "Reloaded Dev")
    });
    let building = panes(&pane.log(&identity))
        .iter()
        .filter(|(_, text)| text.starts_with("Building Dev"))
        .count();
    assert_eq!(building, 2);

    // Stopping development is logged, and the file is kept.
    let file = pane.launcher.extension_log_file(&identity).unwrap();
    pane.launcher.stop_developing(&identity);
    assert!(pane.launcher.extension_log_file(&identity).is_none());
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.ends_with(" pane Development stopped\n"), "{text}");
}

#[test]
fn a_flood_is_cut_and_limited_and_pane_says_how_much_it_dropped() {
    for source in [
        "sample_settings",
        "sample_settings_js",
        "sample_settings_ts",
    ] {
        flood(source);
    }
}

fn flood(source: &str) {
    let pane = Pane::new();
    let (_, identity) = pane.developing(source);
    assert_eq!(
        pane.run("Flood the log"),
        Status::Result("Flooded the log".into())
    );
    // The note of dropped lines comes once their second is over.
    std::thread::sleep(Duration::from_millis(1_100));
    let lines = pane.log(&identity);
    let written = written(&lines);
    let long = &written[0].2;
    assert!(long.starts_with(&"x".repeat(LINE_LIMIT)), "{}", &long[..40]);
    assert!(
        long.ends_with(&format!(
            "(Pane cut {} more bytes of this line)",
            5_000 - LINE_LIMIT
        )),
        "{}",
        &long[LINE_LIMIT..]
    );
    let kept = written.len() as u64;
    let dropped: u64 = panes(&lines)
        .iter()
        .filter_map(|(_, text)| {
            text.strip_prefix("Pane dropped ")?
                .split(' ')
                .next()?
                .parse::<u64>()
                .ok()
        })
        .sum();
    assert_eq!(kept + dropped, 1_501, "{:?}", panes(&lines));
    // However slow the machine, no more than the limit in one second.
    if dropped > 0 {
        assert!(kept >= u64::from(LINES_PER_SECOND));
    }
    // Pane is still responsive.
    assert_eq!(
        pane.run("Write to the log"),
        Status::Result("Wrote to the log".into())
    );
}

#[test]
fn javascript_and_typescript_log_through_console_and_log_what_they_throw() {
    for source in ["sample_settings_js", "sample_settings_ts"] {
        let pane = Pane::new();
        let (_, identity) = pane.developing(source);
        assert_eq!(
            pane.run("Write to the log"),
            Status::Result("Wrote to the log".into()),
            "{source}"
        );
        use LogLevel::*;
        use LogStream::*;
        assert_eq!(
            written(&pane.log(&identity)),
            vec![
                (Stdout, Debug, "a debug line".to_string()),
                (Stdout, Info, "an info line".into()),
                (Stderr, Warn, "a warning line".into()),
                (Stderr, Error, "an error line".into()),
                (Stdout, Info, "a printed line".into()),
                (Stdout, Info, "a formatted line with 2 substitutions".into()),
            ],
            "{source}"
        );

        // What a handler throws is the error it answers with, and is
        // logged with its stack: a developed package shows it as the error
        // overlay (#214), whose trace is that stack.
        pane.run("Fail");
        let view = pane.launcher.view();
        assert!(matches!(view.screen, Screen::Crash { .. }), "{source}");
        assert_eq!(view.title, "Dev failed", "{source}");
        let details = view.details().to_vec();
        assert!(
            details[0].contains("The extension reported an error: failed on purpose"),
            "{source}: {details:?}"
        );
        assert!(
            details[1..].iter().any(|line| line.contains("at ")),
            "{source}: {details:?}"
        );
        let lines = written(&pane.log(&identity));
        let thrown = lines
            .iter()
            .position(|(_, _, text)| text == "Error: failed on purpose")
            .unwrap_or_else(|| panic!("{source}: {lines:#?}"));
        let (stream, level, frame) = &lines[thrown + 1];
        assert_eq!((*stream, *level), (Stderr, Error), "{source}");
        assert!(frame.trim_start().starts_with("at "), "{source}: {frame}");
    }
}

#[test]
fn a_package_not_developed_keeps_a_small_window_and_writes_nothing_to_disk() {
    let pane = Pane::new();
    let (_, identity) = pane.install("sample_settings");
    pane.run("Write to the log");
    assert_eq!(written(&pane.log(&identity)).len(), 6);
    pane.run("Flood the log");
    assert!(pane.log(&identity).len() <= WINDOW_LINES);
    assert!(pane.launcher.extension_log_file(&identity).is_none());
    assert_eq!(pane.log_files(), Vec::<PathBuf>::new());
    // Nobody follows the log of a package not developed.
    let followed = pane.launcher.follow_extension_log(&identity);
    pane.run("Write to the log");
    assert_eq!(followed.try_iter().count(), 0);
}

#[test]
fn unresponsive_calls_and_pauses_are_logged() {
    let pane = Pane::new();
    pane.runtime.set_limits(Limits {
        compute: Duration::from_secs(2),
        warn: Duration::from_secs(1),
        unresponsive: Duration::from_secs(3),
    });
    let (_, identity) = pane.install("sample_settings");
    assert!(matches!(pane.run("Stop responding"), Status::Error(_)));
    for _ in 0..2 {
        assert!(matches!(pane.run("Crash"), Status::Error(_)));
    }
    let ours = panes(&pane.log(&identity));
    assert!(
        ours.iter().any(|(level, text)| *level == LogLevel::Error
            && text.starts_with("The extension stopped responding")),
        "{ours:?}"
    );
    assert!(
        ours.iter().any(|(level, text)| *level == LogLevel::Warn
            && text.starts_with("Pane paused the extension: ")),
        "{ours:?}"
    );
}

#[test]
fn a_developed_package_s_logs_screen_is_one_of_its_operations_and_its_build_s_details() {
    let pane = Pane::new();
    let (folder, identity) = pane.developing("sample_settings");

    // Settings offers it among the package's operations while it is
    // developed: "Logs for Dev", which shows its Logs screen, whose lines
    // are the window's to draw.
    let logs = pane
        .launcher
        .extension_operations()
        .into_iter()
        .find(|operation| operation.kind == OperationKind::Logs)
        .unwrap();
    assert_eq!(logs.title, "Logs for Dev");
    assert_eq!(logs.owner, Some(identity.clone()));
    block_on(pane.launcher.run_extension_operation(&logs));
    let view = pane.launcher.view();
    assert!(matches!(&view.screen, Screen::ExtensionLog { identity: shown } if *shown == identity));
    assert_eq!(view.title, "Logs for Dev");
    assert!(view.rows.is_empty());
    assert_eq!(pane.launcher.selected_action().label, "Copy line");
    // Opened by an operation, it goes back to root search.
    pane.launcher.back();
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));

    // A failed build's details offer it beside building again; back from
    // it is the extension list, at its row.
    fs::write(folder.join("source.txt"), "error: it does not build").unwrap();
    wait_until("the failed build", || {
        pane.launcher
            .development(&identity)
            .is_some_and(|development| development.failure.is_some())
    });
    manage(&pane.launcher);
    select_title(&pane.launcher, "Why Dev did not build");
    block_on(pane.launcher.activate_selected());
    assert_eq!(titles(&pane.launcher), ["Build Dev again", "Logs for Dev"]);
    select_title(&pane.launcher, "Logs for Dev");
    block_on(pane.launcher.activate_selected());
    assert_eq!(pane.launcher.view().title, "Logs for Dev");
    pane.launcher.back();
    let view = pane.launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Logs for Dev")
    );

    // Not developed, a package offers none.
    pane.launcher.stop_developing(&identity);
    let operations = pane.launcher.extension_operations();
    assert!(!operations.iter().any(|o| o.kind == OperationKind::Logs));
}

#[test]
fn the_log_file_opens_through_pane_and_clearing_the_log_keeps_it() {
    let pane = Pane::new();
    let (_, identity) = pane.developing("sample_settings");
    pane.run("Write to the log");
    let file = pane.launcher.extension_log_file(&identity).unwrap();
    assert_eq!(
        block_on(pane.launcher.open_extension_log_file(&identity)),
        Ok(format!("Opened {}", file.display()))
    );
    assert_eq!(*pane.opened.0.lock().unwrap(), std::slice::from_ref(&file));

    // Clearing forgets the lines Pane keeps; its log file keeps them.
    pane.launcher.clear_extension_log(&identity);
    assert!(pane.log(&identity).is_empty());
    let kept = fs::read_to_string(&file).unwrap();
    assert!(
        kept.contains(" info  stdout [open] an info line\n"),
        "{kept}"
    );

    // Once it is not developed, there is no log file to open.
    pane.launcher.stop_developing(&identity);
    assert_eq!(
        block_on(pane.launcher.open_extension_log_file(&identity)),
        Err("Dev has no log file: it is not being developed".into())
    );
}
