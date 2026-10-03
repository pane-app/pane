//! The Settings window's Shortcuts page, on GPUI's test platform: the
//! installed commands grouped by their extensions with their aliases and
//! global hotkeys, the alias edited inline — committed, refused,
//! cancelled and cleared — then reached from root search; the filter; and
//! the catalog following the package lifecycle and a restart, with the
//! window's own watcher asking for the redraw. The packages are the query
//! sample (Echo, which takes a query) and the Rust sample (Hello); the
//! hotkey system is a fake that accepts every registration.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, SavedData, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// The keystroke that opens Settings on this platform: Cmd+, on macOS,
/// Ctrl+, on Windows and Linux.
fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-,"
    }
}

/// A fake hotkey system that accepts every registration, so the recorded
/// hotkeys are active.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
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

/// Copies the assembled query sample (Echo, a command that takes a query)
/// to `folder`.
fn query_package(folder: &Path) -> PathBuf {
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-query");
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

/// Writes a package folder whose one command is the Rust sample.
fn hello_package(folder: &Path) -> PathBuf {
    let guest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

/// The command id of the package at `folder`: its identity's key and its
/// manifest's command id.
fn command_id(folder: &Path) -> String {
    format!("{}#{}", key_of(folder), manifest_id_of(folder))
}

/// The package at `folder`'s identity key, the group's stable key.
fn key_of(folder: &Path) -> String {
    PackageIdentity::local(folder).unwrap().key()
}

/// The id the package at `folder`'s manifest gives its one command.
fn manifest_id_of(folder: &Path) -> &'static str {
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    if manifest.contains("\"id\": \"echo\"") {
        "echo"
    } else {
        "hello"
    }
}

/// A selector for `debug_bounds`, which takes a static one: the ids the
/// page keys its elements by are the commands', which the test builds.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Records the aliases and hotkeys `records` name into the data folder
/// before a launcher is made of it, as a Pane that ran before would have
/// left them.
fn seed(data: &TempDir, aliases: &[(String, &str)], hotkeys: &[(String, &str)]) {
    let dir = data.path().join("extensions");
    fs::create_dir_all(&dir).unwrap();
    let aliases = aliases
        .iter()
        .map(|(id, alias)| (id.clone(), serde_json::json!(alias)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    fs::write(
        dir.join("aliases.json"),
        serde_json::json!({ "version": 1, "aliases": aliases, "fallbacks": [] }).to_string(),
    )
    .unwrap();
    let hotkeys = hotkeys
        .iter()
        .map(|(id, hotkey)| (id.clone(), serde_json::json!(hotkey)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    fs::write(
        dir.join("hotkeys.json"),
        serde_json::json!({ "version": 1, "hotkeys": hotkeys }).to_string(),
    )
    .unwrap();
}

/// The aliases.json record as it stands on disk.
fn aliases_record(data: &TempDir) -> String {
    fs::read_to_string(data.path().join("extensions").join("aliases.json")).unwrap()
}

/// Opens the launcher window over `data` and installs the packages whose
/// source folders `packages` holds, then opens the Settings window on the
/// Shortcuts page. Returns the launcher window, the Settings window, the
/// fake hotkey system the launcher registers through, and the launcher's
/// test context.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &TempDir,
    packages: &[&Path],
) -> (
    Entity<LauncherWindow>,
    WindowHandle<SettingsWindow>,
    Arc<FakeHotkeys>,
    &'a mut VisualTestContext,
) {
    let hotkeys = Arc::new(FakeHotkeys::default());
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_hotkeys(hotkeys.clone());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Install each package as the user does: the install row opens the
    // folder picker, the chosen package is previewed and Enter installs.
    for folder in packages {
        choose_folder(&window, cx, folder.to_path_buf());
        cx.simulate_keystrokes("enter");
        settle(&window, cx);
    }
    // Settings, on its Shortcuts page.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    let section = settings_cx
        .debug_bounds("section-Shortcuts")
        .expect("the Shortcuts section");
    settings_cx.simulate_click(section.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcuts-title").is_some(),
        "the Shortcuts page is showing"
    );
    (window, settings, hotkeys, cx)
}

/// Presses Enter on the install row and answers the folder picker with
/// `folder`.
fn choose_folder(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, folder: PathBuf) {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let view = launcher.view();
    let index = view
        .rows
        .iter()
        .position(|row| row.title == "Install extension from folder…")
        .expect("the install row");
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "a folder picker opened");
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![folder])
    });
    settle(window, cx);
}

