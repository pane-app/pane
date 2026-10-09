//! The Keyboard page (#77): the bounded set of in-app navigation actions
//! — previous/next result, invoke selected action, back, return to root,
//! dismiss launcher and open Settings — rebindable through the Settings
//! window, with the rebinds taking effect in the launcher's window at
//! once, persisting and surviving a restart. Drives the real windows
//! through GPUI's test platform, as `window.rs` drives the launcher's:
//! the rebinds are recorded through the page's own recorders, and what
//! they change is observed through the window's input events, its footer
//! and menu hints, and the record on disk. The native side — what the
//! keys do on each operating system, IME composition through a real
//! input method — is recorded natively in
//! `docs/evidence/settings-77/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

// Pane registers no sample command (#162): the tests that drive the
// samples register them themselves.
#[path = "support/samples.rs"]
mod samples;

#[path = "../../pane-core/tests/support/service.rs"]
mod service;

use settle::settle;

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/setup.rs"]
mod setup;
#[path = "support/wait.rs"]
mod wait;

use a11y::a11y;
use setup::{
    CTRL, actions_shortcut, actions_shortcut_name, init_settings, settings_shortcut,
    settings_shortcut_name,
};
use wait::{until, until_record_holds};

/// The fake system: what Pane registered, for checking the launcher's
/// dismissal leaves the global hotkeys running (a press still works
/// after).
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

/// The launcher window over `launcher`, with the record of `data` in
/// force: the settings are initialized before the keys are bound, as the
/// binary does, so the bindings the record holds are the ones the window
/// starts with.
fn open_launcher<'a>(
    cx: &'a mut TestAppContext,
    launcher: Launcher,
    data: Option<&Path>,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    init_settings(data, cx);
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// The launcher window over the Rust sample alone.
fn open_sample<'a>(
    cx: &'a mut TestAppContext,
    data: Option<&Path>,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    open_launcher(
        cx,
        Launcher::new(Runtime::start(), samples::sample_commands()),
        data,
    )
}

/// The launcher window's handle, for liveness checks.
fn handle_of(cx: &mut VisualTestContext) -> WindowHandle<LauncherWindow> {
    cx.update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window")
}

/// Opens the Settings window over the launcher `cx` drives, as the
/// open-Settings binding does.
fn open_settings(cx: &mut VisualTestContext) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    let settings = cx
        .cx
        .update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SettingsWindow>())
                .next()
        })
        .expect("Settings opened");
    let settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    (settings, settings_cx)
}

/// The Settings window's Keyboard page: opened by the open-Settings
/// binding and the sidebar, its context handed back with the window it
/// belongs to.
fn keyboard_page(cx: &mut VisualTestContext) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let (settings, mut settings_cx) = open_settings(cx);
    // A tall window, so every action's row is in view below the Behavior
    // section.
    settings_cx.simulate_resize(gpui::size(gpui::px(860.), gpui::px(1000.)));
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-Keyboard");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("keyboard").is_some(),
        "the Keyboard page is showing"
    );
    (settings, settings_cx)
}

/// The keystroke that returns to root on this platform.
fn root_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-escape"
    } else {
        "shift-escape"
    }
}

/// The keystroke that dismisses the launcher on this platform.
fn dismiss_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-w"
    }
}

/// Records `keystroke` as the binding of the action whose row selector is
/// `row`, through the page's recorder: the row is clicked, the keys are
/// pressed, and the page answers.
fn record(settings_cx: &mut VisualTestContext, row: &'static str, keystroke: &str) {
    click(settings_cx, row);
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes(keystroke);
    settings_cx.run_until_parked();
}

/// Clicks the element whose debug selector is `selector` in `cx`.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// The accessible nodes of the window `cx` drives, as the window tests
/// read them.
fn accessible_nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let json = a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The node with this role and label.
fn node<'a>(nodes: &'a [serde_json::Value], role: &str, label: &str) -> &'a serde_json::Value {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
        .unwrap_or_else(|| panic!("no {role} labelled {label:?} in {nodes:#?}"))
}

/// Runs `cx` until the settings record in `data` holds `field` mapped to
/// `id`, as the Keyboard page writes them: the save the page started is
/// written off the window's thread.
fn until_record(cx: &mut VisualTestContext, data: &Path, field: &str, id: &str) {
    let held = format!("\"{field}\": \"{id}\"");
    until_record_holds(cx, data, &held);
}

/// Whether the record in `data` holds `field` mapped to `id`.
fn record_holds(data: &Path, field: &str, id: &str) -> bool {
    fs::read_to_string(data.join("settings.json"))
        .map(|text| text.contains(&format!("\"{field}\": \"{id}\"")))
        .unwrap_or(false)
}

