//! Quicklinks, a default extension in Raycast's shape (#149), through the
//! launcher's public interface with the real package `cargo xtask guests`
//! assembles in `target/guests/packages/quicklinks`, a recording system for
//! opening and the clipboard and a recording window (`support/system.rs`,
//! `support/feedback.rs`), so that nothing opens and no clipboard changes:
//!
//! - its four commands are listed with their modes: Search Quicklinks and
//!   Create Quicklink open a screen, Import and Export Quicklinks run;
//! - Create Quicklink is a form, which takes a link of any scheme, a file, a
//!   folder or an application, with an optional application to open it
//!   with, and refuses an empty or malformed target on its field;
//! - Search Quicklinks lists each quicklink with its icon, name and target,
//!   and the actions Open, Open With…, Copy Link, Edit, Duplicate and
//!   Delete, which open, copy (closing the window, Copy Link with a HUD),
//!   open the form filled in, and delete once confirmed;
//! - quicklinks are indexed results root search ranks with commands, opened
//!   through the system's `open`, with or without their application, and a
//!   quick slot holds one by its identity, also once it is renamed;
//! - Export copies them as JSON with a HUD, Import adds the new ones and
//!   says how many it added and skipped, or why it could not;
//! - the quicklinks the first version saved survive the upgrade, and so
//!   does a quick slot pinning its command.
//!
//! Prior art: the first version's quicklinks suite, `quick_slots.rs` and
//! `system.rs`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::feedback::WindowRequest;
use pane_core::system::Clip;
use pane_core::{
    ConfirmAnswer, Hud, IconSource, Launcher, ResultAction, RowKind, Runtime, Screen, SlotChange,
    Status, SubmenuState, ToastStyle, WindowPresence,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/system.rs"]
mod recording;
#[path = "support/rows.rs"]
mod rows;

use feedback::{RecordingWindow, shown};
use recording::{Done, RecordingSystem};
use rows::{select_title, titles};

/// How long a guest may take to answer: compiling it once is included.
const PROMPTLY: Duration = Duration::from_secs(60);

/// The actions of a quicklink's item, by their place.
const OPEN: usize = 0;
const OPEN_WITH: usize = 1;
const COPY_LINK: usize = 2;
const EDIT: usize = 3;
const DUPLICATE: usize = 4;
const DELETE: usize = 5;

/// The assembled default extension.
fn package() -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/quicklinks");
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// Where this system keeps a file, a folder and an application the tests
/// name (the recording system opens nothing, so they need not exist).
struct Places {
    file: &'static str,
    folder: &'static str,
    application: &'static str,
}

fn places() -> Places {
    if cfg!(target_os = "windows") {
        Places {
            file: r"C:\Windows\win.ini",
            folder: r"C:\Windows",
            application: r"C:\Windows\System32\notepad.exe",
        }
    } else if cfg!(target_os = "macos") {
        Places {
            file: "/etc/hosts",
            folder: "/Applications",
            application: "/System/Applications/TextEdit.app",
        }
    } else {
        Places {
            file: "/etc/hosts",
            folder: "/tmp",
            application: "/usr/bin/xdg-open",
        }
    }
}

fn opened(target: &str, application: Option<&str>) -> Done {
    Done::Opened {
        target: target.into(),
        application: application.map(str::to_owned),
    }
}

/// Pane's data location for one test, which outlives restarts, and the
/// recording system every start of it is given.
struct Pane {
    data: TempDir,
    system: Arc<RecordingSystem>,
}

/// One start of Pane: its launcher and the recording window attached.
struct Started {
    launcher: Launcher,
    window: Arc<RecordingWindow>,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            data: tempfile::tempdir().unwrap(),
            system: Arc::new(RecordingSystem::default()),
        }
    }

    /// Starts Pane on this data location, as after a restart.
    fn start(&self) -> Started {
        let runtime = Runtime::start().unwrap();
        runtime.set_applications(self.system.clone());
        let launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
                .with_system(self.system.clone())
                .with_quick_slots(self.data.path());
        let window = RecordingWindow::attach(&launcher);
        Started { launcher, window }
    }

    /// Starts Pane and installs Quicklinks.
    fn with_quicklinks(&self) -> Started {
        let started = self.start();
        block_on(started.launcher.install_package(&package()));
        assert!(
            matches!(started.launcher.view().status, Status::Result(_)),
            "{:?}",
            started.launcher.view().status
        );
        to_root(&started.launcher);
        started
    }

    /// The installed Quicklinks' identity key.
    fn key(&self, launcher: &Launcher) -> String {
        launcher.packages()[0].identity.key()
    }
}