/// The open Settings windows: the window list is the one-window registry
/// the app itself uses.
fn settings_windows(cx: &TestAppContext) -> Vec<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .collect()
    })
}

/// A test context for the Settings window, to drive it as its own window.
fn settings_context(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
) -> VisualTestContext {
    VisualTestContext::from_window(AnyWindowHandle::from(*settings), &cx.cx)
}

/// The label of the node assistive technology treats as focused in the
/// window `cx` drives.
fn focused_label(cx: &mut VisualTestContext) -> Option<String> {
    let (label, _) = accessibility(cx);
    label
}

/// The window's accessibility tree as (focused label, raw JSON), forced on
/// so the tree is built regardless of platform accessibility.
fn accessibility(cx: &mut VisualTestContext) -> (Option<String>, String) {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let field = |node: &serde_json::Value, key: &str| {
        node["aria"][key].as_str().unwrap_or_default().to_owned()
    };
    let focused = ["active_descendant_focus", "gpui_focus"]
        .iter()
        .find_map(|key| tree[key].as_str())
        .map(|id| field(&nodes[id], "label"));
    (focused, json)
}

/// Runs `cx` until the window's accessibility tree contains `text`, so
/// that work arriving from other threads (an alias being recorded) has
/// been drawn and captured: the captured tree follows the drawn frame,
/// which on the Windows test platform can be a frame behind the drawn
/// one, so the tree is polled rather than read once. Returns the tree's
/// JSON, which holds the frame the text was found in.
fn until_text(cx: &mut VisualTestContext, text: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let (_, json) = accessibility(cx);
        if json.contains(text) {
            return json;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the window to draw {text}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens the inline alias editor for the command `id` by clicking its
/// cell, in a fresh Settings window's context.
fn edit_alias(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
    id: &str,
) -> VisualTestContext {
    let mut settings_cx = settings_context(settings, cx);
    let cell = settings_cx
        .debug_bounds(selector(format!("shortcut-alias-{id}")))
        .expect("the alias cell");
    settings_cx.simulate_click(cell.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcut-editor").is_some(),
        "the editor opened"
    );
    settings_cx
}

/// The titles of the rows the view lists.
fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

#[gpui::test]
fn the_page_lists_installed_commands_with_their_alias_and_hotkey(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    // Records a Pane that ran before left: an alias and a hotkey for
    // Echo, an alias for Say hello, and an alias for a command of a
    // package that is not installed (a record edited by hand).
    seed(
        &data,
        &[
            (command_id(&query), "ec"),
            (command_id(&hello), "hi"),
            ("local:/nowhere#gone".into(), "gone"),
        ],
        &[(command_id(&query), "ctrl+alt+g")],
    );
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query, &hello]);
    let mut settings_cx = settings_context(&settings, cx);

    // The filter, the column labels and both groups, with the commands'
    // rows.
    assert!(settings_cx.debug_bounds("shortcut-filter").is_some());
    assert!(settings_cx.debug_bounds("shortcut-columns").is_some());
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-group-{}", key_of(&query))))
            .is_some(),
        "the Query sample group"
    );
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-group-{}", key_of(&hello))))
            .is_some(),
        "the Hello group"
    );
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-row-{}", command_id(&query))))
            .is_some(),
        "Echo's row"
    );

    // The alias and the hotkey, as recorded; the hotkey in the form this
    // system displays it. The columns' values are named for assistive
    // technology, and the group's source is displayed.
    let echo_hotkey = Shortcut::parse("ctrl+alt+g").unwrap().to_string();
    let (_, json) = accessibility(&mut settings_cx);
    for label in [
        "Alias for Echo: ec",
        "Alias for Say hello: hi",
        "Hotkey for Say hello: none",
    ] {
        assert!(
            json.contains(&format!("\"label\": \"{label}\"")),
            "{label} is named, {json}"
        );
    }
    assert!(
        json.contains(&format!("\"label\": \"Hotkey for Echo: {echo_hotkey}\"")),
        "the hotkey's keys are named, {json}"
    );
    assert!(json.contains("local folder"), "the source is displayed");

    // The record of the package that is not installed: a group of its
    // own, its choice's inactive reason drawn under it.
    assert!(
        settings_cx
            .debug_bounds("shortcut-group-not-installed")
            .is_some(),
        "the not-installed group"
    );
    assert!(
        settings_cx
            .debug_bounds("shortcut-alias-inactive-local:/nowhere#gone")
            .is_some(),
        "the record is explained"
    );

    // Nothing was activated to draw any of this: the launcher sits at
    // root search, with the last install's outcome still on its status
    // line and nothing running.
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(!matches!(view.status, Status::Running));
}

