//! Pane's Settings window: its three entry points converge on one window,
//! which closes without quitting Pane, keeps its keyboard input to itself,
//! and answers for its titlebar controls. Drives the real windows through
//! GPUI's test platform, as `window.rs` drives the launcher's.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*, px,
};
use pane::{APP_VERSION, LauncherWindow, SettingsWindow};
use pane_core::{Launcher, Runtime, Screen, Status};

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

/// Records the links the launcher is asked to open, so that no browser
/// opens; taking a moment to answer, as a system handler does.
#[derive(Default)]
struct RecordedLinks(std::sync::Mutex<Vec<String>>);

impl pane_core::LinkOpener for RecordedLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        std::thread::sleep(Duration::from_millis(10));
        self.0.lock().unwrap().push(url.into());
        Ok(())
    }
}

/// A link opener that refuses, to see the About page explain it.
struct RefusingLinks;

impl pane_core::LinkOpener for RefusingLinks {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("no program to open web links is installed".into())
    }
}

type Opened<'a> = (
    gpui::Entity<LauncherWindow>,
    Arc<RecordedLinks>,
    &'a mut VisualTestContext,
);

/// Opens the launcher window over a launcher whose links are recorded,
/// ready for the Settings window's documentation entry.
fn open_launcher(cx: &mut TestAppContext) -> Opened<'_> {
    let links = Arc::new(RecordedLinks::default());
    let launcher =
        Launcher::new(Runtime::start(), pane::sample_commands()).with_link_opener(links.clone());
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, links, cx)
}

/// Opens the launcher window over a launcher whose links refuse.
fn open_refusing(
    cx: &mut TestAppContext,
) -> (gpui::Entity<LauncherWindow>, &mut VisualTestContext) {
    let launcher = Launcher::new(Runtime::start(), pane::sample_commands())
        .with_link_opener(Arc::new(RefusingLinks));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
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

/// Runs `cx` until `done` returns a value, so that work arriving from
/// other threads (a link opening) has been drawn.
fn until<T>(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if let Some(value) = done(cx) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the window to draw"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_three_entry_points_converge_on_one_focused_settings_window(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    // The Settings root result opens the window.
    cx.simulate_input("settings");
    settle(&launcher, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let first = settings_windows(cx)
        .pop()
        .expect("the root result opened Settings");
    assert_eq!(settings_windows(cx).len(), 1, "no second window appeared");
    assert!(cx.cx.update(|cx| first.is_active(cx)).unwrap_or(false));

    // The local shortcut focuses the same window.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![first], "the shortcut focused it");

    // The ellipsis menu's Settings entry, too.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    let item = cx
        .debug_bounds("menu-item-Settings")
        .expect("the menu item");
    cx.simulate_click(item.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![first], "the menu focused it");

    // All three ways left the launcher where it was.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.status, Status::Idle);
}

#[gpui::test]
fn closing_settings_reopens_a_new_window_without_ending_the_launcher(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    // The test platform's quit is a no-op, so the application staying
    // alive is what closing Settings must leave observable: the launcher
    // window stays and still works. (Closing the launcher itself quits
    // Pane in the binary, whose close hook compares the window ids; the
    // same behavior is checked natively in docs/evidence/settings-72/.)
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0, "Settings closed");
    assert!(
        cx.windows().contains(&launcher_window),
        "the launcher window stayed"
    );

    // The launcher still works: its rows still open.
    cx.simulate_keystrokes("enter");
    let view = settle(&launcher, cx);
    assert_eq!(view.screen, Screen::Command, "the launcher answers");

    // Reopening makes a new window, focused.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let second = settings_windows(cx).pop().expect("Settings reopened");
    assert_ne!(second, settings, "a new window, not the closed one");
    assert!(cx.cx.update(|cx| second.is_active(cx)).unwrap_or(false));
}

#[gpui::test]
fn hiding_the_launcher_leaves_settings_open_and_usable(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");

    // Hiding the launcher window — as its later global hotkey will —
    // closes nothing: Settings' lifetime is its own.
    cx.cx
        .update_window(launcher_window, |_, window, _| window.set_visible(false))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![settings], "Settings stayed");

    // And it still answers: its About page's content is drawn.
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("about-version").is_some());
}

