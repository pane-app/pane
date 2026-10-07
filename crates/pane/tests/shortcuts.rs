//! The Settings window's Shortcuts page, on GPUI's test platform: the
//! installed commands grouped by their extensions with their aliases and
//! global hotkeys, the alias edited inline — committed, refused,
//! cancelled and cleared — then reached from root search; the hotkey
//! recorded inline — registered, swapped, refused for collisions with
//! another command and with the Open Pane binding, refused when another
//! application has the keys, rolled back when the record cannot be
//! written, cleared, opened from another application, and kept through
//! the package lifecycle and a restart; the filter; and the catalog
//! following the package lifecycle and a restart, with the window's own
//! watcher asking for the redraw. The packages are the query sample
//! (Echo, which takes a query) and the Rust sample (Hello); the hotkey
//! system is a fake that accepts every registration unless told another
//! application holds a shortcut.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext,
    WindowHandle, prelude::*, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, SavedData, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/paint.rs"]
mod paint;

use settle::{settle, settle_shown};

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/setup.rs"]
mod setup;
#[path = "support/wait.rs"]
mod wait;

use a11y::{accessibility, focused_label};
use setup::settings_shortcut;
use wait::{frame, settle_frames, until};

/// The keystrokes of the Open Pane default on this platform: Cmd+Space's
/// Option part on macOS, Ctrl+Alt+Space elsewhere.
fn open_pane_keystrokes() -> &'static str {
    if cfg!(target_os = "macos") {
        "alt-space"
    } else {
        "ctrl-alt-space"
    }
}

/// A fake hotkey system that accepts every registration unless another
/// application holds the shortcut, so the recorded hotkeys are active.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
    taken: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        if self.taken.lock().unwrap().contains(shortcut) {
            return Err(HotkeyError::Taken);
        }
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

/// Writes a package folder whose one command declares that it runs on no
/// operating system, so the page explains it and offers no recording.
fn unavailable_package(folder: &Path) -> PathBuf {
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
  "title": "Nowhere",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "nowhere", "title": "Say nothing", "component": "nowhere.wasm", "platforms": [] }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("nowhere.wasm")).unwrap();
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

/// The hotkeys.json record as it stands on disk.
fn hotkeys_record(data: &TempDir) -> String {
    fs::read_to_string(data.path().join("extensions").join("hotkeys.json")).unwrap()
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

/// The group with `key`'s disclosure as the last frame drew it: its look —
/// 0 collapsed, 1 expanded — while the disclosure is in flight, which the
/// chevron's angle and the commands' arrival both follow; `None` when the
/// frame drew it settled, which is also all reduced motion ever reports.
/// See [`SettingsWindow::group_disclosure`].
fn disclosure(
    settings: &WindowHandle<SettingsWindow>,
    key: &str,
    cx: &mut VisualTestContext,
) -> Option<f32> {
    settings
        .read_with(cx, |window, _| window.group_disclosure(key))
        .expect("the Settings window is open")
}

/// The group with `key`'s arriving commands as the last frame drew them:
/// the whole block's (offset from rest in px, opacity) while the
/// disclosure is in flight; `None` when the frame drew them settled,
/// which is also all reduced motion ever draws. See
/// [`SettingsWindow::group_arrival`].
fn group_arrival(
    settings: &WindowHandle<SettingsWindow>,
    key: &str,
    cx: &mut VisualTestContext,
) -> Option<(f32, f32)> {
    settings
        .read_with(cx, |window, _| window.group_arrival(key))
        .expect("the Settings window is open")
}

/// Waits until the focused label satisfies `check`, then returns it.
/// Polled, not read once: the captured tree can follow the drawn frame
/// by one on the Windows test platform, as [`until_text`]'s comment
/// says.
fn focused_label_eventually(cx: &mut VisualTestContext, check: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let label = focused_label(cx).unwrap_or_default();
        if check(&label) {
            return label;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the focus to move: {label}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Clicks the group with `key`'s header, toggling it. The pointer moves
/// onto the header first, as a user's does: a click's landing alone does
/// not tell a row it is hovered, and the wash the header keeps would
/// never settle with the layout saying the pointer is gone and the paint
/// saying it is there.
fn click_group(cx: &mut VisualTestContext, key: &str) {
    let header = cx
        .debug_bounds(selector(format!("shortcut-group-{key}")))
        .expect("the group header");
    cx.simulate_mouse_move(header.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(header.center(), Modifiers::none());
    cx.run_until_parked();
}

/// Moves the pointer off the window, as a user's does when it leaves —
/// for the reason [`click_group`] gives, from the leaving side: content
/// that changes under a parked pointer leaves the wash it implied
/// running until the pointer's next real move.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
}

/// Starts the inline hotkey recorder for the command `id` by clicking
/// its cell, in a fresh Settings window's context.
fn record_hotkey(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
    id: &str,
) -> VisualTestContext {
    let mut settings_cx = settings_context(settings, cx);
    let cell = settings_cx
        .debug_bounds(selector(format!("shortcut-hotkey-{id}")))
        .expect("the hotkey cell");
    settings_cx.simulate_click(cell.center(), Modifiers::none());
    settings_cx.run_until_parked();
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; ")),
        "the recorder is listening: {label:?}"
    );
    settings_cx
}

/// Presses the hotkey `shortcut`, as the system's adapter would report it
/// while any application has focus (the clock moved past the window's
/// Open Pane repeat guard, so two of these are two genuine presses).
fn press(window: &Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
}

/// The launcher window's handle, for focus checks.
fn handle_of(cx: &mut VisualTestContext) -> WindowHandle<LauncherWindow> {
    cx.update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window")
}

/// Whether the window `handle` is the focused one, as the platform
/// reports.
fn is_active<T: Render + 'static>(handle: &WindowHandle<T>, cx: &mut VisualTestContext) -> bool {
    cx.cx.update(|cx| handle.is_active(cx)).unwrap_or(false)
}

/// Whether the launcher window is hidden by the Open Pane hotkey.
fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
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
    // the row that sends the text, and Enter sends it. Echo's toast says
    // what it heard.
    cx.simulate_input("ec hello");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Echo");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Send “hello” · alias ec")
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Echo heard “hello”".into())
    );

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
        settle_shown(&window, cx),
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
        settle_shown(&window, cx),
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

    // Tab from the alias cell reaches the row's hotkey cell, and Enter
    // starts its recorder, as the click does; Escape cancels it, and the
    // focus stays on the cell, a tab stop again.
    settings_cx.simulate_keystrokes("tab");
    assert_eq!(
        focused_label(&mut settings_cx).as_deref(),
        Some("Hotkey for Echo: none")
    );
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; Hotkey for Echo")),
        "the recorder is listening: {label:?}"
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(
        focused_label(&mut settings_cx).as_deref(),
        Some("Hotkey for Echo: none")
    );

    // The group header's keys expand and collapse it. Tab from the hotkey
    // cell reaches the Hello group's header.
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