#[gpui::test]
fn an_alias_edited_inline_is_found_by_root_search_and_survives_a_restart(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    // A record left before: Say hello has the alias "hi".
    seed(&data, &[(command_id(&hello), "hi")], &[]);
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query, &hello]);

    // The editor opens by click, commits with Enter, and the change is
    // recorded: the status line says what typing it now finds.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    settings_cx.simulate_input("ec");
    settings_cx.simulate_keystrokes("enter");
    let json = until_text(&mut settings_cx, "Typing “ec” now finds Echo");
    assert!(
        json.contains("Alias for Echo: ec"),
        "the cell now shows the alias, {json}"
    );

    // Root search follows at once: the alias, a space and more text lists
    // the row that sends the text, and Enter sends it. Echo answers.
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Echo");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Echo heard “hello”".into()));

    // The record on disk is the one the alias form writes, by command id.
    assert!(
        aliases_record(&data).contains("#echo\": \"ec\""),
        "the alias is recorded, {}",
        aliases_record(&data)
    );

    // A restart of Pane over the same data folder keeps it.
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let restarted = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_hotkeys(Arc::new(FakeHotkeys::default()));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(restarted, window, cx));
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Echo");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Echo heard “hello”".into())
    );
    // And the Shortcuts page of a Settings window over that launcher
    // shows the alias.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    let section = settings_cx
        .debug_bounds("section-Shortcuts")
        .expect("the Shortcuts section");
    settings_cx.simulate_click(section.center(), Modifiers::none());
    settings_cx.run_until_parked();
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Alias for Echo: ec"),
        "the alias survived the restart, {json}"
    );
}

#[gpui::test]
fn a_refused_alias_shows_the_reason_and_keeps_editing(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    // Say hello already has the alias "hi", so giving it to Echo is
    // refused.
    seed(&data, &[(command_id(&hello), "hi")], &[]);
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query, &hello]);

    // Another command's alias, a space, and over the limit: each refusal
    // shows next to the field and keeps the editor open for another try.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    for (typed, reason) in [
        (
            "hi",
            "“hi” is already the alias of Say hello: change it there first, or choose another",
        ),
        ("two words", "An alias is one word, without spaces"),
        (
            "abcdefghijklmnopqrstuvwxyz0123456",
            "An alias has at most 32 characters",
        ),
    ] {
        settings_cx.simulate_input(typed);
        settings_cx.simulate_keystrokes("enter");
        settings_cx.run_until_parked();
        assert!(
            settings_cx.debug_bounds("shortcut-alias-error").is_some(),
            "{typed} was refused"
        );
        assert!(
            settings_cx.debug_bounds("shortcut-editor").is_some(),
            "the editor stayed open"
        );
        assert!(
            settings_cx.debug_bounds("shortcut-status").is_none(),
            "no success was claimed"
        );
        let (_, json) = accessibility(&mut settings_cx);
        assert!(json.contains(reason), "{reason} is shown, {json}");
        // Clear the field for the next try.
        for _ in 0..typed.chars().count() {
            settings_cx.simulate_keystrokes("backspace");
        }
    }

    // Escaping closes the editor with nothing changed.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("shortcut-editor").is_none());
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Alias for Echo: none"),
        "the alias is unchanged, {json}"
    );
    assert!(
        !aliases_record(&data).contains("#echo"),
        "no record was written for Echo, {}",
        aliases_record(&data)
    );
    // Root search finds Say hello by its own alias, not Echo by a new one.
    cx.simulate_input("hi");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Say hello");
}

