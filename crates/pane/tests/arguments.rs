//! Pane's inline argument fields through the native window, on GPUI's
//! test platform, with real key events: the selected row's fields show
//! after the query; Enter with a required one blank takes focus to it and
//! runs nothing, and Enter inside a blank one marks it; the mark comes
//! only after the field was left; Tab, Shift+Tab and the arrows at the
//! fields' edges move between the fields and the query, Up and Down doing
//! nothing while a field has the keys; Escape from a field returns to the
//! query with its text selected; a password is concealed from assistive
//! technology; an alias and a space enter the fields, carrying what is
//! typed next — held with the query's list, when it is not yet published
//! — into the first one; and a no-view command's global hotkey still
//! shows the hidden window for the argument form, which stands in for
//! the fields wherever they do not show. The commands are the Rust
//! arguments sample's "Greet" (a required name, an optional secret, a
//! tone) and "Stamp" (a required label), from `cargo xtask guests`, which
//! tell what they ran with in a toast; the system's hotkeys are a fake
//! that records what Pane registers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{published, settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

#[path = "support/a11y.rs"]
mod a11y;

use a11y::{a11y, focused_label};

/// What a required field left empty says, next to it and in the status
/// line after its label.
const MISSING: &str = "Enter a value to run the command";

#[derive(Default)]
struct FakeSystem {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.registered.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.registered
            .lock()
            .unwrap()
            .retain(|kept| kept != shortcut);
    }
}

/// A window over a launcher with the Rust arguments sample installed,
/// through its preview and Enter, as a user installs it; `change` edits
/// its `pane.json` first.
fn installed(
    cx: &mut TestAppContext,
    system: Arc<FakeSystem>,
    change: impl FnOnce(&mut serde_json::Value),
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    tempfile::TempDir,
    PathBuf,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-arguments", &sources.path().join("arguments"));
    edit_manifest(&folder, change);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(system);
    let preview = folder.clone();
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Arguments sample".into())
    );
    // Root search shown afresh leaves the launcher idle (the install's
    // result owns the status strip until then), so the tests below can
    // tell that nothing ran from the status line alone. The window is
    // shown the change as the launcher's own tests redraw it.
    window.update(cx, |window, cx| {
        window.launcher().show_root_search();
        cx.notify();
    });
    settle(&window, cx);
    (window, cx, sources, data, folder)
}