/// Whether the launcher window is hidden (the state the window drove, as
/// the test platform's own visibility is not observable from outside
/// GPUI).
fn hidden(window: &gpui::Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Presses the Open Pane hotkey `shortcut`, as the system's adapter would
/// report it while any application had focus, past the repeat guard.
fn press(window: &gpui::Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
}

/// The number of Settings windows open.
fn settings_windows(cx: &TestAppContext) -> usize {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter(|window| window.downcast::<SettingsWindow>().is_some())
            .count()
    })
}

/// Waits until the open custom view shows `expected` as its value, which
/// it does once the guest's answer to the last event has arrived.
fn until_color(window: &gpui::Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let shown = cx.read_entity(window, |window, _| {
            window
                .launcher()
                .view()
                .custom_view()
                .map(|view| view.frame.value.as_str().to_owned())
        });
        if shown.as_deref() == Some(expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected}; the view shows {shown:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn a_rebind_takes_effect_at_once_is_saved_and_survives_a_restart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // Two results to move between, with the query keeping focus.
    cx.simulate_input("script sample");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));

    // Record Ctrl+N as the next result's binding.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-next-result", "ctrl-n");
    until_record(&mut settings_cx, data.path(), "next-result", "ctrl-n");

    // The launcher's keys follow at once: Ctrl+N moves the selection,
    // with the query still focused, and Down no longer does.
    cx.simulate_keystrokes("ctrl-n");
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes("down");
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "Down no longer moves the selection"
    );
    // The page shows the binding it holds.
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("Next result with {CTRL}+N")),
        "the page shows the binding, {tree}"
    );

    // A fresh application over the same record: the same bindings, from
    // the keys the first window starts with.
    let mut fresh = cx.cx.new_app();
    init_settings(Some(data.path()), &mut fresh);
    fresh.update(pane::bind_keys);
    let (window, fresh_cx) = fresh.add_window_view(|window, cx| {
        LauncherWindow::new(
            Launcher::new(Runtime::start(), samples::sample_commands()),
            window,
            cx,
        )
    });
    fresh_cx.simulate_input("script sample");
    settle(&window, fresh_cx);
    fresh_cx.simulate_keystrokes("ctrl-n");
    assert_eq!(
        settle(&window, fresh_cx).selected,
        Some(1),
        "the fresh window follows the record"
    );
    fresh_cx.simulate_keystrokes("down");
    assert_eq!(
        settle(&window, fresh_cx).selected,
        Some(1),
        "Down is not the record's binding"
    );
}

#[gpui::test]
fn the_footers_keycap_follows_the_invoke_binding(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // The default: the footer's action announces Enter, with the return
    // key's cap.
    let nodes = accessible_nodes(cx);
    let action = node(&nodes, "Button", "Open command");
    assert_eq!(action["keyboard_shortcut"].as_str(), Some("Enter"));
    node(&nodes, "Image", "Enter");

    // Record Ctrl+J as the invoke binding.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(
        &mut settings_cx,
        "keyboard-invoke-selected-action",
        "ctrl-j",
    );
    until_record(
        &mut settings_cx,
        data.path(),
        "invoke-selected-action",
        "ctrl-j",
    );

    // The keycaps follow: the button announces Ctrl+J, and the caps name
    // it — no return-key cap is left behind.
    let ctrl_j = format!("{CTRL}+J");
    let nodes = accessible_nodes(cx);
    let action = node(&nodes, "Button", "Open command");
    assert_eq!(action["keyboard_shortcut"].as_str(), Some(ctrl_j.as_str()));
    node(&nodes, "Image", &ctrl_j);
    assert!(
        !nodes.iter().any(|node| node["label"] == "Enter"),
        "no stale Enter keycap, {nodes:#?}"
    );

    // The new key opens the selected command; Enter no longer does.
    cx.simulate_keystrokes("ctrl-j");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: String::new()
        },
        "Enter opens nothing"
    );
}

/// A chord on the invoke action is shown and announced whole — every
/// modifier its own cap beside the return key, the button's shortcut the
/// full chord — and the chord, not the bare key, is what opens the
/// selection. Shift+Enter is never shown or read as Enter.
/// Open actions follows the Keyboard page: a recorded key opens the
/// panel, and the Actions button and the footer's hint show and announce
/// it.
#[gpui::test]
fn the_actions_binding_follows_the_keyboard_page(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    let nodes = accessible_nodes(cx);
    let button = node(&nodes, "Button", "Actions");
    assert_eq!(
        button["keyboard_shortcut"].as_str(),
        Some(actions_shortcut_name())
    );

    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-open-actions", "ctrl-j");
    until_record(&mut settings_cx, data.path(), "open-actions", "ctrl-j");

    let ctrl_j = format!("{CTRL}+J");
    let nodes = accessible_nodes(cx);
    let button = node(&nodes, "Button", "Actions");
    assert_eq!(button["keyboard_shortcut"].as_str(), Some(ctrl_j.as_str()));
    node(&nodes, "Image", &ctrl_j);
    cx.simulate_keystrokes("ctrl-j");
    settle(&window, cx);
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    cx.simulate_keystrokes("ctrl-j");
    settle(&window, cx);
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));
}

