//! The system host functions and the standard actions built from them
//! (#145, ADR 0037), through the launcher's public interface with the
//! actions sample in Rust, JavaScript and TypeScript, real guests `cargo
//! xtask guests` assembles, a recording window and a recording system
//! (`support/system.rs`, passed in as the link opener and the clipboard
//! are): copy puts text or a file on the clipboard, concealed or not;
//! read-clipboard answers text, a file or nothing; open takes any URL
//! scheme, a file, a folder or an application, with or without a named
//! application; reveal shows a path; trash moves paths and names those it
//! could not; none of them closes the window. The standard actions (Copy,
//! Open, Open With…, Show in Explorer, Move to Recycle Bin) act, then close
//! the window, Copy and Move to Recycle Bin with a HUD, or keep it open
//! with a toast. A launcher given no system answers each function clearly,
//! and that is never a reason to pause the package. Prior art:
//! `quicklinks.rs` (a recording opener) and `feedback.rs` (the window).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::system::Clip;
use pane_core::{Hud, Launcher, Runtime, Screen, Status, SubmenuState, ToastStyle, WindowPresence};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/system.rs"]
mod recording;
#[path = "support/rows.rs"]
mod rows;

use feedback::{RecordingWindow, shown};
use recording::{Done, KEPT, RecordingSystem};
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

/// The "System" item's actions, by their place.
const COPY_TEXT: usize = 0;
const COPY_TEXT_CONCEALED: usize = 1;
const COPY_FILE: usize = 2;
const COPY_FILE_CONCEALED: usize = 3;
const READ_CLIPBOARD: usize = 4;
const OPEN_WEBSITE: usize = 5;
const OPEN_MAIL: usize = 6;
const OPEN_SETTINGS: usize = 7;
const OPEN_FILE: usize = 8;
const OPEN_FOLDER: usize = 9;
const OPEN_APPLICATION: usize = 10;
const OPEN_FILE_WITH_APPLICATION: usize = 11;
const REVEAL_FILE: usize = 12;
const TRASH_FILES: usize = 13;

/// The "Standard actions" item's actions, by their place.
const COPY: usize = 0;
const COPY_PASSWORD: usize = 1;
const COPY_KEEPING_OPEN: usize = 2;
const COPY_A_FILE: usize = 3;
const OPEN: usize = 4;
const OPEN_WITH: usize = 5;
const SHOW: usize = 6;
const MOVE_TO_TRASH: usize = 7;

/// What the sample copies, and the secret it copies concealed.
const COPIED_TEXT: &str = "Copied by the actions sample";
const SECRET: &str = "hunter2";

/// Where the sample points on this system, as `places` in the samples.
struct Places {
    file: &'static str,
    folder: &'static str,
    application: &'static str,
    trash: [&'static str; 2],
}

fn places() -> Places {
    if cfg!(target_os = "windows") {
        Places {
            file: r"C:\Windows\win.ini",
            folder: r"C:\Windows",
            application: r"C:\Windows\System32\notepad.exe",
            trash: [
                r"C:\pane-sample\Delete me.txt",
                r"C:\pane-sample\Keep me.txt",
            ],
        }
    } else if cfg!(target_os = "macos") {
        Places {
            file: "/etc/hosts",
            folder: "/Applications",
            application: "/System/Applications/TextEdit.app",
            trash: [
                "/tmp/pane-sample/Delete me.txt",
                "/tmp/pane-sample/Keep me.txt",
            ],
        }
    } else {
        Places {
            file: "/etc/hosts",
            folder: "/tmp",
            application: "/usr/bin/xdg-open",
            trash: [
                "/tmp/pane-sample/Delete me.txt",
                "/tmp/pane-sample/Keep me.txt",
            ],
        }
    }
}

/// The file manager's name on this system, as the standard action says it.
fn file_manager() -> &'static str {
    if cfg!(target_os = "windows") {
        "Explorer"
    } else if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    }
}

/// The trash's name on this system.
fn trash_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Recycle Bin"
    } else {
        "Trash"
    }
}

