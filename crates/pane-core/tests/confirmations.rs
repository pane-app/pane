//! Confirmations a command asks for before it does something it cannot
//! undo (#146, `feedback.confirm`), through the launcher's public interface
//! with the actions sample in Rust, JavaScript and TypeScript, real guests
//! `cargo xtask guests` assembles, and a recording window: a confirmation
//! shows its title, message and buttons and answers true only for the
//! primary button (the dismiss button, a click outside it and the window
//! losing the focus answer false); a hidden launcher is shown for it, and a
//! background launch is answered that none is available there; "Don't ask
//! again" remembers a button's answer per package and key, across a
//! restart, a disable and an update, until "Reset confirmations" on the
//! package's card or uninstalling forgets it; and while a confirmation
//! waits, another package's calls complete.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::{
    ConfirmAnswer, Confirmation, Launcher, PackageIdentity, Runtime, SavedData, Screen, Status,
    WindowPresence,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::RecordingWindow;
use rows::{manage, select_title, titles};

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

/// How long a guest may take to ask or answer: compiling it once is
/// included.
const PROMPTLY: Duration = Duration::from_secs(60);

/// The "Confirm" item's actions, by their place.
const DELETE: usize = 0;
const ASK: usize = 1;
const CLOSE_AND_ASK: usize = 2;
const ASK_IN_THE_BACKGROUND: usize = 3;