#[gpui::test]
fn a_chord_on_the_invoke_action_is_shown_announced_and_pressed_whole(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    let (_settings, mut settings_cx) = keyboard_page(cx);
    let ctrl_shift_j = format!("{CTRL}+Shift+J");
    for (keystroke, id, name) in [
        ("shift-enter", "shift-enter", "Shift+Enter"),
        ("ctrl-shift-j", "ctrl-shift-j", ctrl_shift_j.as_str()),
    ] {
        record(
            &mut settings_cx,
            "keyboard-invoke-selected-action",
            keystroke,
        );
        until_record(&mut settings_cx, data.path(), "invoke-selected-action", id);
        let nodes = accessible_nodes(cx);
        let action = node(&nodes, "Button", "Open command");
        assert_eq!(action["keyboard_shortcut"].as_str(), Some(name));
        node(&nodes, "Image", name);
        assert!(
            !nodes.iter().any(|node| node["label"] == "Enter"),
            "no bare Enter hint for {name}, {nodes:#?}"
        );

        // The chord opens the selected command; the bare Enter does not.
        cx.simulate_keystrokes("enter");
        let view = settle(&window, cx);
        assert!(
            matches!(view.screen, Screen::Root { .. }),
            "Enter alone opens nothing under {name}"
        );
        cx.simulate_keystrokes(keystroke);
        let view = settle(&window, cx);
        assert_eq!(
            (view.screen, view.title.as_str()),
            (Screen::Command, "Rust sample"),
            "{name} opens the selection"
        );
        cx.simulate_keystrokes("escape");
        settle(&window, cx);
    }
}

/// A long chord on the invoke action stays inside the footer of a narrow
/// launcher: the label gives way, the caps keep their size, and the
/// button never reaches past the strip's right edge.
#[gpui::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS's spelled-out caps outgrow the narrow footer: #117"
)]
fn a_long_invoke_chord_stays_inside_a_narrow_footer(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(
        &mut settings_cx,
        "keyboard-invoke-selected-action",
        "ctrl-alt-shift-pagedown",
    );
    until_record(
        &mut settings_cx,
        data.path(),
        "invoke-selected-action",
        "ctrl-alt-shift-pagedown",
    );
    cx.simulate_resize(gpui::size(gpui::px(300.), gpui::px(360.)));
    settle(&window, cx);
    let footer = cx.debug_bounds("status-idle").expect("the footer is drawn");
    let button = cx
        .debug_bounds("primary-action")
        .expect("the action is drawn");
    let caps = cx
        .debug_bounds("primary-action-keys")
        .expect("the chord is drawn");
    assert!(
        button.right() <= footer.right() && caps.right() <= button.right(),
        "the chord stays inside the footer: {button:?} {caps:?} in {footer:?}"
    );
    let nodes = accessible_nodes(cx);
    node(
        &nodes,
        "Image",
        if cfg!(target_os = "macos") {
            "Control+Option+Shift+Page Down"
        } else {
            "Ctrl+Alt+Shift+Page Down"
        },
    );
}

#[gpui::test]
fn the_menus_hint_follows_the_settings_binding(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_window, cx) = open_sample(cx, Some(data.path()));

    // Record Ctrl+9 as the open-Settings binding.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-open-settings", "ctrl-9");
    until_record(&mut settings_cx, data.path(), "open-settings", "ctrl-9");

    // The menu's Settings entry shows the binding in force as its hint.
    click(cx, "footer-menu");
    cx.run_until_parked();
    let nodes = accessible_nodes(cx);
    node(&nodes, "MenuItem", "Settings");
    assert!(
        nodes
            .iter()
            .any(|node| node["role"] == "Image" && node["label"] == format!("{CTRL}+9")),
        "the menu's hint shows the binding, {nodes:#?}"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed");

    // The new key opens Settings, and the default no longer does.
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 0, "Settings closed");
    cx.simulate_keystrokes("ctrl-9");
    cx.run_until_parked();
    assert_eq!(
        settings_windows(cx),
        1,
        "the recorded binding opens Settings"
    );
    // A second press focuses the same window, not another one.
    cx.simulate_keystrokes("ctrl-9");
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 1);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    assert_eq!(
        settings_windows(cx),
        1,
        "the default no longer opens Settings"
    );
}