#[gpui::test]
fn escape_cancels_the_edit_without_changing_anything(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    seed(&data, &[(command_id(&query), "ec")], &[]);
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query]);

    // The editor opens filled with the current alias; changing it and
    // escaping leaves the alias as it was.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    settings_cx.simulate_keystrokes("backspace backspace");
    settings_cx.simulate_input("xy");
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("shortcut-editor").is_none());
    assert!(settings_cx.debug_bounds("shortcut-status").is_none());
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Alias for Echo: ec"),
        "the alias is unchanged, {json}"
    );
    // Nothing was written: the record is exactly what was seeded.
    let mut aliases = serde_json::Map::new();
    aliases.insert(command_id(&query), serde_json::json!("ec"));
    let expected =
        serde_json::json!({ "version": 1, "aliases": aliases, "fallbacks": [] }).to_string();
    assert_eq!(aliases_record(&data), expected);

    // Root search still finds the command by its alias, not by what was
    // typed and escaped.
    cx.simulate_input("xy");
    let view = settle(&window, cx);
    assert!(
        view.rows.iter().all(|row| row.title != "Echo"),
        "nothing matches the escaped text, {:?}",
        titles(&view)
    );
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Echo heard “hello”".into())
    );
}

#[gpui::test]
fn an_empty_commit_clears_the_alias(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    seed(&data, &[(command_id(&query), "ec")], &[]);
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query]);

    // An empty field, saved, removes the alias — as the alias form's does.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    settings_cx.simulate_keystrokes("backspace backspace");
    settings_cx.simulate_keystrokes("enter");
    let json = until_text(&mut settings_cx, "Echo has no alias now");
    assert!(
        json.contains("Alias for Echo: none"),
        "the cell shows none, {json}"
    );
    // The record no longer holds the alias.
    assert!(
        !aliases_record(&data).contains("\"ec\""),
        "the record was rewritten, {}",
        aliases_record(&data)
    );
    // And root search no longer finds the command by it: the alias, a
    // space and more text lists nothing without the alias.
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert!(
        view.rows.iter().all(|row| row.title != "Echo"),
        "no alias row is offered, {:?}",
        titles(&view)
    );
}

#[gpui::test]
fn a_change_that_cannot_be_recorded_explains_and_keeps_the_last_record(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    // A record Pane cannot read is never replaced, so every write fails.
    fs::create_dir_all(data.path().join("extensions").join("aliases.json")).unwrap();
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query]);

    // The alias applies at once — the page shows it — but cannot be
    // recorded, which puts back what was last recorded: none. The status
    // line explains, and the page follows the restored record.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    settings_cx.simulate_input("ec");
    settings_cx.simulate_keystrokes("enter");
    let json = until_text(&mut settings_cx, "Could not keep the change:");
    assert!(
        json.contains("Alias for Echo: none"),
        "the alias was put back, {json}"
    );

    // Root search follows the restored record: the alias, a space and
    // more text lists nothing.
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert!(
        view.rows.iter().all(|row| row.title != "Echo"),
        "no alias row is offered, {:?}",
        titles(&view)
    );
}