/// Back to root search, wherever `launcher` is.
fn to_root(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Waits until the commands a command launched have run or opened.
fn launches_done(launcher: &Launcher) {
    assert!(launcher.wait_for_launches(PROMPTLY), "a launch did not end");
}

/// Types `query` into root search and chooses the row titled `title`,
/// waiting for what it does.
fn choose(launcher: &Launcher, query: &str, title: &str) {
    to_root(launcher);
    search(launcher, query);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// The open form's title and its fields' values, by id.
fn form(launcher: &Launcher) -> (String, Vec<(String, String)>) {
    let view = launcher.view();
    let form = view.form().expect("a form is open");
    let values = form
        .fields
        .iter()
        .map(|field| (field.id.clone(), field.value.clone()))
        .collect();
    (view.title.clone(), values)
}

/// Fills in the open form's fields and submits it; what the user reads.
fn submit(launcher: &Launcher, values: &[(&str, &str)]) -> Status {
    for (field, value) in values {
        launcher.set_field_value(field, value);
    }
    block_on(launcher.submit_form());
    launches_done(launcher);
    shown(launcher)
}

/// Opens Create Quicklink from root search.
fn open_create(launcher: &Launcher) {
    choose(launcher, "create quicklink", "Create Quicklink");
    assert_eq!(form(launcher).0, "Create Quicklink");
}

/// Creates the quicklink `name` for `link`, opening with `application`
/// (empty for the system's handler), through Create Quicklink.
fn create(launcher: &Launcher, name: &str, link: &str, application: &str) {
    open_create(launcher);
    let said = submit(
        launcher,
        &[("name", name), ("link", link), ("application", application)],
    );
    assert_eq!(said, Status::Result(format!("Created “{name}”")));
    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "back to root search: {:?}",
        launcher.view().screen
    );
}

/// Opens Search Quicklinks from root search.
fn open_search(launcher: &Launcher) {
    choose(launcher, "search quicklinks", "Search Quicklinks");
    let view = launcher.view();
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Quicklinks"),
        "{:?}",
        view.status
    );
}

/// Selects the quicklink titled `title` in Search Quicklinks; its item id.
fn select_link(launcher: &Launcher, title: &str) -> String {
    select_title(launcher, title);
    let view = launcher.view();
    view.rows[view.selected.unwrap()].id.clone()
}

/// A call that may wait on the user, running on a thread of its own.
struct Running(thread::JoinHandle<()>);

impl Running {
    fn ended(self) {
        let started = Instant::now();
        while !self.0.is_finished() {
            assert!(started.elapsed() < PROMPTLY, "the call did not end");
            thread::sleep(Duration::from_millis(5));
        }
        self.0.join().unwrap();
    }
}

/// Runs the action at `index` of the quicklink titled `title`, on a thread
/// of its own.
fn act(launcher: &Launcher, title: &str, index: usize) -> Running {
    let item = select_link(launcher, title);
    let running = launcher.run_item_action(&item, index);
    Running(thread::spawn(move || block_on(running)))
}

/// Runs the action at `index` of the quicklink titled `title` and waits
/// for it, and for what it launched.
fn run(launcher: &Launcher, title: &str, index: usize) {
    act(launcher, title, index).ended();
    launches_done(launcher);
}