#[gpui::test]
fn a_hotkey_recorded_inline_swaps_the_registration_and_opens_the_command(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    // Say hello has Ctrl+Alt+P recorded, registered at startup beside the
    // Open Pane default.
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let hello_id = command_id(&hello);
    let old = Shortcut::parse("ctrl+alt+p").unwrap();
    let open_pane = Shortcut::open_pane_default();
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![open_pane.clone(), old.clone()]
    );

    // Clicking the hotkey cell starts the recorder, which holds the focus:
    // the keys pressed next are captured — the sidebar's navigation does
    // not act, and the page stays.
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("down up");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcuts-title").is_some(),
        "the sidebar did not move to another page"
    );
    assert!(
        settings_cx.debug_bounds("appearance").is_none(),
        "no other page is showing"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; ")),
        "the recorder is still listening: {label:?}"
    );

    // The keys pressed are the binding: the new one is registered before
    // the old one is released, the record is written by the same write the
    // hotkey screen makes, and the status line says what it came to.
    settings_cx.simulate_keystrokes("ctrl-alt-g");
    let recorded = Shortcut::parse("ctrl+alt+g").unwrap();
    let json = until_text(&mut settings_cx, &format!("{recorded} now opens Say hello"));
    assert!(
        json.contains(&format!("Hotkey for Say hello: {recorded}")),
        "the cell shows the recorded hotkey, {json}"
    );
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![open_pane.clone(), recorded.clone()],
        "the old registration was released for the new one"
    );
    assert!(
        hotkeys_record(&data).contains("\"ctrl+alt+g\""),
        "the record holds it, {}",
        hotkeys_record(&data)
    );

    // A press, as the system would report it while another application
    // has focus, opens the command in the existing launcher; the released
    // binding opens nothing.
    press(&window, &recorded, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);
    assert_eq!(view.title, "Rust sample", "the guest's own view");
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    press(&window, &old, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing opened");
}

#[gpui::test]
fn escape_cancels_and_a_key_without_a_modifier_is_explained(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let hello_id = command_id(&hello);
    let old = Shortcut::parse("ctrl+alt+p").unwrap();

    // A key without Ctrl, Alt or Super is explained under the cell and
    // keeps the recorder listening, since it would take over typing.
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("b");
    let json = until_text(&mut settings_cx, "does not take over typing");
    assert!(
        json.contains("does not take over typing"),
        "the key without a modifier is explained, {json}"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; ")),
        "the recorder keeps listening: {label:?}"
    );

    // Escape cancels: nothing was registered, released or written, and
    // the cell shows the binding it had, with no status line claimed.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcut-status").is_none(),
        "no status line was claimed"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert_eq!(
        label.as_deref(),
        Some(format!("Hotkey for Say hello: {old}").as_str())
    );
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![Shortcut::open_pane_default(), old.clone()]
    );
    let mut seeded = serde_json::Map::new();
    seeded.insert(hello_id, serde_json::json!("ctrl+alt+p"));
    let expected = serde_json::json!({ "version": 1, "hotkeys": seeded }).to_string();
    assert_eq!(hotkeys_record(&data), expected, "nothing was written");

    // And the binding still opens the command.
    press(&window, &old, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);
}