fn copied(clip: Clip, concealed: bool) -> Done {
    Done::Copied { clip, concealed }
}

fn opened(target: &str, application: Option<&str>) -> Done {
    Done::Opened {
        target: target.into(),
        application: application.map(str::to_owned),
    }
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
    /// Pane with a recording system.
    fn with(fixture: &Fixture) -> Pane {
        Pane::start(fixture, true)
    }

    /// Pane given no system: each system function says so.
    fn without_system(fixture: &Fixture) -> Pane {
        Pane::start(fixture, false)
    }

    fn start(fixture: &Fixture, with_system: bool) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let system = Arc::new(RecordingSystem::default());
        let runtime = Runtime::start().unwrap();
        runtime.set_applications(system.clone());
        let mut launcher =
            Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"));
        if with_system {
            launcher = launcher.with_system(system.clone());
        }
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

    /// Runs the action at `index` of the item `item` ("system",
    /// "standard"), as the Actions panel chooses it.
    fn act(&self, item: &str, index: usize) {
        select_title(&self.launcher, title_of(item));
        block_on(self.launcher.run_item_action(item, index));
    }

    /// Checks that the last action closed the window, then showed `hud`
    /// (if any), and summons the window again, as the user would.
    fn closed(&self, hud: Option<&str>, what: &str) {
        let requests = self.window.take();
        assert_eq!(
            requests.first(),
            Some(&WindowRequest::Hide),
            "{what}: {requests:?}"
        );
        let huds: Vec<&Hud> = requests
            .iter()
            .filter_map(|request| match request {
                WindowRequest::Hud(hud) => Some(hud),
                _ => None,
            })
            .collect();
        let expected = hud.map(|title| Hud::new(ToastStyle::Success, title));
        assert_eq!(huds, expected.iter().collect::<Vec<_>>(), "{what}");
        assert_eq!(self.launcher.window_presence(), WindowPresence::Hidden);
        assert_eq!(self.launcher.view().screen, Screen::Command, "{what}");
        self.launcher.set_window_presence(WindowPresence::Shown);
    }
}