/// The confirmation the launcher shows, once the command asked for it.
fn asked(launcher: &Launcher) -> pane_core::Confirmation {
    let started = Instant::now();
    loop {
        if let Some(confirmation) = launcher.confirmation() {
            return confirmation;
        }
        assert!(started.elapsed() < PROMPTLY, "no confirmation was asked");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Checks that the window was closed (then showed `hud`, if any), and
/// shows it again, as the user would summon it.
fn closed(started: &Started, hud: Option<&str>, what: &str) {
    let requests = started.window.take();
    assert_eq!(
        requests.first(),
        Some(&WindowRequest::Hide),
        "{what}: {requests:?}"
    );
    let huds: Vec<Hud> = requests
        .iter()
        .filter_map(|request| match request {
            WindowRequest::Hud(hud) => Some(hud.clone()),
            _ => None,
        })
        .collect();
    let expected: Vec<Hud> = hud
        .map(|title| Hud {
            title: title.into(),
            style: ToastStyle::Success,
        })
        .into_iter()
        .collect();
    assert_eq!(huds, expected, "{what}");
    assert_eq!(started.launcher.window_presence(), WindowPresence::Hidden);
    started.launcher.set_window_presence(WindowPresence::Shown);
}

#[test]
fn its_four_commands_are_listed_with_their_modes() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();

    search(&launcher, "quicklink");
    let listed = titles(&launcher);
    for (title, action) in [
        ("Search Quicklinks", "Open command"),
        ("Create Quicklink", "Open command"),
        ("Import Quicklinks", "Run command"),
        ("Export Quicklinks", "Run command"),
    ] {
        assert!(listed.iter().any(|row| row == title), "{title}: {listed:?}");
        select_title(&launcher, title);
        assert_eq!(launcher.selected_action().label, action, "{title}");
    }
    // The first version's single command is gone.
    assert!(!listed.iter().any(|row| row == "Quicklinks"), "{listed:?}");

    // With none saved, Search Quicklinks offers to create one.
    open_search(&launcher);
    assert_eq!(titles(&launcher), ["Create Quicklink"]);
    block_on(launcher.activate_selected());
    launches_done(&launcher);
    assert_eq!(form(&launcher).0, "Create Quicklink");
}

#[test]
fn create_quicklink_is_a_form_that_saves_and_returns_to_root_search() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();

    open_create(&launcher);
    let (_, values) = form(&launcher);
    assert_eq!(
        values,
        [
            ("name".to_owned(), String::new()),
            ("link".to_owned(), String::new()),
            ("application".to_owned(), String::new()),
        ],
        "a new quicklink's form starts empty"
    );
    // Back leaves the command for root search.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));

    create(
        &launcher,
        "Pane issues",
        "https://github.com/pane-app/pane/issues",
        "",
    );
    search(&launcher, "pane iss");
    assert_eq!(titles(&launcher), ["Pane issues"]);
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("https://github.com/pane-app/pane/issues")
    );
}

#[test]
fn the_form_refuses_empty_and_malformed_targets_on_their_fields() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com", "");
    open_create(&launcher);

    let link_message = "Enter a link with its scheme, such as https:// or mailto:, or the full \
                        path of a file, folder or application";
    let rejected: [((&str, &str, &str), String); 10] = [
        (("", "https://example.com", ""), "Name: Enter a name".into()),
        (
            ("Example", "", ""),
            "Link: Enter a link, or the path of a file, folder or application".into(),
        ),
        (
            ("Example", "example.com", ""),
            format!("Link: {link_message}"),
        ),
        (
            ("Example", "docs/readme.txt", ""),
            format!("Link: {link_message}"),
        ),
        (
            ("Example", "https://", ""),
            "Link: The address has no host".into(),
        ),
        (
            ("Example", "mailto:", ""),
            "Link: Enter what the link opens after “mailto:”".into(),
        ),
        (
            ("Example", "https://exa mple.com", ""),
            "Link: A link cannot contain spaces".into(),
        ),
        (
            ("docs", "https://example.com", ""),
            "Name: A quicklink named “Docs” already exists".into(),
        ),
        (
            ("Example", "https://example.com", "Nothing installed"),
            "Open With: No installed application is named “Nothing installed”; enter its \
             name as Pane lists it, or its full path"
                .into(),
        ),
        (
            ("Example", "https://example.com", "app"),
            "Open With: No installed application is named “app”; enter its name as Pane \
             lists it, or its full path"
                .into(),
        ),
    ];
    for ((name, link, application), error) in rejected {
        let said = submit(
            &launcher,
            &[("name", name), ("link", link), ("application", application)],
        );
        assert_eq!(said, Status::Error(error), "{name} {link} {application}");
        assert!(launcher.view().form().is_some(), "the form stays open");
    }

    // Corrected, the same form saves; nothing refused was saved.
    let said = submit(
        &launcher,
        &[
            ("name", "Example"),
            ("link", "HTTPS://example.com/a?b=c#d"),
            ("application", "notepad"),
        ],
    );
    assert_eq!(said, Status::Result("Created “Example”".into()));
    open_search(&launcher);
    assert_eq!(titles(&launcher), ["Docs", "Example"]);
}