#[gpui::test]
fn collisions_with_another_command_and_with_open_pane_are_refused(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    // Say hello has Ctrl+Alt+P; the Open Pane default is registered beside
    // it at startup.
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&query, &hello]);
    let echo_id = command_id(&query);
    let taken = Shortcut::parse("ctrl+alt+p").unwrap();
    let open_pane = Shortcut::open_pane_default();

    // Recording another command's keys for Echo is refused: a binding is
    // never silently overwritten. The reason shows under the cell and the
    // recorder keeps listening.
    let mut settings_cx = record_hotkey(&settings, cx, &echo_id);
    settings_cx.simulate_keystrokes("ctrl-alt-p");
    let json = until_text(&mut settings_cx, "already opens Say hello");
    assert!(
        json.contains(&format!(
            "{taken} already opens Say hello: remove it there first, or press another shortcut."
        )),
        "the collision is explained, {json}"
    );

    // So are the keys the Open Pane binding holds: Pane's own binding
    // keeps working and nothing is written for Echo.
    settings_cx.simulate_keystrokes(open_pane_keystrokes());
    let json = until_text(&mut settings_cx, "opens Pane itself");
    assert!(
        json.contains(&format!(
            "{open_pane} opens Pane itself: choose another shortcut for Echo, or change Pane's \
             hotkey in Settings."
        )),
        "the Open Pane collision is explained, {json}"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; ")),
        "the recorder keeps listening: {label:?}"
    );
    assert!(
        !hotkeys_record(&data).contains("#echo"),
        "nothing was written"
    );

    // Escape cancels, and the two bindings work independently: with the
    // Settings window holding focus, Pane's own binding summons the
    // launcher, and the next press — the launcher now focused — hides it.
    // The command's binding opens its command after.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![open_pane.clone(), taken.clone()]
    );
    let handle = handle_of(cx);
    press(&window, &open_pane, cx);
    cx.run_until_parked();
    assert!(is_active(&handle, cx), "the launcher took focus");
    assert!(!hidden(&window, cx), "the launcher was not hidden");
    press(&window, &open_pane, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the focused launcher hid");
    press(&window, &taken, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);
}

#[gpui::test]
fn a_registration_another_application_has_is_refused_and_keeps_the_binding(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let hello_id = command_id(&hello);
    let old = Shortcut::parse("ctrl+alt+p").unwrap();
    let wanted = Shortcut::parse("ctrl+alt+b").unwrap();

    // Another application has Ctrl+Alt+B: the system refuses the
    // registration, which is explained apart from a collision with Pane's
    // own bindings, and the working binding is untouched.
    hotkeys.taken.lock().unwrap().push(wanted.clone());
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    let json = until_text(&mut settings_cx, "cannot be used");
    assert!(
        json.contains(&format!(
            "{wanted} cannot be used: another application or the system already uses it. Press \
             another shortcut."
        )),
        "the system's refusal is explained, {json}"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_some_and(|label| label.starts_with("Recording; ")),
        "the recorder keeps listening: {label:?}"
    );
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![Shortcut::open_pane_default(), old.clone()]
    );
    press(&window, &old, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);

    // The keys free now, the same try lands: the binding is replaced.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    hotkeys.taken.lock().unwrap().clear();
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    until_text(&mut settings_cx, &format!("{wanted} now opens Say hello"));
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![Shortcut::open_pane_default(), wanted.clone()]
    );
    assert!(
        hotkeys_record(&data).contains("\"ctrl+alt+b\""),
        "the record holds it, {}",
        hotkeys_record(&data)
    );
}

