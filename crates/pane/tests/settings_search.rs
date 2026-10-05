//! The Settings window's sidebar search, on GPUI's test platform: the
//! one field Cmd+F / Ctrl+F focuses, finding the settings the pages
//! registered — the Appearance choices, the Shortcuts filter, the
//! Extensions page's management rows and install rows, the About
//! documentation — with the arrows and Enter driving the results, the
//! pointer doing the same, Escape clearing and then leaving, no results
//! said, unavailable entries explained, a control revealed on its page
//! where it takes no focus (and the Shortcuts filter focused where it
//! does), and registrations appearing and going as the launcher's
//! packages change, found and lost without stale focus or blank pages.
//! Drives the real windows, as `settings.rs` and `shortcuts.rs` do.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle,
    prelude::*, px, size,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::{Launcher, Runtime, SavedData};

#[path = "support/settle.rs"]
mod settle;

#[path = "support/paint.rs"]
mod paint;

use paint::paints_fill_at;
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

/// The keystroke that focuses the Settings search: Cmd+F on macOS,
/// Ctrl+F on Windows and Linux.
fn find_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    }
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

/// What `open` hands back: the launcher window, the Settings window, and
/// the launcher's own test context to drive it with.
type Opened<'a> = (
    Entity<LauncherWindow>,
    WindowHandle<SettingsWindow>,
    &'a mut VisualTestContext,
);

/// Opens the launcher window and the Settings window over it, with the
/// window's keyboard focus on the sidebar's sections, as a user opening
/// Settings from the shortcut has.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let launcher = Launcher::new(Runtime::start(), pane::sample_commands());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    (window, settings, cx)
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

/// The label of the selected search result, as the window's accessibility
/// tree reports the sidebar's selection.
fn selected_result(cx: &mut VisualTestContext) -> Option<String> {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().and_then(|nodes| {
        nodes
            .values()
            .find(|node| node["aria"]["selected"].as_bool().unwrap_or(false))
            .and_then(|node| node["aria"]["label"].as_str().map(str::to_owned))
    })
}

/// Delivers the frames the search's reveal asks for, as the native frame
/// loop would: the reveal waits for the jumped-to page's paint, then
/// scrolls on the next frame, then repaints with the scrolled place. The
/// test platform delivers no frames on its own, so this is the only thing
/// that advances them. Bounded, so a chain that never stopped asking for
/// frames fails the test instead of hanging it.
fn pump(cx: &mut VisualTestContext) {
    for _ in 0..16 {
        // The frames carry the clock with them, as the section-transition
        // tests' do: a jump is a page change, whose arrival settles over
        // its bounded span, and the reveal — which waits for that rest —
        // scrolls and repaints after it. The budget of sixteen 25ms
        // frames covers the 150ms span, the wait and the scroll's own
        // frames with room to spare, and stays short of the watcher's
        // 500ms tick.
        cx.cx.executor().advance_clock(Duration::from_millis(25));
        let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        if ran == 0 {
            return;
        }
    }
    panic!("the reveal never stopped asking for frames");
}

/// Advances the test clock past the window's watcher interval, so the
/// watcher's tick sees a change the user did not make in the window.
fn tick(cx: &mut VisualTestContext) {
    cx.cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
}

#[gpui::test]
fn find_focuses_the_search_and_typing_matches_registered_settings(cx: &mut TestAppContext) {
    let (launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    // The window's keyboard focus is the sidebar's sections: assistive
    // technology reads the list's active descendant — the selected
    // section, the General page the window opens on.
    assert_eq!(focused_label(&mut sc).as_deref(), Some("General"));

    // Cmd+F / Ctrl+F focuses the one search input, from anywhere in the
    // Settings window.
    sc.simulate_keystrokes(find_shortcut());
    assert_eq!(focused_label(&mut sc).as_deref(), Some("Search settings"));

    // Typing finds real registered host settings: the Appearance choices.
    // The result identifies its page and its control — the setting and
    // the group it sits in — and the sidebar is showing the search's
    // results, not the sections.
    sc.simulate_input("dark");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Dark").is_some(),
        "the Dark theme choice is a result"
    );
    let (_, json) = accessibility(&mut sc);
    assert!(
        json.contains("Appearance · Theme"),
        "the result names the page and the group: {json}"
    );
    assert!(
        json.contains("Settings search results"),
        "the list says what it is showing: {json}"
    );
    assert!(
        sc.debug_bounds("section-Shortcuts").is_none(),
        "the sections are replaced by the results while the query shows"
    );

    // Enter opens the result: the page it names, the query cleared (the
    // sections return), and the keyboard handed back to the sidebar —
    // page navigation, restored — since the choice takes no focus.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(sc.debug_bounds("appearance").is_some(), "the page opened");
    assert!(
        sc.debug_bounds("settings-search-result-Dark").is_none(),
        "the query cleared"
    );
    assert!(
        sc.debug_bounds("section-Shortcuts").is_some(),
        "the sections are back"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Appearance"),
        "the sidebar has the keyboard focus, its selected section read"
    );

    // Nothing reached the launcher window behind Settings.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.query(), Some(""));
}