#[test]
fn each_quicklink_shows_its_icon_name_and_target_and_its_actions() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    let places = places();
    // A web address nothing answers at, so its favicon falls back.
    create(&launcher, "Local site", "http://127.0.0.1:9/", "");
    create(&launcher, "Mail", "mailto:someone@example.com", "");
    create(&launcher, "Hosts", places.file, "Zed");

    open_search(&launcher);
    let view = launcher.view();
    let rows: Vec<(&str, Option<&str>)> = view
        .rows
        .iter()
        .map(|row| (row.title.as_str(), row.subtitle.as_deref()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Local site", Some("http://127.0.0.1:9/")),
            ("Mail", Some("mailto:someone@example.com")),
            ("Hosts", Some(places.file)),
        ]
    );
    let presentation = launcher.presentation();
    let icons: Vec<IconSource> = presentation
        .rows
        .iter()
        .map(|row| row.icon.clone().expect("each row has an icon").source)
        .collect();
    // The favicon's and the file's fallbacks show while they load (and the
    // favicon's for good: nothing answers there); a link of another scheme
    // shows a link.
    assert!(
        matches!(&icons[0], IconSource::Builtin { name, .. } if name == "global")
            || matches!(&icons[0], IconSource::Image { .. }),
        "{:?}",
        icons[0]
    );
    assert!(
        matches!(&icons[1], IconSource::Builtin { name, .. } if name == "link"),
        "{:?}",
        icons[1]
    );
    assert!(
        matches!(&icons[2], IconSource::Builtin { name, .. } if name == "document")
            || matches!(&icons[2], IconSource::Image { .. }),
        "{:?}",
        icons[2]
    );
    // The one opening with an application names it.
    let accessories: Vec<Vec<String>> = presentation
        .rows
        .iter()
        .map(|row| {
            row.accessories
                .iter()
                .map(|shown| shown.text.clone())
                .collect()
        })
        .collect();
    assert_eq!(
        accessories,
        [vec![], vec![], vec!["Zed".to_owned()]],
        "{accessories:?}"
    );

    select_link(&launcher, "Mail");
    let listed = launcher.item_actions().expect("a quicklink has actions");
    let actions: Vec<(&str, Option<String>, bool, bool)> = listed
        .actions
        .iter()
        .map(|action| {
            (
                action.title.as_str(),
                action.shortcut.as_ref().map(|keys| keys.id()),
                action.destructive,
                action.submenu,
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            ("Open", None, false, false),
            ("Open With…", None, false, true),
            ("Copy Link", Some("ctrl-shift-c".to_owned()), false, false),
            ("Edit", Some("ctrl-e".to_owned()), false, false),
            ("Duplicate", None, false, false),
            ("Delete", None, true, false),
        ]
    );
    assert_eq!(launcher.selected_action().label, "Open", "Enter opens");
}

#[test]
fn open_open_with_and_copy_link_act_then_close_the_window() {
    let pane = Pane::new();
    let started = pane.with_quicklinks();
    let launcher = &started.launcher;
    let places = places();
    create(launcher, "Docs", "https://docs.example.com", "");
    create(launcher, "Hosts", places.file, "Notepad");
    open_search(launcher);
    started.window.take();
    pane.system.take();

    // Open, Enter: the system opens it, then the window closes.
    select_link(launcher, "Docs");
    block_on(launcher.activate_selected());
    assert_eq!(
        pane.system.take(),
        [opened("https://docs.example.com", None)]
    );
    closed(&started, None, "Open");

    // With the application it names.
    run(launcher, "Hosts", OPEN);
    assert_eq!(
        pane.system.take(),
        [opened(places.file, Some("app:Notepad"))]
    );
    closed(&started, None, "Open with its application");

    // Open With…: the installed applications, by name.
    let item = select_link(launcher, "Docs");
    block_on(launcher.open_submenu(&item, OPEN_WITH));
    let submenu = launcher.submenu().expect("Open With… is open");
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
    assert_eq!(names, ["code editor", "Notepad", "Zed"]);
    block_on(launcher.run_submenu_entry(&item, 2));
    assert_eq!(
        pane.system.take(),
        [opened("https://docs.example.com", Some("app:Zed"))]
    );
    closed(&started, None, "Open With Zed");

    // Copy Link: copied, the window closed, then the HUD.
    run(launcher, "Docs", COPY_LINK);
    assert_eq!(
        pane.system.take(),
        [Done::Copied {
            clip: Clip::Text("https://docs.example.com".into()),
            concealed: false,
        }]
    );
    closed(&started, Some("Copied to Clipboard"), "Copy Link");
}