#[gpui::test]
fn a_save_that_fails_rolls_the_registration_back(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let hello_id = command_id(&hello);
    let kept = Shortcut::parse("ctrl+alt+p").unwrap();
    let wanted = Shortcut::parse("ctrl+alt+b").unwrap();

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    let record = data.path().join("extensions").join("hotkeys.json");
    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();

    // The change registers and takes effect on the page, but cannot be
    // recorded: what was last recorded is back, with the registration
    // following it, and the status line explains.
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("ctrl-alt-b");
    let json = until_text(&mut settings_cx, "Could not keep the change:");
    assert!(
        json.contains(&format!("Hotkey for Say hello: {kept}")),
        "the kept binding is back on the page, {json}"
    );
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![Shortcut::open_pane_default(), kept.clone()],
        "the registration was rolled back"
    );

    // The kept binding still opens the command, and the one that could
    // not be saved opens nothing.
    press(&window, &kept, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    press(&window, &wanted, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing opened");
}

#[gpui::test]
fn a_recorded_hotkey_survives_a_restart_and_the_package_lifecycle(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let hello_id = command_id(&hello);
    let recorded = Shortcut::parse("ctrl+alt+g").unwrap();

    // Record a hotkey inline; the write lands.
    let mut settings_cx = record_hotkey(&settings, cx, &hello_id);
    settings_cx.simulate_keystrokes("ctrl-alt-g");
    until_text(&mut settings_cx, &format!("{recorded} now opens Say hello"));
    until(&mut settings_cx, |_| {
        hotkeys_record(&data)
            .contains("\"ctrl+alt+g\"")
            .then_some(())
    });
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let identity = launcher
        .packages()
        .iter()
        .find(|package| package.title() == "Hello")
        .expect("Hello installed")
        .identity
        .clone();

    // Disabling the package stops the registration and explains the
    // binding as not active; enabling restores it.
    let tick = |settings_cx: &mut VisualTestContext| {
        settings_cx
            .cx
            .executor()
            .advance_clock(Duration::from_millis(600));
        settings_cx.run_until_parked();
    };
    block_on(launcher.set_enabled(&identity, false));
    tick(&mut settings_cx);
    let json = until_text(&mut settings_cx, "Not active: Hello is disabled");
    assert!(
        json.contains(&format!("Hotkey for Say hello: {recorded}")),
        "the choice is kept and explained, {json}"
    );
    assert!(!hotkeys.registered.lock().unwrap().contains(&recorded));
    block_on(launcher.set_enabled(&identity, true));
    tick(&mut settings_cx);
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-hotkey-inactive-{hello_id}")))
            .is_none(),
        "the binding is active again"
    );
    assert!(hotkeys.registered.lock().unwrap().contains(&recorded));

    // A fresh application over the same data folder registers what the
    // record holds, the Shortcuts page shows it, and the binding opens
    // the command.
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let fresh_hotkeys = Arc::new(FakeHotkeys::default());
    let restarted = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_hotkeys(fresh_hotkeys.clone());
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(restarted, window, cx));
    cx.run_until_parked();
    // The fresh application registered the record's choice at startup,
    // and the Open Pane default beside it when its window attached the
    // launcher to the host settings.
    assert_eq!(
        fresh_hotkeys.registered.lock().unwrap().clone(),
        vec![recorded.clone(), Shortcut::open_pane_default()],
        "the fresh application registered the record's choice"
    );
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
        json.contains(&format!("Hotkey for Say hello: {recorded}")),
        "the hotkey survived the restart, {json}"
    );
    press(&window, &recorded, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);
}

#[gpui::test]
fn clearing_a_hotkey_by_keyboard_releases_and_forgets_it(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    seed(&data, &[], &[(command_id(&hello), "ctrl+alt+p")]);
    let (window, settings, hotkeys, cx) = open(cx, &data, &[&hello]);
    let cleared = Shortcut::parse("ctrl+alt+p").unwrap();

    // The Clear button beside the recorded hotkey is a tab stop: Tab from
    // the filter reaches the group header, the alias cell, the hotkey
    // cell, then the Clear button, and Enter clears.
    let mut settings_cx = settings_context(&settings, cx);
    let filter = settings_cx
        .debug_bounds("shortcut-filter")
        .expect("the filter");
    settings_cx.simulate_click(filter.center(), Modifiers::none());
    settings_cx.run_until_parked();
    for _ in 0..4 {
        settings_cx.simulate_keystrokes("tab");
    }
    assert_eq!(
        focused_label(&mut settings_cx).as_deref(),
        Some("Clear the hotkey for Say hello")
    );
    settings_cx.simulate_keystrokes("enter");
    let json = until_text(&mut settings_cx, "Say hello has no hotkey now");
    assert!(
        json.contains("Hotkey for Say hello: none"),
        "the cell shows none, {json}"
    );
    assert!(
        !json.contains("Clear the hotkey for Say hello"),
        "the Clear button is gone, {json}"
    );
    let (label, _) = accessibility(&mut settings_cx);
    assert_eq!(
        label.as_deref(),
        Some("Hotkey for Say hello: none"),
        "the focus returned to the hotkey cell"
    );

    // The registration is released and the record rewritten without it,
    // so the keys open nothing.
    assert_eq!(
        hotkeys.registered.lock().unwrap().clone(),
        vec![Shortcut::open_pane_default()]
    );
    assert!(
        !hotkeys_record(&data).contains("ctrl+alt+p"),
        "the record was rewritten without it, {}",
        hotkeys_record(&data)
    );
    press(&window, &cleared, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "nothing opened");
}

