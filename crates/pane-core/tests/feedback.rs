//! What a command does after it acts (#141, ADR 0037), through the
//! launcher's public interface with the actions sample in Rust,
//! JavaScript and TypeScript, real guests `cargo xtask guests` assembles,
//! and a recording window: `close` hides the window and decides what its
//! next showing shows (and empties root search when asked), `pop-to-root`
//! and `clear-search` do what they name, and in a call no window was shown
//! for each answers so; `show-hud` closes the window, then the window shows
//! the HUD for 1.2 seconds, or 3 for a failure; one toast at a time, a
//! replaced toast's handle doing nothing and an update changing the toast
//! shown; a toast's actions run their callbacks, with their shortcuts
//! bound; while the window is hidden or compact a toast is a HUD; an
//! action's answer reaches no status line, and an error it answers is a
//! failure toast offering to copy it; a no-view run's failure is such a
//! toast too, and a toast it left animated is hidden when it ends; and a
//! command's row subtitle shows, is matched, survives a restart and is
//! forgotten on uninstall.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::{
    Binding, Hud, Launcher, NextShowing, PackageIdentity, Runtime, SavedData, Screen, Status,
    ToastSlot, ToastStyle, WindowPresence,
};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;
#[path = "support/feedback.rs"]
mod window_fake;

use rows::select_title;
use window_fake::{RecordingWindow, shown};

/// One language's actions sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its view command's title in root search.
    command: &'static str,
    /// Its no-view commands' titles' language suffix (" (JavaScript)").
    suffix: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-actions",
    title: "Actions sample",
    command: "Actions",
    suffix: "",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-actions-js",
    title: "JavaScript actions sample",
    command: "Actions (JavaScript)",
    suffix: " (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-actions-ts",
    title: "TypeScript actions sample",
    command: "Actions (TypeScript)",
    suffix: " (TypeScript)",
};

/// How long a launch another command started may take: compiling the
/// guest once is included.
const PROMPTLY: Duration = Duration::from_secs(30);

/// The "Window" item's actions, by their place.
const CLOSE: usize = 0;
const CLOSE_TO_ROOT: usize = 1;
const CLOSE_KEEPING_SCREEN: usize = 2;
const POP_TO_ROOT: usize = 4;
const POP_TO_ROOT_CLEARING: usize = 5;
const CLEAR_SEARCH: usize = 6;
const IN_THE_BACKGROUND: usize = 7;

/// The "Feedback" item's actions, by their place.
const SHOW_HUD: usize = 0;
const SHOW_FAILURE_HUD: usize = 1;
const START_UPLOAD: usize = 2;
const FINISH_UPLOAD: usize = 3;
const UPLOAD: usize = 4;
const HIDE_TOAST: usize = 5;
const FAIL: usize = 6;
const SET_SUBTITLE: usize = 7;
const CLEAR_SUBTITLE: usize = 8;

/// One test's Pane, with one language's sample installed and a recording
/// window attached.
struct Pane {
    _sources: TempDir,
    data: TempDir,
    folder: PathBuf,
    launcher: Launcher,
    window: Arc<RecordingWindow>,
}

impl Pane {
    fn with(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let launcher = launcher_in(&data);
        let window = RecordingWindow::attach(&launcher);
        let folder = copy(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let pane = Pane {
            _sources: sources,
            data,
            folder,
            launcher,
            window,
        };
        pane.to_root();
        pane
    }

    /// Back to root search, wherever the launcher is.
    fn to_root(&self) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
    }

    /// Opens the sample's view command from root search.
    fn open(&self, fixture: &Fixture) {
        self.to_root();
        block_on(self.launcher.set_query(fixture.command));
        select_title(&self.launcher, fixture.command);
        block_on(self.launcher.activate_selected());
        assert_eq!(
            (
                &self.launcher.view().screen,
                self.launcher.view().title.as_str()
            ),
            (&Screen::Command, "Actions sample"),
            "{:?}",
            self.launcher.view().status
        );
    }

    /// Runs the action at `index` of the item `item` ("window", "alpha"),
    /// as the Actions panel chooses it.
    fn act(&self, item: &str, index: usize) {
        let title = match item {
            "alpha" => "Alpha note",
            "window" => "Window",
            "feedback" => "Feedback",
            other => panic!("no item {other}"),
        };
        select_title(&self.launcher, title);
        block_on(self.launcher.run_item_action(item, index));
    }

    /// Types `query` in root search and runs the row titled `title`.
    fn run(&self, query: &str, title: &str) {
        self.to_root();
        block_on(self.launcher.set_query(query));
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
    }

