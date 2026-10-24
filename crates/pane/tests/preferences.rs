//! Preferences (#143) in the native windows, on GPUI's test platform, with
//! real key events and the Rust preferences sample from `cargo xtask
//! guests`: the Setup screen a command shows before its first run (its
//! sentence, its fields with their descriptions, the package's help, a
//! password drawn as dots), submitted with Enter or left with Escape; the
//! "Needs setup" a row says meanwhile; the controls of an extension's page
//! in Settings › Extensions, each saving as it changes; and the Actions
//! panel's "Configure Command…" and "Configure Extension…", which open
//! that card, with "Disable Command" and "Uninstall Extension…" beside
//! them and the chords of the three that have one (#493). The core's
//! rules (the other languages, what is kept) are `pane-core`'s
//! `preferences.rs`.

use std::path::PathBuf;

use gpui::{
    AnyWindowHandle, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext,
    WindowHandle, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::assembled_package;

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/setup.rs"]
mod setup;

#[path = "support/wait.rs"]
mod wait;

use setup::{actions_shortcut, settings_shortcut};

/// A launcher window over a launcher with the Rust preferences sample
/// installed, at root search, and the sample's folders.
struct Opened<'a> {
    window: Entity<LauncherWindow>,
    cx: &'a mut VisualTestContext,
    launcher: Launcher,
    folder: PathBuf,
    sources: TempDir,
    _data: TempDir,
}

fn opened(cx: &mut TestAppContext) -> Opened<'_> {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled_package("sample-preferences", &sources.path().join("preferences"));
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    futures::executor::block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Preferences sample".into())
    );
    launcher.show_root_search();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let shared = launcher.clone();
    let (window, cx) =
        cx.add_window_view(move |window, cx| LauncherWindow::new(shared, window, cx));
    settle(&window, cx);
    Opened {
        window,
        cx,
        launcher,
        folder,
        sources,
        _data: data,
    }
}

impl Opened<'_> {
    /// A folder that exists, for the notes folder.
    fn notes(&self) -> String {
        let notes = self.sources.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        notes.to_str().unwrap().to_owned()
    }

    /// The value the user set for the preference kept as `key`.
    fn value(&self, key: &str) -> Option<String> {
        let identity = PackageIdentity::local(&self.folder).unwrap();
        let card = self.launcher.preferences_of(&identity).unwrap();
        card.fields
            .iter()
            .chain(card.commands.iter().flat_map(|command| &command.fields))
            .find(|field| field.key == key)
            .and_then(|field| field.value.clone())
    }
}

/// Every accessibility node's properties in the window `cx` drives.
fn nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let json = a11y::a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The node with this role and label.
fn node(nodes: &[serde_json::Value], role: &str, label: &str) -> serde_json::Value {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("no {role} labelled {label:?} in {nodes:#?}"))
}