/// One test's Pane, with one language's sample installed and a recording
/// window attached.
struct Pane {
    _sources: TempDir,
    data: TempDir,
    folder: PathBuf,
    identity: PackageIdentity,
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
        let identity = PackageIdentity::local(&folder).unwrap();
        Pane {
            _sources: sources,
            data,
            folder,
            identity,
            launcher,
            window,
        }
    }

    /// This test's Pane started again over the same extensions, as after a
    /// restart, with a recording window attached.
    fn restarted(&self) -> (Launcher, Arc<RecordingWindow>) {
        let launcher = launcher_in(&self.data);
        let window = RecordingWindow::attach(&launcher);
        (launcher, window)
    }

    /// The remembered answers' keys of the sample.
    fn remembered(&self) -> Vec<String> {
        self.launcher.remembered_confirmations(&self.identity)
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

/// Back to root search, wherever `launcher` is.
fn to_root(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

/// Opens the sample's view command from root search.
fn open(launcher: &Launcher, fixture: &Fixture) {
    to_root(launcher);
    block_on(launcher.set_query(fixture.command));
    select_title(launcher, fixture.command);
    block_on(launcher.activate_selected());
    assert_eq!(
        (&launcher.view().screen, launcher.view().title.as_str()),
        (&Screen::Command, "Actions sample"),
        "{:?}",
        launcher.view().status
    );
}

/// Opens the extension manager from wherever `launcher` is.
fn to_manage(launcher: &Launcher) {
    to_root(launcher);
    block_on(launcher.set_query(""));
    manage(launcher);
}

/// A call that may wait on a confirmation, running on a thread of its own.
struct Running {
    thread: thread::JoinHandle<()>,
}

impl Running {
    /// Waits for the call to end.
    fn ended(self) {
        let started = Instant::now();
        while !self.thread.is_finished() {
            assert!(started.elapsed() < PROMPTLY, "the call did not end");
            thread::sleep(Duration::from_millis(5));
        }
        self.thread.join().unwrap();
    }
}

/// Starts the "Confirm" item's action at `index` of the open sample, on a
/// thread of its own: it may wait on the user.
fn act(launcher: &Launcher, index: usize) -> Running {
    select_title(launcher, "Confirm");
    let running = launcher.run_item_action("confirm", index);
    Running {
        thread: thread::spawn(move || block_on(running)),
    }
}

/// Types `query` in root search and starts the row titled `title`, on a
/// thread of its own.
fn launch_from_root(launcher: &Launcher, query: &str, title: &str) -> Running {
    to_root(launcher);
    block_on(launcher.set_query(query));
    select_title(launcher, title);
    let running = launcher.activate_selected();
    Running {
        thread: thread::spawn(move || block_on(running)),
    }
}

/// The confirmation the launcher shows, once the command asked for it.
fn asked(launcher: &Launcher) -> Confirmation {
    let started = Instant::now();
    loop {
        if let Some(confirmation) = launcher.confirmation() {
            return confirmation;
        }
        assert!(
            started.elapsed() < PROMPTLY,
            "no confirmation was asked: {:?}",
            launcher.view()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// The title of the toast in the footer, if one shows.
fn toast(launcher: &Launcher) -> Option<String> {
    launcher.toast().map(|shown| shown.toast.title)
}

/// Asks with "Ask", answers `answer`, and says what the command toasted.
fn ask_and_answer(launcher: &Launcher, answer: ConfirmAnswer) -> Option<String> {
    let running = act(launcher, ASK);
    let confirmation = asked(launcher);
    launcher.answer_confirmation(confirmation.id, answer, false);
    running.ended();
    toast(launcher)
}

/// Runs "Delete", which answers at once from the remembered answer, and
/// says what the command toasted; panics if it asked.
fn delete_at_once(launcher: &Launcher) -> Option<String> {
    let running = act(launcher, DELETE);
    running.ended();
    assert_eq!(launcher.confirmation(), None, "it asked nothing");
    toast(launcher)
}

/// Runs "Delete", which asks, ticks "Don't ask again" (or not) and
/// confirms, and says what the command toasted.
fn delete_confirming(launcher: &Launcher, dont_ask_again: bool) -> Option<String> {
    let running = act(launcher, DELETE);
    let confirmation = asked(launcher);
    assert!(confirmation.rememberable);
    launcher.answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, dont_ask_again);
    running.ended();
    toast(launcher)
}

fn a_confirmation_shows_its_title_message_and_buttons(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    let running = act(&pane.launcher, DELETE);
    let delete = asked(&pane.launcher);
    assert_eq!(
        (
            delete.title.as_str(),
            delete.message.as_deref(),
            delete.primary.as_str(),
            delete.dismiss.as_str(),
            delete.destructive,
            delete.rememberable
        ),
        (
            "Delete the note?",
            Some("It cannot be brought back."),
            "Delete",
            "Cancel",
            true,
            true
        ),
        "{}",
        fixture.title
    );
    assert_eq!(pane.window.confirmations(), 1, "the window draws it");
    pane.launcher
        .answer_confirmation(delete.id, ConfirmAnswer::Dismissed, false);
    running.ended();

    let running = act(&pane.launcher, ASK);
    let ask = asked(&pane.launcher);
    assert_eq!(
        (
            ask.title.as_str(),
            ask.message,
            ask.primary.as_str(),
            ask.dismiss.as_str(),
            ask.destructive,
            ask.rememberable
        ),
        ("Go on?", None, "Go On", "Stop", false, false)
    );
    pane.launcher
        .answer_confirmation(ask.id, ConfirmAnswer::Dismissed, false);
    running.ended();
}

fn only_the_primary_button_confirms(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    assert_eq!(
        ask_and_answer(&pane.launcher, ConfirmAnswer::Confirmed).as_deref(),
        Some("Went on"),
        "{}",
        fixture.title
    );
    assert_eq!(
        ask_and_answer(&pane.launcher, ConfirmAnswer::Dismissed).as_deref(),
        Some("Stopped")
    );
    // A click outside it.
    assert_eq!(
        ask_and_answer(&pane.launcher, ConfirmAnswer::Left).as_deref(),
        Some("Stopped")
    );
    // The window losing the focus.
    let running = act(&pane.launcher, ASK);
    asked(&pane.launcher);
    pane.launcher.window_deactivated();
    running.ended();
    assert_eq!(toast(&pane.launcher).as_deref(), Some("Stopped"));
    assert_eq!(pane.launcher.confirmation(), None);
    // Nothing failed, and the screen is the command's still.
    assert_eq!(pane.launcher.view().status, Status::Idle);
    assert_eq!(pane.launcher.view().screen, Screen::Command);
}

fn a_hidden_launcher_is_shown_for_the_confirmation(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    // "Close and Ask" closes the window, then asks.
    let running = act(&pane.launcher, CLOSE_AND_ASK);
    let confirmation = asked(&pane.launcher);
    assert_eq!(
        confirmation.title, "Asked while hidden",
        "{}",
        fixture.title
    );
    assert_eq!(
        pane.window.take(),
        [WindowRequest::Hide, WindowRequest::Confirmation],
        "hidden, then shown for the confirmation"
    );
    // The window shows itself, over the screen it was left on.
    pane.launcher.set_window_presence(WindowPresence::Shown);
    assert_eq!(pane.launcher.view().screen, Screen::Command);
    pane.launcher
        .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
    running.ended();
    assert_eq!(
        toast(&pane.launcher).as_deref(),
        Some("Confirmed while hidden")
    );

    // A no-view command run while the launcher is hidden (as its hotkey
    // runs it) has the window show itself to ask.
    let confirm_run = format!("Confirm Run{}", fixture.suffix);
    pane.launcher.set_window_presence(WindowPresence::Hidden);
    let running = launch_from_root(&pane.launcher, &confirm_run, &confirm_run);
    let confirmation = asked(&pane.launcher);
    assert_eq!(confirmation.title, "Run it?");
    assert_eq!(pane.window.confirmations(), 1);
    pane.launcher.set_window_presence(WindowPresence::Shown);
    pane.launcher
        .answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
    running.ended();
    assert_eq!(toast(&pane.launcher).as_deref(), Some("Ran"));
}

fn a_background_launch_is_answered_that_no_confirmation_is_available(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    act(&pane.launcher, ASK_IN_THE_BACKGROUND).ended();
    assert!(pane.launcher.wait_for_launches(PROMPTLY));
    let shown = pane.launcher.toast().expect("the command's toast");
    assert_eq!(shown.toast.title, "Not asked", "{}", fixture.title);
    let reason = shown.toast.message.unwrap_or_default();
    assert!(reason.contains("not available"), "{reason}");
    assert_eq!(pane.launcher.confirmation(), None);
    assert_eq!(pane.window.confirmations(), 0, "nothing was shown");
    assert_eq!(pane.window.take(), [], "the window stays as it is");
    // A refusal is an answer: the package runs on.
    assert!(
        pane.launcher
            .packages()
            .iter()
            .any(|package| package.identity == pane.identity && package.enabled)
    );
    assert_eq!(
        ask_and_answer(&pane.launcher, ConfirmAnswer::Confirmed).as_deref(),
        Some("Went on")
    );
}

fn dont_ask_again_answers_at_once_across_a_restart_a_disable_and_an_update(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    // Answered without ticking it, it asks again.
    assert_eq!(
        delete_confirming(&pane.launcher, false).as_deref(),
        Some("Deleted"),
        "{}",
        fixture.title
    );
    assert!(pane.remembered().is_empty());
    // "Ask" offers nothing to remember: ticked or not, it asks again.
    let running = act(&pane.launcher, ASK);
    let ask = asked(&pane.launcher);
    pane.launcher
        .answer_confirmation(ask.id, ConfirmAnswer::Confirmed, true);
    running.ended();
    assert!(pane.remembered().is_empty());

    // Ticked, the answer is remembered and given at once from then on.
    assert_eq!(
        delete_confirming(&pane.launcher, true).as_deref(),
        Some("Deleted")
    );
    assert_eq!(pane.remembered(), ["delete-note"]);
    assert_eq!(delete_at_once(&pane.launcher).as_deref(), Some("Deleted"));

    // After a disable.
    to_root(&pane.launcher);
    block_on(pane.launcher.set_enabled(&pane.identity, false));
    block_on(pane.launcher.set_enabled(&pane.identity, true));
    open(&pane.launcher, fixture);
    assert_eq!(delete_at_once(&pane.launcher).as_deref(), Some("Deleted"));

    // After an update.
    to_root(&pane.launcher);
    block_on(pane.launcher.preview_package(&pane.folder));
    select_title(&pane.launcher, "Update");
    block_on(pane.launcher.activate_selected());
    assert!(
        matches!(&pane.launcher.view().status, Status::Result(updated) if updated.starts_with("Updated")),
        "{:?}",
        pane.launcher.view().status
    );
    open(&pane.launcher, fixture);
    assert_eq!(delete_at_once(&pane.launcher).as_deref(), Some("Deleted"));
    assert_eq!(pane.remembered(), ["delete-note"]);

    // After a restart: this Pane stops, and another starts over the same
    // extensions.
    assert!(
        pane.launcher.wait_for_confirmations_recorded(PROMPTLY),
        "the answer was recorded"
    );
    let Pane {
        _sources,
        data,
        identity,
        launcher,
        ..
    } = pane;
    drop(launcher);
    let restarted = launcher_in(&data);
    let _window = RecordingWindow::attach(&restarted);
    assert_eq!(
        restarted.remembered_confirmations(&identity),
        ["delete-note"]
    );
    open(&restarted, fixture);
    assert_eq!(delete_at_once(&restarted).as_deref(), Some("Deleted"));
}

fn a_dismissal_with_dont_ask_again_is_remembered_too_but_a_click_outside_never(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    // A click outside it, ticked: nothing is remembered.
    let running = act(&pane.launcher, DELETE);
    let delete = asked(&pane.launcher);
    pane.launcher
        .answer_confirmation(delete.id, ConfirmAnswer::Left, true);
    running.ended();
    assert_eq!(
        toast(&pane.launcher).as_deref(),
        Some("Kept"),
        "{}",
        fixture.title
    );
    assert!(pane.remembered().is_empty());

    // The dismiss button, ticked: "Kept" from then on.
    let running = act(&pane.launcher, DELETE);
    let delete = asked(&pane.launcher);
    pane.launcher
        .answer_confirmation(delete.id, ConfirmAnswer::Dismissed, true);
    running.ended();
    assert_eq!(pane.remembered(), ["delete-note"]);
    assert_eq!(delete_at_once(&pane.launcher).as_deref(), Some("Kept"));
}

fn reset_confirmations_on_the_card_and_uninstalling_forget_the_answers(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    open(&pane.launcher, fixture);
    let reset_row = format!("Reset confirmations of {}", fixture.title);
    // Nothing remembered: the card offers no reset.
    to_manage(&pane.launcher);
    assert!(!titles(&pane.launcher).contains(&reset_row));

    open(&pane.launcher, fixture);
    delete_confirming(&pane.launcher, true);
    assert_eq!(pane.remembered(), ["delete-note"]);
    to_manage(&pane.launcher);
    select_title(&pane.launcher, &reset_row);
    block_on(pane.launcher.activate_selected());
    assert!(
        matches!(pane.launcher.view().status, Status::Result(_)),
        "{}: {:?}",
        fixture.title,
        pane.launcher.view().status
    );
    assert!(pane.remembered().is_empty());
    assert!(
        !titles(&pane.launcher).contains(&reset_row),
        "the row went with the answers"
    );
    // The command asks again; reset, nothing is left to restart with.
    open(&pane.launcher, fixture);
    let running = act(&pane.launcher, DELETE);
    let delete = asked(&pane.launcher);
    pane.launcher
        .answer_confirmation(delete.id, ConfirmAnswer::Confirmed, true);
    running.ended();
    assert_eq!(pane.remembered(), ["delete-note"]);
    assert!(pane.launcher.reset_confirmations(&pane.identity));
    assert!(!pane.launcher.reset_confirmations(&pane.identity));
    assert!(pane.launcher.wait_for_confirmations_recorded(PROMPTLY));
    let (restarted, _window) = pane.restarted();
    assert!(
        restarted
            .remembered_confirmations(&pane.identity)
            .is_empty()
    );
    drop(restarted);

    // Uninstalling forgets them, whatever data is kept.
    open(&pane.launcher, fixture);
    delete_confirming(&pane.launcher, true);
    assert_eq!(pane.remembered(), ["delete-note"]);
    to_root(&pane.launcher);
    block_on(pane.launcher.uninstall(&pane.identity, SavedData::Keep));
    assert!(pane.remembered().is_empty());
    block_on(pane.launcher.install_package(&pane.folder));
    assert!(pane.remembered().is_empty());
    open(&pane.launcher, fixture);
    let running = act(&pane.launcher, DELETE);
    let delete = asked(&pane.launcher);
    pane.launcher
        .answer_confirmation(delete.id, ConfirmAnswer::Dismissed, false);
    running.ended();
    assert!(pane.launcher.wait_for_confirmations_recorded(PROMPTLY));
    let (restarted, _window) = pane.restarted();
    assert!(
        restarted
            .remembered_confirmations(&pane.identity)
            .is_empty(),
        "forgotten in the record too"
    );
}

fn another_packages_call_completes_while_a_confirmation_waits(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let calculator =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator");
    block_on(pane.launcher.install_package(&calculator));
    assert!(
        matches!(pane.launcher.view().status, Status::Result(_)),
        "{:?}",
        pane.launcher.view().status
    );
    open(&pane.launcher, fixture);
    let running = act(&pane.launcher, ASK);
    let ask = asked(&pane.launcher);

    // The calculator, another package, answers root search meanwhile.
    to_root(&pane.launcher);
    block_on(pane.launcher.set_query("1 + 1"));
    assert_eq!(
        titles(&pane.launcher).first().map(String::as_str),
        Some("2"),
        "{}: {:?}",
        fixture.title,
        pane.launcher.view()
    );
    assert!(
        !running.thread.is_finished(),
        "the confirmation still waits"
    );
    assert_eq!(
        pane.launcher.confirmation().map(|shown| shown.id),
        Some(ask.id)
    );

    pane.launcher
        .answer_confirmation(ask.id, ConfirmAnswer::Confirmed, false);
    running.ended();
    assert_eq!(toast(&pane.launcher).as_deref(), Some("Went on"));
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
    a_confirmation_shows_its_title_message_and_buttons,
    only_the_primary_button_confirms,
    a_hidden_launcher_is_shown_for_the_confirmation,
    a_background_launch_is_answered_that_no_confirmation_is_available,
    dont_ask_again_answers_at_once_across_a_restart_a_disable_and_an_update,
    a_dismissal_with_dont_ask_again_is_remembered_too_but_a_click_outside_never,
    reset_confirmations_on_the_card_and_uninstalling_forget_the_answers,
    another_packages_call_completes_while_a_confirmation_waits,
);