#[gpui::test]
fn filtering_narrows_the_groups_and_their_commands(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query, &hello]);
    let mut settings_cx = settings_context(&settings, cx);

    let query_group = selector(format!("shortcut-group-{}", key_of(&query)));
    let hello_group = selector(format!("shortcut-group-{}", key_of(&hello)));
    let echo_row = selector(format!("shortcut-row-{}", command_id(&query)));
    let hello_row = selector(format!("shortcut-row-{}", command_id(&hello)));

    // Clicking the filter focuses it; typing narrows what is drawn, by
    // command and by extension, as displayed.
    let filter = settings_cx
        .debug_bounds("shortcut-filter")
        .expect("the filter");
    settings_cx.simulate_click(filter.center(), Modifiers::none());
    settings_cx.run_until_parked();
    settings_cx.simulate_input("echo");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds(echo_row).is_some(), "Echo matches");
    assert!(
        settings_cx.debug_bounds(hello_group).is_none(),
        "the Hello group is filtered out"
    );
    assert!(settings_cx.debug_bounds(hello_row).is_none());

    // A command of a group the filter matches by its extension's name
    // stays: "query" names the Query sample, whose commands are shown.
    settings_cx.simulate_keystrokes("backspace backspace backspace backspace");
    settings_cx.simulate_input("query");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds(query_group).is_some());
    assert!(settings_cx.debug_bounds(echo_row).is_some());
    assert!(
        settings_cx.debug_bounds(hello_group).is_none(),
        "the Hello group still does not match"
    );

    // The command's own name narrows to its row.
    settings_cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
    settings_cx.simulate_input("hello");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds(hello_row).is_some(),
        "Say hello matches"
    );
    assert!(settings_cx.debug_bounds(query_group).is_none());

    // Nothing matching says so; clearing the filter brings everything
    // back.
    settings_cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
    settings_cx.simulate_input("zzz");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcuts-empty").is_some(),
        "no matches are explained"
    );
    settings_cx.simulate_keystrokes("backspace backspace backspace");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds(query_group).is_some());
    assert!(settings_cx.debug_bounds(hello_group).is_some());
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn the_page_is_reachable_by_keyboard_and_names_its_controls(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query, &hello]);
    let mut settings_cx = settings_context(&settings, cx);

    // The filter field is named and takes the focus; Tab reaches the group
    // headers and the alias cells, in the order they are drawn.
    let filter = settings_cx
        .debug_bounds("shortcut-filter")
        .expect("the filter");
    settings_cx.simulate_click(filter.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert_eq!(
        focused_label(&mut settings_cx).as_deref(),
        Some("Filter commands and extensions")
    );
    settings_cx.simulate_keystrokes("tab");
    let label = focused_label(&mut settings_cx).unwrap_or_default();
    assert!(
        label.starts_with("Query sample, local folder"),
        "the group header is next: {label}"
    );
    settings_cx.simulate_keystrokes("tab");
    assert_eq!(
        focused_label(&mut settings_cx).as_deref(),
        Some("Alias for Echo: none")
    );

    // Enter opens the editor from the keyboard, as the click does; the
    // field holds the focus, typing edits and Enter commits.
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("shortcut-editor").is_some());
    settings_cx.simulate_input("ec");
    settings_cx.simulate_keystrokes("enter");
    // The status line says the keyboard edit committed, as the click's
    // did, and the focus returned to the row's cell.
    let _ = until_text(&mut settings_cx, "Typing “ec” now finds Echo");
    let (label, _) = accessibility(&mut settings_cx);
    assert_eq!(
        label.as_deref(),
        Some("Alias for Echo: ec"),
        "the commit returned the focus to the cell"
    );

    // The group header's keys expand and collapse it. The commit returned
    // the focus to Echo's cell; Tab from there reaches the Hello group's
    // header.
    let hello_group = selector(format!("shortcut-group-{}", key_of(&hello)));
    let hello_row = selector(format!("shortcut-row-{}", command_id(&hello)));
    settings_cx.simulate_keystrokes("tab");
    let label = focused_label(&mut settings_cx).unwrap_or_default();
    assert!(
        label.starts_with("Hello, local folder"),
        "the Hello header follows: {label}"
    );
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds(hello_group).is_some());
    assert!(
        settings_cx.debug_bounds(hello_row).is_none(),
        "the group collapsed"
    );
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds(hello_row).is_some(), "it expanded");

    // What the page did reached the launcher: the alias typed through the
    // keyboard works in root search.
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
}

