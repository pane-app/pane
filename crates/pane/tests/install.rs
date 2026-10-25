//! Installing a local extension package through the native window, on
//! GPUI's test platform: the chosen package is previewed, Enter installs it
//! and its command runs; and the extension-management flow's screens as the
//! launcher window draws them while it holds them (Settings drives the
//! flow, #168; the install rows' folder picker is Settings', tested in
//! `settings.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::package;

const CREATE_ROW: &str = "Create Extension…";
const IMPORT_ROW: &str = "Import Extension…";
const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
/// Root search's row for the pass the user asks for, listed once an
/// extension is installed (#267).
const CHECK_ROW: &str = "Check for Extension Updates";
const MANAGE_ROW: &str = "Manage Extensions";
const SETTINGS_ROW: &str = "Settings…";

fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &TempDir,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// Moves the selection to the root row titled `title`, with the down key:
/// the blank query's order (#199) collates Pane's own rows among the
/// commands, so the row is found by its title, never its place.
fn select_row(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let view = settle(window, cx);
    let index = view
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("the {title} row is listed"));
    for _ in 0..index {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(
        settle(window, cx)
            .selected
            .and_then(|selected| view.rows.get(selected))
            .map(|row| row.title.as_str()),
        Some(title),
        "the down keys selected the {title} row"
    );
    view
}

/// Previews the package in `folder` in the window, as the folder Settings'
/// picker chose is previewed (#168), or as `pane --install` names one.
fn choose_folder(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    folder: Option<PathBuf>,
) -> LauncherView {
    if let Some(folder) = folder {
        window.update_in(cx, |launcher, window, cx| {
            launcher.preview_package(&folder, window, cx);
        });
    }
    settle(window, cx)
}

#[gpui::test]
fn a_chosen_package_is_previewed_installed_and_run(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            SETTINGS_ROW
        ]
    );

    let view = choose_folder(&window, cx, Some(folder));
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Hello");
    assert!(
        cx.debug_bounds("detail-Version: 1.0.0").is_some(),
        "details are rendered"
    );
    assert!(cx.debug_bounds("row-Install").is_some());

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        titles(&view),
        [
            CHECK_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            MANAGE_ROW,
            "Say hello",
            SETTINGS_ROW
        ]
    );
    assert_eq!(view.status, Status::Result("Installed Hello".into()));
    assert!(cx.debug_bounds("status-result").is_some());

    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Rust guest".into())
    );
}

/// Whether the screen shown draws a heading line above its content, and
/// whether the footer's left names a command (its debug selectors).
fn heading_and_footer_command(cx: &mut VisualTestContext) -> (bool, bool) {
    (
        cx.debug_bounds("screen-heading").is_some(),
        cx.debug_bounds("footer-command").is_some(),
    )
}

/// A preview's details scroll from the keyboard when they overflow
/// (#499): Shift+Page Down and Shift+Up scroll them, and the selection
/// stays on Install.
#[gpui::test]
fn shifted_keys_scroll_a_previews_details_and_keep_the_selection(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    // A short window, so the details overflow their share of it.
    cx.simulate_resize(gpui::size(px(640.), px(170.)));
    let view = choose_folder(&window, cx, Some(folder));
    assert!(matches!(view.screen, Screen::Package { .. }));
    let selected = view.selected;
    assert!(selected.is_some(), "a choice is selected");
    let scrolled = |cx: &mut VisualTestContext| {
        let window = window.clone();
        cx.update(|frame, cx| window.read(cx).detail_scrolled(frame))
    };
    assert_eq!(scrolled(cx), Some(0.), "the details overflow, at their top");

    cx.simulate_keystrokes("shift-pagedown");
    settle(&window, cx);
    let paged = scrolled(cx).expect("the details overflow");
    assert!(paged > 0., "Shift+Page Down scrolled the details: {paged}");
    cx.simulate_keystrokes("shift-up");
    settle(&window, cx);
    let stepped = scrolled(cx).expect("the details overflow");
    assert!(stepped < paged, "Shift+Up scrolled them back: {stepped}");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Package { .. }));
    assert_eq!(view.selected, selected, "the selection stayed");
}

