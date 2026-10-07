//! A confirmation a command asks for (#146) in the launcher's window, with
//! real key events and the Rust actions sample installed as a package: the
//! dialog shows its title, message and buttons over the command's screen,
//! Enter confirms and Escape dismisses, a click outside it and the window
//! losing the focus answer that the user did not confirm, the primary
//! button is drawn destructive when asked, Space ticks "Don't ask again" and
//! the answer is then given without asking, the window shows itself for a
//! confirmation asked while it is hidden, and the extension's card in
//! Settings › Extensions resets the remembered answers. The core's rules
//! (the other languages, restarts, updates, uninstalling, background
//! launches) are `pane-core`'s `confirmations.rs`.

use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext,
    WindowHandle, prelude::*, px,
};
use pane::{LauncherWindow, SettingsWindow};
use pane_core::{Confirmation, Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/packages.rs"]
mod packages;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/setup.rs"]
mod setup;

use settle::{settle, settle_bare, settle_shown};
use setup::{actions_shortcut, settings_shortcut};

/// The folders one test's package and extensions are kept in.
struct Folders {
    _sources: TempDir,
    _data: TempDir,
    identity: PackageIdentity,
}

/// The launcher with the Rust actions sample installed as a package, its
/// "Actions" command opened with Enter and its "Confirm" item selected.
fn opened(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext, Folders) {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let folder =
        packages::assembled_package("sample-actions", &sources.path().join("sample-actions"));
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Actions sample".into())
    );
    let identity = PackageIdentity::local(&folder).unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("Actions");
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), "Actions", "{view:?}");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Actions sample"),
        "{:?}",
        view.status
    );
    select("Confirm", &window, cx);
    (
        window,
        cx,
        Folders {
            _sources: sources,
            _data: data,
            identity,
        },
    )
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