#[test]
fn edit_opens_the_form_filled_in_and_saves_back_to_the_list() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    let places = places();
    create(&launcher, "Docs", "https://docs.example.com", "");
    create(&launcher, "News", "https://news.example.com", "");
    create(&launcher, "Editor", places.file, places.application);

    open_search(&launcher);
    run(&launcher, "Docs", EDIT);
    assert_eq!(
        form(&launcher),
        (
            "Edit “Docs”".to_owned(),
            vec![
                ("name".to_owned(), "Docs".to_owned()),
                ("link".to_owned(), "https://docs.example.com".to_owned()),
                ("application".to_owned(), String::new()),
            ]
        )
    );
    // Renaming onto another quicklink's name is refused.
    assert_eq!(
        submit(&launcher, &[("name", "news")]),
        Status::Error("Name: A quicklink named “News” already exists".into())
    );
    let said = submit(
        &launcher,
        &[("name", "Manual"), ("link", "https://docs.example.org")],
    );
    assert_eq!(said, Status::Result("Saved “Manual”".into()));
    // Back on the list, which shows the change.
    let view = launcher.view();
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Quicklinks")
    );
    assert_eq!(titles(&launcher), ["Manual", "News", "Editor"]);
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("https://docs.example.org")
    );

    // An application named by its path is filled in by its path.
    run(&launcher, "Editor", EDIT);
    let (_, values) = form(&launcher);
    assert_eq!(
        values[2],
        ("application".to_owned(), places.application.to_owned())
    );
    launcher.back();

    // Root search finds it by its new name only, and by its target.
    search(&launcher, "manual");
    assert_eq!(titles(&launcher), ["Manual"]);
    search(&launcher, "docs");
    assert_eq!(titles(&launcher), ["Manual"], "found by its target");
}

#[test]
fn duplicate_opens_the_form_filled_in_as_a_copy() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com", "Zed");

    open_search(&launcher);
    run(&launcher, "Docs", DUPLICATE);
    assert_eq!(
        form(&launcher),
        (
            "Duplicate Quicklink".to_owned(),
            vec![
                ("name".to_owned(), "Docs copy".to_owned()),
                ("link".to_owned(), "https://docs.example.com".to_owned()),
                ("application".to_owned(), "Zed".to_owned()),
            ]
        )
    );
    assert_eq!(
        submit(&launcher, &[]),
        Status::Result("Created “Docs copy”".into())
    );
    open_search(&launcher);
    assert_eq!(titles(&launcher), ["Docs", "Docs copy"]);
    // A second copy takes the next free name.
    run(&launcher, "Docs", DUPLICATE);
    assert_eq!(form(&launcher).1[0].1, "Docs copy 2");
}

#[test]
fn delete_is_destructive_and_deletes_only_once_confirmed() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com", "");
    create(&launcher, "News", "https://news.example.com", "");
    open_search(&launcher);

    let running = act(&launcher, "Docs", DELETE);
    let confirmation = asked(&launcher);
    assert_eq!(
        (
            confirmation.title.as_str(),
            confirmation.primary.as_str(),
            confirmation.destructive
        ),
        ("Delete “Docs”?", "Delete", true)
    );
    launcher.answer_confirmation(confirmation.id, ConfirmAnswer::Dismissed, false);
    running.ended();
    assert_eq!(titles(&launcher), ["Docs", "News"], "kept");

    let running = act(&launcher, "Docs", DELETE);
    let confirmation = asked(&launcher);
    launcher.answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
    running.ended();
    assert_eq!(titles(&launcher), ["News"]);
    assert_eq!(shown(&launcher), Status::Result("Deleted “Docs”".into()));
    to_root(&launcher);
    search(&launcher, "docs");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