/// The results are the sidebar's own items (#97), not the launcher's
/// result rows: as wide as the search field above them, 36px tall with
/// the place each setting lives under its name, and the selected one in
/// the sidebar's white 9% wash.
#[gpui::test]
fn the_results_are_sidebar_items_not_launcher_rows(cx: &mut TestAppContext) {
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("dark");
    sc.run_until_parked();
    let result = sc
        .debug_bounds("settings-search-result-Dark")
        .expect("the Dark result");
    let field = sc
        .debug_bounds("settings-search-field")
        .expect("the search field");
    assert_eq!(result.left(), field.left());
    assert_eq!(result.size.width, field.size.width);
    assert_eq!(result.size.height, px(36.), "the sidebar item's height");
    assert!(
        paints_fill_at(&mut sc, result, 0xFFFFFF17),
        "the selected result takes the sidebar's white 9% wash"
    );
}

#[gpui::test]
fn arrows_move_the_selection_and_the_pointer_opens_what_is_clicked(cx: &mut TestAppContext) {
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    // "theme" matches the Appearance page's own entry and its theme
    // choices, in registration order; the first is selected.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("theme");
    sc.run_until_parked();
    assert_eq!(
        selected_result(&mut sc).as_deref(),
        Some("Appearance"),
        "the first result is selected"
    );

    // The arrows move the selection, and stop at the ends: "theme"
    // matches the Appearance page's own entry and its three theme
    // choices, in registration order.
    sc.simulate_keystrokes("down");
    assert_eq!(selected_result(&mut sc).as_deref(), Some("System"));
    sc.simulate_keystrokes("down");
    assert_eq!(selected_result(&mut sc).as_deref(), Some("Light"));
    sc.simulate_keystrokes("up");
    assert_eq!(selected_result(&mut sc).as_deref(), Some("System"));
    sc.simulate_keystrokes("down down down");
    assert_eq!(
        selected_result(&mut sc).as_deref(),
        Some("Dark"),
        "Down at the end stays at the last result"
    );

    // Enter opens the selected result, as it did of the first.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(sc.debug_bounds("appearance").is_some());
    assert_eq!(focused_label(&mut sc).as_deref(), Some("Appearance"));

    // The pointer does the same: a click on a result opens it — here the
    // About page's documentation entry.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("documentation");
    sc.run_until_parked();
    let row = sc
        .debug_bounds("settings-search-result-Documentation")
        .expect("the documentation result");
    sc.simulate_click(row.center(), Modifiers::none());
    sc.run_until_parked();
    assert!(sc.debug_bounds("about").is_some(), "the About page opened");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("About"),
        "the sidebar has the keyboard focus, its selected section read"
    );

    // A query that matches nothing says so, and Enter on no result does
    // nothing: the page stays, the field keeps its text and its focus.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("zzz");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-empty").is_some(),
        "the no-results line shows"
    );
    let (_, json) = accessibility(&mut sc);
    assert!(
        json.contains("No settings match “zzz”"),
        "the query is quoted back: {json}"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("about").is_some(),
        "Enter on no result opened nothing"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Search settings"),
        "the field keeps the focus"
    );
}

#[gpui::test]
fn escape_clears_the_query_then_returns_to_page_navigation(cx: &mut TestAppContext) {
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    // Escape with a query clears it, as it does in every search of
    // Pane's: the sections return, and the field keeps its focus for
    // another query.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("dark");
    sc.run_until_parked();
    assert!(sc.debug_bounds("settings-search-result-Dark").is_some());
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Dark").is_none(),
        "the query cleared"
    );
    assert!(
        sc.debug_bounds("section-Shortcuts").is_some(),
        "the sections are back"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Search settings"),
        "the field still has the focus"
    );

    // Escape with an empty query leaves the search: the sections take
    // the keyboard back.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("General"),
        "page navigation has the keyboard focus, its selected section read"
    );

    // Page navigation also leaves the search: a query showing clears when
    // the sections' keys move the page. Tab reaches the sections from the
    // field; Down walks the sections — the Shortcuts page is three Down
    // presses from the General page the window opens on (General, the
    // Launcher page, Appearance, then Shortcuts).
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("dark");
    sc.run_until_parked();
    sc.simulate_keystrokes("tab");
    sc.simulate_keystrokes("down down down");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("section-Shortcuts").is_some(),
        "the query cleared with the page change"
    );
    assert!(sc.debug_bounds("settings-search-result-Dark").is_none());
    assert!(
        sc.debug_bounds("shortcuts").is_some(),
        "the sections' key moved the page"
    );
}