#[gpui::test]
fn a_command_unavailable_here_keeps_its_hotkey_display_only(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let nowhere = unavailable_package(&sources.path().join("nowhere"));
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&nowhere]);
    let nowhere_id = format!("{}#nowhere", key_of(&nowhere));

    // The command is unavailable on every system: its hotkey cell is a
    // plain label that explains, not a recorder — clicking it starts
    // nothing — and no Clear button is offered.
    let mut settings_cx = settings_context(&settings, cx);
    let cell = settings_cx
        .debug_bounds(selector(format!("shortcut-hotkey-{nowhere_id}")))
        .expect("the hotkey cell");
    settings_cx.simulate_click(cell.center(), Modifiers::none());
    settings_cx.run_until_parked();
    let (label, json) = accessibility(&mut settings_cx);
    assert!(
        label
            .as_deref()
            .is_none_or(|label| !label.starts_with("Recording; ")),
        "no recorder started: {label:?}"
    );
    assert!(
        json.contains("Not active: Not available on"),
        "the unavailability is explained, {json}"
    );
    assert!(
        json.contains("this command supports no operating system"),
        "the reason names the command, {json}"
    );
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-hotkey-clear-{nowhere_id}")))
            .is_none(),
        "no Clear button is offered"
    );
    // The launcher itself sits at root search, untouched.
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(matches!(view.screen, Screen::Root { .. }));
}

/// The setup the disclosure tests share: the Settings window on the
/// Shortcuts page over the Query sample, with the section arrival that
/// brought the page already settled — the page starts idle, and so does
/// the group.
fn opened_shortcuts(
    cx: &mut TestAppContext,
    data: &TempDir,
    packages: &[&Path],
) -> (
    Entity<LauncherWindow>,
    WindowHandle<SettingsWindow>,
    VisualTestContext,
) {
    let (window, settings, _hotkeys, cx) = open(cx, data, packages);
    let mut settings_cx = settings_context(&settings, cx);
    // Drain the section arrival that brought the page: what follows
    // starts from a settled, idle window.
    settle_frames(&mut settings_cx);
    assert_eq!(
        frame(&mut settings_cx, Duration::ZERO),
        0,
        "a settled page asks for no frame"
    );
    (window, settings, settings_cx)
}

/// The page's controls are the Settings families (#99), not launcher rows:
/// the filter is a 34px well (black 24% under its ring), a command's alias
/// cell a 30px well in the command's settings row, and a group's header a
/// list header that takes the sidebar item's white 5% under the pointer
/// at once — no fade, no frame — with no root-row wash anywhere.
#[gpui::test]
fn the_pages_controls_are_the_settings_families(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (_window, _settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query]);
    let sc = &mut settings_cx;
    pointer_leaves(sc);
    settle_frames(sc);

    let filter = sc.debug_bounds("shortcut-filter").expect("the filter");
    assert_eq!(filter.size.height, px(34.), "a field's well");
    assert!(paint::paints_fill_at(sc, filter, 0x0000003D), "black 24%");

    let id = command_id(&query);
    let alias = sc
        .debug_bounds(selector(format!("shortcut-alias-{id}")))
        .expect("the alias cell");
    assert_eq!(alias.size.height, px(30.), "an inline well");
    assert!(paint::paints_fill_at(sc, alias, 0x0000003D), "black 24%");
    let row = sc
        .debug_bounds(selector(format!("shortcut-row-{id}")))
        .expect("the command's row");
    assert!(row.size.height >= px(44.), "a settings row's floor");
    for root_wash in [0xFFFFFF09, 0xFFFFFF16] {
        assert!(!paint::paints_fill_at(sc, row, root_wash));
    }

    let header = sc
        .debug_bounds(selector(format!("shortcut-group-{}", key_of(&query))))
        .expect("the group's header");
    sc.simulate_mouse_move(header.center(), None::<MouseButton>, Modifiers::none());
    sc.run_until_parked();
    assert!(
        paint::paints_fill_at(sc, header, 0xFFFFFF0D),
        "the header's white 5% under the pointer"
    );
    assert_eq!(frame(sc, Duration::ZERO), 0, "the hover asks for no frame");
    pointer_leaves(sc);
    sc.run_until_parked();
    assert_eq!(settle_frames(sc), 0, "the window is idle");
}