#[test]
fn a_quicklink_of_any_target_opens_through_the_system_with_or_without_its_application() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    let places = places();
    for (name, link, application) in [
        ("Write mail", "mailto:someone@example.com", ""),
        ("Display settings", "ms-settings:display", ""),
        ("Hosts file", places.file, ""),
        ("System folder", places.folder, ""),
        ("Text editor", places.application, ""),
        ("Hosts in Zed", places.file, "zed"),
        ("Site in editor", "https://example.com", places.application),
    ] {
        create(&launcher, name, link, application);
    }
    pane.system.take();

    for (query, title, done) in [
        (
            "write mail",
            "Write mail",
            opened("mailto:someone@example.com", None),
        ),
        (
            "display settings",
            "Display settings",
            opened("ms-settings:display", None),
        ),
        ("hosts file", "Hosts file", opened(places.file, None)),
        (
            "system folder",
            "System folder",
            opened(places.folder, None),
        ),
        (
            "text editor",
            "Text editor",
            opened(places.application, None),
        ),
        (
            "hosts in zed",
            "Hosts in Zed",
            opened(places.file, Some("app:Zed")),
        ),
        (
            "site in editor",
            "Site in editor",
            opened("https://example.com", Some(places.application)),
        ),
    ] {
        choose(&launcher, query, title);
        assert_eq!(pane.system.take(), [done], "{title}");
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Opened {title}")),
            "{title}"
        );
        assert_eq!(launcher.view().query(), Some(query), "root search stays");
    }
}

#[test]
fn quicklinks_are_indexed_results_ranked_with_commands() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    create(
        &launcher,
        "Notes on exporting",
        "https://notes.example.com",
        "",
    );

    search(&launcher, "export");
    let listed = titles(&launcher);
    let command = listed.iter().position(|title| title == "Export Quicklinks");
    let quicklink = listed
        .iter()
        .position(|title| title == "Notes on exporting");
    assert!(
        matches!((command, quicklink), (Some(command), Some(quicklink)) if command < quicklink),
        "the better match first, whatever kind it is: {listed:?}"
    );
    let presentation = launcher.presentation();
    let row = &presentation.rows[quicklink.unwrap()];
    assert_eq!(row.kind, Some(RowKind::Link));
    assert!(row.answer.is_none(), "not a computed result");

    // Found by its target too, with the rows the address is below it
    // (#195); other words find nothing.
    search(&launcher, "notes.example");
    assert_eq!(
        titles(&launcher),
        ["Notes on exporting", "Open in Browser", "Create Quicklink"]
    );
    search(&launcher, "gitlab");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    // A blank query lists commands, not quicklinks.
    search(&launcher, "");
    assert!(!titles(&launcher).contains(&"Notes on exporting".to_owned()));
}

#[test]
fn a_pinned_quicklink_keeps_its_slot_through_a_restart_and_a_rename() {
    let pane = Pane::new();
    {
        let Started { launcher, .. } = pane.with_quicklinks();
        create(&launcher, "Docs", "https://docs.example.com", "");
        search(&launcher, "docs");
        select_title(&launcher, "Docs");
        let target = launcher.view().rows[launcher.view().selected.unwrap()]
            .id
            .clone();
        let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
        assert_eq!(change, SlotChange::Changed(Some(0)));
        block_on(recorded);

        // Renamed, it is still the one pinned.
        open_search(&launcher);
        run(&launcher, "Docs", EDIT);
        assert_eq!(
            submit(&launcher, &[("name", "Manual")]),
            Status::Result("Saved “Manual”".into())
        );
    }

    let Started { launcher, .. } = pane.start();
    let waiting = &launcher.quick_slots()[0];
    assert_eq!(
        waiting.unavailable.as_deref(),
        Some("Waiting for Search Quicklinks to list it")
    );
    block_on(launcher.resolve_quick_slots());
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "Manual");
    assert!(slot.ready());
    pane.system.take();
    block_on(launcher.activate_quick_slot(0));
    assert_eq!(
        pane.system.take(),
        [opened("https://docs.example.com", None)]
    );

    // Deleted, its slot stays and says why it cannot run.
    open_search(&launcher);
    let running = act(&launcher, "Manual", DELETE);
    let confirmation = asked(&launcher);
    launcher.answer_confirmation(confirmation.id, ConfirmAnswer::Confirmed, false);
    running.ended();
    to_root(&launcher);
    search(&launcher, "manual");
    let slot = &launcher.quick_slots()[0];
    assert_eq!(
        slot.unavailable.as_deref(),
        Some("Search Quicklinks no longer lists it")
    );
}