fn selector(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

/// Types `query` into root search and presses Enter on its best match.
fn launch(opened: &mut Opened<'_>, query: &str) {
    let cx = &mut *opened.cx;
    cx.simulate_input(query);
    settle(&opened.window, cx);
    cx.simulate_keystrokes("enter");
    settle(&opened.window, cx);
}

#[gpui::test]
fn the_setup_screen_asks_for_what_is_unset_and_launches_on_enter(cx: &mut TestAppContext) {
    let mut opened = opened(cx);
    launch(&mut opened, "show preferences");
    let view = settle(&opened.window, opened.cx);
    let form = view.form().expect("the Setup screen");
    let _ = form;
    let cx = &mut *opened.cx;

    // The sentence and the package's help beside the fields are the
    // tree's own texts (#241), drawn as labels; each field with its
    // description.
    assert!(cx.debug_bounds("field-apiKey").is_some());
    assert!(cx.debug_bounds("field-description-apiKey").is_some());
    assert!(cx.debug_bounds(selector("field-show#folder")).is_some());
    assert!(
        cx.debug_bounds("field-units").is_none(),
        "a default asks nothing"
    );
    let shown = nodes(cx);
    node(
        &shown,
        "Label",
        "Set these up before using Show preferences",
    );
    node(&shown, "Label", "Setting up the preferences sample");
    assert!(
        shown.iter().any(|node| node["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("The API key is any text"))),
        "the help beside the fields: {shown:#?}"
    );

    // The API key, focused first, is hidden as it is typed.
    cx.simulate_input("abc");
    settle(&opened.window, cx);
    // A password field, as assistive technology names it (#237).
    let field = node(&nodes(cx), "PasswordInput", "API key");
    assert_eq!(field["value"], "\u{2022}\u{2022}\u{2022}");

    // Tab reaches the folder; Enter saves both and opens the command.
    let notes = {
        let notes = opened.sources.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        notes.to_str().unwrap().to_owned()
    };
    cx.simulate_keystrokes("tab");
    cx.simulate_input(&notes);
    settle(&opened.window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&opened.window, cx);
    assert_eq!(view.screen, Screen::Command, "{:?}", view.status);
    assert_eq!(view.title, "Preferences");
    assert!(
        view.rows
            .iter()
            .any(|row| row.title == format!("Notes folder: {notes}"))
    );
    assert_eq!(opened.value("apiKey").as_deref(), Some("abc"));
}

/// A folder preference on the Setup screen has the Settings card's
/// "Choose…": it opens the system's folder picker, and the folder chosen
/// fills the field.
#[gpui::test]
fn the_setup_screen_chooses_a_folder_with_the_systems_picker(cx: &mut TestAppContext) {
    let mut opened = opened(cx);
    launch(&mut opened, "show preferences");
    let notes = opened.notes();
    let cx = &mut *opened.cx;
    let choose = cx
        .debug_bounds(selector("field-choose-show#folder"))
        .expect("the folder's Choose… is drawn");
    assert!(
        cx.debug_bounds("field-choose-apiKey").is_none(),
        "a password is typed"
    );
    cx.simulate_mouse_move(choose.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(choose.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "a folder picker opened");
    let chosen = PathBuf::from(&notes);
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![chosen])
    });
    let view = settle(&opened.window, cx);
    let form = view.form().expect("the Setup screen stays");
    // The folder field's chosen path, drawn by the field's well (#241).
    let field = node(&nodes(cx), "TextInput", "Notes folder");
    assert_eq!(field["value"], notes.as_str());
    let _ = form;
}

#[gpui::test]
fn escape_leaves_the_setup_screen_and_launches_nothing(cx: &mut TestAppContext) {
    let mut opened = opened(cx);
    launch(&mut opened, "report preferences");
    assert!(
        settle(&opened.window, opened.cx).form().is_some(),
        "the Setup screen"
    );
    opened.cx.simulate_keystrokes("escape");
    let view = settle(&opened.window, opened.cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "report preferences".into()
        }
    );
    // "Report preferences" would have shown a toast.
    assert_eq!(
        settle_shown(&opened.window, opened.cx),
        Status::Idle,
        "nothing ran"
    );
    assert!(
        opened.cx.debug_bounds("field-apiKey").is_none(),
        "the setup screen is gone"
    );
}

#[gpui::test]
fn a_command_that_needs_setup_says_so_on_its_row(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("tick");
    settle(&opened.window, cx);
    let row = node(&nodes(cx), "ListBoxOption", "Tick");
    let description = row["description"].as_str().unwrap_or_default().to_owned();
    assert!(description.ends_with("Needs setup"), "{description}");

    // Set up on its card, it no longer needs setup.
    let identity = PackageIdentity::local(&opened.folder).unwrap();
    futures::executor::block_on(
        opened
            .launcher
            .set_preference(&identity, "apiKey", Some("abc")),
    )
    .unwrap();
    cx.simulate_keystrokes("backspace");
    cx.simulate_input("k");
    settle(&opened.window, cx);
    let row = node(&nodes(cx), "ListBoxOption", "Tick");
    let description = row["description"].as_str().unwrap_or_default().to_owned();
    assert!(!description.contains("Needs setup"), "{description}");
}

/// The open Settings windows.
fn settings_windows(cx: &TestAppContext) -> Vec<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .collect()
    })
}