#[gpui::test]
fn the_windows_close_shortcut_is_captured_while_a_recorder_listens(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_window, cx) = open_sample(cx, Some(data.path()));

    // Recording captures keys without executing them: the window's own
    // close shortcut — the dismiss binding's default — is swallowed
    // while a recorder listens, so the Settings window stays open and
    // the keys reach the binding being recorded (here: refused, for
    // colliding with the dismiss binding that has them).
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-back");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes(dismiss_shortcut());
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("already dismisses the launcher"),
        "the keys reached the binding, {tree}"
    );
    assert!(
        tree.contains("Recording; Back"),
        "the recorder keeps listening, {tree}"
    );
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("keyboard").is_some(),
        "the Settings window stayed open"
    );
    // The find shortcut is captured too: it records, and the search
    // field is not focused.
    let find = if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    };
    settings_cx.simulate_keystrokes(find);
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "back", find);
    let tree = a11y(&mut settings_cx);
    assert!(
        !tree.contains("Recording; Back"),
        "the recording ended with the keys, {tree}"
    );
    // The window still closes on it when no recorder is listening.
    settings_cx.simulate_keystrokes(dismiss_shortcut());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 0, "the window closed");
}

#[gpui::test]
fn a_collision_with_another_action_is_refused_and_keeps_the_recorder_listening(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // Back is Ctrl+B.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-back", "ctrl-b");
    until_record(&mut settings_cx, data.path(), "back", "ctrl-b");

    // Taking the same keys for the next result is refused, with the other
    // action named, and the recorder keeps listening.
    click(&mut settings_cx, "keyboard-next-result");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("ctrl-b");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("{CTRL}+B already goes back")),
        "the collision is explained, {tree}"
    );
    assert!(
        tree.contains("Recording; Next result"),
        "the recorder keeps listening, {tree}"
    );
    // Nothing was kept: the record holds what it held, and the keys still
    // act as they did.
    assert!(!record_holds(data.path(), "next-result", "ctrl-b"));
    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(1), "Down still moves");

    // Escape cancels the recording, changing nothing.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        !tree.contains("Recording;"),
        "the recorder is no longer listening, {tree}"
    );
}

#[gpui::test]
fn a_binding_that_would_take_over_typing_is_refused(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-next-result");
    settings_cx.run_until_parked();
    // A plain letter, a plain editing key and the platform's select-all
    // are all refused: text editing stays owned by the focused field.
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    for (keystroke, expected) in [
        ("j", "is protected: it types a character"),
        ("backspace", "is protected: it deletes text"),
        (select_all, "is protected: it selects all text"),
    ] {
        settings_cx.simulate_keystrokes(keystroke);
        settings_cx.run_until_parked();
        let tree = a11y(&mut settings_cx);
        assert!(
            tree.contains(expected),
            "{keystroke} is refused with “{expected}”, {tree}"
        );
        assert!(
            tree.contains("Recording; Next result"),
            "the recorder keeps listening, {tree}"
        );
    }
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();

    // The keys still type and edit in the query field as they did.
    cx.simulate_input("ja");
    cx.simulate_keystrokes("backspace");
    assert_eq!(settle(&window, cx).query(), Some("j"));
}

#[gpui::test]
fn a_reset_returns_to_the_default_through_the_same_checks(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // Back is Ctrl+B; the reset row appears for it. A tall window, so
    // every row stays in view as the resets add theirs.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-back", "ctrl-b");
    until_record(&mut settings_cx, data.path(), "back", "ctrl-b");
    let nodes = accessible_nodes(&mut settings_cx);
    node(&nodes, "Button", "Reset Back to Escape");
    // The rebound key backs out of an opened command.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-b");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "Ctrl+B backed out"
    );

    // Reset: back to Escape, through the same path a recording takes.
    click(&mut settings_cx, "keyboard-reset-back");
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "back", "escape");
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "Escape backs out"
    );
    // The reset stays, naming no default: nothing is left to reset.
    let nodes = accessible_nodes(&mut settings_cx);
    node(&nodes, "Button", "Reset Back");

    // A reset that would land on another action's binding is refused:
    // Open Settings moves to Ctrl+9, Dismiss takes the freed default's
    // place, and resetting Open Settings back to its default is refused.
    record(&mut settings_cx, "keyboard-open-settings", "ctrl-9");
    until_record(&mut settings_cx, data.path(), "open-settings", "ctrl-9");
    record(
        &mut settings_cx,
        "keyboard-dismiss-launcher",
        settings_shortcut(),
    );
    until_record(
        &mut settings_cx,
        data.path(),
        "dismiss-launcher",
        settings_shortcut(),
    );
    click(&mut settings_cx, "keyboard-reset-open-settings");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!(
            "{} already dismisses the launcher",
            settings_shortcut_name()
        )),
        "the reset collision is explained, {tree}"
    );
    assert!(
        record_holds(data.path(), "open-settings", "ctrl-9"),
        "the reset kept nothing"
    );
}

#[gpui::test]
fn a_save_that_fails_rolls_the_binding_back(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // A change that lands and is saved.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-next-result", "ctrl-n");
    until_record(&mut settings_cx, data.path(), "next-result", "ctrl-n");

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Another change: it takes effect, but cannot be saved.
    record(&mut settings_cx, "keyboard-next-result", "ctrl-m");
    until(&mut settings_cx, |cx| {
        a11y(cx)
            .contains("Pane could not save your choice")
            .then_some(())
    });

    // The failure is explained, and the binding the record holds is the
    // one that works: the change that could not be saved did not keep the
    // keys it took.
    cx.simulate_input("script sample");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-n");
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "the recorded binding still moves"
    );
    cx.simulate_keystrokes("ctrl-m");
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "the unsaved binding no longer does"
    );
}