/// No heading line above an extension's view or Manage extensions (#162):
/// an installed command's list starts with its content, and the footer's
/// left names it by its own icon, as its package ships it, and its
/// title; Manage extensions is named there too. A package's preview
/// names itself in its top bar, beside the back button (#482), as
/// Raycast's Store page does, and a question is an alert over the screen
/// it was asked from, with no heading or top bar of its own, as Raycast
/// asks.
#[gpui::test]
fn only_the_cores_own_screens_have_a_heading_line(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    settle(&window, cx);
    assert_eq!(
        heading_and_footer_command(cx),
        (false, false),
        "root search"
    );

    let magnifier = cx
        .debug_bounds("search-magnifier")
        .expect("root search shows the magnifier");

    // The preview's top bar: its back button in the magnifier's place, no
    // plain heading, and the selection on Install, where the keyboard is.
    let view = choose_folder(&window, cx, Some(folder.clone()));
    assert!(matches!(view.screen, Screen::Package { .. }));
    assert_eq!(
        heading_and_footer_command(cx),
        (false, false),
        "a package's preview has no plain heading"
    );
    assert!(
        cx.debug_bounds("top-bar").is_some(),
        "the preview's top bar"
    );
    assert!(cx.debug_bounds("top-bar-title").is_some());
    let back = cx
        .debug_bounds("search-back")
        .expect("the top bar's back button");
    assert_eq!(back, magnifier, "the button takes the magnifier's place");
    assert_eq!(
        view.rows[view.selected.expect("a choice is selected")].title,
        "Install"
    );

    // A click goes back one level, as Escape does: root search again.
    cx.simulate_click(back.center(), Modifiers::none());
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert!(cx.debug_bounds("top-bar").is_none());

    choose_folder(&window, cx, Some(folder));
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(
        heading_and_footer_command(cx),
        (false, true),
        "the installed command's list"
    );
    assert!(
        cx.debug_bounds("icon-footer-command").is_some(),
        "the command's own icon names it in the footer"
    );
    assert!(cx.debug_bounds("footer-command-title").is_some());

    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    // The flow Settings drives (#168); the launcher window still draws it.
    let view = enter_flow(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        heading_and_footer_command(cx),
        (false, true),
        "Manage extensions"
    );

    // "Clear cache of Hello" asks first, as Raycast asks: an alert over
    // the list, which stays drawn beneath it, rather than a page with a
    // heading or a top bar.
    cx.simulate_keystrokes("down down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Confirm { .. }));
    assert!(cx.debug_bounds("confirmation").is_some(), "the alert");
    assert!(
        cx.debug_bounds("row-Clear cache of Hello").is_some(),
        "the list beneath it"
    );
    assert_eq!(
        heading_and_footer_command(cx),
        (false, true),
        "Manage extensions' own chrome, beneath the alert"
    );
    assert!(
        cx.debug_bounds("top-bar").is_none() && cx.debug_bounds("search-back").is_none(),
        "a question has no top bar"
    );
}

#[gpui::test]
fn an_unsupported_folder_is_explained_and_escape_returns_to_root(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(cx, &data);

    let view = choose_folder(&window, cx, Some(sources.path().to_path_buf()));

    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    // A preview that cannot be installed offers no rows.
    assert_eq!(titles(&view), Vec::<String>::new());
    assert!(
        cx.debug_bounds("status-error").is_some(),
        "the reason is rendered"
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            SETTINGS_ROW
        ]
    );
}