/// A window over a launcher with the Rust arguments sample and the
/// calculator installed, "gr" given as "Greet"'s alias: a query asks the
/// calculator for its results, so its list is held while it answers and
/// the keys typed under it wait for it (#203).
fn with_calculator(
    cx: &mut TestAppContext,
) -> (
    Entity<LauncherWindow>,
    &mut VisualTestContext,
    tempfile::TempDir,
    PathBuf,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let calculator =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator");
    assert!(
        calculator.exists(),
        "{} is missing; run `cargo xtask guests`",
        calculator.display()
    );
    let folder = assembled_package("sample-arguments", &sources.path().join("arguments"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    for package in [&calculator, &folder] {
        let pending = launcher.install_package(package);
        cx.foreground_executor().block_on(pending);
    }
    let greet = format!("{}#greet", PackageIdentity::local(&folder).unwrap().key());
    let set = launcher.set_alias(&greet, "gr");
    block_on(set.expect("the alias is accepted"));
    // Shown afresh, root search leaves the launcher idle: the last
    // install's result otherwise owns the status strip, hiding what the
    // tests below learn from it.
    launcher.show_root_search();
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx, data, folder)
}

/// Changes the `pane.json` in `folder` with `change`.
fn edit_manifest(folder: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let file = folder.join("pane.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    change(&mut manifest);
    fs::write(&file, manifest.to_string()).unwrap();
}

/// The query field's text.
fn query_text(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// The argument field `name`'s text, as the window shows it.
fn argument_text(window: &Entity<LauncherWindow>, cx: &VisualTestContext, name: &str) -> String {
    let field = cx.read_entity(window, |window, _| window.argument_field(name));
    match field {
        Some(field) => cx.read_entity(&field, |field, _| field.as_str().to_owned()),
        None => panic!("no argument field {name}"),
    }
}

/// What the accessibility node named `label` carries as its value, and as
/// its description of the field's state, if a node is named so.
fn node_of(cx: &mut VisualTestContext, label: &str) -> Option<(String, String)> {
    let tree: serde_json::Value = serde_json::from_str(&a11y(cx)).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| &node["aria"])
        .find(|aria| aria["label"].as_str() == Some(label))
        .map(|aria| {
            (
                aria["value"].as_str().unwrap_or_default().to_owned(),
                aria["description"].as_str().unwrap_or_default().to_owned(),
            )
        })
}

#[gpui::test]
fn enter_with_a_blank_required_argument_marks_it_and_runs_nothing(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |_| {});

    // Typing "greet" selects its row: its three fields show after the
    // query, named for assistive technology with their state.
    cx.simulate_input("greet");
    let view = settle(&window, cx);
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Greet")
    );
    // Each field is a labelled editable node inside the search combo box's
    // group, named by its placeholder, with its required or optional state
    // as its description.
    for (field, state) in [
        ("Name", "Required"),
        ("Secret", "Optional"),
        ("Tone", "Optional"),
    ] {
        let (_, description) =
            node_of(cx, field).unwrap_or_else(|| panic!("the {field} field is named for the tree"));
        assert_eq!(description, state, "the {field} field's state");
    }
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));

    // Enter with the required name blank: nothing runs, and focus moves to
    // the name, which is not yet marked — it has not been left blank.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "nothing ran");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    assert!(!a11y(cx).contains(MISSING), "nothing is marked yet");

    // Tab leaves the name blank: it is marked from then on, the tone and
    // the secret never (they are optional).
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    assert!(a11y(cx).contains(MISSING), "the name is marked");

    // Enter inside a blank required argument marks it and says what it
    // waits for, running nothing.
    cx.simulate_keystrokes("shift-tab");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error("Enter Name".into()));
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // Typed there, the mark clears and Enter runs the command once with
    // the values; the password is concealed from assistive technology
    // too, reading as dots. Root search stays as it was, its query kept.
    cx.simulate_input("Ada");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("s3cret");
    assert!(!a11y(cx).contains("s3cret"), "the password is read out");
    assert_eq!(
        node_of(cx, "Secret").map(|(value, _)| value),
        Some("••••••".into()),
        "the password reads as dots"
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result(
            "Greet run 1 from root-search: name=Ada, secret (6 characters); fallback text: none"
                .into()
        )
    );
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    assert!(cx.debug_bounds("toast-success").is_some());
}

#[gpui::test]
fn escape_from_a_field_returns_to_the_query_with_its_text_selected(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |_| {});

    cx.simulate_input("greet");
    settle(&window, cx);
    // Enter takes focus to the blank required name; typing there and
    // leaving with Escape selects the query's text.
    cx.simulate_keystrokes("enter");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_input("Ad");
    cx.simulate_keystrokes("escape");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));
    assert_eq!(query_text(&window, cx), "greet");
    // The selection replaces the query with what is typed next.
    cx.simulate_input("s");
    assert_eq!(query_text(&window, cx), "s");

    // The usual Escape follows: the query clears, and nothing has run —
    // the next run is the first.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: String::new()
        }
    );
    assert_eq!(view.status, Status::Idle);
    cx.simulate_input("greet");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Grace");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Greet run 1 from root-search: name=Grace; fallback text: none".into())
    );
}