#[gpui::test]
fn escape_clears_the_query_then_hides_the_launcher_which_keeps_running(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher =
        Launcher::new(Runtime::start(), samples::sample_commands()).with_hotkeys(system.clone());
    let (window, cx) = open_launcher(cx, launcher, Some(data.path()));
    let handle = handle_of(cx);

    // A query: Escape clears it.
    cx.simulate_input("zz");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));

    // An empty root: Escape hides the launcher — hidden, not closed: the
    // window stays live, Pane keeps running and the global hotkey still
    // summons it.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the empty root hid the launcher");
    assert!(
        cx.cx
            .update(|cx| cx.windows().contains(&AnyWindowHandle::from(handle))),
        "the hidden window is still the live launcher window"
    );
    press(&window, &Shortcut::open_pane_default(), cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hotkey shows it again");
    assert_eq!(settle(&window, cx).status, Status::Idle, "nothing failed");
}

#[gpui::test]
fn escape_cancels_an_active_composition_before_it_acts(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;

    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    cx.simulate_input("a");
    settle(&window, cx);

    // What a platform input method does: mark composing text in the
    // focused query field.
    let input = cx.read_entity(&window, |window, _| window.query_field());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        })
    });
    assert_eq!(settle(&window, cx).query(), Some("aにほ"));

    // The back key cancels the composition first: its marked text is
    // discarded and the key goes no further — not back, not hidden.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("a"), "the composition was discarded");
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "the key did not go back"
    );
    assert!(!hidden(&window, cx), "the key did not hide the window");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert_eq!(input.marked_text_range(window, cx), None);
        })
    });

    // The next press clears the query, and the one after hides.
    cx.simulate_keystrokes("escape");
    assert_eq!(settle(&window, cx).query(), Some(""));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(hidden(&window, cx));
}

#[gpui::test]
fn the_back_key_dismisses_an_open_menu_before_it_acts(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // The menu is open, with a query behind it; the back key — rebound
    // away from Escape, so it is not the menu's own key — dismisses the
    // menu first and goes no further.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-back", "ctrl-b");
    cx.simulate_input("zz");
    settle(&window, cx);
    click(cx, "footer-menu");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    cx.simulate_keystrokes("ctrl-b");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu was dismissed");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"), "the key did not go back");
    // And the next press clears the query, as back does.
    cx.simulate_keystrokes("ctrl-b");
    assert_eq!(settle(&window, cx).query(), Some(""));
}

#[gpui::test]
fn return_to_root_leaves_whatever_screen_is_open(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // Deep in the launcher: a command's form.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);

    // The root binding returns to root search from wherever it is, with
    // the search ready to type.
    cx.simulate_keystrokes(root_shortcut());
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { ref query } if query.is_empty()),
        "{:?}",
        view.screen
    );
    cx.simulate_input("jav");
    assert_eq!(settle(&window, cx).query(), Some("jav"));
}

#[gpui::test]
fn window_local_actions_stay_in_their_windows(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let system = Arc::new(FakeSystem::default());
    let launcher =
        Launcher::new(Runtime::start(), samples::sample_commands()).with_hotkeys(system.clone());
    let (window, cx) = open_launcher(cx, launcher, Some(data.path()));
    let handle = handle_of(cx);

    // The close-window shortcut in the Settings window closes only
    // Settings: the launcher is neither closed nor hidden, and Pane keeps
    // running.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 0, "Settings closed");
    assert!(!hidden(&window, cx), "the launcher was not dismissed");
    assert!(
        cx.cx
            .update(|cx| cx.windows().contains(&AnyWindowHandle::from(handle))),
        "the launcher window is still open"
    );

    // The dismiss binding in the launcher's window hides only the
    // launcher: Settings stays open, and the global hotkey still works —
    // Pane keeps running in the background.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    cx.simulate_keystrokes(dismiss_shortcut());
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the launcher was dismissed");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("keyboard").is_some(),
        "Settings stays open and answers"
    );
    assert!(
        system
            .registered
            .lock()
            .unwrap()
            .contains(&Shortcut::open_pane_default()),
        "the global hotkey stayed registered"
    );
    press(&window, &Shortcut::open_pane_default(), cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hotkey shows the launcher again");
}