#[gpui::test]
fn an_installed_package_is_disabled_and_enabled_from_the_extension_list(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    choose_folder(&window, cx, Some(folder));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CHECK_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            MANAGE_ROW,
            "Say hello",
            SETTINGS_ROW
        ]
    );

    enter_flow(&window, cx);
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Extensions { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Extensions");
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello",
            "Alias for Say hello",
            "Develop Hello",
            "Update extensions automatically"
        ]
    );
    assert!(
        cx.debug_bounds("row-Hello").is_some(),
        "the package is listed"
    );

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Disabled Hello".into()));
    let subtitle = view.rows[0].subtitle.clone().unwrap_or_default();
    assert!(subtitle.starts_with("Disabled"), "{subtitle}");
    assert!(cx.debug_bounds("status-result").is_some());
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CHECK_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );

    enter_flow(&window, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Enabled Hello".into())
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CHECK_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            MANAGE_ROW,
            "Say hello",
            SETTINGS_ROW
        ]
    );
}

/// Selects the row titled `title` on the screen shown and presses Enter.
fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let view = launcher.view();
    let index = titles(&view)
        .iter()
        .position(|row| *row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&view)));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

/// Replaces the package's component with the built guest `name`, as a new
/// build of the package would.
fn rebuild(folder: &Path, name: &str) {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
}

/// Installs the package in `folder` through the folder picker.
fn install(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, folder: &Path) {
    choose_folder(window, cx, Some(folder.to_path_buf()));
    cx.simulate_keystrokes("enter");
    settle(window, cx);
}

/// Enters the extension list (the flow Settings drives, #168) and clicks
/// the row whose debug selector is `row`. The pointer moves onto the row
/// first, as a user's does: the list selects the row under a moving
/// pointer, so the click runs it, where a click with no movement before it
/// only selects an unselected row.
fn click_in_extension_list(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    row: &'static str,
) -> LauncherView {
    enter_flow(window, cx);
    let row = cx.debug_bounds(row).expect("row rendered").center();
    for at in [row - gpui::point(px(1.), px(0.)), row] {
        cx.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
    }
    cx.simulate_click(row, Modifiers::none());
    settle(window, cx)
}

#[gpui::test]
fn an_installed_package_is_reloaded_from_the_extension_list(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);

    rebuild(&folder, "sample_js");
    let view = click_in_extension_list(&window, cx, "row-Reload Hello");
    assert_eq!(view.status, Status::Result("Reloaded Hello".into()));
    assert!(cx.debug_bounds("status-result").is_some());

    // Pane stayed open; the command now runs the new code. The blank
    // query's order (#199) collates Pane's own rows among the commands,
    // so "Say hello" is reached by its title, not its place.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    select_row(&window, cx, "Say hello");
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).title, "JavaScript sample");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the JavaScript guest".into())
    );
}

#[gpui::test]
fn a_reload_that_fails_to_start_offers_retry(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);

    rebuild(&folder, "failing_start");
    let view = click_in_extension_list(&window, cx, "row-Reload Hello");
    let Status::Error(message) = &view.status else {
        panic!("expected an error, got {:?}", view.status);
    };
    assert!(
        message.starts_with("Reloaded Hello, but it failed to start"),
        "{message}"
    );
    assert!(cx.debug_bounds("status-error").is_some());
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Retry starting Hello",
            "Why Hello is paused",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello",
            "Alias for Say hello",
            "Develop Hello",
            "Update extensions automatically"
        ]
    );

    // The details are a screen of their own, with Retry.
    cx.simulate_keystrokes("down down enter");
    let view = settle(&window, cx);
    assert_eq!(view.title, "Why Hello is paused");
    assert!(
        cx.debug_bounds("detail-Hello could not start.").is_some(),
        "the details are rendered"
    );
    assert_eq!(titles(&view), ["Retry starting Hello"]);
    // Escape returns to the details row; Retry is just above it.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("up enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Started Hello".into()));
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello",
            "Alias for Say hello",
            "Develop Hello",
            "Update extensions automatically"
        ]
    );
}