    /// The title of the toast in the footer, if one shows.
    fn toast(&self) -> Option<String> {
        self.launcher.toast().map(|shown| shown.toast.title)
    }

    /// The subtitle root search shows for the sample's view command.
    fn subtitle(&self, fixture: &Fixture) -> Option<String> {
        self.to_root();
        block_on(self.launcher.set_query(""));
        self.launcher
            .view()
            .rows
            .into_iter()
            .find(|row| row.title == fixture.command)
            .unwrap_or_else(|| panic!("no row {}", fixture.command))
            .subtitle
    }
}

/// A launcher over the extensions kept in `data`.
fn launcher_in(data: &TempDir) -> Launcher {
    Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
}

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn copy(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

fn an_actions_answer_is_a_toast_and_never_the_status_line(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    select_title(&pane.launcher, "Alpha note");
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.launcher.view().status,
        Status::Idle,
        "{}",
        fixture.title
    );
    let shown_toast = pane.launcher.toast().expect("a toast shows");
    assert_eq!(shown_toast.toast.style, ToastStyle::Success);
    assert_eq!(shown_toast.toast.title, "Open: Alpha note");
    assert_eq!(pane.window.take(), [], "the window stays as it is");
}

fn close_hides_the_window_and_decides_its_next_showing(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);

    pane.act("window", CLOSE);
    assert_eq!(
        pane.window.take(),
        [WindowRequest::Hide],
        "{}",
        fixture.title
    );
    assert_eq!(pane.launcher.window_presence(), WindowPresence::Hidden);
    assert_eq!(pane.launcher.take_next_showing(), NextShowing::BySetting);
    assert_eq!(
        pane.launcher.view().screen,
        Screen::Command,
        "the screen stays"
    );
    assert_eq!(shown(&pane.launcher), Status::Idle, "nothing failed");

    pane.launcher.set_window_presence(WindowPresence::Shown);
    pane.act("window", CLOSE_KEEPING_SCREEN);
    assert_eq!(pane.window.take(), [WindowRequest::Hide]);
    assert_eq!(pane.launcher.take_next_showing(), NextShowing::Restore);
    assert_eq!(pane.launcher.view().screen, Screen::Command);

    pane.launcher.set_window_presence(WindowPresence::Shown);
    pane.act("window", CLOSE_TO_ROOT);
    assert_eq!(pane.window.take(), [WindowRequest::Hide]);
    assert_eq!(pane.launcher.take_next_showing(), NextShowing::BySetting);
    assert_eq!(
        pane.launcher.view().screen,
        Screen::Root {
            query: String::new()
        },
        "root search, now"
    );
}

fn close_empties_root_search_when_asked(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    // "Window functions", run from root search with its title typed,
    // closes the window asking to empty root search's query.
    let title = format!("Window functions{}", fixture.suffix);
    pane.run(&title, &title);
    assert_eq!(
        pane.launcher.view().query(),
        Some(""),
        "{}: the query was emptied",
        fixture.title
    );
    assert_eq!(pane.launcher.window_presence(), WindowPresence::Hidden);
    let requests = pane.window.take();
    assert_eq!(requests.first(), Some(&WindowRequest::Hide), "{requests:?}");
    // The window being hidden, its toast saying what each answered is a
    // HUD.
    assert_eq!(
        requests.get(1),
        Some(&WindowRequest::Hud(Hud {
            title: "close: true, pop to root: true, clear search: true".into(),
            style: ToastStyle::Success,
        })),
        "{requests:?}"
    );
}

fn pop_to_root_and_clear_search_do_what_they_name(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("window", POP_TO_ROOT);
    assert_eq!(
        pane.launcher.view().screen,
        Screen::Root {
            query: String::new()
        },
        "{}",
        fixture.title
    );
    assert_eq!(pane.window.take(), [], "the window stays open");

    pane.open(fixture);
    pane.act("window", POP_TO_ROOT_CLEARING);
    assert_eq!(pane.launcher.view().query(), Some(""));

    // Clearing a list's search field when it has none changes nothing,
    // and says the window was shown.
    pane.open(fixture);
    pane.act("window", CLEAR_SEARCH);
    assert_eq!(pane.launcher.view().screen, Screen::Command);
    assert_eq!(shown(&pane.launcher), Status::Idle);
    assert_eq!(pane.window.take(), []);
}