#[gpui::test]
fn a_form_still_submits_with_the_rebound_key_and_the_footer_button(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // Open the form, and rebind the invoke action to Ctrl+J.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down down down down enter");
    settle(&window, cx);
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(
        &mut settings_cx,
        "keyboard-invoke-selected-action",
        "ctrl-j",
    );
    until_record(
        &mut settings_cx,
        data.path(),
        "invoke-selected-action",
        "ctrl-j",
    );

    // The rebound key submits the form, as Enter did.
    cx.simulate_input("Ada");
    cx.simulate_keystrokes("ctrl-j");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Hello, Ada, from the Rust guest".into())
    );

    // The footer's button, the pointer's path to the same action, still
    // does: back out, reopen the form with the rebound key, and submit
    // with the button — while the launcher is idle, so the action strip
    // is the button.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-j");
    settle(&window, cx);
    cx.simulate_input("Grace");
    click(cx, "primary-action");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Hello, Grace, from the Rust guest".into()),
        "the button still submits"
    );
}

#[gpui::test]
fn a_custom_view_keeps_its_own_keys_under_a_rebound_binding(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));

    // The next result's binding is rebound to Ctrl+N.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-next-result", "ctrl-n");
    until_record(&mut settings_cx, data.path(), "next-result", "ctrl-n");

    // The color view: its own keys still reach it — Down changes the
    // color, as the view's own binding — while the launcher's navigation
    // under the rebind needs the new keys, and the back key closes the
    // view as ever.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-n ctrl-n ctrl-n ctrl-n ctrl-n enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::CustomView(_)),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Choose a color");
    cx.simulate_keystrokes("down");
    until_color(&window, cx, "Dark blue, #0D47A1");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command, "the back key closed the view");
}

#[gpui::test]
fn the_rebound_back_key_clears_a_command_search_before_leaving_it(cx: &mut TestAppContext) {
    use service::Service;

    /// Copies the assembled search sample to `folder`, as the command
    /// search tests' fixture.
    fn package(folder: &Path) -> PathBuf {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages/sample-search");
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

    let service = Service::start();
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("search"));
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = open_launcher(cx, launcher, Some(data.path()));

    // Back is Ctrl+B.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    record(&mut settings_cx, "keyboard-back", "ctrl-b");

    // Install the search sample and open its command, as the command
    // search tests do: point it at this test's service.
    window.update_in(cx, |window, w, cx| {
        window.preview_package(&folder, w, cx);
    });
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    cx.simulate_input(&service.url());
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    // The rebound back key leaves the form, as Escape did.
    cx.simulate_keystrokes("ctrl-b");
    settle(&window, cx);

    // A search in the command's own field.
    cx.simulate_input("aurora");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: "aurora".into()
        }
    );
    assert_eq!(
        service.requests().last().map(String::as_str),
        Some("/search?q=aurora")
    );

    // The rebound back key clears the command's search first, then
    // leaves it — as Escape always did.
    cx.simulate_keystrokes("ctrl-b");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        },
        "the search was cleared first"
    );
    cx.simulate_keystrokes("ctrl-b");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "then it left");
}

/// The navigation bindings' modifier as the keys name it: Alt, as Raycast
/// for Windows holds them; Control on macOS, where Option types
/// characters.
const NAV: &str = if cfg!(target_os = "macos") {
    "ctrl"
} else {
    "alt"
};

/// The navigation bindings' modifier as a binding's text names it.
const NAV_NAME: &str = if cfg!(target_os = "macos") {
    "Control"
} else {
    "Alt"
};

/// The navigation bindings' keystroke for `key`: [`NAV`] and the key.
fn nav(key: &str) -> String {
    format!("{NAV}-{key}")
}

/// Chooses the navigation bindings through the page's select: its trigger
/// opens the choices, and the choice whose row selector is `choice`
/// (`keyboard-navigation-` and the choice's id) takes it.
fn choose_navigation(settings_cx: &mut VisualTestContext, choice: &'static str) {
    click(settings_cx, "keyboard-navigation");
    settings_cx.run_until_parked();
    click(settings_cx, choice);
    settings_cx.run_until_parked();
}

#[gpui::test]
fn escape_set_to_hide_hides_from_any_screen_and_the_reopening_decides_what_shows_next(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    // The Launcher page's reopening choice, recorded ahead of the run as
    // a Pane that ran before would have: a quick return restores the
    // view, one after 90 seconds starts from root search.
    fs::write(
        data.path().join("settings.json"),
        "{ \"version\": 1, \"reopening\": \"after-90-seconds\" }",
    )
    .unwrap();
    let launcher = Launcher::new(Runtime::start(), samples::sample_commands())
        .with_hotkeys(Arc::new(FakeSystem::default()));
    let (window, cx) = open_launcher(cx, launcher, Some(data.path()));
    let default = Shortcut::open_pane_default();

    // Escape hides Pane, chosen through the page's own segment.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-escape-hide");
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "escapeBehavior", "hide");

    // A command's view: Escape hides the launcher at once instead of
    // going back to root search.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        hidden(&window, cx),
        "Escape hid the launcher from a command"
    );
    // Back within 90 seconds: the reopening restores the command's view.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hotkey shows it again");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command, "the view it was left on");

    // Root search with a query: Escape hides rather than clearing it.
    cx.simulate_keystrokes(root_shortcut());
    settle(&window, cx);
    cx.simulate_input("zz");
    assert_eq!(settle(&window, cx).query(), Some("zz"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(hidden(&window, cx), "Escape hid the launcher from a query");
    // Away for 90 seconds: the reopening starts from root search.
    cx.executor().advance_clock(Duration::from_secs(90));
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.query(), Some(""), "the query was left behind");
}