/// Expanding a group discloses on the shared policy: the rows mount at
/// once — the real layout, hit targets and all — and arrive as one block
/// over the tiny shift and fade, while the chevron turns on the same
/// timeline (the look drives both). Collapsing unmounts the rows at once
/// — the departing content never lingers — while the chevron turns back.
/// Both directions complete within their bounded span and leave the
/// window asking for no frame at all.
#[gpui::test]
fn expanding_and_collapsing_disclose_one_coordinated_block(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (window, settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query]);
    let key = key_of(&query);
    let row = selector(format!("shortcut-row-{}", command_id(&query)));
    let commands = selector(format!("commands-{key}"));

    // Groups start expanded and settled.
    assert!(settings_cx.debug_bounds(row).is_some());
    assert!(disclosure(&settings, &key, &mut settings_cx).is_none());

    // Collapsing unmounts the rows at once — no fading-out departure to
    // catch a click or an accessibility node — while the disclosure
    // starts the chevron's turn back from fully expanded.
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds(row).is_none(),
        "the rows are gone at once"
    );
    let look = disclosure(&settings, &key, &mut settings_cx).expect("the chevron is turning");
    assert!(
        look > 0.95,
        "the collapse starts from the expanded endpoint: {look}"
    );
    // Frames pass: the turn completes within its span, and the window
    // goes idle.
    assert!(frame(&mut settings_cx, Duration::from_millis(190)) >= 1);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "the turn completed"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a settled page asks for no frame"
    );

    // Expanding mounts the rows at once and arrives the whole block: the
    // commands container starts the full shift below rest and faint, and
    // the disclosure — the chevron's own timeline — starts from the
    // collapsed endpoint, so the two begin together.
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds(row).is_some(),
        "the rows are mounted at once"
    );
    let look = disclosure(&settings, &key, &mut settings_cx).expect("the group is disclosing");
    assert!(
        look < 0.05,
        "the expansion starts from the collapsed endpoint: {look}"
    );
    let (arrival_offset, _) =
        group_arrival(&settings, &key, &mut settings_cx).expect("the commands are arriving");
    let in_flight = settings_cx.debug_bounds(commands).expect("the commands");
    let header = settings_cx
        .debug_bounds(selector(format!("shortcut-group-{key}")))
        .expect("the group header");
    // Frames pass, and the arrival progresses without restarting.
    assert!(frame(&mut settings_cx, Duration::from_millis(40)) >= 1);
    let progressed = disclosure(&settings, &key, &mut settings_cx).unwrap();
    assert!(
        progressed > look,
        "the disclosure progressed: {progressed} from {look}"
    );
    // Past the disclosure span, the next delivered frame lands the block
    // at rest and asks for no further frame.
    assert!(frame(&mut settings_cx, Duration::from_millis(150)) >= 1);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "the disclosure completed"
    );
    let settled = settings_cx.debug_bounds(commands).expect("the commands");
    assert_eq!(
        in_flight.origin.y - settled.origin.y,
        px(arrival_offset),
        "the whole block of rows was shifted exactly the arrival's offset below its rest"
    );
    // The offset is paint only: the layout — the page's real height, so
    // its scroll range — was the expanded one from the first frame, and
    // the content above the block never moved with the disclosure.
    assert_eq!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-group-{key}")))
            .expect("the group header"),
        header,
        "the content above the arriving block stayed anchored"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a settled page asks for no frame"
    );
    let _ = window;
}

/// The arriving commands are interactive from the first frame: while the
/// disclosure is still in flight, clicking a row's alias cell opens its
/// editor — the state has already flipped; only the paint eases in.
#[gpui::test]
fn the_arriving_commands_are_interactive_from_the_first_frame(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (window, settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query]);
    let key = key_of(&query);

    // Collapse, settled — then expand, and click a row's alias cell while
    // the arrival is still in flight.
    click_group(&mut settings_cx, &key);
    settle_frames(&mut settings_cx);
    click_group(&mut settings_cx, &key);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_some(),
        "the arrival is in flight"
    );
    let cell = settings_cx
        .debug_bounds(selector(format!("shortcut-alias-{}", command_id(&query))))
        .expect("the alias cell, mid-arrival");
    // The pointer moves onto the cell before pressing it, as a user's
    // does, so the wash it starts settles instead of stranding between
    // the layout's and the paint's say on the hover.
    settings_cx.simulate_mouse_move(cell.center(), None::<MouseButton>, Modifiers::none());
    settings_cx.simulate_click(cell.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcut-editor").is_some(),
        "the arriving content took the input"
    );
    // And the disclosure still completes after the editor opened.
    settle_frames(&mut settings_cx);
    assert!(disclosure(&settings, &key, &mut settings_cx).is_none());
    let _ = window;
}