#[gpui::test]
fn an_installed_package_cache_is_cleared_after_confirming(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    enter_flow(&window, cx);
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello",
            "Alias for Say hello",
            "Develop Hello",
            "Update extensions automatically"
        ]
    );

    // The third row asks first, with an alert saying what is kept: Clear
    // Cache in red on Enter, Cancel on Escape. Escape keeps the cache and
    // returns to that row.
    cx.simulate_keystrokes("down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Confirm { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Clear the cache of Hello?");
    assert!(cx.debug_bounds("confirmation-title").is_some());
    let kept = "confirmation-line-Its settings, content and credentials are kept.";
    assert!(cx.debug_bounds(kept).is_some(), "what is kept is rendered");
    assert!(
        cx.debug_bounds("confirmation-destructive").is_some(),
        "Clear Cache is drawn destructive"
    );
    assert!(cx.debug_bounds("confirmation-dismiss").is_some());
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(2)));
    assert!(
        cx.debug_bounds("confirmation").is_none(),
        "the alert is gone"
    );

    // Enter on the row asks again, and Enter on the alert clears it.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        (view.status, view.selected),
        (Status::Result("Cleared the cache of Hello".into()), Some(2))
    );
    assert!(cx.debug_bounds("status-result").is_some());
}

/// Whether the element with debug selector `element` lies wholly inside the
/// list.
fn row_is_visible(cx: &mut VisualTestContext, element: &'static str) -> bool {
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    // The list draws only the rows in view and a few past its edges
    // (#165): a row it did not draw is not visible.
    let Some(element) = cx.debug_bounds(element) else {
        return false;
    };
    element.top() >= list.top() && element.bottom() <= list.bottom()
}

/// A question asked from a scrolled list is an alert over it: the list
/// stays where it was beneath it, and the alert's buttons are in the
/// window however short it is, its message giving up its room first.
#[gpui::test]
fn a_question_over_a_scrolled_extension_list_keeps_the_list_and_fits_the_window(
    cx: &mut TestAppContext,
) {
    for height in [420., 220.] {
        assert_question_over_scrolled_list(cx, height);
    }
}

fn assert_question_over_scrolled_list(cx: &mut TestAppContext, height: f32) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    // Long source paths, as in a real checkout, make the alert's message
    // wrap.
    let nested = sources
        .path()
        .join("code/pane/.claude/worktrees/agent-0123456789abcdef0/target/guests/packages");
    for name in ["one", "two", "three", "four", "five", "six"] {
        let folder = package(&nested.join(name));
        cx.foreground_executor()
            .block_on(launcher.install_package(&folder));
    }
    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));
    // Both the regular and short window overflow the extension list.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(height)));
    launcher.back();
    assert!(matches!(
        enter_flow(&window, cx).screen,
        Screen::Extensions { .. }
    ));
    // The last Uninstall row, which the six hotkey rows follow.
    let last = titles(&launcher.view())
        .iter()
        .rposition(|title| title.starts_with("Uninstall "))
        .unwrap();
    launcher.select(last);
    // Drawn twice: the list's size is known once laid out.
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert!(
        !row_is_visible(cx, "row-Hello"),
        "the list is scrolled to its last row"
    );

    let scrolled = cx.debug_bounds("rows").expect("the list is rendered");

    // That row asks to uninstall the sixth package, with an alert over the
    // list.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Confirm { .. }));
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    // The list beneath is the one the question was asked from, where it
    // was: still scrolled to its last rows.
    assert_eq!(
        cx.debug_bounds("rows").expect("the list stays drawn"),
        scrolled
    );
    assert!(
        !row_is_visible(cx, "row-Hello"),
        "the list beneath stays scrolled"
    );
    // The alert and its buttons are in the window.
    let size = cx.update(|window, _| window.viewport_size());
    for part in [
        "confirmation",
        "confirmation-primary",
        "confirmation-dismiss",
        "confirmation-option",
    ] {
        let bounds = cx
            .debug_bounds(part)
            .unwrap_or_else(|| panic!("{part} is drawn"));
        assert!(
            bounds.top() >= gpui::px(0.) && bounds.bottom() <= size.height,
            "{part} is in the {height}px window: {bounds:?}"
        );
    }
}