#[test]
fn export_copies_json_with_a_hud_and_import_adds_what_is_new() {
    let pane = Pane::new();
    let started = pane.with_quicklinks();
    let launcher = &started.launcher;
    create(launcher, "Docs", "https://docs.example.com", "");
    create(launcher, "Mail", "mailto:someone@example.com", "Zed");
    started.window.take();
    pane.system.take();

    choose(launcher, "export quicklinks", "Export Quicklinks");
    let copied = match pane.system.take().as_slice() {
        [
            Done::Copied {
                clip: Clip::Text(text),
                concealed: false,
            },
        ] => text.clone(),
        other => panic!("{other:?}"),
    };
    closed(&started, Some("Copied 2 quicklinks as JSON"), "Export");
    let exported: serde_json::Value = serde_json::from_str(&copied).unwrap();
    assert_eq!(
        exported,
        serde_json::json!([
            {"name": "Docs", "link": "https://docs.example.com"},
            {"name": "Mail", "link": "mailto:someone@example.com", "openWith": "app:Zed"},
        ])
    );

    // Into another Pane: every one is new.
    let other = Pane::new();
    let imported = other.with_quicklinks();
    other.system.set_clipboard(Some(Clip::Text(copied.clone())));
    choose(&imported.launcher, "import quicklinks", "Import Quicklinks");
    assert_eq!(
        shown(&imported.launcher),
        Status::Result("Added 2 quicklinks, skipped 0".into())
    );
    open_search(&imported.launcher);
    assert_eq!(titles(&imported.launcher), ["Docs", "Mail"]);
    run(&imported.launcher, "Mail", OPEN);
    assert_eq!(
        other.system.take(),
        [opened("mailto:someone@example.com", Some("app:Zed"))]
    );

    // Into this one: one new, one already saved, one not a quicklink.
    pane.system.set_clipboard(Some(Clip::Text(
        r#"[{"name": "docs", "link": "https://elsewhere.example.com"},
            {"name": "News", "link": "https://news.example.com"},
            {"name": "Broken"},
            {"name": "Spaced", "link": "https://exa mple.com"}]"#
            .into(),
    )));
    choose(launcher, "import quicklinks", "Import Quicklinks");
    assert_eq!(
        shown(launcher),
        Status::Result("Added 1 quicklink, skipped 3".into())
    );
    // Root search finds the new one from the next query.
    search(launcher, "news");
    assert_eq!(titles(launcher), ["News"]);
}