/// Selects the row titled `title` with the arrow keys.
fn select(title: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    for _ in 0..8 {
        if selected_title(&settle(window, cx)) == title {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    panic!("no row {title}");
}

/// Chooses the selected item's action whose title `filter` finds first,
/// through the Actions panel, as a user does, without waiting for it to
/// end: it may wait on a confirmation.
fn choose(filter: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    // The keys go where the last frame put them: not into a panel or a
    // dialog an earlier choice closed.
    settle_bare(window, cx);
    cx.simulate_keystrokes(actions_shortcut());
    settle(window, cx);
    cx.simulate_input(filter);
    settle(window, cx);
    cx.simulate_keystrokes("enter");
}

/// Runs the window until the confirmation the command asked for is drawn,
/// with the focus, and answers it.
fn asked(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Confirmation {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let (asked, drawn) = cx.read_entity(window, |window, _| {
            (
                window.launcher().confirmation(),
                window.confirmation_drawn(),
            )
        });
        if let Some(asked) = asked
            && drawn.map(|(id, _)| id) == Some(asked.id)
            && cx.debug_bounds("confirmation").is_some()
        {
            return asked;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: no confirmation was drawn"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs the window until the launcher no longer runs an action: a hidden
/// window draws nothing, so this does not wait for a frame.
fn done(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(window, |window, _| window.launcher().view());
        if view.status != Status::Running {
            return view;
        }
        assert!(Instant::now() < deadline, "timed out: {view:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What the command's toast said once it answered.
fn answered(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Status {
    done(window, cx);
    settle_shown(window, cx)
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

fn remembered(
    window: &Entity<LauncherWindow>,
    cx: &VisualTestContext,
    identity: &PackageIdentity,
) -> Vec<String> {
    cx.read_entity(window, |window, _| {
        window.launcher().remembered_confirmations(identity)
    })
}

/// The dialog shows its title, message and buttons; Enter confirms and
/// Escape dismisses, and the focus goes back to the list.
#[gpui::test]
fn enter_confirms_and_escape_dismisses(cx: &mut TestAppContext) {
    let (window, cx, _folders) = opened(cx);
    choose("ask", &window, cx);
    let ask = asked(&window, cx);
    assert_eq!(ask.title, "Go on?");
    assert!(cx.debug_bounds("confirmation-title").is_some());
    assert!(
        cx.debug_bounds("confirmation-message").is_none(),
        "none given"
    );
    assert!(cx.debug_bounds("confirmation-primary").is_some());
    assert!(cx.debug_bounds("confirmation-dismiss").is_some());
    assert!(
        cx.debug_bounds("confirmation-destructive").is_none(),
        "not destructive"
    );
    assert!(
        cx.debug_bounds("confirmation-dont-ask-again").is_none(),
        "nothing to remember"
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(answered(&window, cx), Status::Result("Went on".into()));
    assert!(cx.debug_bounds("confirmation").is_none(), "it went");
    // Enter was the dialog's: the item's primary action ("Delete") did not
    // run as well.
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().confirmation()),
        None
    );

    choose("ask", &window, cx);
    asked(&window, cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(answered(&window, cx), Status::Result("Stopped".into()));
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command, "Escape left the screen alone");
    assert!(!hidden(&window, cx));
}

/// A click outside the dialog, or the window losing the focus, answers
/// that the user did not confirm.
#[gpui::test]
fn a_click_outside_or_the_window_losing_the_focus_answers_no(cx: &mut TestAppContext) {
    let (window, cx, _folders) = opened(cx);
    choose("ask", &window, cx);
    asked(&window, cx);
    cx.simulate_click(gpui::point(px(4.), px(4.)), Modifiers::none());
    assert_eq!(answered(&window, cx), Status::Result("Stopped".into()));

    // Only an active window can lose the focus: the test platform opens
    // none active.
    cx.update(|window, _| window.activate_window());
    choose("ask", &window, cx);
    asked(&window, cx);
    cx.deactivate_window();
    assert_eq!(answered(&window, cx), Status::Result("Stopped".into()));
}

/// "Delete" draws its primary button destructive and offers "Don't ask
/// again": Space ticks it, and confirmed so, the next "Delete" answers
/// without asking.
#[gpui::test]
fn a_destructive_confirmation_remembers_its_answer_once_ticked(cx: &mut TestAppContext) {
    let (window, cx, folders) = opened(cx);
    choose("delete", &window, cx);
    let delete = asked(&window, cx);
    assert!(delete.destructive);
    assert!(cx.debug_bounds("confirmation-destructive").is_some());
    assert!(cx.debug_bounds("confirmation-message").is_some());
    assert!(cx.debug_bounds("confirmation-dont-ask-again").is_some());
    assert!(
        cx.debug_bounds("confirmation-dont-ask-again-ticked")
            .is_none()
    );

    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&window, |window, _| window.confirmation_drawn()),
        Some((delete.id, true))
    );
    assert!(
        cx.debug_bounds("confirmation-dont-ask-again-ticked")
            .is_some()
    );
    // Space again unticks it, and again ticks it.
    cx.simulate_keystrokes("space space");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("confirmation-dont-ask-again-ticked")
            .is_some()
    );

    cx.simulate_keystrokes("enter");
    assert_eq!(answered(&window, cx), Status::Result("Deleted".into()));
    assert_eq!(remembered(&window, cx, &folders.identity), ["delete-note"]);

    // Asked again, it is answered at once: nothing is drawn.
    choose("delete", &window, cx);
    assert_eq!(answered(&window, cx), Status::Result("Deleted".into()));
    assert!(cx.debug_bounds("confirmation").is_none());
    assert_eq!(
        cx.read_entity(&window, |window, _| window.confirmation_drawn()),
        None
    );
}

/// A click on "Don't ask again" ticks it too; a click on the dismiss button
/// dismisses, and that answer is remembered.
#[gpui::test]
fn clicking_the_box_and_the_dismiss_button(cx: &mut TestAppContext) {
    let (window, cx, folders) = opened(cx);
    choose("delete", &window, cx);
    asked(&window, cx);
    let tick = cx
        .debug_bounds("confirmation-dont-ask-again")
        .expect("the box is drawn")
        .center();
    cx.simulate_mouse_move(tick, None::<MouseButton>, Modifiers::none());
    cx.simulate_click(tick, Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("confirmation-dont-ask-again-ticked")
            .is_some()
    );
    let dismiss = cx
        .debug_bounds("confirmation-dismiss")
        .expect("the dismiss button is drawn")
        .center();
    cx.simulate_mouse_move(dismiss, None::<MouseButton>, Modifiers::none());
    cx.simulate_click(dismiss, Modifiers::none());
    assert_eq!(answered(&window, cx), Status::Result("Kept".into()));
    assert_eq!(remembered(&window, cx, &folders.identity), ["delete-note"]);
}

/// "Close and Ask" closes the window, then asks: the window shows itself
/// again, over the command's screen, for the confirmation.
#[gpui::test]
fn a_hidden_window_shows_itself_for_a_confirmation(cx: &mut TestAppContext) {
    let (window, cx, _folders) = opened(cx);
    choose("close and ask", &window, cx);
    let ask = asked(&window, cx);
    assert_eq!(ask.title, "Asked while hidden");
    assert!(!hidden(&window, cx), "shown again for it");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answered(&window, cx),
        Status::Result("Confirmed while hidden".into())
    );
    assert_eq!(settle(&window, cx).screen, Screen::Command);
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

/// Opens Settings on its Extensions page, as its own window context, tall
/// enough that the page's cards are in reach without scrolling.
fn open_extensions(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_mouse_move(extensions.center(), None::<MouseButton>, Modifiers::none());
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();
    settings_cx
}

/// The package's card in Settings › Extensions offers "Reset
/// confirmations" while answers are remembered; clicked, it forgets them
/// and leaves the card.
#[gpui::test]
fn the_extensions_card_resets_the_remembered_answers(cx: &mut TestAppContext) {
    let (window, cx, folders) = opened(cx);
    const RESET: &str = "extension-row-Reset confirmations of Actions sample";
    choose("delete", &window, cx);
    asked(&window, cx);
    cx.simulate_keystrokes("space enter");
    assert_eq!(answered(&window, cx), Status::Result("Deleted".into()));
    assert_eq!(remembered(&window, cx, &folders.identity), ["delete-note"]);

    let mut settings_cx = open_extensions(cx);
    let reset = settings_cx
        .debug_bounds(RESET)
        .expect("the card offers Reset confirmations")
        .center();
    settings_cx.simulate_mouse_move(reset, None::<MouseButton>, Modifiers::none());
    settings_cx.simulate_click(reset, Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        remembered(&window, cx, &folders.identity).is_empty(),
        "forgotten"
    );
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds(RESET).is_none(),
        "nothing left to reset"
    );
}
