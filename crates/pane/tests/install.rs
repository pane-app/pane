//! Installing a local extension package through the native window, on
//! GPUI's test platform: the install row opens a folder picker, the chosen
//! package is previewed, Enter installs it and its command runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};
use tempfile::TempDir;

const INSTALL_ROW: &str = "Install extension from folder…";
const MANAGE_ROW: &str = "Manage extensions…";

/// Writes a package folder whose one command is the Rust sample.
fn package(folder: &Path) -> PathBuf {
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

/// Lets the window apply replies that arrive from other threads.
fn settle(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.status != Status::Running {
            return view;
        }
        assert!(Instant::now() < deadline, "the launcher did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
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
    assert_eq!(titles(&settle(&window, cx)), [INSTALL_ROW]);

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
    assert_eq!(titles(&view), ["Say hello", INSTALL_ROW, MANAGE_ROW]);
    assert_eq!(view.status, Status::Result("Installed Hello".into()));
    assert!(cx.debug_bounds("status-result").is_some());

    cx.simulate_keystrokes("enter");
    assert_eq!(settle(&window, cx).screen, Screen::Command);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
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
    assert!(view.rows.is_empty());
    assert!(
        cx.debug_bounds("status-error").is_some(),
        "the reason is rendered"
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(titles(&settle(&window, cx)), [INSTALL_ROW]);
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
        ["Say hello", INSTALL_ROW, MANAGE_ROW]
    );

    cx.simulate_keystrokes("down down enter");
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
            "Hotkey for Say hello"
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
    assert_eq!(titles(&settle(&window, cx)), [INSTALL_ROW, MANAGE_ROW]);

    cx.simulate_keystrokes("down enter");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Enabled Hello".into())
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(
        titles(&settle(&window, cx)),
        ["Say hello", INSTALL_ROW, MANAGE_ROW]
    );
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
/// debug selector is `row`.
fn click_in_extension_list(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    row: &'static str,
) -> LauncherView {
    let manage = cx.debug_bounds("row-Manage extensions…").expect("row");
    cx.simulate_click(manage.center(), Modifiers::none());
    settle(window, cx);
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
        settle(&window, cx).status,
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
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello"
        ]
    );

    // The Reload row stays selected; Retry is next.
    cx.simulate_keystrokes("down enter");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Result("Started Hello".into()));
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello"
        ]
    );
}

#[gpui::test]
fn an_installed_package_cache_is_cleared_after_confirming(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    cx.simulate_keystrokes("down down enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert_eq!(
        titles(&view),
        [
            "Hello",
            "Reload Hello",
            "Clear cache of Hello",
            "Uninstall Hello",
            "Hotkey for Say hello"
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
    // The size of Pane's window: the extension list of six packages
    // overflows it.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
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
    // The list shows the first choice from its top (the long source paths
    // leave the list less room than one row).
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let first = cx
        .debug_bounds("row-Uninstall and keep saved data")
        .expect("it is rendered");
    assert_eq!(
        first.top(),
        list.top(),
        "the selected first choice is shown from its top"
    );
}

#[gpui::test]
fn an_installed_package_is_uninstalled_after_choosing_what_to_keep(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);
    install(&window, cx, &folder);
    cx.simulate_keystrokes("down down enter");
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
    assert!(view.rows.is_empty());
    assert!(cx.debug_bounds("status-result").is_some());

    // Root search no longer offers its command, nor the extension list.
    cx.simulate_keystrokes("escape");
    assert_eq!(titles(&settle(&window, cx)), [INSTALL_ROW]);
    assert!(folder.join("pane.json").exists(), "the source is kept");
}