#[test]
fn import_answers_a_failure_toast_for_what_is_not_quicklinks() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();

    for (clipboard, message) in [
        (
            Some(Clip::Text("not JSON".into())),
            "The clipboard does not hold quicklinks as JSON: ",
        ),
        (
            Some(Clip::Text(r#"{"name": "Docs"}"#.into())),
            "The clipboard does not hold quicklinks as JSON: it is not a list of quicklinks",
        ),
        (
            None,
            "The clipboard holds no text: copy the quicklinks' JSON first",
        ),
        (
            Some(Clip::File(PathBuf::from(places().file))),
            "The clipboard holds no text: copy the quicklinks' JSON first",
        ),
    ] {
        pane.system.set_clipboard(clipboard);
        choose(&launcher, "import quicklinks", "Import Quicklinks");
        let toast = launcher.toast().expect("a toast").toast;
        assert_eq!(toast.style, ToastStyle::Failure);
        assert_eq!(toast.title, "Could not import quicklinks");
        assert!(
            toast
                .message
                .as_deref()
                .unwrap_or_default()
                .starts_with(message),
            "{:?}",
            toast.message
        );
    }
    open_search(&launcher);
    assert_eq!(titles(&launcher), ["Create Quicklink"], "nothing was added");
}

#[test]
fn quicklinks_the_first_version_saved_survive_the_upgrade_with_its_pin() {
    let pane = Pane::new();
    let key = {
        let Started { launcher, .. } = pane.with_quicklinks();
        pane.key(&launcher)
    };
    // What the first version saved: one line per quicklink, a name, a tab
    // and its address; and a quick slot pinning its one command.
    let mut packages = serde_json::Map::new();
    packages.insert(
        key.clone(),
        serde_json::json!({
            "quicklinks": "Docs\thttps://docs.example.com\nNews\thttps://news.example.com\n"
        }),
    );
    fs::write(
        pane.data.path().join("extensions").join("content.json"),
        serde_json::json!({ "version": 1, "packages": packages }).to_string(),
    )
    .unwrap();
    fs::write(
        pane.data.path().join("quick-slots.json"),
        serde_json::json!({
            "version": 2,
            "pins": [{"command": format!("{key}#quicklinks")}]
        })
        .to_string(),
    )
    .unwrap();

    let Started { launcher, .. } = pane.start();
    let slot = &launcher.quick_slots()[0];
    assert_eq!(slot.title, "Search Quicklinks");
    assert!(slot.ready(), "{:?}", slot.unavailable);
    block_on(launcher.activate_quick_slot(0));
    assert_eq!(launcher.view().title, "Quicklinks");
    assert_eq!(titles(&launcher), ["Docs", "News"]);

    to_root(&launcher);
    search(&launcher, "news");
    assert_eq!(titles(&launcher), ["News"]);
    pane.system.take();
    block_on(launcher.activate_selected());
    assert_eq!(
        pane.system.take(),
        [opened("https://news.example.com", None)]
    );

    // Edited and saved, the next start reads what this one saved.
    open_search(&launcher);
    run(&launcher, "Docs", EDIT);
    assert_eq!(
        submit(&launcher, &[("application", "Notepad")]),
        Status::Result("Saved “Docs”".into())
    );
    drop(launcher);
    let Started { launcher, .. } = pane.start();
    search(&launcher, "docs");
    pane.system.take();
    block_on(launcher.activate_selected());
    assert_eq!(
        pane.system.take(),
        [opened("https://docs.example.com", Some("app:Notepad"))]
    );
}

#[test]
fn a_disabled_quicklinks_hides_its_quicklinks_and_keeps_them() {
    let pane = Pane::new();
    let Started { launcher, .. } = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com", "");
    let identity = launcher.packages()[0].identity.clone();

    block_on(launcher.set_enabled(&identity, false));
    search(&launcher, "docs");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    search(&launcher, "quicklinks");
    assert_eq!(titles(&launcher), Vec::<String>::new(), "nor its commands");

    block_on(launcher.set_enabled(&identity, true));
    search(&launcher, "doc");
    assert_eq!(titles(&launcher), ["Docs"]);
}

#[test]
fn a_launcher_that_does_not_reach_the_system_explains_it() {
    let pane = Pane::new();
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(pane.system.clone());
    let launcher =
        Launcher::with_packages(Ok(runtime), vec![], pane.data.path().join("extensions"));
    block_on(launcher.install_package(&package()));
    to_root(&launcher);
    create(&launcher, "Docs", "https://docs.example.com", "");
    search(&launcher, "docs");

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Could not open Docs: Not available: this Pane does not reach the system's \
             clipboard, open things or recycle them"
                .into()
        )
    );
    // Root search stays usable. ("Create Quicklink" matches too: its
    // subtitle says "from root search".)
    search(&launcher, "search quicklinks");
    assert_eq!(
        titles(&launcher).first().map(String::as_str),
        Some("Search Quicklinks")
    );
}

/// A link handler that records what it is asked to open.
#[derive(Default)]
struct RecordingOpener(std::sync::Mutex<Vec<String>>);

impl pane_core::LinkOpener for RecordingOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(url.into());
        Ok(())
    }
}

#[test]
fn pane_opens_a_computed_link_of_any_scheme() {
    // The faulty fixture offers a file: link for "file link". Opening is
    // unfiltered (ADR 0037): the system's handler gets it.
    let pane = Pane::new();
    let opener = Arc::new(RecordingOpener::default());
    let launcher = Launcher::with_packages(
        Runtime::start(),
        vec![],
        pane.data.path().join("extensions"),
    )
    .with_link_opener(opener.clone());
    let folder = pane.data.path().join("faulty");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{"manifestVersion": 1, "title": "Faulty", "apiVersion": "0.1",
            "commands": [{"id": "faulty", "title": "Faulty answers",
            "component": "command.wasm", "rootResults": true}]}"#,
    )
    .unwrap();
    let faulty = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/faulty.wasm");
    assert!(
        faulty.exists(),
        "{} is missing; run `cargo xtask guests`",
        faulty.display()
    );
    fs::copy(faulty, folder.join("command.wasm")).unwrap();
    block_on(launcher.install_package(&folder));
    to_root(&launcher);

    search(&launcher, "file link");
    assert_eq!(titles(&launcher), ["A local file"]);
    block_on(launcher.activate_selected());

    assert_eq!(*opener.0.lock().unwrap(), ["file:///etc/hosts"]);
}