#[gpui::test]
fn keys_in_settings_and_the_launcher_stay_in_their_windows(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);

    // Typing in the launcher narrows its results and touches nothing in
    // Settings.
    cx.simulate_input("rust");
    settle(&launcher, cx);
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.query(), Some("rust"));
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("about-version").is_some(),
        "the About page is unchanged"
    );

    // Keys in Settings — the sidebar's navigation, which has one section —
    // reach no launcher key: the query stays, no selection moves.
    settings_cx.simulate_keystrokes("down up enter");
    settings_cx.run_until_parked();
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.query(), Some("rust"), "the query was untouched");
    assert_eq!(view.selected, Some(0), "no row was selected");
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // And what Settings shows is still its own page: the About page, the
    // one section the window offers.
    assert!(settings_cx.debug_bounds("about").is_some());
    assert_eq!(
        settings_cx.debug_bounds("section-About").map(|_| "About"),
        Some("About"),
        "the sidebar stayed on About"
    );

    // The launcher's keys, in turn, never reach Settings.
    cx.simulate_keystrokes("escape");
    settle(&launcher, cx);
    let query = cx.read_entity(&launcher, |window, _| {
        window.launcher().view().query().map(str::to_owned)
    });
    assert_eq!(
        query,
        Some("".into()),
        "the launcher's Escape cleared its own query"
    );
}

#[gpui::test]
fn the_footer_menu_opens_traverses_dismisses_and_restores_focus(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    assert!(
        cx.debug_bounds("footer-menu").is_some(),
        "the menu button is the footer's leftmost control"
    );
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Rust sample"),
        "root search's selected result has focus"
    );

    // The keyboard reaches the menu: Tab from the query field, then Enter
    // presses the button.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("More actions"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open");
    // Focus moved into it, onto its one item.
    assert_eq!(focused_label(cx).as_deref(), Some("Settings"));

    // The popup overlays the list: it sits above the footer strip, over
    // the results.
    let menu = cx.debug_bounds("menu").expect("the menu");
    let footer = cx.debug_bounds("status-idle").expect("the footer");
    assert!(
        menu.bottom() <= footer.top(),
        "the menu is above the footer"
    );

    // Escape dismisses it, restoring the focus it took — the menu's
    // button, which had focus when the menu opened, back in command of
    // the footer.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu is closed");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("More actions"),
        "focus is restored to what had it: the menu's button"
    );
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.status, Status::Idle, "nothing was activated");

    // Enter activates the menu's selected entry — the keyboard path to
    // the Settings window, converging on the same window as the other
    // entry points. Focus is back on the button, so Enter opens the menu
    // again.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open again");
    assert_eq!(focused_label(cx).as_deref(), Some("Settings"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let settings = settings_windows(cx)
        .pop()
        .expect("the menu's item opened the Settings window");
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed with it");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("More actions"),
        "focus is restored to what had it: the menu's button"
    );
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0);

    // An outside click dismisses it the same way, and consumes the click:
    // the launcher result underneath is not activated.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    let row = cx.debug_bounds("row-Rust sample").expect("a result row");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed");
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "no row opened");
    assert_eq!(view.status, Status::Idle);
    assert_eq!(view.selected, Some(0), "the selection did not move");
    assert_eq!(settings_windows(cx).len(), 0, "no Settings window opened");
}