#[gpui::test]
fn an_installed_package_is_uninstalled_after_choosing_what_to_keep(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    enter_flow(&window, cx);
    settle(&window, cx);

    // The fourth row asks first, with an alert: Uninstall in red, Cancel,
    // and "Delete saved data", unticked. Escape keeps it installed and
    // returns to that row.
    cx.simulate_keystrokes("down down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Confirm { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Uninstall Hello?");
    assert!(
        cx.debug_bounds("confirmation-line-Saved data: none")
            .is_some(),
        "the saved data is rendered"
    );
    assert!(cx.debug_bounds("confirmation-destructive").is_some());
    assert!(cx.debug_bounds("confirmation-option").is_some());
    assert!(
        cx.debug_bounds("confirmation-option-ticked").is_none(),
        "keeping the data is the default"
    );
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(3)));

    // Space ticks the checkbox; Tab walks to Cancel, past the checkbox,
    // and Enter there presses Cancel: nothing is uninstalled.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("space");
    settle(&window, cx);
    assert!(cx.debug_bounds("confirmation-option-ticked").is_some());
    cx.simulate_keystrokes("tab tab enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(3)));

    // Asked again, the checkbox starts unticked; ticked, Enter uninstalls
    // and deletes the saved data.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert!(cx.debug_bounds("confirmation-option-ticked").is_none());
    cx.simulate_keystrokes("space enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.status,
        Status::Result("Uninstalled Hello and deleted its saved data".into())
    );
    // Only the global automatic-update row, which ends the list, is left.
    assert_eq!(titles(&view), ["Update extensions automatically"]);
    assert!(cx.debug_bounds("status-result").is_some());

    // Root search no longer offers its command, nor the extension list.
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            SETTINGS_ROW
        ]
    );
    assert!(folder.join("pane.json").exists(), "the source is kept");
}

/// Whether the settings file keeps settings for `key`. The file is parsed, since
/// JSON escapes the backslashes of a Windows path in a key.
fn keeps_settings(settings: &Path, key: &str) -> bool {
    let text = fs::read_to_string(settings).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved["packages"].get(key).is_some()
}

#[gpui::test]
fn retained_data_is_deleted_from_the_extension_list_after_confirming(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    // A setting the package saved earlier, as its commands would.
    let key = PackageIdentity::local(&folder).unwrap().key();
    let extensions = data.path().join("extensions");
    let settings = extensions.join("settings.json");
    fs::create_dir_all(&extensions).unwrap();
    let saved = serde_json::json!({ "version": 1, "packages": { &key: { "style": "formal" } } });
    fs::write(&settings, saved.to_string()).unwrap();
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    // Uninstall it, keeping its saved data.
    enter_flow(&window, cx);
    press_enter_on(&window, cx, "Uninstall Hello");
    // The alert's Enter, its checkbox unticked.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        titles(&view),
        [
            "Delete retained data of Hello",
            "Update extensions automatically"
        ]
    );
    assert!(
        cx.debug_bounds("row-Delete retained data of Hello")
            .is_some()
    );

    // It asks first, with an alert saying what is kept: Escape keeps the
    // data and returns to its row, as a click outside the alert does.
    let view = press_enter_on(&window, cx, "Delete retained data of Hello");
    assert_eq!(view.title, "Delete the retained data of Hello?");
    assert!(
        cx.debug_bounds("confirmation-line-Retained data: 1 setting")
            .is_some(),
        "what is kept is rendered"
    );
    assert!(cx.debug_bounds("confirmation-destructive").is_some());
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(0)));
    press_enter_on(&window, cx, "Delete retained data of Hello");
    let corner = gpui::point(px(4.), px(4.));
    cx.simulate_click(corner, Modifiers::none());
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(0)));
    assert!(keeps_settings(&settings, &key));

    // Enter on the alert is its primary button, Delete Data.
    press_enter_on(&window, cx, "Delete retained data of Hello");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        view.status,
        Status::Result("Deleted the retained data of Hello".into())
    );
    // Only the global automatic-update row, which ends the list, is left.
    assert_eq!(titles(&view), ["Update extensions automatically"]);
    assert!(cx.debug_bounds("status-result").is_some());
    assert!(!keeps_settings(&settings, &key));

    // Nothing is left to manage.
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            CREATE_ROW,
            IMPORT_ROW,
            INSTALL_ROW,
            GIT_ROW,
            NPM_ROW,
            SETTINGS_ROW
        ]
    );
    assert!(folder.join("pane.json").exists(), "the source is kept");
}