fn with_no_window_each_window_function_says_none_was_shown(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("window", IN_THE_BACKGROUND);
    assert!(pane.launcher.wait_for_launches(PROMPTLY));
    assert_eq!(
        pane.toast().as_deref(),
        Some("close: false, pop to root: false, clear search: false"),
        "{}",
        fixture.title
    );
    assert_eq!(pane.window.take(), [], "nothing was hidden");
    assert_eq!(pane.launcher.view().screen, Screen::Command);
}

fn a_hud_closes_the_window_and_stays_its_time(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("feedback", SHOW_HUD);
    let copied = Hud {
        title: "Copied to Clipboard".into(),
        style: ToastStyle::Success,
    };
    assert_eq!(
        pane.window.take(),
        [WindowRequest::Hide, WindowRequest::Hud(copied.clone())],
        "{}",
        fixture.title
    );
    assert_eq!(copied.duration(), Duration::from_millis(1200));
    assert_eq!(pane.launcher.window_presence(), WindowPresence::Hidden);

    pane.act("feedback", SHOW_FAILURE_HUD);
    let failed = Hud {
        title: "Could not copy".into(),
        style: ToastStyle::Failure,
    };
    // Already hidden: only the HUD.
    assert_eq!(pane.window.take(), [WindowRequest::Hud(failed.clone())]);
    assert_eq!(failed.duration(), Duration::from_secs(3));
}

fn one_toast_at_a_time_updated_and_never_by_a_stale_handle(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("feedback", START_UPLOAD);
    let started = pane.launcher.toast().expect("the upload's toast");
    assert_eq!(
        (started.toast.style, started.toast.title.as_str()),
        (ToastStyle::Animated, "Uploading…"),
        "{}",
        fixture.title
    );

    pane.act("feedback", FINISH_UPLOAD);
    let finished = pane.launcher.toast().expect("the upload's toast");
    assert_eq!(finished.id, started.id, "updated, not replaced");
    assert!(finished.revision > started.revision);
    assert_eq!(finished.toast.style, ToastStyle::Success);
    assert_eq!(finished.toast.text(), "Uploaded: notes.txt");
    let primary = finished.toast.primary.as_ref().expect("Open");
    let secondary = finished.toast.secondary.as_ref().expect("Retry");
    assert_eq!(
        (primary.title.as_str(), secondary.title.as_str()),
        ("Open", "Retry")
    );
    assert_eq!(
        finished
            .toast
            .bound_to(&Binding::parse("ctrl-shift-r").unwrap()),
        Some(ToastSlot::Secondary)
    );

    // Another toast replaces it; the upload's handle then does nothing.
    pane.act("feedback", START_UPLOAD);
    pane.act("alpha", 0);
    assert_eq!(pane.toast().as_deref(), Some("Open: Alpha note"));
    pane.act("feedback", FINISH_UPLOAD);
    assert_eq!(pane.toast().as_deref(), Some("Open: Alpha note"));
    pane.act("feedback", HIDE_TOAST);
    assert_eq!(pane.toast().as_deref(), Some("Open: Alpha note"));

    // Its own handle hides it.
    pane.act("feedback", START_UPLOAD);
    pane.act("feedback", HIDE_TOAST);
    assert_eq!(pane.toast(), None);
}

fn a_toasts_actions_call_the_command_back(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("feedback", UPLOAD);
    let uploaded = pane.launcher.toast().expect("the upload's toast");
    block_on(
        pane.launcher
            .run_toast_action(uploaded.id, ToastSlot::Primary),
    );
    assert_eq!(
        pane.toast().as_deref(),
        Some("Opened the upload"),
        "{}",
        fixture.title
    );

    pane.act("feedback", UPLOAD);
    let uploaded = pane.launcher.toast().expect("the upload's toast");
    block_on(
        pane.launcher
            .run_toast_action(uploaded.id, ToastSlot::Secondary),
    );
    let retried = pane.launcher.toast().expect("the retried upload's toast");
    assert_ne!(retried.id, uploaded.id, "a new upload");
    assert_eq!(retried.toast.text(), "Uploaded: notes.txt");
    // A replaced toast's actions run nothing.
    block_on(
        pane.launcher
            .run_toast_action(uploaded.id, ToastSlot::Primary),
    );
    assert_eq!(
        pane.launcher.toast().map(|shown| shown.id),
        Some(retried.id)
    );
}

fn while_the_window_is_hidden_or_compact_a_toast_is_a_hud(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    for presence in [WindowPresence::Hidden, WindowPresence::Compact] {
        pane.launcher.set_window_presence(presence);
        pane.act("feedback", UPLOAD);
        assert_eq!(
            pane.launcher.toast(),
            None,
            "{}: {presence:?}",
            fixture.title
        );
        assert_eq!(
            pane.window.huds(),
            [
                Hud {
                    title: "Uploading…".into(),
                    style: ToastStyle::Animated
                },
                Hud {
                    title: "Uploaded: notes.txt".into(),
                    style: ToastStyle::Success
                },
            ],
            "{presence:?}"
        );
        pane.window.take();
    }
}