#[gpui::test]
fn tab_shift_tab_and_the_arrows_move_between_the_fields_and_the_query(cx: &mut TestAppContext) {
    // Greet's name made optional and its secret required: Tab from the
    // query focuses the first EMPTY argument, the name — not the first
    // required one, as the form does.
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |manifest| {
        let arguments = &mut manifest["commands"][0]["arguments"];
        arguments[0]["required"] = false.into();
        arguments[1]["required"] = true.into();
    });

    cx.simulate_input("greet");
    let view = settle(&window, cx);
    let selected = view.selected;
    // Tab: the first empty field, then the next, then the next, then back
    // to the query.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Tone"));
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));

    // Shift+Tab from the query: the last field; from the first, the query.
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Tone"));
    cx.simulate_keystrokes("shift-tab");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));

    // Right at the query's end enters the first field; Left at the name's
    // start returns to the query.
    cx.simulate_keystrokes("right");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("left");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));

    // A field with text: Left moves the caret, and only at its start
    // leaves the field.
    cx.simulate_keystrokes("right");
    cx.simulate_input("Ab");
    cx.simulate_keystrokes("left");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("left");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("left");
    assert_eq!(focused_label(cx).as_deref(), Some("Search"));

    // Up and Down do nothing while an argument field has the keys: the
    // list does not move.
    cx.simulate_keystrokes("tab");
    let view = settle(&window, cx);
    let selected_now = view.selected;
    assert_eq!(selected_now, selected);
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(
        view.selected, selected,
        "the list did not move while a field had the keys"
    );

    // Nothing ran throughout: Enter takes focus to the blank required
    // secret instead, the status line saying nothing.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "nothing ran");
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
}

#[gpui::test]
fn an_alias_and_a_space_fill_the_first_argument_through_the_held_keys(cx: &mut TestAppContext) {
    let (window, cx, _data, _folder) = with_calculator(cx);

    // "gr" is Greet's alias: the space typed after it waits for the
    // query's list, and the characters typed behind it land behind the
    // held space.
    cx.simulate_input("gr");
    cx.simulate_keystrokes("space");
    assert_eq!(query_text(&window, cx), "gr", "the space is held");
    cx.simulate_input("hello");
    let view = published(&window, cx);
    assert_eq!(view.query(), Some("gr "));
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Greet")
    );

    // The alias stays in the field; "hello" landed in the first argument,
    // the caret at its end, and the focus is in it. Enter runs the
    // command from its alias with the value.
    assert_eq!(argument_text(&window, cx, "name"), "hello");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Greet run 1 from alias: name=hello; fallback text: none".into())
    );
}

/// Presses the global hotkey `shortcut`, as the system's adapter would
/// report it, past the window's repeat guard for the Open Pane hotkey.
fn press(window: &Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
    cx.run_until_parked();
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Runs the window until the launcher shows the form titled `title`.
fn until_form(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, title: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.form().is_some() && view.title == title {
            return;
        }
        assert!(Instant::now() < deadline, "timed out: {view:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn a_no_view_hotkey_shows_the_hidden_window_for_the_form(cx: &mut TestAppContext) {
    let system = Arc::new(FakeSystem::default());
    let (window, cx, _sources, _data, folder) = installed(cx, system.clone(), |_| {});
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let shortcut = Shortcut::parse("ctrl+alt+s").unwrap();
    let command = format!("{}#stamp", PackageIdentity::local(&folder).unwrap().key());
    block_on(
        launcher
            .set_hotkey(&command, Some(shortcut.clone()))
            .expect("the hotkey is accepted"),
    );
    assert!(system.registered.lock().unwrap().contains(&shortcut));

    // The Open Pane hotkey hides the focused launcher (a launcher without
    // focus is brought forward first).
    let open_pane = Shortcut::open_pane_default();
    press(&window, &open_pane, cx);
    if !hidden(&window, cx) {
        press(&window, &open_pane, cx);
    }
    assert!(hidden(&window, cx), "the launcher hid");

    // "Stamp" needs its label: the window is shown for its form — the
    // fields stand in for it only in root search, where the hotkey's
    // launch does not come from.
    press(&window, &shortcut, cx);
    assert!(!hidden(&window, cx), "the window was shown");
    until_form(&window, cx, "Stamp");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(focused_label(cx).as_deref(), Some("Label"));
    cx.simulate_input("x");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Stamped x from hotkey".into())
    );
}