#[gpui::test]
fn a_package_that_keeps_crashing_is_paused_and_retried(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // A package titled Hello whose command is the faulty fixture: its third
    // item traps.
    let folder = package(&sources.path().join("hello"));
    rebuild(&folder, "faulty");
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);

    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
    cx.simulate_keystrokes("down down enter");
    for _ in 0..2 {
        let view = settle(&window, cx);
        assert!(
            matches!(&view.status, Status::Error(text) if text.starts_with("The extension crashed")),
            "{:?}",
            view.status
        );
        cx.simulate_keystrokes("enter");
    }

    // The third crash pauses it: the toast says so, and root search lists
    // its command with why it does not run.
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    let Status::Error(toast) = &view.status else {
        panic!("expected the toast, got {:?}", view.status);
    };
    assert!(
        toast.starts_with("Hello crashed 3 times within 5 minutes and is paused"),
        "{toast}"
    );
    assert!(cx.debug_bounds("status-error").is_some());
    assert!(
        cx.debug_bounds("unavailable-reason-Say hello").is_some(),
        "the paused command's reason is rendered"
    );

    // The extension list shows it paused, with Retry; Retry starts it.
    let view = click_in_extension_list(&window, cx, "row-Retry Hello");
    assert_eq!(view.status, Status::Result("Started Hello".into()));
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].unavailable, None);
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
}

/// Writes an operations fixture package titled `title` in `folder`,
/// publishing `echo` 1 and declaring `dependencies` (JSON array contents).
fn operations_package(folder: &Path, title: &str, dependencies: &str) -> PathBuf {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/operations_fixture.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::copy(guest, folder.join("fixture.wasm")).unwrap();
    let manifest = format!(
        r#"{{
            "manifestVersion": 1,
            "title": "{title}",
            "apiVersion": "0.2",
            "operations": [{{ "id": "echo", "version": 1, "component": "fixture.wasm" }}],
            "dependencies": [{dependencies}]
        }}"#
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
    folder.to_path_buf()
}

#[gpui::test]
fn uninstalling_a_required_dependency_shows_its_dependent_and_the_choices_in_the_window(
    cx: &mut TestAppContext,
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    // Long source paths, as in a real checkout, make the details wrap.
    let nested = sources
        .path()
        .join("code/pane/.claude/worktrees/agent-0123456789abcdef0/target/guests/packages");
    operations_package(&nested.join("greeter"), "Greeter", "");
    let caller = operations_package(
        &nested.join("caller"),
        "Caller",
        r#"{ "id": "greeter", "source": "local:../greeter",
             "operations": [{ "id": "echo", "version": 1 }] }"#,
    );
    cx.foreground_executor()
        .block_on(launcher.install_package(&caller));
    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    launcher.back();
    settle(&window, cx);
    enter_flow(&window, cx);

    let view = press_enter_on(&window, cx, "Uninstall Greeter");
    assert_eq!(
        view.title,
        "Uninstall Greeter and the extensions that require it?"
    );
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    // The alert names the extension that requires it and each one's
    // saved data, with Uninstall All on Enter and "Delete saved data".
    for line in [
        "confirmation-line-Caller, which requires Greeter",
        "confirmation-line-Saved data: Greeter none · Caller none",
    ] {
        assert!(cx.debug_bounds(line).is_some(), "{line} is rendered");
    }
    assert!(cx.debug_bounds("confirmation-option").is_some());
    // However long the message, the buttons are in the window.
    let primary = cx
        .debug_bounds("confirmation-primary")
        .expect("Uninstall All is drawn");
    assert!(primary.bottom() <= gpui::px(420.), "{primary:?}");

    // Escape keeps both installed; Enter on the alert uninstalls both.
    cx.simulate_keystrokes("escape");
    assert!(matches!(
        settle(&window, cx).screen,
        Screen::Extensions { .. }
    ));
    assert_eq!(launcher.packages().len(), 2);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result(
            "Uninstalled Greeter and Caller, which requires it; their settings and content are \
             kept"
                .into()
        )
    );
    assert!(launcher.packages().is_empty());
}

