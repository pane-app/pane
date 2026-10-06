//! Pane's argument form through the native window, on GPUI's test platform,
//! with real key events: Enter on a command that needs an argument shows
//! the form, focus on its first required field that is empty; Enter with
//! that field still empty keeps the form and takes focus back to it; a
//! password is concealed from assistive technology; Escape runs nothing;
//! and a no-view command's global hotkey shows the hidden window for the
//! form. The commands are the Rust arguments sample's "Greet" (a required
//! name, an optional secret, a tone) and "Stamp" (a required label), from
//! `cargo xtask guests`; the system's hotkeys are a fake that records what
//! Pane registers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

#[path = "support/a11y.rs"]
mod a11y;

use a11y::{a11y, focused_label};

/// What a required field left empty says, after its label in the status
/// line.
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
    (window, cx, sources, data, folder)
}

/// Changes the `pane.json` in `folder` with `change`.
fn edit_manifest(folder: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let file = folder.join("pane.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    change(&mut manifest);
    fs::write(&file, manifest.to_string()).unwrap();
}

/// Types `query` into root search and presses Enter on the row it selects,
/// which must be `title`; the view once the window drew it.
fn launch(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    query: &str,
    title: &str,
) -> LauncherView {
    cx.simulate_input(query);
    let view = settle(window, cx);
    let selected = view.selected.map(|index| view.rows[index].title.as_str());
    assert_eq!(selected, Some(title));
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

/// The value the open form holds in field `id`.
fn field_value(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, id: &str) -> String {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    let form = view.form().expect("a form is open");
    let field = form.fields.iter().find(|field| field.id == id);
    field.expect("the field exists").value.clone()
}

#[gpui::test]
fn enter_with_the_required_field_empty_takes_focus_back_to_it(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |_| {});

    // Enter on "Greet" shows its form, focus on the empty required name.
    let view = launch(&window, cx, "greet", "Greet");
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(view.title, "Greet");
    for field in ["field-name", "field-secret", "field-tone"] {
        assert!(cx.debug_bounds(field).is_some(), "{field} is drawn");
    }
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // From the secret, Enter with the name still empty runs nothing and
    // takes focus back to the name.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    cx.simulate_input("s3cret");
    assert_eq!(field_value(&window, cx, "secret"), "s3cret");
    // The secret is concealed from assistive technology too.
    assert!(!a11y(cx).contains("s3cret"), "the password is read out");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error(format!("Name: {MISSING}")));
    assert!(view.form().is_some(), "the form stays");
    assert!(cx.debug_bounds("field-error-name").is_some());
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // Typed there, Enter runs the command once with the values.
    cx.simulate_input("Ada");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(
            "Greet run 1 from root-search: name=Ada, secret (6 characters), tone=warm; \
             fallback text: none"
                .into()
        )
    );
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
}

#[gpui::test]
fn escape_on_the_argument_form_runs_nothing(cx: &mut TestAppContext) {
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |_| {});

    let view = launch(&window, cx, "greet", "Greet");
    assert!(view.form().is_some(), "{:?}", view.screen);
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    assert_eq!(view.status, Status::Idle);

    // Nothing ran: the next submission is the first run.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    cx.simulate_input("Grace");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result(
            "Greet run 1 from root-search: name=Grace, tone=warm; fallback text: none".into()
        )
    );
}

#[gpui::test]
fn focus_starts_on_the_first_required_field_that_is_empty(cx: &mut TestAppContext) {
    // Greet's name made optional and its secret required: the form opens
    // on the secret, the second field.
    let (window, cx, _sources, _data, _folder) = installed(cx, Arc::default(), |manifest| {
        let arguments = &mut manifest["commands"][0]["arguments"];
        arguments[0]["required"] = false.into();
        arguments[1]["required"] = true.into();
    });

    let view = launch(&window, cx, "greet", "Greet");
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Error(format!("Secret: {MISSING}")));
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
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

    // "Stamp" needs its label: the window is shown for its form.
    press(&window, &shortcut, cx);
    assert!(!hidden(&window, cx), "the window was shown");
    until_form(&window, cx, "Stamp");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(focused_label(cx).as_deref(), Some("Label"));
    cx.simulate_input("x");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Stamped x from hotkey".into())
    );
}
