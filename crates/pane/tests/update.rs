//! The update Pane applies by itself, in the native window on GPUI's test
//! platform: what its pass came to reaches the window by itself, as a
//! failure's announcement — a pass that failed something is announced
//! once, the next time the launcher is shown — and the update results
//! view the announcement's View Details opens works by keyboard, as any
//! list does. The wiring is the real app's order — the launcher's
//! constructor starts the updater's thread before `with_development`
//! wires the channel that tells the window — so a thread holding a
//! launcher snapshot from before the wiring still has to reach the
//! window.
//!
//! The registry is a local one on 127.0.0.1 (`pane-core`'s test support)
//! or a closed port standing for one that is down; nothing reaches the
//! network. The launcher's clock is a manual one, so the first check — a
//! minute after Pane starts — comes when the test moves it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::clipboard::{Clock as _, ManualClock, SystemClock};
use pane_core::develop::Toolchains;
use pane_core::npm::Registry as NpmRegistry;
use pane_core::tray::TrayAction;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};

#[path = "../../pane-core/tests/support/npm_registry.rs"]
mod npm_registry;

use npm_registry::{Registry, greeter_files, pack};

/// A port of 127.0.0.1 that refuses connections for as long as this is
/// kept, as a registry that is down does: a socket bound to it that never
/// listens, so nothing else can take the port meanwhile. The pane-core
/// fixture this imitates; tokio's socket, whose `net` feature pane-core
/// already turns on for this build.
struct ClosedPort(tokio::net::TcpSocket);

impl ClosedPort {
    fn new() -> ClosedPort {
        let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
        socket
            .bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("a free port");
        ClosedPort(socket)
    }

    fn port(&self) -> u16 {
        self.0.local_addr().unwrap().port()
    }
}

#[path = "support/settle.rs"]
mod settle;

#[path = "support/setup.rs"]
mod setup;

use settle::{settle, until};
use setup::{actions_shortcut, settings_shortcut};

/// The npm name of the first package the tests install.
const NAME: &str = "@pane-samples/greeter";

/// The npm name of the second package the tests install, titled
/// differently so a search can tell its row from the first's.
const SECOND: &str = "@pane-tests/greeter";

/// The toast key on this system.
const TOAST_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-t"
} else {
    "ctrl-t"
};

/// The Greeter sample's files as a second package, titled "Second
/// greeter" and named `SECOND`: told apart from the first in the
/// results, and its tarball holds the package asked for.
fn second_files(guests: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut files = greeter_files(guests, "0.1.0");
    for (path, contents) in &mut files {
        let (field, value) = match *path {
            "package.json" => ("name", serde_json::json!(SECOND)),
            "pane.json" => ("title", serde_json::json!("Second greeter")),
            _ => continue,
        };
        let mut json: serde_json::Value = serde_json::from_slice(contents).unwrap();
        json[field] = value;
        *contents = serde_json::to_vec_pretty(&json).unwrap();
    }
    files
}

/// The launcher as the real app builds it up to the development wiring:
/// the constructor — whose installation starts the updater's thread — and
/// the registry and clock, before `with_development` (which wires the
/// channel that tells the window of background changes) runs after, so a
/// thread holding a launcher snapshot from before the wiring still has
/// to reach the window.
fn launcher(
    runtime: &Runtime,
    data: &tempfile::TempDir,
    registry: &Registry,
    clock: &Arc<ManualClock>,
) -> Launcher {
    Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
        .with_npm_registry(NpmRegistry::local(registry.url()).unwrap())
        .with_clock(clock.clone())
}

/// A launcher reading a registry that refuses connections, as one that is
/// down does: its first check fails every package it considers.
fn failing_launcher(
    runtime: &Runtime,
    data: &tempfile::TempDir,
    closed: &ClosedPort,
    clock: &Arc<ManualClock>,
) -> Launcher {
    let url = format!("http://127.0.0.1:{}/", closed.port());
    Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
        .with_npm_registry(NpmRegistry::local(&url).unwrap())
        .with_clock(clock.clone())
}