/// Writes a package titled `title` in `folder` whose command and `echo`
/// operation the operations fixture serves, declaring `dependencies` (JSON
/// array contents).
fn command_package(folder: &Path, title: &str, dependencies: &str) -> PathBuf {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/operations_fixture.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::copy(guest, folder.join("fixture.wasm")).unwrap();
    let manifest = format!(
        r#"{{
            "manifestVersion": 1,
            "title": "{title}",
            "apiVersion": "0.2",
            "commands": [{{ "id": "call", "title": "Call", "component": "fixture.wasm" }}],
            "operations": [{{ "id": "echo", "version": 1, "component": "fixture.wasm" }}],
            "dependencies": [{dependencies}]
        }}"#
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
    folder.to_path_buf()
}

#[gpui::test]
fn a_waiting_command_s_row_shows_the_reason_and_enter_offers_the_fix(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let greeter = command_package(&sources.path().join("greeter"), "Greeter", "");
    let caller = command_package(
        &sources.path().join("caller"),
        "Caller",
        r#"{ "id": "greeter", "source": "local:../greeter",
             "operations": [{ "id": "echo", "version": 1 }] }"#,
    );
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    cx.foreground_executor()
        .block_on(launcher.install_package(&caller));
    // The greeter is disabled, so Caller's command waits for it (#152).
    let greeter = PackageIdentity::local(&greeter).unwrap();
    cx.foreground_executor()
        .block_on(launcher.set_enabled(&greeter, false));

    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));

    // The waiting command stays listed, with what it needs under its row.
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, "Call");
    assert_eq!(view.selected, Some(0));
    assert_eq!(
        view.rows[0]
            .unavailable
            .as_ref()
            .map(|reason| reason.reason().to_owned()),
        Some("Needs Greeter, which is disabled".to_owned())
    );
    assert!(
        cx.debug_bounds("unavailable-reason-Call").is_some(),
        "the waiting command's reason is rendered"
    );

    // Enter shows the reason, with the row that fixes it.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.title, "Why Call cannot run");
    assert!(
        cx.debug_bounds("detail-Needs Greeter, which is disabled.")
            .is_some(),
        "the reason is rendered"
    );
    assert_eq!(titles(&view), ["Enable Greeter"]);
    assert!(cx.debug_bounds("row-Enable Greeter").is_some());

    // Choosing it enables the greeter; the command comes back by itself.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Enabled Greeter".into()));
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.rows[0].unavailable, None);
    // Its command opens again.
    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
}

/// The `provides` of a package providing the fixture's capability, as
/// manifest members.
const PROVIDES: &str = r#","provides": [{ "capability": "fixture:greet@1", "component":
     "fixture.wasm", "operations": ["greet"] }]"#;

/// Writes a package titled `title` in `folder` around the capabilities
/// fixture component, with `members` (manifest members, each starting with
/// a comma) and, when `command` is not empty, one command titled `command`
/// that calls the capability.
fn capability_package(folder: &Path, title: &str, command: &str, members: &str) -> PathBuf {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/capabilities_fixture.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::copy(guest, folder.join("fixture.wasm")).unwrap();
    let manifest = if command.is_empty() {
        format!(r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.2"{members} }}"#)
    } else {
        format!(
            r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.2",
                 "commands": [{{ "id": "fixture", "title": "{command}",
                                 "component": "fixture.wasm" }}]{members} }}"#
        )
    };
    fs::write(folder.join("pane.json"), manifest).unwrap();
    folder.to_path_buf()
}