/// The title of the sample's item `id`.
fn title_of(id: &str) -> &'static str {
    match id {
        "system" => "System",
        "standard" => "Standard actions",
        other => panic!("no item {other}"),
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

fn each_system_function_does_only_what_it_names(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let places = places();
    let file = || Clip::File(PathBuf::from(places.file));
    for (index, title, done) in [
        (
            COPY_TEXT,
            "Copy Text",
            copied(Clip::Text(COPIED_TEXT.into()), false),
        ),
        (
            COPY_TEXT_CONCEALED,
            "Copy Text Concealed",
            copied(Clip::Text(SECRET.into()), true),
        ),
        (COPY_FILE, "Copy File", copied(file(), false)),
        (
            COPY_FILE_CONCEALED,
            "Copy File Concealed",
            copied(file(), true),
        ),
        (
            OPEN_WEBSITE,
            "Open Website",
            opened("https://example.com", None),
        ),
        (
            OPEN_MAIL,
            "Open Mail",
            opened("mailto:someone@example.com", None),
        ),
        (
            OPEN_SETTINGS,
            "Open Settings",
            opened("ms-settings:display", None),
        ),
        (OPEN_FILE, "Open File", opened(places.file, None)),
        (OPEN_FOLDER, "Open Folder", opened(places.folder, None)),
        (
            OPEN_APPLICATION,
            "Open Application",
            opened(places.application, None),
        ),
        (
            OPEN_FILE_WITH_APPLICATION,
            "Open File With Application",
            opened(places.file, Some(places.application)),
        ),
        (
            REVEAL_FILE,
            "Reveal File",
            Done::Revealed(PathBuf::from(places.file)),
        ),
    ] {
        pane.act("system", index);
        assert_eq!(pane.system.take(), [done], "{}: {title}", fixture.title);
        assert_eq!(
            shown(&pane.launcher),
            Status::Result(format!("{title}: done")),
            "{}",
            fixture.title
        );
    }
    // Each did only what it named: the window stayed, and no HUD showed.
    assert_eq!(pane.window.take(), []);
    assert_eq!(pane.launcher.window_presence(), WindowPresence::Shown);
}

fn the_clipboard_reads_as_text_a_file_or_nothing(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let file = places().file;
    let read = || {
        pane.act("system", READ_CLIPBOARD);
        shown(&pane.launcher)
    };

    pane.system.set_clipboard(Some(Clip::Text("Hello".into())));
    assert_eq!(
        read(),
        Status::Result("Clipboard: text “Hello”".into()),
        "{}",
        fixture.title
    );
    pane.system
        .set_clipboard(Some(Clip::File(PathBuf::from(file))));
    assert_eq!(read(), Status::Result(format!("Clipboard: file {file}")));
    pane.system.set_clipboard(None);
    assert_eq!(read(), Status::Result("Clipboard: empty".into()));

    // What the command copied reads back, concealed or not.
    pane.act("system", COPY_TEXT_CONCEALED);
    assert_eq!(
        read(),
        Status::Result(format!("Clipboard: text “{SECRET}”"))
    );
    assert_eq!(
        pane.system.take(),
        [copied(Clip::Text(SECRET.into()), true)],
        "reading changes nothing"
    );
    assert_eq!(pane.window.take(), []);
}

fn trash_moves_paths_and_names_those_it_could_not(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let [moved, kept] = places().trash;
    pane.act("system", TRASH_FILES);
    assert_eq!(
        pane.system.take(),
        [Done::Trashed(vec![
            PathBuf::from(moved),
            PathBuf::from(kept)
        ])],
        "{}",
        fixture.title
    );
    assert!(kept.ends_with(KEPT.0));
    assert_eq!(
        shown(&pane.launcher),
        Status::Error(format!(
            "The extension reported an error: Could not move 1 of 2 items to the {}: {kept}: {}",
            trash_name(),
            KEPT.1
        ))
    );
    assert_eq!(pane.window.take(), [], "the window stays");
}

fn the_standard_actions_act_then_close_the_window(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let places = places();
    select_title(&pane.launcher, "Standard actions");
    let listed = pane
        .launcher
        .item_actions()
        .expect("the standard actions are listed");
    let titles: Vec<&str> = listed
        .actions
        .iter()
        .map(|action| action.title.as_str())
        .collect();
    let show = format!("Show in {}", file_manager());
    let move_to_trash = format!("Move to {}", trash_name());
    assert_eq!(
        titles,
        [
            "Copy to Clipboard",
            "Copy Password",
            "Copy and Keep Open",
            "Copy File",
            "Open",
            "Open With…",
            show.as_str(),
            move_to_trash.as_str(),
        ],
        "{}",
        fixture.title
    );
    assert!(listed.actions[OPEN_WITH].submenu, "Open With… is a submenu");
    assert!(listed.actions[MOVE_TO_TRASH].destructive);
    assert!(
        listed
            .actions
            .iter()
            .enumerate()
            .all(|(index, action)| action.destructive == (index == MOVE_TO_TRASH))
    );

    // Copy, the primary action: copied, the window closed, then the HUD.
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.system.take(),
        [copied(Clip::Text(COPIED_TEXT.into()), false)]
    );
    pane.closed(Some("Copied to Clipboard"), "Copy");

    pane.act("standard", COPY_PASSWORD);
    assert_eq!(
        pane.system.take(),
        [copied(Clip::Text(SECRET.into()), true)]
    );
    pane.closed(Some("Copied to Clipboard"), "a concealed Copy");

    pane.act("standard", COPY_A_FILE);
    assert_eq!(
        pane.system.take(),
        [copied(Clip::File(PathBuf::from(places.file)), false)]
    );
    pane.closed(Some("Copied to Clipboard"), "Copy of a file");

    pane.act("standard", OPEN);
    assert_eq!(pane.system.take(), [opened("https://example.com", None)]);
    pane.closed(None, "Open");

    pane.act("standard", SHOW);
    assert_eq!(
        pane.system.take(),
        [Done::Revealed(PathBuf::from(places.file))]
    );
    pane.closed(None, "Show in Explorer");

    pane.act("standard", MOVE_TO_TRASH);
    assert_eq!(
        pane.system.take(),
        [Done::Trashed(vec![PathBuf::from(places.trash[0])])]
    );
    pane.closed(
        Some(&format!("Moved to {}", trash_name())),
        "Move to Recycle Bin",
    );
}