/// A test context for the Settings window.
fn settings_context(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
) -> VisualTestContext {
    VisualTestContext::from_window(AnyWindowHandle::from(*settings), &cx.cx)
}

/// Opens Settings on the Preferences sample's page, under the Extensions
/// group (#168), tall enough to reach its preferences.
fn open_extensions(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.simulate_resize(gpui::size(px(760.), px(1400.)));
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-Extensions");
    click(&mut settings_cx, "extension-entry-Preferences sample");
    settings_cx
}

/// Clicks the element whose debug selector is `name`, scrolled into view
/// first, as its user would.
fn click(cx: &mut VisualTestContext, name: &str) {
    let name = selector(name);
    let viewport = cx.update(|window, _| window.viewport_size());
    for _ in 0..10 {
        let bounds = cx
            .debug_bounds(name)
            .unwrap_or_else(|| panic!("no {name} is drawn"));
        if bounds.bottom() <= viewport.height {
            break;
        }
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: gpui::point(bounds.center().x, viewport.height / 2.),
            delta: gpui::ScrollDelta::Pixels(gpui::point(
                px(0.),
                viewport.height - bounds.bottom() - px(24.),
            )),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
    }
    let bounds = cx
        .debug_bounds(name)
        .unwrap_or_else(|| panic!("no {name} is drawn"));
    cx.simulate_mouse_move(bounds.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn the_card_edits_every_type_with_its_control_and_saves_on_change(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let mut settings = open_extensions(opened.cx);

    // The package's preferences, then each command's under its title; a
    // required one that is unset is in the error state.
    for drawn in [
        "preference-apiKey",
        "preference-units",
        "preference-greeting",
        "preference-verbose",
        "preference-command-show",
        "preference-show#folder",
        "preference-show#notes",
        "preference-show#editor",
        "preference-command-report",
        "preference-report#loud",
        "preference-error-apiKey",
        "preference-error-show#folder",
        "preference-choose-show#folder",
        "preference-choose-show#notes",
        "preference-choose-show#editor",
    ] {
        assert!(
            settings.debug_bounds(selector(drawn)).is_some(),
            "{drawn} is drawn"
        );
    }
    assert!(settings.debug_bounds("preference-error-units").is_none());

    // A checkbox is a switch.
    click(&mut settings, "preference-toggle-verbose");
    assert_eq!(opened.value("verbose").as_deref(), Some("true"));
    click(&mut settings, "preference-toggle-verbose");
    assert_eq!(opened.value("verbose").as_deref(), Some("false"));

    // A dropdown is a select, as Settings' own choices are: its trigger, a
    // combo box named by the preference, shows the value in force (the
    // default here) and opens the options under a search field; choosing
    // one saves it, and the trigger shows it.
    let trigger = node(&nodes(&mut settings), "ComboBox", "Units");
    assert_eq!(trigger["value"].as_str(), Some("Metric"), "{trigger:#?}");
    click(&mut settings, "preference-select-units");
    assert!(
        settings
            .debug_bounds("preference-select-units-query")
            .is_some(),
        "the popup's search field shows"
    );
    let trigger = node(&nodes(&mut settings), "ComboBox", "Units");
    assert_eq!(trigger["expanded"].as_bool(), Some(true), "{trigger:#?}");
    click(&mut settings, "preference-select-units-imperial");
    assert_eq!(opened.value("units").as_deref(), Some("imperial"));
    // The popup's exit runs to its end, so nothing of it covers the rows
    // clicked next.
    wait::settle_frames(&mut settings);
    assert!(
        settings
            .debug_bounds("preference-select-units-popup")
            .is_none(),
        "the popup closed"
    );
    let trigger = node(&nodes(&mut settings), "ComboBox", "Units");
    assert_eq!(trigger["value"].as_str(), Some("Imperial"), "{trigger:#?}");
    assert_eq!(trigger["expanded"].as_bool(), Some(false), "{trigger:#?}");

    // Text is a field, saved as it is typed.
    click(&mut settings, "preference-field-greeting");
    settings.simulate_input("Hi");
    settings.run_until_parked();
    assert_eq!(opened.value("greeting").as_deref(), Some("Hi"));

    // A password is hidden as it is typed, and leaves the error state.
    click(&mut settings, "preference-field-apiKey");
    settings.simulate_input("xyz");
    settings.run_until_parked();
    assert_eq!(opened.value("apiKey").as_deref(), Some("xyz"));
    let field = node(&nodes(&mut settings), "TextInput", "API key");
    assert_eq!(field["value"], "\u{2022}\u{2022}\u{2022}");
    assert!(settings.debug_bounds("preference-error-apiKey").is_none());

    // A folder's field takes a path, as its Choose… button would fill it.
    let notes = opened.notes();
    click(&mut settings, selector("preference-field-show#folder"));
    settings.simulate_input(&notes);
    settings.run_until_parked();
    assert_eq!(opened.value("show#folder").as_deref(), Some(notes.as_str()));
    assert!(
        settings
            .debug_bounds(selector("preference-error-show#folder"))
            .is_none()
    );

    // The launcher sees each change: the command runs without setup now.
    let cx = opened.cx;
    cx.simulate_input("show preferences");
    settle(&opened.window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&opened.window, cx);
    assert_eq!(view.screen, Screen::Command, "{:?}", view.status);
    assert!(view.rows.iter().any(|row| row.title == "Greeting: Hi"));
    assert!(view.rows.iter().any(|row| row.title == "Units: imperial"));
}

#[gpui::test]
fn configure_entries_open_the_extensions_card_in_settings(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("tick");
    settle(&opened.window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(&opened.window, cx);
    assert!(cx.debug_bounds("action-Configure Extension…").is_some());
    assert!(
        cx.debug_bounds("action-Configure Command…").is_none(),
        "Tick declares none of its own"
    );
    cx.simulate_keystrokes("escape");
    settle(&opened.window, cx);

    for _ in 0..4 {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("show preferences");
    settle(&opened.window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(&opened.window, cx);
    assert!(cx.debug_bounds("action-Configure Command…").is_some());
    assert!(cx.debug_bounds("action-Configure Extension…").is_some());
    assert!(settings_windows(cx).is_empty());
    cx.simulate_input("configure command");
    settle(&opened.window, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    // Settings opened on the extension's page (#168), at the command's
    // preferences.
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extensions").is_some());
    assert!(
        settings_cx
            .debug_bounds("preference-command-show")
            .is_some()
    );
}

/// Root search's chords for Pane's own result actions on this system
/// (#493), and their names as the panel's entries tell assistive
/// technology.
const CONFIGURE_COMMAND: &str = if cfg!(target_os = "macos") {
    "cmd-shift-,"
} else {
    "ctrl-shift-,"
};
const CONFIGURE_EXTENSION: &str = if cfg!(target_os = "macos") {
    "cmd-alt-,"
} else {
    "ctrl-alt-,"
};
const DISABLE_COMMAND: &str = if cfg!(target_os = "macos") {
    "cmd-alt-shift-d"
} else {
    "ctrl-alt-shift-d"
};
const CONFIGURE_COMMAND_NAME: &str = if cfg!(target_os = "macos") {
    "Shift+Command+,"
} else {
    "Ctrl+Shift+,"
};
const CONFIGURE_EXTENSION_NAME: &str = if cfg!(target_os = "macos") {
    "Option+Command+,"
} else {
    "Ctrl+Alt+,"
};
const DISABLE_COMMAND_NAME: &str = if cfg!(target_os = "macos") {
    "Option+Shift+Command+D"
} else {
    "Ctrl+Alt+Shift+D"
};

fn actions_open(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| window.read(cx).actions_open())
}

/// An installed command's row lists its extension's entries under Pane
/// (#493): Configure Command… where it declares preferences, Configure
/// Extension… always, Disable Command and Uninstall Extension…, the last
/// destructive; the chorded ones show their chords.
#[gpui::test]
fn the_extension_entries_show_their_chords_and_uninstall_is_destructive(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("show preferences");
    settle(&opened.window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(&opened.window, cx);
    assert!(cx.debug_bounds("action-group-Pane").is_some());

    let nodes = nodes(cx);
    let entry = |label: &str| node(&nodes, "MenuItem", label);
    assert_eq!(
        entry("Configure Command…")["keyboard_shortcut"],
        CONFIGURE_COMMAND_NAME
    );
    assert_eq!(
        entry("Configure Extension…")["keyboard_shortcut"],
        CONFIGURE_EXTENSION_NAME
    );
    assert_eq!(
        entry("Disable Command")["keyboard_shortcut"],
        DISABLE_COMMAND_NAME
    );
    let uninstall = entry("Uninstall Extension…");
    assert_eq!(uninstall["keyboard_shortcut"], serde_json::Value::Null);
    assert_eq!(uninstall["description"], "Destructive");
    assert_eq!(
        entry("Disable Command")["description"],
        serde_json::Value::Null
    );
}

/// Configure Command's chord, without the panel, opens Settings at the
/// selected command's preferences.
#[gpui::test]
fn the_configure_command_chord_opens_its_preferences(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("show preferences");
    settle(&opened.window, cx);
    assert!(settings_windows(cx).is_empty());
    cx.simulate_keystrokes(CONFIGURE_COMMAND);
    cx.run_until_parked();

    assert!(!actions_open(&opened.window, cx));
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extensions").is_some());
    assert!(
        settings_cx
            .debug_bounds("preference-command-show")
            .is_some()
    );
}

/// Configure Extension's chord, with the panel open over the row, closes
/// the panel and opens Settings on the extension's page.
#[gpui::test]
fn the_configure_extension_chord_runs_from_the_open_panel(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("tick");
    settle(&opened.window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(&opened.window, cx);
    assert!(actions_open(&opened.window, cx));
    cx.simulate_keystrokes(CONFIGURE_EXTENSION);
    cx.run_until_parked();

    assert!(!actions_open(&opened.window, cx));
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extensions").is_some());
}

/// Disable Command's chord turns the selected command off: it leaves root
/// search, and the status says so.
#[gpui::test]
fn the_disable_command_chord_turns_the_command_off(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("tick");
    let view = settle(&opened.window, cx);
    assert!(view.rows.iter().any(|row| row.title == "Tick"), "{view:?}");
    cx.simulate_keystrokes(DISABLE_COMMAND);
    let view = settle(&opened.window, cx);

    assert!(view.rows.iter().all(|row| row.title != "Tick"), "{view:?}");
    assert_eq!(view.status, Status::Result("Turned off Tick".into()));
    let identity = PackageIdentity::local(&opened.folder).unwrap();
    let package = opened
        .launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity)
        .expect("still installed");
    assert!(!package.command_enabled("tick"));
}

/// Uninstall Extension… opens the launcher's "Uninstall X?" confirmation,
/// all by keys: Escape there returns to the search, and Enter on its first
/// row uninstalls the extension, back on the search without its rows.
#[gpui::test]
fn uninstall_extension_asks_and_is_answered_by_keys(cx: &mut TestAppContext) {
    let opened = opened(cx);
    let cx = opened.cx;
    cx.simulate_input("tick");
    settle(&opened.window, cx);
    let choose_uninstall = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(actions_shortcut());
        settle(&opened.window, cx);
        cx.simulate_input("uninstall");
        settle(&opened.window, cx);
        cx.simulate_keystrokes("enter");
        settle(&opened.window, cx)
    };

    let view = choose_uninstall(cx);
    assert!(
        matches!(view.screen, Screen::Confirm { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Uninstall Preferences sample?");
    assert!(!actions_open(&opened.window, cx));

    cx.simulate_keystrokes("escape");
    let view = settle(&opened.window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "tick".into()
        }
    );
    assert!(view.rows.iter().any(|row| row.title == "Tick"));

    choose_uninstall(cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&opened.window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "tick".into()
        }
    );
    assert!(view.rows.iter().all(|row| row.title != "Tick"), "{view:?}");
    assert!(opened.launcher.packages().is_empty());
}