#[gpui::test]
fn escape_closes_settings_when_nothing_inside_uses_it_and_only_while_chosen(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let (_window, cx) = open_sample(cx, Some(data.path()));

    // On, the default. A listening recorder takes Escape: the recording
    // ends, and the window stays.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-back");
    settings_cx.run_until_parked();
    assert!(a11y(&mut settings_cx).contains("Recording; Back"));
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(!tree.contains("Recording;"), "the recording ended, {tree}");
    assert_eq!(settings_windows(cx), 1, "the recorder took the key");

    // An open select takes it: its choices close, and the window stays.
    click(&mut settings_cx, "keyboard-navigation");
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("keyboard-navigation-emacs")
            .is_some(),
        "the choices are open"
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(settings_windows(cx), 1, "the select took the key");

    // The search takes it: the query is cleared, then the search is left.
    settings_cx.simulate_keystrokes(find_shortcut());
    settings_cx.simulate_input("back");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(settings_windows(cx), 1, "the search cleared its query");
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(settings_windows(cx), 1, "the search was left");
    assert!(
        settings_cx.debug_bounds("section-Keyboard").is_some(),
        "the sections are back"
    );

    // Nothing left to use it: Escape closes the window.
    settings_cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 0, "Escape closed Settings");

    // Off, through the page's switch: Escape leaves the window open.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-escape-closes");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"escapeClosesSettings\": false",
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    assert_eq!(settings_windows(cx), 1, "the window stays open");
    assert!(settings_cx.debug_bounds("keyboard").is_some());

    // On again: it closes again.
    click(&mut settings_cx, "keyboard-escape-closes");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"escapeClosesSettings\": true",
    );
    settings_cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), 0, "Escape closed Settings");
}

#[gpui::test]
fn the_navigation_bindings_move_the_selection_unless_an_action_has_their_keys(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    // Two results to move between, with the query keeping focus.
    cx.simulate_input("script sample");
    assert_eq!(settle(&window, cx).selected, Some(0));

    // The choices are Raycast's, each labelled with the keys it binds,
    // and none clashes with the default keys: Vim Motions is available
    // beside Open actions' Ctrl+K.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-navigation");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    for label in [
        format!("Emacs ({NAV_NAME}+P, {NAV_NAME}+N)"),
        format!("Vim Motions ({NAV_NAME}+K, {NAV_NAME}+J)"),
    ] {
        assert!(tree.contains(&label), "{label} is offered, {tree}");
    }
    assert!(
        !tree.contains("is Open actions"),
        "no choice is refused, {tree}"
    );

    // Emacs: Alt+N and Alt+P move the selection, beside Down and Up.
    click(&mut settings_cx, "keyboard-navigation-emacs");
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "navigationBindings", "emacs");
    cx.simulate_keystrokes(&nav("n"));
    assert_eq!(settle(&window, cx).selected, Some(1), "Alt+N moves down");
    cx.simulate_keystrokes(&nav("p"));
    assert_eq!(settle(&window, cx).selected, Some(0), "Alt+P moves up");
    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(1), "Down still moves");
    cx.simulate_keystrokes("up");
    assert_eq!(settle(&window, cx).selected, Some(0), "Up still moves");

    // Recording one of their keys for an action is refused while they are
    // chosen, and the recorder keeps listening.
    record(&mut settings_cx, "keyboard-back", &nav("n"));
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("{NAV_NAME}+N already moves the selection")),
        "the refusal is explained, {tree}"
    );
    assert!(
        tree.contains("Recording; Back"),
        "the recorder keeps listening, {tree}"
    );
    assert!(!record_holds(data.path(), "back", &nav("n")));
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();

    // Vim Motions, with Open actions on its default: Alt+J and Alt+K move
    // the selection while Emacs's keys no longer do, and Open actions'
    // own key still opens the Actions panel.
    choose_navigation(&mut settings_cx, "keyboard-navigation-vim");
    until_record(&mut settings_cx, data.path(), "navigationBindings", "vim");
    cx.simulate_keystrokes(&nav("j"));
    assert_eq!(settle(&window, cx).selected, Some(1), "Alt+J moves down");
    cx.simulate_keystrokes(&nav("k"));
    assert_eq!(settle(&window, cx).selected, Some(0), "Alt+K moves up");
    cx.simulate_keystrokes(&nav("n"));
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "Alt+N no longer moves"
    );
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    assert!(
        cx.read_entity(&window, |window, _| window.actions_open()),
        "Open actions still opens the panel"
    );
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    assert!(!cx.read_entity(&window, |window, _| window.actions_open()));

    // None removes the extra keys; the set's own keep moving.
    choose_navigation(&mut settings_cx, "keyboard-navigation-none");
    until_record(&mut settings_cx, data.path(), "navigationBindings", "none");
    cx.simulate_keystrokes(&nav("j"));
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "Alt+J no longer moves");
    assert_eq!(view.query(), Some("script sample"));
    cx.simulate_keystrokes("down");
    assert_eq!(settle(&window, cx).selected, Some(1), "Down still moves");
    cx.simulate_keystrokes("up");
    settle(&window, cx);

    // A set whose keys an action has been rebound to is refused: with
    // Open actions on Alt+P, Emacs is listed with the conflict named, and
    // choosing it keeps nothing.
    record(&mut settings_cx, "keyboard-open-actions", &nav("p"));
    until_record(&mut settings_cx, data.path(), "open-actions", &nav("p"));
    click(&mut settings_cx, "keyboard-navigation");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains(&format!("{NAV_NAME}+P is Open actions")),
        "the conflict is named, {tree}"
    );
    click(&mut settings_cx, "keyboard-navigation-emacs");
    settings_cx.run_until_parked();
    assert!(
        record_holds(data.path(), "navigationBindings", "none"),
        "the refused choice was not kept"
    );
    // The choices are still open: Escape is theirs.
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();
    cx.simulate_keystrokes(&nav("n"));
    assert_eq!(settle(&window, cx).selected, Some(0), "Alt+N does not move");
}