#[gpui::test]
fn the_menu_button_toggles_and_assistive_technology_sees_it_named(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    // Clicking the button opens; clicking it again with the menu open
    // closes it — the popup's outside-click dismissal consumes the second
    // click before the button can reopen it.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("menu").is_none(),
        "the button toggled closed"
    );

    // The button and its item are named controls, with the open state.
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes: Vec<&serde_json::Value> = tree["nodes"].as_object().unwrap().values().collect();
    let button = nodes
        .iter()
        .find(|node| node["aria"]["role"] == "Button" && node["aria"]["label"] == "More actions")
        .expect("the menu button is named");
    assert_eq!(
        button["aria"]["expanded"],
        serde_json::json!(true),
        "the open state is announced"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["aria"]["role"] == "Menu" && node["aria"]["label"] == "More actions"),
        "the menu is announced"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["aria"]["role"] == "MenuItem" && node["aria"]["label"] == "Settings"),
        "the menu's item is announced"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn the_about_page_shows_the_real_version_and_opens_the_documentation(cx: &mut TestAppContext) {
    let (launcher, links, cx) = open_launcher(cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The version is the real one this build runs, drawn and announced.
    assert!(
        settings_cx.debug_bounds("about-version").is_some(),
        "the version row is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains(&format!("\"Pane {APP_VERSION}\"")),
        "the real version is rendered"
    );

    // The documentation entry opens Pane's repository with the launcher's
    // link opener, off the window's thread, and reports what happened.
    let link = settings_cx
        .debug_bounds("about-documentation")
        .expect("the documentation entry");
    settings_cx.simulate_click(link.center(), Modifiers::none());
    until(&mut settings_cx, |cx| {
        cx.debug_bounds("about-status").map(|_| ())
    });
    assert_eq!(
        links.0.lock().unwrap().as_slice(),
        ["https://github.com/hoangvu12/pane"],
        "the repository documentation was opened"
    );
    // Opening it left the launcher where it was.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

#[gpui::test]
fn a_refused_documentation_link_is_explained_on_the_page(cx: &mut TestAppContext) {
    let (launcher, cx) = open_refusing(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    let link = settings_cx
        .debug_bounds("about-documentation")
        .expect("the documentation entry");
    settings_cx.simulate_click(link.center(), Modifiers::none());
    until(&mut settings_cx, |cx| {
        cx.debug_bounds("about-status").map(|_| ())
    });

    // The refusal is the page's status, not a silent failure.
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains(
            "Could not open the documentation: no program to open web links is installed"
        ),
        "the refusal is explained, {json}"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.status, Status::Idle, "the launcher is untouched");
}

#[gpui::test]
fn the_settings_window_keeps_its_layout_at_small_sizes(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);

    // The window's floor: at its smallest usable size the sidebar and the
    // page both stay laid out — nothing reaches past the panel's edge.
    settings_cx.simulate_resize(gpui::size(px(560.), px(400.)));
    settings_cx.run_until_parked();
    let sidebar = settings_cx
        .debug_bounds("section-About")
        .expect("the sidebar is laid out");
    let page = settings_cx
        .debug_bounds("settings-page")
        .expect("the page is laid out");
    let version = settings_cx
        .debug_bounds("about-version")
        .expect("the version row is laid out");
    assert!(
        sidebar.right() <= page.left(),
        "the sidebar is beside the page"
    );
    assert!(
        version.right() <= page.right(),
        "the version stays within the page"
    );
}

/// Windows-only: the titlebar's painted caption buttons, whose window
/// control areas route to the system's close, minimize and maximize.
#[cfg(target_os = "windows")]
#[gpui::test]
fn the_titlebars_window_controls_close_only_the_settings_window(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The three platform controls are there, and named.
    assert!(settings_cx.debug_bounds("window-minimize").is_some());
    assert!(settings_cx.debug_bounds("window-maximize").is_some());
    let close = settings_cx
        .debug_bounds("window-close")
        .expect("the close button");
    let (_, json) = accessibility(&mut settings_cx);
    for label in ["Minimize", "Maximize", "Close"] {
        assert!(
            json.contains(&format!("\"label\": \"{label}\"")),
            "the {label} control is named"
        );
    }

    // Clicking close closes only the Settings window: on Windows the
    // system takes the click through the hit test; on the test platform
    // the same behavior comes from the button's own handler.
    settings_cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0, "Settings closed");
    assert!(
        cx.windows().contains(&launcher_window),
        "the launcher window stayed"
    );

    // The launcher still answers.
    cx.simulate_keystrokes("escape");
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}