#[gpui::test]
fn the_catalog_follows_disabling_enabling_and_uninstalling(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    seed(
        &data,
        &[(command_id(&hello), "hi")],
        &[(command_id(&hello), "ctrl+alt+h")],
    );
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&query, &hello]);
    let mut settings_cx = settings_context(&settings, cx);
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let identity = launcher
        .packages()
        .iter()
        .find(|package| package.title() == "Hello")
        .expect("Hello installed")
        .identity
        .clone();
    let hello_id = command_id(&hello);
    let alias_inactive = selector(format!("shortcut-alias-inactive-{hello_id}"));
    let hotkey_inactive = selector(format!("shortcut-hotkey-inactive-{hello_id}"));
    let hello_group = selector(format!("shortcut-group-{}", key_of(&hello)));

    // The page redraws by itself while the user does nothing in it: its
    // watcher sees the catalog change (the test's clock is what waits for
    // its tick, since nothing else advances time on the test platform).
    let tick = |settings_cx: &mut VisualTestContext| {
        settings_cx
            .cx
            .executor()
            .advance_clock(Duration::from_millis(600));
        settings_cx.run_until_parked();
    };

    // The hotkey record follows the package with the system too: it is
    // registered while Hello is installed and enabled, released while it
    // is disabled, and gone with it when it is uninstalled.
    let registered = |hotkeys: &FakeHotkeys, shortcut: &Shortcut| {
        hotkeys.registered.lock().unwrap().contains(shortcut)
    };
    let hello_hotkey = Shortcut::parse("ctrl+alt+h").unwrap();
    assert!(
        registered(&hotkeys, &hello_hotkey),
        "the seeded hotkey is registered"
    );

    // Disabling the package in the launcher marks its choices not active
    // and says why, without removing anything.
    block_on(launcher.set_enabled(&identity, false));
    tick(&mut settings_cx);
    assert!(
        settings_cx.debug_bounds(alias_inactive).is_some(),
        "the alias is marked not active"
    );
    assert!(
        settings_cx.debug_bounds(hotkey_inactive).is_some(),
        "the hotkey is marked not active"
    );
    let _ = until_text(&mut settings_cx, "Not active: Hello is disabled");
    assert!(settings_cx.debug_bounds(hello_group).is_some());
    // The alias stays recorded: the record on disk is untouched, and the
    // hotkey is released with the system.
    assert!(aliases_record(&data).contains("\"hi\""));
    assert!(!registered(&hotkeys, &hello_hotkey));

    // Enabling the package brings the choices back to active.
    block_on(launcher.set_enabled(&identity, true));
    tick(&mut settings_cx);
    assert!(settings_cx.debug_bounds(alias_inactive).is_none());
    assert!(settings_cx.debug_bounds(hotkey_inactive).is_none());
    assert!(registered(&hotkeys, &hello_hotkey));

    // Uninstalling the package removes its group — and forgets its
    // choices, which no group of their own brings back.
    block_on(launcher.uninstall(&identity, SavedData::Keep));
    tick(&mut settings_cx);
    assert!(
        settings_cx.debug_bounds(hello_group).is_none(),
        "the Hello group is gone"
    );
    assert!(
        settings_cx
            .debug_bounds("shortcut-group-not-installed")
            .is_none(),
        "its choices were forgotten, not left as records"
    );
    assert!(
        !aliases_record(&data).contains("\"hi\""),
        "the record was rewritten without them, {}",
        aliases_record(&data)
    );
    assert!(!registered(&hotkeys, &hello_hotkey));
    // The other package's group is still there.
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-group-{}", key_of(&query))))
            .is_some()
    );
}