/// Runs the window until `what` holds of the launcher: its state can
/// change on other threads (the updater's) before the window is told to
/// redraw, so a test that read it then would race the frame that shows
/// it.
fn until_launcher(
    window: &gpui::Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    mut what: impl FnMut(&Launcher) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if cx.read_entity(window, |window, _| what(window.launcher())) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the launcher never came to what was waited for"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until something named `debug` is drawn, as the tests'
/// bounds find it; `what` names it in the panic.
fn until_drawn(cx: &mut VisualTestContext, debug: &'static str, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while cx.debug_bounds(debug).is_none() {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "{what} was never drawn");
        thread::sleep(Duration::from_millis(5));
    }
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

/// Clicks the row whose debug selector is `row`, as its user would: the
/// pointer moving onto it first.
fn click_row(settings_cx: &mut VisualTestContext, row: &'static str) {
    let bounds = settings_cx
        .debug_bounds(row)
        .unwrap_or_else(|| panic!("no {row} on the Extensions page"));
    settings_cx.simulate_mouse_move(
        bounds.center(),
        None::<gpui::MouseButton>,
        Modifiers::none(),
    );
    settings_cx.simulate_click(bounds.center(), Modifiers::none());
    settings_cx.run_until_parked();
}

/// A launcher window on a Pane started again on data holding the two
/// packages, whose first check — a minute after it starts, when the test
/// moves the clock — fails both, the registry it reads refusing
/// connections. The window is hidden while the pass runs: the record
/// holds the two failures and nothing is announced yet, so the test can
/// show the launcher when it wants the announcement.
fn hidden_behind_a_failed_check(
    cx: &mut TestAppContext,
) -> (gpui::Entity<LauncherWindow>, &mut VisualTestContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(NAME, "0.1.0", pack(&greeter_files(&guests, "0.1.0")));
    registry.publish(SECOND, "0.1.0", pack(&second_files(&guests)));
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);

    // The earlier Pane: both packages installed, then stopped.
    let runtime = Runtime::start().unwrap();
    let clock = Arc::new(ManualClock::at(SystemClock.now()));
    let first = launcher(&runtime, &data, &registry, &clock);
    futures::executor::block_on(first.install_npm(NAME));
    futures::executor::block_on(first.install_npm(SECOND));
    drop(first);

    // The Pane started again, as the smoke restarts it: its first check
    // fails both packages, and the window follows the launcher's
    // background changes; nothing here touches it.
    let closed = ClosedPort::new();
    let (sender, changes) = pane_core::changes::channel();
    let second = failing_launcher(&runtime, &data, &closed, &clock)
        .with_development(Arc::new(Toolchains::from_env(None)), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(second, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    // Hidden (Escape at an empty root search hides the launcher), so the
    // pass runs while nothing shows: the failure waits for the next
    // showing.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.read_entity(&window, |window, _| window.hidden()));
    clock.advance(Duration::from_secs(62));
    until_launcher(&window, cx, |launcher| {
        launcher.update_results().failed.len() == 2
    });
    (window, cx)
}

/// Shows the hidden launcher (the tray's Open Pane, as the hotkey's show
/// path is): the failures its check recorded are announced, the toast
/// drawn in a frame.
fn announce_by_showing(
    window: &gpui::Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> pane_core::ShownToast {
    window.update_in(cx, |view, window, cx| {
        view.tray_selected(TrayAction::OpenPane, window, cx)
    });
    until_drawn(cx, "toast-failure", "the failure toast");
    window.update(cx, |window, _| {
        window.launcher().toast().expect("the failure toast")
    })
}

/// Opens the update results view through the announcement toast's View
/// Details: the toast key focuses the action, Enter chooses it.
fn view_details(window: &gpui::Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(TOAST_KEY);
    settle(window, cx);
    cx.simulate_keystrokes("enter");
    until(window, cx, |view| {
        matches!(view.screen, Screen::UpdateResults { .. })
    });
}

#[gpui::test]
fn a_failed_check_is_announced_by_itself_when_the_launcher_is_shown(cx: &mut TestAppContext) {
    let (window, cx) = hidden_behind_a_failed_check(cx);

    // Nothing is announced while the launcher is hidden.
    assert!(cx.debug_bounds("toast-failure").is_none());
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().toast()),
        None
    );

    // Shown, the failure is announced by itself, in a drawn frame: the
    // record the updater's thread wrote reached the window through the
    // channel, with no user action.
    let toast = announce_by_showing(&window, cx);
    assert_eq!(toast.toast.title, "2 extension updates failed");
    assert_eq!(
        toast
            .toast
            .primary
            .as_ref()
            .map(|action| action.title.as_str()),
        Some("View Details")
    );

    // View Details opens the results view, drawn with its rows and the
    // groups' labels; a failure's detail ends as the record does.
    view_details(&window, cx);
    until_drawn(cx, "section-Failed", "the Failed group");
    until_drawn(cx, "results-row-Greeter from npm", "the failed row");
    assert!(cx.read_entity(&window, |window, _| window.update_results_shown()));
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert_eq!(view.title, "Update Results");
    assert_eq!(view.rows.len(), 2);
    assert!(view.rows.iter().all(|row| {
        row.subtitle
            .as_deref()
            .is_some_and(|detail| detail.ends_with("It keeps running its installed code."))
    }));
}

#[gpui::test]
fn the_update_results_view_works_by_keyboard(cx: &mut TestAppContext) {
    let (window, cx) = hidden_behind_a_failed_check(cx);
    announce_by_showing(&window, cx);
    view_details(&window, cx);

    // The search field has the focus, as root search's does when it is
    // on screen: typing filters the rows by what they say.
    cx.simulate_keystrokes("f r o m");
    let view = until(&window, cx, |view| view.rows.len() == 1);
    assert_eq!(view.rows[0].title, "Greeter from npm");
    // Backspacing the search lists every row again.
    for _ in 0..4 {
        cx.simulate_keystrokes("backspace");
    }
    let view = until(&window, cx, |view| view.rows.len() == 2);

    // The arrows move the selection through the rows.
    assert_eq!(view.selected, Some(0));
    cx.simulate_keystrokes("down");
    let view = until(&window, cx, |view| view.selected == Some(1));
    assert_eq!(view.rows[1].title, "Second greeter");
    cx.simulate_keystrokes("up");
    until(&window, cx, |view| view.selected == Some(0));

    // The Actions panel offers the selected row's actions: showing its
    // extension's page, retrying it (a Failed row: an asked pass over
    // that extension alone), and copying its details.
    cx.simulate_keystrokes(actions_shortcut());
    until_drawn(cx, "action-Show Extension", "the Show Extension entry");
    until_drawn(cx, "action-Retry", "the Retry entry");
    until_drawn(cx, "action-Copy Details", "the Copy Details entry");
    // Down moves to Retry, down again to Copy Details; Enter runs it, and
    // the details are on the clipboard.
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        let expected = view.rows[0].title.clone();
        if copied
            .as_deref()
            .is_some_and(|copied| copied.starts_with(&expected))
        {
            assert!(
                copied
                    .unwrap()
                    .contains("It keeps running its installed code.")
            );
            break;
        }
        assert!(Instant::now() < deadline, "the details were not copied");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        settle(&window, cx).status,
        Status::Result("Copied the details".into())
    );

    // Enter opens the selected row's extension's page in Settings.
    cx.simulate_keystrokes("enter");
    let expected = PackageIdentity::npm(NAME).key();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let shown = settings_windows(cx)
            .first()
            .and_then(|settings| {
                settings
                    .update(cx, |settings, _, _| settings.shown_extension())
                    .ok()
            })
            .flatten();
        if shown.is_some() {
            assert_eq!(shown, Some(expected), "the selected row's extension");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the extension's page never showed"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_extensions_group_in_settings_shows_the_update_results(cx: &mut TestAppContext) {
    let (window, cx) = hidden_behind_a_failed_check(cx);
    // The record is there without the announcement (the launcher stays
    // hidden): Settings reads it through the launcher's own operations.
    assert_eq!(
        cx.read_entity(&window, |window, _| window
            .launcher()
            .update_results()
            .failed
            .len()),
        2
    );

    // Shown again (the announcement follows; the Settings window is
    // separate), Settings opens from the launcher, at the Extensions
    // group.
    announce_by_showing(&window, cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();

    // The group's page offers the record as a row of the launcher's own;
    // choosing it opens the results screen in place of the page, its
    // groups drawn.
    click_row(&mut settings_cx, "extension-row-Update Results");
    until_drawn(&mut settings_cx, "section-Failed", "the Failed group");
    until_drawn(
        &mut settings_cx,
        "extension-row-Greeter from npm",
        "the failed row",
    );

    // A row opens its extension's page, as the launcher window's Enter
    // opens it in Settings.
    click_row(&mut settings_cx, "extension-row-Greeter from npm");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        settings_cx.run_until_parked();
        let shown = settings
            .update(&mut settings_cx, |settings, _, _| {
                settings.shown_extension()
            })
            .ok()
            .flatten();
        if let Some(shown) = shown {
            assert_eq!(shown, PackageIdentity::npm(NAME).key());
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the extension's page never showed"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// The metadata path the registry is asked for the package `name`.
fn metadata_path(name: &str) -> String {
    format!("/{}", name.replace('/', "%2f"))
}

/// The Greeter sample's files at `version` whose `pane.json` declares a
/// dependency no registry holds: a newer version the pass fails to
/// install, as a preview would refuse it.
fn failing_files(guests: &Path, version: &str) -> Vec<(&'static str, Vec<u8>)> {
    let mut files = greeter_files(guests, version);
    for (path, contents) in &mut files {
        if *path != "pane.json" {
            continue;
        }
        let mut json: serde_json::Value = serde_json::from_slice(contents).unwrap();
        json["dependencies"] = serde_json::json!([{
            "id": "nobody",
            "source": "npm:nobody",
            "operations": [{ "id": "nothing", "version": 1 }],
        }]);
        *contents = serde_json::to_vec_pretty(&json).unwrap();
    }
    files
}

/// A launcher window on a Pane with the two packages installed, the
/// registry holding a newer good version of the first and a newer
/// uninstallable one of the second, and the first's automatic updates
/// turned off: the pass the clock brings (a minute after Pane starts)
/// fails the second and skips the first, so its failure is announced when
/// the launcher is shown and the record holds both rows. The registry is
/// returned with the window: it must outlive the fixture, whose passes
/// and the test's own read from it.
fn skipped_and_failed(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<LauncherWindow>,
    Registry,
    &mut VisualTestContext,
) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(NAME, "0.1.0", pack(&greeter_files(&guests, "0.1.0")));
    registry.publish(SECOND, "0.1.0", pack(&second_files(&guests)));
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    let clock = Arc::new(ManualClock::at(SystemClock.now()));
    let first = launcher(&runtime, &data, &registry, &clock);
    futures::executor::block_on(first.install_npm(NAME));
    futures::executor::block_on(first.install_npm(SECOND));
    // The first's automatic updates are turned off, as its row in the
    // extension list does it: the pass skips it for the user's switch.
    first.manage_extensions();
    let index = first
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Update Greeter from npm automatically")
        .expect("the first's automatic updates row");
    first.select(index);
    futures::executor::block_on(first.activate_selected());
    drop(first);

    // The Pane the window follows: its first check fails the second (the
    // newer version's dependency cannot be installed) and skips the
    // first (the user's switch), hidden while it runs.
    registry.publish(NAME, "0.2.0", pack(&greeter_files(&guests, "0.2.0")));
    registry.publish(SECOND, "0.2.0", pack(&failing_files(&guests, "0.2.0")));
    let (sender, changes) = pane_core::changes::channel();
    let second = launcher(&runtime, &data, &registry, &clock)
        .with_development(Arc::new(Toolchains::from_env(None)), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(second, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    clock.advance(Duration::from_secs(62));
    until_launcher(&window, cx, |launcher| {
        let recorded = launcher.update_results();
        recorded.failed.len() == 1 && recorded.skipped.len() == 1
    });
    (window, registry, cx)
}

#[gpui::test]
fn the_root_search_row_starts_the_pass_and_its_toast_ends_with_view_details(
    cx: &mut TestAppContext,
) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(NAME, "0.1.0", pack(&greeter_files(&guests, "0.1.0")));
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    let clock = Arc::new(ManualClock::at(SystemClock.now()));
    let first = launcher(&runtime, &data, &registry, &clock);
    futures::executor::block_on(first.install_npm(NAME));
    registry.publish(NAME, "0.2.0", pack(&greeter_files(&guests, "0.2.0")));
    drop(first);

    let (sender, changes) = pane_core::changes::channel();
    let second = launcher(&runtime, &data, &registry, &clock)
        .with_development(Arc::new(Toolchains::from_env(None)), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(second, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });

    // Typing finds the row; the registry holds the metadata answer back,
    // so the pass stays in its checking phase and the window draws the
    // toast that follows it.
    let held = registry.hold(&metadata_path(NAME));
    cx.simulate_keystrokes("c h e c k");
    let selected = |view: &pane_core::LauncherView| {
        view.selected.is_some_and(|index| {
            view.rows
                .get(index)
                .is_some_and(|row| row.title == "Check for Extension Updates")
        })
    };
    until(&window, cx, selected);
    cx.simulate_keystrokes("enter");
    until_drawn(cx, "toast-animated", "the pass's progress toast");
    drop(held);

    // The pass goes on: the extension is updated and the toast ends as
    // its summary, in a drawn frame; its View Details opens the results
    // view, drawn with its groups' labels and the updated row.
    until_drawn(cx, "toast-success", "the ending toast");
    view_details(&window, cx);
    until_drawn(cx, "section-Updated", "the Updated group");
    until_drawn(cx, "results-row-Greeter from npm", "the updated row");
    assert!(cx.read_entity(&window, |window, _| window.update_results_shown()));
    until_launcher(&window, cx, |launcher| {
        launcher.update_results().updated.len() == 1
    });
}

#[gpui::test]
fn the_settings_extensions_group_button_starts_the_pass(cx: &mut TestAppContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(NAME, "0.1.0", pack(&greeter_files(&guests, "0.1.0")));
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    let clock = Arc::new(ManualClock::at(SystemClock.now()));
    let first = launcher(&runtime, &data, &registry, &clock);
    futures::executor::block_on(first.install_npm(NAME));
    registry.publish(NAME, "0.2.0", pack(&greeter_files(&guests, "0.2.0")));
    drop(first);

    let (sender, changes) = pane_core::changes::channel();
    let second = launcher(&runtime, &data, &registry, &clock)
        .with_development(Arc::new(Toolchains::from_env(None)), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(second, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });

    // Settings opens at the Extensions group: the Check for updates
    // button beside the automatic-updates switch, with when Pane last
    // checked under it — not yet.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();
    for drawn in [
        "extension-row-Update extensions automatically",
        "extension-check-updates",
        "extension-last-checked",
    ] {
        assert!(
            settings_cx.debug_bounds(drawn).is_some(),
            "{drawn} is drawn"
        );
    }

    // Clicking the button starts the same pass the root search row does:
    // the launcher's toast follows it, and when it ends the extension is
    // updated and the page says when Pane last checked.
    click_row(&mut settings_cx, "extension-check-updates");
    until_launcher(&window, cx, |launcher| {
        launcher.update_results().updated.len() == 1
    });
    until_drawn(&mut settings_cx, "extension-check-updates", "the button");
    let checked = cx.read_entity(&window, |window, _| {
        window.launcher().last_extension_check()
    });
    assert!(checked.is_some(), "the page says when Pane last checked");
}

#[gpui::test]
fn retry_from_the_results_view_checks_that_extension_alone(cx: &mut TestAppContext) {
    let (window, cx) = hidden_behind_a_failed_check(cx);
    announce_by_showing(&window, cx);
    view_details(&window, cx);

    // The results view lists the two failures; the second row's Actions
    // panel offers Retry.
    until(&window, cx, |view| view.rows.len() == 2);
    cx.simulate_keystrokes("down");
    until(&window, cx, |view| {
        view.selected.is_some_and(|index| {
            view.rows
                .get(index)
                .is_some_and(|row| row.title == "Second greeter")
        })
    });
    cx.simulate_keystrokes(actions_shortcut());
    until_drawn(cx, "action-Retry", "the Retry entry");

    // Down moves to Retry — the panel opens on Show Extension — and Enter
    // runs it: an asked pass over that extension alone, against a
    // registry that is still down — it fails again, and the record holds
    // only its row (the pane-core tests hold its toast's say).
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    until_launcher(&window, cx, |launcher| {
        let recorded = launcher.update_results();
        recorded.failed.len() == 1 && recorded.failed[0].title == "Second greeter"
    });
    // The view lists the retried extension's row alone.
    until(&window, cx, |view| {
        view.rows.len() == 1 && view.rows[0].title == "Second greeter"
    });
}

#[gpui::test]
fn update_now_from_the_results_view_updates_the_skipped_extension(cx: &mut TestAppContext) {
    let (window, _registry, cx) = skipped_and_failed(cx);
    // The failure is announced, and View Details opens the results view:
    // the first row is the Skipped one, the second the Failed.
    announce_by_showing(&window, cx);
    view_details(&window, cx);
    let view = until(&window, cx, |view| view.rows.len() == 2);
    assert_eq!(view.rows[0].title, "Greeter from npm");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Automatic updates of it are off")
    );

    // The Actions panel offers Update Now on that row; down moves to it —
    // the panel opens on Show Extension — and Enter runs it: an asked
    // pass over that extension alone, which updates it — the record
    // holding its row (the pane-core tests hold its toast's say), and
    // the view listing it alone.
    cx.simulate_keystrokes(actions_shortcut());
    until_drawn(cx, "action-Update Now", "the Update Now entry");
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (recorded, view, toast) = cx.read_entity(&window, |window, _| {
            (
                window.launcher().update_results(),
                window.launcher().view(),
                window.launcher().toast(),
            )
        });
        if recorded.updated.len() == 1
            && recorded.updated[0].title == "Greeter from npm"
            && recorded.updated[0].detail == "0.1.0 → 0.2.0"
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the Update Now pass: {recorded:#?} / {view:?} / {toast:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    until(&window, cx, |view| {
        view.rows.len() == 1 && view.rows[0].title == "Greeter from npm"
    });
}
