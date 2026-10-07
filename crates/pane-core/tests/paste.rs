//! Paste, the front application and selected text (#148, ADR 0037),
//! through the launcher's public interface with the actions sample in
//! Rust, JavaScript and TypeScript, real guests `cargo xtask guests`
//! assembles, a recording window and a recording system
//! (`support/system.rs`, passed in as the link opener and the clipboard
//! are). No test needs a real application in front.
//!
//! Where the system can paste, paste closes the window, puts the content
//! on the clipboard, has the application paste it and puts back what the
//! clipboard held, unless something else was copied meanwhile. Where it
//! cannot yet, paste answers "not available", distinct from a failure, and
//! leaves the window open; the SDKs' standard Paste then copies and says so
//! in a HUD. The front application answers its name and icon, or none; the
//! selected text answers the text, "nothing selected" or a failure. The
//! real adapters answer "not available on this system yet" for each, and
//! that is never a reason to pause the package.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::system::{Clip, FrontApplication, SystemError};
use pane_core::{Hud, Launcher, Runtime, Screen, Status, ToastStyle, WindowPresence};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/system.rs"]
mod recording;
#[path = "support/rows.rs"]
mod rows;

use feedback::{RecordingWindow, shown};
use recording::{Done, NOT_YET, RecordingSystem};
use rows::select_title;

/// One language's actions sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
    /// Its view command's title in root search.
    command: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-actions",
    title: "Actions sample",
    command: "Actions",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-actions-js",
    title: "JavaScript actions sample",
    command: "Actions (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-actions-ts",
    title: "TypeScript actions sample",
    command: "Actions (TypeScript)",
};

/// The "Paste" item's actions, by their place.
const PASTE: usize = 0;
const PASTE_TO: usize = 1;
const PASTE_DIRECTLY: usize = 2;
const FRONT_APPLICATION: usize = 3;
const SEARCH_SELECTION: usize = 4;

/// What the sample pastes.
const PASTED_TEXT: &str = "Pasted by the actions sample";

/// What the clipboard held before a paste.
const BEFORE: &str = "The user's own copy";

/// What the standard Paste says when it copied instead.
const PASTE_FALLBACK: &str = "Copied — paste is not available here yet";

/// The sample's text for something not available here yet.
fn not_available(why: &str) -> Status {
    Status::Error(format!("Not available here yet: {why}"))
}

/// An error the command answered with, as its failure toast reads.
fn failure(why: &str) -> Status {
    Status::Error(format!("The extension reported an error: {why}"))
}

fn pasted_text() -> Clip {
    Clip::Text(PASTED_TEXT.into())
}

/// Copied, concealed or not.
fn copied(clip: Clip, concealed: bool) -> Done {
    Done::Copied { clip, concealed }
}

/// One test's Pane, with one language's sample installed, its command
/// open, and a recording window and system attached.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    launcher: Launcher,
    window: Arc<RecordingWindow>,
    system: Arc<RecordingSystem>,
}

impl Pane {
    /// Pane with a recording system, told what to answer by `arrange`
    /// before the command opens (it titles "Paste to …" as it renders).
    fn with(fixture: &Fixture, arrange: impl FnOnce(&RecordingSystem)) -> Pane {
        let system = Arc::new(RecordingSystem::default());
        arrange(&*system);
        Pane::start(fixture, system, true)
    }

    /// Pane with this system's own adapters.
    fn native(fixture: &Fixture) -> Pane {
        Pane::start(fixture, Arc::new(RecordingSystem::default()), false)
    }