#[gpui::test]
fn a_jump_reveals_the_control_on_its_page(cx: &mut TestAppContext) {
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    // The window opens on the General page; the Appearance page's
    // overflow is what the reveal has to scroll through, so walk to it.
    let appearance = sc
        .debug_bounds("section-Appearance")
        .expect("the Appearance section");
    sc.simulate_click(appearance.center(), Modifiers::none());
    sc.run_until_parked();

    // A short window — shorter than the floor the app asks the system
    // for, which the layout still handles — so the Appearance page
    // overflows its viewport far enough that revealing a lower choice
    // has to scroll, not just land in view.
    sc.cx
        .simulate_window_resize(AnyWindowHandle::from(settings), size(px(560.), px(300.)));
    sc.run_until_parked();
    let page = sc.debug_bounds("settings-page").expect("the page area");
    let solid = sc
        .debug_bounds("appearance-material-Solid")
        .expect("the Solid choice");
    assert!(
        solid.top() - page.top() > px(40.),
        "the choice starts well below the top of the page area: {solid:?} in {page:?}"
    );

    // The jump: search for the choice and press Enter.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("solid");
    sc.run_until_parked();
    sc.simulate_keystrokes("enter");
    // The reveal runs over the frames after the jump's page paints.
    pump(&mut sc);

    let solid = sc
        .debug_bounds("appearance-material-Solid")
        .expect("the Solid choice");
    let page = sc.debug_bounds("settings-page").expect("the page area");
    assert!(
        (solid.top() - page.top()).abs() <= px(1.),
        "the control was revealed at the top of the page area: {solid:?} in {page:?}"
    );
}

#[gpui::test]
fn a_jump_to_the_shortcuts_filter_focuses_it(cx: &mut TestAppContext) {
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    // The Shortcuts page's filter is the one control of the registered
    // settings that takes keyboard focus.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("filter");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Filter commands")
            .is_some(),
        "the filter is a result"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();

    // The jump opens the Shortcuts page and focuses the filter — the
    // control — ready to filter the commands.
    assert!(sc.debug_bounds("shortcuts").is_some(), "the page opened");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Filter commands and extensions"),
        "the control took the focus"
    );
    assert!(
        sc.debug_bounds("section-Shortcuts").is_some(),
        "the query cleared; the sections are back"
    );
}

#[gpui::test]
fn an_override_marks_the_choices_unavailable_in_the_results(cx: &mut TestAppContext) {
    // The host settings with an override in force for this process, so
    // the Appearance choices are listed but cannot be used here.
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides {
                theme: Some(pane_core::ThemePreference::Dark),
                material: None,
            },
            cx,
        )
    });
    let (_launcher, settings, cx) = open(cx);
    let mut sc = settings_context(&settings, cx);

    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("light");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Light").is_some(),
        "the Light choice is listed, honestly"
    );
    // The result says why it cannot be used: the override, the same
    // reason the page gives.
    let (_, json) = accessibility(&mut sc);
    assert!(
        json.contains("PANE_THEME=dark overrides the saved choice for this process"),
        "the override is the reason: {json}"
    );

    // The jump still opens the page and reveals the choice, which the
    // page keeps showing, disabled, with its notice.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(sc.debug_bounds("appearance").is_some(), "the page opened");
    assert!(
        sc.debug_bounds("appearance-theme-Light").is_some(),
        "the choice is revealed"
    );
    assert!(
        sc.debug_bounds("appearance-override").is_some(),
        "the page explains the override"
    );
}

#[gpui::test]
fn registrations_that_appear_and_go_are_found_and_lost(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hello = hello_package(&sources.path().join("hello"));
    // A launcher that installs packages: the Extensions page's management
    // rows are registered settings, so the search follows the packages.
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    );
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut sc = settings_context(&settings, cx);

    // Nothing is installed: the search finds no Hello.
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("hello");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-empty").is_some(),
        "no settings match yet"
    );

    // The user installs Hello in the launcher window — the folder
    // picker, then Enter on the preview — and the Settings window's
    // watcher asks for the redraw, so the search — its query still
    // showing — finds the registered management row.
    choose_folder(&window, cx, hello);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    tick(&mut sc);
    assert!(
        sc.debug_bounds("settings-search-result-Hello").is_some(),
        "the installed extension's row is found"
    );

    // Enter jumps to it: the Extensions page opens, the row revealed.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    pump(&mut sc);
    assert!(sc.debug_bounds("extensions").is_some(), "the page opened");
    assert!(
        sc.debug_bounds("extension-row-Hello").is_some(),
        "the extension's row is showing"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Extensions"),
        "the sidebar has the keyboard focus, its selected section read"
    );

    // The package is uninstalled: the entry goes, and the same query now
    // matches nothing — no stale row, no stale focus, no blank page.
    let identity = cx
        .read_entity(&window, |window, _| window.launcher().clone())
        .packages()
        .iter()
        .find(|package| package.title() == "Hello")
        .expect("Hello installed")
        .identity
        .clone();
    block_on(
        cx.read_entity(&window, |window, _| window.launcher().clone())
            .uninstall(&identity, SavedData::Keep),
    );
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("hello");
    sc.run_until_parked();
    tick(&mut sc);
    assert!(
        sc.debug_bounds("settings-search-result-Hello").is_none(),
        "the uninstalled extension's row is gone"
    );
    assert!(
        sc.debug_bounds("settings-search-empty").is_some(),
        "the query matches nothing now"
    );
    assert!(
        sc.debug_bounds("extensions").is_some(),
        "the page stays, not a blank one"
    );
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
