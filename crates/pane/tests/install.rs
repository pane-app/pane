//! Installing a local extension package through the native window, on
//! GPUI's test platform: the install row opens a folder picker, the chosen
//! package is previewed, Enter installs it and its command runs.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

#[path = "support/packages.rs"]
mod packages;

use packages::package;

const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const MANAGE_ROW: &str = "Manage extensions…";
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

/// Presses Enter on the install row and answers the folder picker.
fn choose_folder(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    folder: Option<PathBuf>,
) -> LauncherView {
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "a folder picker opened");
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        folder.map(|folder| vec![folder])
    });
    settle(window, cx)
}

#[gpui::test]
fn a_chosen_package_is_previewed_installed_and_run(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    assert_eq!(
        titles(&settle(&window, cx)),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW]
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
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
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

#[gpui::test]
fn cancelling_the_folder_picker_stays_on_root(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open(cx, &data);

    let view = choose_folder(&window, cx, None);

    assert_eq!((view.query(), &view.status), (Some(""), &Status::Idle));
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
        [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW]
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
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );

    cx.simulate_keystrokes("down down down down enter");
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
        [INSTALL_ROW, NPM_ROW, GIT_ROW, MANAGE_ROW, SETTINGS_ROW]
    );

    cx.simulate_keystrokes("down down down enter");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Enabled Hello".into())
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
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

/// From root search, clicks Manage extensions… and then the row whose
/// debug selector is `row`. The pointer moves onto Manage extensions…
/// first, as a user's does: root search selects the row under a moving
/// pointer, so the click runs it, where a click with no movement before
/// it only selects an unselected row.
fn click_in_extension_list(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    row: &'static str,
) -> LauncherView {
    let manage = cx
        .debug_bounds("row-Manage extensions…")
        .expect("row")
        .center();
    for at in [manage - gpui::point(px(1.), px(0.)), manage] {
        cx.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
    }
    cx.simulate_click(manage, Modifiers::none());
    let view = settle(window, cx);
    assert!(
        matches!(view.screen, Screen::Extensions { .. }),
        "the click opened the extension list: {:?}",
        view.screen
    );
    let row = cx.debug_bounds(row).expect("row rendered");
    cx.simulate_click(row.center(), Modifiers::none());
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

    // Pane stayed open; the command now runs the new code.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
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
    cx.simulate_keystrokes("down down down down enter");
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

    // The third row asks first, saying what is kept; Escape keeps the cache
    // and returns to that row.
    cx.simulate_keystrokes("down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Confirm { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Clear the cache of Hello?");
    assert_eq!(titles(&view), ["Clear cache", "Cancel"]);
    let kept = "detail-Pane deletes the data this extension keeps as its cache. Its settings, \
                content and credentials are kept, and the extension does not run.";
    assert!(cx.debug_bounds(kept).is_some(), "what is kept is rendered");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(2)));

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
    let element = cx.debug_bounds(element).expect("it is rendered");
    element.top() >= list.top() && element.bottom() <= list.bottom()
}

#[gpui::test]
fn a_short_confirmation_after_a_scrolled_extension_list_shows_its_first_choice(
    cx: &mut TestAppContext,
) {
    for height in [420., 220.] {
        assert_confirmation_scroll_reset(cx, height);
    }
}