    fn start(fixture: &Fixture, system: Arc<RecordingSystem>, recording: bool) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start();
        runtime.set_applications(system.clone());
        let launcher = Launcher::with_packages(runtime, vec![], data.path().join("extensions"));
        let launcher = if recording {
            launcher.with_system(system.clone())
        } else {
            launcher.with_system(pane_core::system::native())
        };
        let window = RecordingWindow::attach(&launcher);
        let folder = copy(fixture.package, &sources.path().join(fixture.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        block_on(launcher.set_query(fixture.command));
        select_title(&launcher, fixture.command);
        block_on(launcher.activate_selected());
        assert_eq!(
            (&launcher.view().screen, launcher.view().title.as_str()),
            (&Screen::Command, "Actions sample"),
            "{:?}",
            launcher.view().status
        );
        Pane {
            _sources: sources,
            _data: data,
            launcher,
            window,
            system,
        }
    }

    /// Runs the "Paste" item's action at `index`, as the Actions panel
    /// chooses it.
    fn act(&self, index: usize) {
        select_title(&self.launcher, "Paste");
        block_on(self.launcher.run_item_action("paste", index));
    }

    /// The titles of the "Paste" item's actions.
    fn titles(&self) -> Vec<String> {
        select_title(&self.launcher, "Paste");
        self.launcher
            .item_actions()
            .expect("the Paste item's actions are listed")
            .actions
            .into_iter()
            .map(|action| action.title)
            .collect()
    }

    /// Checks that the last action closed the window, then showed `huds`,
    /// and summons the window again, as the user would.
    fn closed(&self, huds: &[Hud], what: &str) {
        let requests = self.window.take();
        assert_eq!(
            requests.first(),
            Some(&WindowRequest::Hide),
            "{what}: {requests:?}"
        );
        let shown: Vec<&Hud> = requests
            .iter()
            .filter_map(|request| match request {
                WindowRequest::Hud(hud) => Some(hud),
                _ => None,
            })
            .collect();
        assert_eq!(shown, huds.iter().collect::<Vec<_>>(), "{what}");
        assert_eq!(self.launcher.window_presence(), WindowPresence::Hidden);
        assert_eq!(self.launcher.view().screen, Screen::Command, "{what}");
        self.launcher.set_window_presence(WindowPresence::Shown);
    }

    /// Checks that the last action left the window as it was.
    fn stayed(&self, what: &str) {
        assert_eq!(self.window.take(), [], "{what}");
        assert_eq!(self.launcher.window_presence(), WindowPresence::Shown);
    }
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

fn paste_closes_the_window_pastes_and_puts_the_clipboard_back(fixture: &Fixture) {
    let pane = Pane::with(fixture, |system| {
        system.support_paste();
        system.set_clipboard(Some(Clip::Text(BEFORE.into())));
    });
    for (index, what) in [(PASTE, "Paste"), (PASTE_DIRECTLY, "Paste Directly")] {
        pane.act(index);
        assert_eq!(
            pane.system.take(),
            [
                copied(pasted_text(), true),
                Done::Pasted(Some(pasted_text())),
                copied(Clip::Text(BEFORE.into()), true),
            ],
            "{}: {what}",
            fixture.title
        );
        pane.closed(&[], what);
        assert_eq!(pane.system.clipboard(), Some(Clip::Text(BEFORE.into())));
    }
}

fn a_clipboard_changed_meanwhile_is_not_put_back(fixture: &Fixture) {
    let meanwhile = Clip::Text("Copied by another program".into());
    let pane = Pane::with(fixture, |system| {
        system.support_paste();
        system.set_clipboard(Some(Clip::Text(BEFORE.into())));
        system.copy_while_pasting(meanwhile.clone());
    });
    pane.act(PASTE);
    assert_eq!(
        pane.system.take(),
        [
            copied(pasted_text(), true),
            Done::Pasted(Some(pasted_text()))
        ],
        "{}",
        fixture.title
    );
    pane.closed(&[], "Paste");
    assert_eq!(pane.system.clipboard(), Some(meanwhile));
}

fn a_failed_paste_is_an_error_not_a_copy(fixture: &Fixture) {
    let why = "The application refused the paste";
    let pane = Pane::with(fixture, |system| {
        system.support_paste();
        system.fail_paste(why);
        system.set_clipboard(Some(Clip::Text(BEFORE.into())));
    });
    pane.act(PASTE);
    assert_eq!(
        pane.system.take(),
        [
            copied(pasted_text(), true),
            Done::Pasted(Some(pasted_text())),
            copied(Clip::Text(BEFORE.into()), true),
        ],
        "{}: no copy instead",
        fixture.title
    );
    // The window had closed for the paste: the failure is said in a HUD.
    pane.closed(
        &[Hud {
            title: format!("The extension reported an error: {why}"),
            style: ToastStyle::Failure,
        }],
        "a failed Paste",
    );
}

fn where_paste_is_not_available_paste_copies_and_says_so(fixture: &Fixture) {
    let pane = Pane::with(fixture, |system| {
        system.set_clipboard(Some(Clip::Text(BEFORE.into())));
    });
    // The standard Paste copies instead, closes the window and says so.
    pane.act(PASTE);
    assert_eq!(
        pane.system.take(),
        [copied(pasted_text(), false)],
        "{}",
        fixture.title
    );
    pane.closed(
        &[Hud {
            title: PASTE_FALLBACK.into(),
            style: ToastStyle::Success,
        }],
        "Paste where it is not available",
    );

    // The host function itself answers "not available", which is not a
    // failure: the window stays, and nothing pauses the package however
    // often it is asked.
    for _ in 0..4 {
        pane.act(PASTE_DIRECTLY);
        assert_eq!(pane.system.take(), [], "nothing pasted or copied");
        pane.stayed("Paste Directly where it is not available");
        assert_eq!(shown(&pane.launcher), not_available(NOT_YET));
    }
    assert_eq!(pane.launcher.view().screen, Screen::Command);
}

fn front_application_answers_its_name_and_icon_or_none(fixture: &Fixture) {
    let icon = r"C:\Windows\System32\notepad.exe";
    let pane = Pane::with(fixture, |system| {
        system.set_front_application(Ok(Some(FrontApplication {
            name: "Notepad".into(),
            icon: Some(icon.into()),
        })));
    });
    assert_eq!(
        pane.titles(),
        [
            "Paste",
            "Paste to Notepad",
            "Paste Directly",
            "Front Application",
            "Search Selection"
        ],
        "{}",
        fixture.title
    );
    pane.act(FRONT_APPLICATION);
    assert_eq!(
        shown(&pane.launcher),
        Status::Result(format!("Front application: Notepad, icon {icon}"))
    );

    pane.system.set_front_application(Ok(Some(FrontApplication {
        name: "Calculator".into(),
        icon: None,
    })));
    pane.act(FRONT_APPLICATION);
    assert_eq!(
        shown(&pane.launcher),
        Status::Result("Front application: Calculator, icon none".into())
    );

    pane.system.set_front_application(Ok(None));
    pane.act(FRONT_APPLICATION);
    assert_eq!(
        shown(&pane.launcher),
        Status::Result("No application is in front".into())
    );

    pane.system.set_front_application(Err(SystemError::Failed(
        "The window in front could not be read".into(),
    )));
    pane.act(FRONT_APPLICATION);
    assert_eq!(
        shown(&pane.launcher),
        failure("The window in front could not be read")
    );

    pane.system
        .set_front_application(Err(SystemError::NotAvailable(NOT_YET.into())));
    pane.act(FRONT_APPLICATION);
    assert_eq!(shown(&pane.launcher), not_available(NOT_YET));
    pane.stayed("reading the front application");
    assert_eq!(pane.system.take(), []);

    // With no application in front, Paste is titled for the active one.
    let pane = Pane::with(fixture, |system| system.set_front_application(Ok(None)));
    assert_eq!(pane.titles()[PASTE_TO], "Paste to Active App");
}

fn selected_text_tells_nothing_selected_from_a_failure(fixture: &Fixture) {
    let pane = Pane::with(fixture, |system| {
        system.set_selected_text(Ok(Some("pane launcher".into())));
    });
    pane.act(SEARCH_SELECTION);
    assert_eq!(
        pane.system.take(),
        [Done::Opened {
            target: "https://www.google.com/search?q=pane%20launcher".into(),
            application: None,
        }],
        "{}",
        fixture.title
    );
    assert_eq!(
        shown(&pane.launcher),
        Status::Result("Searched for “pane launcher”".into())
    );

    for nothing in [None, Some(String::new())] {
        pane.system.set_selected_text(Ok(nothing));
        pane.act(SEARCH_SELECTION);
        assert_eq!(
            shown(&pane.launcher),
            Status::Error("Nothing is selected".into())
        );
    }

    pane.system.set_selected_text(Err(SystemError::Failed(
        "The selection could not be read".into(),
    )));
    pane.act(SEARCH_SELECTION);
    assert_eq!(
        shown(&pane.launcher),
        failure("The selection could not be read")
    );

    pane.system
        .set_selected_text(Err(SystemError::NotAvailable(NOT_YET.into())));
    pane.act(SEARCH_SELECTION);
    assert_eq!(shown(&pane.launcher), not_available(NOT_YET));

    assert_eq!(pane.system.take(), [], "only the selection was searched");
    pane.stayed("reading the selected text");
}

fn the_real_adapters_say_each_is_not_available_on_this_system_yet(fixture: &Fixture) {
    let pane = Pane::native(fixture);
    let system = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let not_yet = |what: &str| not_available(&format!("{what} is not available on {system} yet"));
    assert_eq!(
        pane.titles()[PASTE_TO],
        "Paste to Active App",
        "{}",
        fixture.title
    );
    // Not the standard Paste: its fallback would copy to the real
    // clipboard.
    pane.act(PASTE_DIRECTLY);
    assert_eq!(
        shown(&pane.launcher),
        not_yet("Pasting into another application")
    );
    pane.stayed("Paste Directly");
    pane.act(FRONT_APPLICATION);
    assert_eq!(
        shown(&pane.launcher),
        not_yet("Reading the application in front")
    );
    pane.act(SEARCH_SELECTION);
    assert_eq!(shown(&pane.launcher), not_yet("Reading the selected text"));
    pane.stayed("reading the selection");
    assert_eq!(pane.system.take(), [], "the recording system was not given");
    assert_eq!(pane.launcher.view().screen, Screen::Command);
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
    paste_closes_the_window_pastes_and_puts_the_clipboard_back,
    a_clipboard_changed_meanwhile_is_not_put_back,
    a_failed_paste_is_an_error_not_a_copy,
    where_paste_is_not_available_paste_copies_and_says_so,
    front_application_answers_its_name_and_icon_or_none,
    selected_text_tells_nothing_selected_from_a_failure,
    the_real_adapters_say_each_is_not_available_on_this_system_yet,
);