/// Reversing a disclosure mid-flight retargets from the presentation on
/// screen: the collapse continues from the look the expansion had
/// reached — no restart from an endpoint, no flash — while the rows
/// unmount at once, and the reversed turn completes within its span and
/// leaves the page idle.
#[gpui::test]
fn reversing_a_disclosure_retargets_from_where_it_is(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (window, settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query]);
    let key = key_of(&query);
    let row = selector(format!("shortcut-row-{}", command_id(&query)));

    // Collapsed and settled, then expanding: the disclosure starts from
    // the collapsed endpoint.
    click_group(&mut settings_cx, &key);
    settle_frames(&mut settings_cx);
    click_group(&mut settings_cx, &key);
    let expanding = disclosure(&settings, &key, &mut settings_cx).expect("it is disclosing");
    assert!(expanding < 0.05);

    // Part way through the expansion, reverse it. The rows unmount at
    // once, and the look the collapse turns back from is the one on
    // screen — the expansion's interrupted value, not the expanded
    // endpoint a fresh collapse would start from.
    assert!(frame(&mut settings_cx, Duration::from_millis(40)) >= 1);
    let mid = disclosure(&settings, &key, &mut settings_cx).expect("still in flight");
    assert!(mid > expanding && mid < 0.95, "mid-flight: {mid}");
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds(row).is_none(),
        "the rows are gone at once"
    );
    let reversed = disclosure(&settings, &key, &mut settings_cx).expect("the turn reversed");
    assert!(
        (reversed - mid).abs() < 0.05,
        "the reversal continued from {mid}: {reversed}"
    );

    // The reversed turn completes within its span and leaves the page
    // asking for no frame.
    assert!(frame(&mut settings_cx, Duration::from_millis(190)) >= 1);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "the reversal completed"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a settled page asks for no frame"
    );
    let _ = window;
}

/// Focus inside a collapsing group moves to the header that controls it,
/// so nothing hidden can keep the input: an open inline editor for one
/// of the group's commands closes without committing (as Escape closes
/// it), and a focused alias cell gives the focus up the same way. Enter
/// on the header afterwards re-expands the group, which proves the
/// header — not the unmounted rows — holds the keyboard focus.
#[gpui::test]
fn collapsing_a_group_moves_focus_inside_it_to_its_header(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (window, settings, _hotkeys, cx) = open(cx, &data, &[&query]);
    let key = key_of(&query);

    // The editor for the group's command is open, holds an uncommitted
    // edit, and holds the focus.
    let mut settings_cx = edit_alias(&settings, cx, &command_id(&query));
    settings_cx.simulate_input("zz");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcut-editor").is_some(),
        "the editor is open"
    );

    // Collapsing unmounts the rows — the editor with them, closed
    // without committing — and the focus moves to the header.
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds("shortcut-editor").is_none(),
        "the editor unmounted with the rows"
    );
    let label = focused_label_eventually(&mut settings_cx, |label| {
        label.starts_with("Query sample, local folder")
    });
    assert!(
        label.starts_with("Query sample, local folder"),
        "the header that controls the group holds the focus: {label}"
    );

    // Enter on the focused header re-expands the group — the header
    // takes the input the hidden rows cannot — and the uncommitted edit
    // is gone: the cell shows none, and nothing reached the record.
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds(selector(format!("shortcut-row-{}", command_id(&query))))
            .is_some(),
        "the group re-expanded from the header"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Alias for Echo: none"),
        "the uncommitted edit was discarded, {json}"
    );
    // The record never came to be: this data dir was never seeded, and
    // closing the editor without committing writes nothing at all.
    assert!(
        !data.path().join("extensions").join("aliases.json").exists(),
        "nothing was written"
    );

    // The same move for a plain alias cell: tab to it, collapse, and the
    // focus returns to the header.
    settings_cx.simulate_keystrokes("tab");
    let label = focused_label_eventually(&mut settings_cx, |label| label == "Alias for Echo: none");
    assert_eq!(
        label.as_str(),
        "Alias for Echo: none",
        "the row's alias cell is focused"
    );
    click_group(&mut settings_cx, &key);
    let label = focused_label_eventually(&mut settings_cx, |label| {
        label.starts_with("Query sample, local folder")
    });
    assert!(
        label.starts_with("Query sample, local folder"),
        "the focus left the collapsing rows for the header: {label}"
    );
    let _ = window;
}