fn a_failure_is_a_toast_offering_to_copy_the_error(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.open(fixture);
    pane.act("feedback", FAIL);
    assert_eq!(
        pane.launcher.view().status,
        Status::Idle,
        "{}",
        fixture.title
    );
    assert_eq!(
        shown(&pane.launcher),
        Status::Error("The extension reported an error: The upload failed".into())
    );
    let failed = pane.launcher.toast().expect("the failure toast");
    assert_eq!(
        failed
            .toast
            .primary
            .as_ref()
            .map(|action| action.title.as_str()),
        Some("Copy Error")
    );
    assert_eq!(
        pane.launcher
            .toast_action_copy(failed.id, ToastSlot::Primary)
            .as_deref(),
        Some("The upload failed")
    );
}

fn a_no_view_runs_failure_and_forgotten_progress_are_ended_by_pane(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let stumble = format!("Stumble{}", fixture.suffix);
    pane.run(&stumble, &stumble);
    let failed = pane.launcher.toast().expect("the failure toast");
    assert_eq!(
        (failed.toast.style, failed.toast.text()),
        (
            ToastStyle::Failure,
            "The extension reported an error: Stumbled on purpose".to_owned()
        ),
        "{}",
        fixture.title
    );
    assert_eq!(
        failed.toast.primary.map(|action| action.title),
        Some("Copy Error".into())
    );

    let spin = format!("Spin{}", fixture.suffix);
    pane.run(&spin, &spin);
    assert_eq!(pane.launcher.toast(), None, "the spinning toast was hidden");
    assert_eq!(pane.launcher.view().status, Status::Idle);
}

fn a_subtitle_shows_is_matched_survives_a_restart_and_goes_on_uninstall(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let declared = pane.subtitle(fixture);
    assert_eq!(
        declared.as_deref(),
        Some("Items with several actions, in sections, with shortcuts")
    );
    pane.open(fixture);
    pane.act("feedback", SET_SUBTITLE);
    assert_eq!(shown(&pane.launcher), Status::Idle, "{}", fixture.title);
    assert_eq!(pane.subtitle(fixture).as_deref(), Some("3 unread"));
    // Root search matches it.
    block_on(pane.launcher.set_query("3 unread"));
    let titles: Vec<String> = pane
        .launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect();
    assert!(titles.contains(&fixture.command.to_owned()), "{titles:?}");

    // Pane keeps it: a restarted Pane shows it.
    assert!(
        pane.launcher.wait_for_subtitles_recorded(PROMPTLY),
        "the subtitle was recorded"
    );
    let restarted = launcher_in(&pane.data);
    let row = restarted
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == fixture.command)
        .expect("the command's row");
    assert_eq!(row.subtitle.as_deref(), Some("3 unread"));
    drop(restarted);

    // Cleared, the manifest's comes back.
    pane.open(fixture);
    pane.act("feedback", CLEAR_SUBTITLE);
    assert_eq!(pane.subtitle(fixture), declared);

    // Forgotten on uninstall: installed again, the row has the manifest's.
    pane.open(fixture);
    pane.act("feedback", SET_SUBTITLE);
    let identity = PackageIdentity::local(&pane.folder).unwrap();
    pane.to_root();
    block_on(pane.launcher.uninstall(&identity, SavedData::Keep));
    block_on(pane.launcher.install_package(&pane.folder));
    pane.to_root();
    assert_eq!(pane.subtitle(fixture), declared);
}

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
    an_actions_answer_is_a_toast_and_never_the_status_line,
    close_hides_the_window_and_decides_its_next_showing,
    close_empties_root_search_when_asked,
    pop_to_root_and_clear_search_do_what_they_name,
    with_no_window_each_window_function_says_none_was_shown,
    a_hud_closes_the_window_and_stays_its_time,
    one_toast_at_a_time_updated_and_never_by_a_stale_handle,
    a_toasts_actions_call_the_command_back,
    while_the_window_is_hidden_or_compact_a_toast_is_a_hud,
    a_failure_is_a_toast_offering_to_copy_the_error,
    a_no_view_runs_failure_and_forgotten_progress_are_ended_by_pane,
    a_subtitle_shows_is_matched_survives_a_restart_and_goes_on_uninstall,
);