#[gpui::test]
fn the_install_preview_lists_the_capabilities_a_package_uses(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    capability_package(&sources.path().join("greeter"), "Greeter", "", PROVIDES);
    let caller = capability_package(
        &sources.path().join("caller"),
        "Caller",
        "Call",
        r#","uses": [{ "capability": "fixture:greet@1", "operations": ["greet"],
             "default": "local:../greeter" }]"#,
    );
    let (window, cx) = open(cx, &data);

    // No provider is installed, so the preview says Pane installs the
    // default provider Caller names, and the Install row says so too.
    let view = choose_folder(&window, cx, Some(caller));
    assert!(
        cx.debug_bounds(
            "detail-Uses fixture:greet@1: no installed extension provides it; Pane installs \
             Greeter, which Caller names"
        )
        .is_some(),
        "the capability line is rendered"
    );
    assert_eq!(titles(&view), ["Install"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some(
            "Copy the package into Pane and add its commands, and install Greeter, which it \
              names"
        )
    );

    // Installing installs the default provider with it, and its command
    // calls through the capability.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Caller with Greeter, which it names".into())
    );
    // The blank query lists Pane's own rows with the command, in its
    // order; the command's row is the selected one.
    assert_eq!(
        view.selected.map(|index| view.rows[index].title.as_str()),
        Some("Call")
    );
    assert_eq!(
        titles(&view),
        [
            "Call",
            "Check for Extension Updates",
            "Create Extension…",
            "Import Extension…",
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Manage Extensions",
            "Settings…"
        ]
    );

    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result(
            r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#.into()
        )
    );
}

#[gpui::test]
fn disable_only_disables_the_dependency_alone_and_its_dependent_waits(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    command_package(&sources.path().join("greeter"), "Greeter", "");
    let caller = command_package(
        &sources.path().join("caller"),
        "Caller",
        r#"{ "id": "greeter", "source": "local:../greeter",
             "operations": [{ "id": "echo", "version": 1 }] }"#,
    );
    let (window, cx) = open(cx, &data);
    install(&window, cx, &caller);

    // The row of the greeter asks about its dependents first, with an
    // alert whose Disable Only stands between Cancel and Disable All.
    let view = click_in_extension_list(&window, cx, "row-Greeter");
    assert!(matches!(view.screen, Screen::Confirm { .. }));
    let secondary = cx
        .debug_bounds("confirmation-secondary")
        .expect("Disable Only is drawn");
    for button in ["confirmation-dismiss", "confirmation-primary"] {
        assert!(cx.debug_bounds(button).is_some(), "{button} is drawn");
    }
    assert!(
        cx.debug_bounds("confirmation-destructive").is_none(),
        "disabling removes nothing"
    );

    // Clicking it disables the greeter alone.
    cx.simulate_click(secondary.center(), Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Disabled Greeter".into()));
    assert_eq!(view.rows[view.selected.unwrap()].title, "Greeter");

    // Caller stays enabled, and its command waits for the greeter.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    let call = view
        .rows
        .iter()
        .find(|row| row.title == "Call")
        .cloned()
        .expect("the command is listed");
    assert_eq!(
        call.unavailable.as_ref().map(|why| why.reason().to_owned()),
        Some("Needs Greeter, which is disabled".to_owned())
    );

    // Enabling the greeter again brings the command back by itself.
    let view = click_in_extension_list(&window, cx, "row-Greeter");
    assert_eq!(view.status, Status::Result("Enabled Greeter".into()));
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    let call = view
        .rows
        .iter()
        .find(|row| row.title == "Call")
        .cloned()
        .expect("the command is listed");
    assert_eq!(call.unavailable, None);
}