fn assert_confirmation_scroll_reset(cx: &mut TestAppContext, height: f32) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    // Long source paths, as in a real checkout, make the confirmation's
    // details wrap, which moves and shrinks its list.
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
    // Both the regular and short window overflow the extension list. In
    // the short window even the confirmation has less room than one row.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(height)));
    launcher.back();
    let manage = titles(&launcher.view())
        .iter()
        .position(|title| *title == MANAGE_ROW)
        .unwrap();
    launcher.select(manage);
    cx.simulate_keystrokes("enter");
    assert!(matches!(
        settle(&window, cx).screen,
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

    // That row asks to uninstall the sixth package: a screen of three
    // short rows, the first selected.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    // The first frame of the new screen scrolls with the list's size and
    // rows as last laid out, those of the extension list; on a real
    // platform nothing else may redraw the window, so it must ask for the
    // next frame to scroll again with the new ones. The test platform
    // delivers that frame when told to.
    let asked = cx.update(|window, cx| window.simulate_next_frame(cx));
    assert!(asked > 0, "the window asks for a frame to scroll again");
    cx.run_until_parked();
    // Compare with this same confirmation opened in a fresh window, which
    // has no inherited scroll offset. A row can start below the viewport's
    // edge because of list padding; it must retain that natural position,
    // not an offset left over from the extension list.
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let first = cx
        .debug_bounds("row-Uninstall and keep saved data")
        .expect("it is rendered");
    if height == 220. {
        assert!(
            list.size.height < first.size.height,
            "the short confirmation viewport is smaller than its first row"
        );
    }
    let size = cx.update(|window, _| window.viewport_size());
    let (fresh_window, fresh_cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));
    fresh_cx.simulate_resize(size);
    settle(&fresh_window, fresh_cx);
    fresh_cx.update(|window, cx| window.simulate_next_frame(cx));
    fresh_cx.run_until_parked();
    let fresh_list = fresh_cx
        .debug_bounds("rows")
        .expect("the fresh list is rendered");
    let fresh_first = fresh_cx
        .debug_bounds("row-Uninstall and keep saved data")
        .expect("the fresh first choice is rendered");
    assert_eq!(
        list.size, fresh_list.size,
        "the confirmation viewports match"
    );
    assert!(
        first.top() >= list.top(),
        "the first choice's top is not clipped"
    );
    assert_eq!(
        first.top() - list.top(),
        fresh_first.top() - fresh_list.top(),
        "the selected first choice has its unscrolled position"
    );
}

#[gpui::test]
fn an_installed_package_is_uninstalled_after_choosing_what_to_keep(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    cx.simulate_keystrokes("down down down down enter");
    settle(&window, cx);

    // The fourth row asks first, with a choice about the saved data; Escape
    // keeps it installed and returns to that row.
    cx.simulate_keystrokes("down down down enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Confirm { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Uninstall Hello?");
    assert_eq!(
        titles(&view),
        [
            "Uninstall and keep saved data",
            "Uninstall and delete saved data",
            "Cancel"
        ]
    );
    assert!(
        cx.debug_bounds("detail-Saved data: none").is_some(),
        "the saved data is rendered"
    );
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(3)));

    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down enter");
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
        [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW]
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
    press_enter_on(&window, cx, MANAGE_ROW);
    press_enter_on(&window, cx, "Uninstall Hello");
    let view = press_enter_on(&window, cx, "Uninstall and keep saved data");
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

    // It asks first, saying what is kept, with Cancel selected: Enter keeps
    // the data and returns to its row, as Escape does.
    let view = press_enter_on(&window, cx, "Delete retained data of Hello");
    assert_eq!(view.title, "Delete the retained data of Hello?");
    assert_eq!(titles(&view), ["Cancel", "Delete retained data"]);
    assert_eq!(view.selected, Some(0));
    assert!(
        cx.debug_bounds("detail-Retained data: 1 setting").is_some(),
        "what is kept is rendered"
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(0)));
    press_enter_on(&window, cx, "Delete retained data of Hello");
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!((view.status, view.selected), (Status::Idle, Some(0)));
    assert!(keeps_settings(&settings, &key));

    press_enter_on(&window, cx, "Delete retained data of Hello");
    let view = press_enter_on(&window, cx, "Delete retained data");
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
        [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW]
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
            "apiVersion": "0.1",
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
    press_enter_on(&window, cx, MANAGE_ROW);

    let view = press_enter_on(&window, cx, "Uninstall Greeter");
    assert_eq!(
        view.title,
        "Uninstall Greeter and the extensions that require it?"
    );
    assert_eq!(
        titles(&view),
        [
            "Uninstall all 2 and keep saved data",
            "Uninstall all 2 and delete saved data",
            "Cancel"
        ]
    );
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    assert!(
        cx.debug_bounds("detail-Saved data: Greeter none · Caller none")
            .is_some(),
        "the saved data is rendered"
    );
    // However long the details, the selected first choice is on screen.
    assert!(row_is_visible(
        cx,
        "row-Uninstall all 2 and keep saved data"
    ));
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    assert!(list.bottom() <= gpui::px(420.), "{list:?}");

    // Escape keeps both installed; the first choice uninstalls both.
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