fn a_standard_action_kept_open_says_it_in_a_toast(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    pane.act("standard", COPY_KEEPING_OPEN);
    assert_eq!(
        pane.system.take(),
        [copied(Clip::Text(COPIED_TEXT.into()), false)],
        "{}",
        fixture.title
    );
    assert_eq!(pane.window.take(), [], "no close, no HUD");
    assert_eq!(pane.launcher.window_presence(), WindowPresence::Shown);
    assert_eq!(
        shown(&pane.launcher),
        Status::Result("Copied to Clipboard".into())
    );
}

fn open_with_lists_the_installed_applications_and_opens_with_the_one_chosen(fixture: &Fixture) {
    let pane = Pane::with(fixture);
    let file = places().file;
    select_title(&pane.launcher, "Standard actions");
    block_on(pane.launcher.open_submenu("standard", OPEN_WITH));
    let submenu = pane.launcher.submenu().expect("Open With… is open");
    assert_eq!(submenu.title, "Open With", "{}", fixture.title);
    assert!(
        matches!(submenu.state, SubmenuState::Listed(_)),
        "{:?}",
        submenu.state
    );
    let names: Vec<&str> = submenu
        .entries()
        .iter()
        .map(|entry| entry.title.as_str())
        .collect();
    // By name, whatever order the system gave them in.
    assert_eq!(names, ["code editor", "Notepad", "Zed"]);
    assert_eq!(pane.system.take(), [], "listing opens nothing");

    block_on(pane.launcher.run_submenu_entry("standard", 1));
    assert_eq!(pane.system.take(), [opened(file, Some("app:Notepad"))]);
    pane.closed(None, "Open With Notepad");
    assert_eq!(pane.launcher.submenu(), None, "the submenu closed");
}

fn without_a_system_each_function_says_so_and_nothing_pauses(fixture: &Fixture) {
    let pane = Pane::without_system(fixture);
    let why = "Not available: this Pane does not reach the system's clipboard, open things or \
               recycle them";
    // More times than crashes would take to pause the package: a refusal
    // is an answer, never a failure of the package's code.
    for _ in 0..4 {
        pane.act("system", COPY_TEXT);
        assert_eq!(
            shown(&pane.launcher),
            Status::Error(format!("The extension reported an error: {why}")),
            "{}",
            fixture.title
        );
    }
    pane.act("system", REVEAL_FILE);
    assert_eq!(
        shown(&pane.launcher),
        Status::Error(format!("The extension reported an error: {why}"))
    );
    let [first, second] = places().trash;
    pane.act("system", TRASH_FILES);
    assert_eq!(
        shown(&pane.launcher),
        Status::Error(format!(
            "The extension reported an error: Could not move 2 of 2 items to the {}: \
             {first}: {why}; {second}: {why}",
            trash_name()
        ))
    );
    // A standard action fails before it closes anything.
    pane.act("standard", COPY);
    assert_eq!(
        shown(&pane.launcher),
        Status::Error(format!("The extension reported an error: {why}"))
    );
    assert_eq!(pane.window.take(), []);
    assert_eq!(pane.system.take(), [], "the recording system was not given");

    // The package still runs: its other actions answer.
    pane.act("standard", COPY_KEEPING_OPEN);
    assert_eq!(
        shown(&pane.launcher),
        Status::Error(format!("The extension reported an error: {why}"))
    );
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
    each_system_function_does_only_what_it_names,
    the_clipboard_reads_as_text_a_file_or_nothing,
    trash_moves_paths_and_names_those_it_could_not,
    the_standard_actions_act_then_close_the_window,
    a_standard_action_kept_open_says_it_in_a_toast,
    open_with_lists_the_installed_applications_and_opens_with_the_one_chosen,
    without_a_system_each_function_says_so_and_nothing_pauses,
);