#[gpui::test]
fn a_navigation_choice_that_fails_to_save_restores_the_recorded_keys(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_sample(cx, Some(data.path()));
    cx.simulate_input("script sample");
    assert_eq!(settle(&window, cx).selected, Some(0));

    // Saved: Emacs.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    choose_navigation(&mut settings_cx, "keyboard-navigation-emacs");
    until_record(&mut settings_cx, data.path(), "navigationBindings", "emacs");

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Vim Motions: it takes effect, but cannot be saved.
    choose_navigation(&mut settings_cx, "keyboard-navigation-vim");
    until(&mut settings_cx, |cx| {
        a11y(cx)
            .contains("Pane could not save your choice")
            .then_some(())
    });

    // The keys the record holds are the ones that work: Vim's stopped,
    // and Emacs's work again.
    cx.simulate_keystrokes(&nav("j"));
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "the unsaved Alt+J no longer moves"
    );
    cx.simulate_keystrokes(&nav("n"));
    assert_eq!(
        settle(&window, cx).selected,
        Some(1),
        "the recorded Alt+N moves again"
    );
}

#[gpui::test]
fn the_behavior_choices_are_applied_by_a_fresh_application(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_window, cx) = open_sample(cx, Some(data.path()));

    // Escape hides Pane, Escape does not close Settings, and Emacs's keys
    // move the selection: each taken through the page's own controls.
    let (_settings, mut settings_cx) = keyboard_page(cx);
    click(&mut settings_cx, "keyboard-escape-hide");
    settings_cx.run_until_parked();
    until_record(&mut settings_cx, data.path(), "escapeBehavior", "hide");
    click(&mut settings_cx, "keyboard-escape-closes");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"escapeClosesSettings\": false",
    );
    choose_navigation(&mut settings_cx, "keyboard-navigation-emacs");
    until_record(&mut settings_cx, data.path(), "navigationBindings", "emacs");

    // A fresh application over the same record.
    let mut fresh = cx.cx.new_app();
    init_settings(Some(data.path()), &mut fresh);
    fresh.update(pane::bind_keys);
    let launcher = Launcher::new(Runtime::start(), samples::sample_commands())
        .with_hotkeys(Arc::new(FakeSystem::default()));
    let (window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.simulate_input("script sample");
    assert_eq!(settle(&window, fresh_cx).selected, Some(0));
    fresh_cx.simulate_keystrokes(&nav("n"));
    assert_eq!(
        settle(&window, fresh_cx).selected,
        Some(1),
        "the recorded Emacs keys move"
    );

    // Escape leaves the fresh Settings window open.
    fresh_cx.simulate_keystrokes(settings_shortcut());
    fresh_cx.run_until_parked();
    let (_settings, mut fresh_settings_cx) = open_settings(fresh_cx);
    fresh_settings_cx.simulate_keystrokes("escape");
    fresh_settings_cx.run_until_parked();
    assert_eq!(
        settings_windows(fresh_cx),
        1,
        "the recorded choice keeps Settings open"
    );

    // And Escape hides the fresh launcher from a query.
    fresh_cx.simulate_keystrokes("escape");
    fresh_cx.run_until_parked();
    assert!(hidden(&window, fresh_cx), "the recorded choice hides");
}

/// The keystroke that focuses the Settings search: Cmd+F on macOS,
/// Ctrl+F on Windows and Linux.
fn find_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    }
}