/// Returning to the Shortcuts page keeps its state — the filter and the
/// collapsed groups — without replaying the children's arrivals: the
/// switch's own section arrival is the only transition, the groups draw
/// settled, and a filter change afterwards still animates nothing.
#[gpui::test]
fn returning_to_the_shortcuts_page_keeps_its_state(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let hello = hello_package(&sources.path().join("hello"));
    let (window, settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query, &hello]);
    let hello_key = key_of(&hello);

    // The page's own state: a filter narrowed to the Hello group, and
    // that group collapsed.
    let filter = settings_cx
        .debug_bounds("shortcut-filter")
        .expect("the filter");
    settings_cx.simulate_click(filter.center(), Modifiers::none());
    settings_cx.run_until_parked();
    settings_cx.simulate_input("hello");
    settings_cx.run_until_parked();
    let hello_row = selector(format!("shortcut-row-{}", command_id(&hello)));
    let query_group = selector(format!("shortcut-group-{}", key_of(&query)));
    assert!(settings_cx.debug_bounds(hello_row).is_some());
    assert!(
        settings_cx.debug_bounds(query_group).is_none(),
        "the filter narrowed the page"
    );
    click_group(&mut settings_cx, &hello_key);
    // A filter shows the commands that match it, whatever the expanded
    // state: collapsing under one turns the chevron but keeps the rows
    // drawn — the collapsed state's own visibility comes once the filter
    // clears, below.
    assert!(
        settings_cx.debug_bounds(hello_row).is_some(),
        "the filter keeps showing the collapsed group's commands"
    );
    settle_frames(&mut settings_cx);

    // Switch away to General, then back to Shortcuts.
    let general = settings_cx
        .debug_bounds("section-General")
        .expect("the General section");
    settings_cx.simulate_mouse_move(general.center(), None::<MouseButton>, Modifiers::none());
    settings_cx.simulate_click(general.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("appearance").is_some(),
        "the General page's Appearance section is drawn"
    );
    let shortcuts = settings_cx
        .debug_bounds("section-Shortcuts")
        .expect("the Shortcuts section");
    settings_cx.simulate_mouse_move(shortcuts.center(), None::<MouseButton>, Modifiers::none());
    settings_cx.simulate_click(shortcuts.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("shortcuts-title").is_some(),
        "the Shortcuts page is drawn"
    );

    // The page's state survived the round trip: the filter still
    // narrows it, and the Hello group is still collapsed — a filter
    // shows the collapsed group's commands, so its collapse is proven
    // where it shows: once the filter clears below, its rows stay
    // unmounted. No group replays an arrival — the only transition in
    // flight is the section's own.
    assert!(
        settings_cx.debug_bounds(query_group).is_none(),
        "the filter was retained"
    );
    assert!(
        settings_cx.debug_bounds(hello_row).is_some(),
        "the filter still shows the collapsed group's commands"
    );
    assert!(
        disclosure(&settings, &hello_key, &mut settings_cx).is_none(),
        "no group replayed its arrival"
    );
    settle_frames(&mut settings_cx);
    // The pointer leaves the sidebar row it came home with, and the wash
    // it held settles with it: the frames the clearing phase counts are
    // the page's own, none of the pointer's.
    pointer_leaves(&mut settings_cx);
    settle_frames(&mut settings_cx);

    // Clearing the filter is a content update: the rows come back with
    // no transition at all, and the collapsed group's stay unmounted.
    // Click the filter first, as the user would: the collapse moved the
    // focus to the group's header, which switching sections never moved.
    let filter = settings_cx
        .debug_bounds("shortcut-filter")
        .expect("the filter");
    settings_cx.simulate_click(filter.center(), Modifiers::none());
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds(query_group).is_some(),
        "clearing the filter brought the rows back"
    );
    assert!(
        settings_cx.debug_bounds(hello_row).is_none(),
        "the collapsed group's rows stayed unmounted"
    );
    assert!(
        disclosure(&settings, &hello_key, &mut settings_cx).is_none(),
        "a filter change animates nothing"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a page without transitions asks for no frame"
    );
    let _ = window;
}

/// Reduced motion settles every disclosure at once: a toggle under it
/// starts no tween and asks for no frame, and reducing motion
/// mid-disclosure ends it on the next drawn frame. Either way the
/// collapsed-state semantics are real from the first frame — the rows
/// unmount and mount at once.
#[gpui::test]
fn reduced_motion_settles_disclosures_at_once(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let query = query_package(&sources.path().join("query"));
    let (window, settings, mut settings_cx) = opened_shortcuts(cx, &data, &[&query]);
    let key = key_of(&query);
    let row = selector(format!("shortcut-row-{}", command_id(&query)));

    // Under reduced motion a toggle starts no disclosure: the frame that
    // draws the change is already settled, and the window asks for no
    // frame for the presentation.
    settings_cx.update(|_, cx| cx.set_reduce_motion(true));
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds(row).is_none(),
        "the rows unmounted at once"
    );
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "reduced motion drew the collapse settled"
    );
    assert_eq!(
        frame(&mut settings_cx, Duration::ZERO),
        0,
        "the window asked for no frame for the presentation"
    );
    click_group(&mut settings_cx, &key);
    assert!(
        settings_cx.debug_bounds(row).is_some(),
        "the rows mounted at once"
    );
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "reduced motion drew the expansion settled"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "the window asked for no frame"
    );

    // Reduced motion engaged mid-disclosure ends it on the next frame.
    // Begin a collapse under full motion, then flip the preference.
    settings_cx.update(|_, cx| cx.set_reduce_motion(false));
    click_group(&mut settings_cx, &key);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_some(),
        "the disclosure began under full motion"
    );
    settings_cx.update(|_, cx| cx.set_reduce_motion(true));
    // The frame the disclosure had asked for draws settled, and asks for
    // nothing further.
    assert!(frame(&mut settings_cx, Duration::ZERO) >= 1);
    assert!(
        disclosure(&settings, &key, &mut settings_cx).is_none(),
        "the disclosure settled the moment reduced motion engaged"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "the window asked for no further frame"
    );
    let _ = window;
}
