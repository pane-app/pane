//! What a command does after it acts (#141) in the launcher's window, with
//! real key events and the Rust actions sample: the toast in the footer
//! where the status line was, its time (3 seconds for a success, paused
//! while the pointer is over it; an animated one stays until it is
//! updated), the toast key that reaches its actions and its actions' own
//! shortcuts, the HUD's window and its time (1.2 seconds, 3 for a
//! failure), and `close` hiding the window. The core's rules (the other
//! languages, the hidden-window rule, stale handles, subtitles) are
//! `pane-core`'s `feedback.rs`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::tray::TrayAction;
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Hud, Launcher, LauncherView, Runtime,
    Screen, ShownToast, Status, ToastStyle,
};

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_bare, settle_shown};

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The toast key on this system.
const TOAST_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-t"
} else {
    "ctrl-t"
};

/// The launcher with the Rust actions sample as its one command, opened
/// with Enter, "Alpha note" selected.
fn opened(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join("sample_actions.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    let command = CommandRegistration {
        id: "actions-sample".into(),
        title: "Actions sample".into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    };
    let launcher = Launcher::new(Runtime::start(), vec![command]);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Actions sample"),
        "{:?}",
        view.status
    );
    (window, cx)
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

/// Selects the row titled `title` with the arrow keys, from "Alpha note".
fn select(title: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    for _ in 0..8 {
        if selected_title(&settle(window, cx)) == title {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    panic!("no row {title}");
}

/// Runs the selected item's action whose title `filter` finds first,
/// through the Actions panel, as a user does, until it has run (the
/// window may have closed meanwhile, so this does not wait for a frame).
fn choose(filter: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    // The keys go where the last frame put them: not into a panel or a
    // dialog an earlier choice closed.
    settle_bare(window, cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(window, cx);
    cx.simulate_input(filter);
    settle(window, cx);
    cx.simulate_keystrokes("enter");
    done(window, cx);
    // While the window shows, the keys pressed next go where a frame of
    // its outcome puts them.
    if !hidden(window, cx) {
        settle(window, cx);
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

/// The toast the footer shows, as the launcher has it.
fn toast(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Option<ShownToast> {
    cx.read_entity(window, |window, _| window.launcher().toast())
}

/// Lets `time` pass on the test clock, and the window draw what it
/// changed.
fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

fn hud(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| window.hud())
}

/// Moves the pointer off the window.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
}

/// An action's toast shows in the footer, where the status line was, and
/// a success leaves after 3 seconds.
#[gpui::test]
fn a_toast_shows_in_the_footer_and_a_success_leaves_after_three_seconds(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Open: Alpha note".into())
    );
    assert_eq!(
        settle(&window, cx).status,
        Status::Idle,
        "not the status line"
    );
    assert!(
        cx.debug_bounds("status-toast").is_some(),
        "the footer's toast"
    );
    assert!(cx.debug_bounds("toast").is_some());
    assert!(cx.debug_bounds("toast-success").is_some());
    assert!(cx.debug_bounds("status-result").is_none());

    wait(Duration::from_millis(2900), cx);
    assert!(toast(&window, cx).is_some(), "still there before 3 seconds");
    wait(Duration::from_millis(200), cx);
    assert!(toast(&window, cx).is_none(), "gone after 3 seconds");
    settle(&window, cx);
    assert!(cx.debug_bounds("toast").is_none());
    assert!(cx.debug_bounds("status-idle").is_some());
}

/// The pointer over a toast pauses its time; once the pointer leaves, the
/// rest of it runs.
#[gpui::test]
fn the_pointer_over_a_toast_pauses_its_time(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    let over = cx
        .debug_bounds("toast")
        .expect("the toast is drawn")
        .center();
    cx.simulate_mouse_move(over, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    wait(Duration::from_secs(10), cx);
    assert!(
        toast(&window, cx).is_some(),
        "paused while the pointer is over it"
    );

    pointer_leaves(cx);
    cx.run_until_parked();
    wait(Duration::from_millis(3100), cx);
    assert!(toast(&window, cx).is_none(), "gone once its time ran");
}

/// An animated toast stays until its command updates it; the update's
/// success then leaves after 3 seconds.
#[gpui::test]
fn an_animated_toast_stays_until_it_is_updated(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    select("Feedback", &window, cx);
    choose("start upload", &window, cx);
    settle(&window, cx);
    assert!(cx.debug_bounds("toast-animated").is_some());
    wait(Duration::from_secs(10), cx);
    let started = toast(&window, cx).expect("the animated toast stays");
    assert_eq!(started.toast.title, "Uploading…");

    choose("finish upload", &window, cx);
    settle(&window, cx);
    let finished = toast(&window, cx).expect("the updated toast");
    assert_eq!(finished.id, started.id);
    assert_eq!(finished.toast.style, ToastStyle::Success);
    assert!(cx.debug_bounds("toast-success").is_some());
    assert!(cx.debug_bounds("toast-message").is_some(), "its message");
    assert!(cx.debug_bounds("toast-action-primary").is_some(), "Open");
    assert!(cx.debug_bounds("toast-action-secondary").is_some(), "Retry");
    wait(Duration::from_millis(3100), cx);
    assert!(toast(&window, cx).is_none());
}

/// The window losing the focus does not end a success toast: it still
/// leaves after its 3 seconds, not before.
#[gpui::test]
fn a_success_toast_keeps_its_time_when_the_window_loses_the_focus(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    // Only an active window can lose the focus: the test platform opens
    // none active.
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert!(cx.debug_bounds("toast-success").is_some());
    cx.deactivate_window();
    cx.run_until_parked();
    wait(Duration::from_millis(1500), cx);
    assert!(toast(&window, cx).is_some(), "a success stays for its time");
    wait(Duration::from_millis(1600), cx);
    assert!(toast(&window, cx).is_none(), "gone once its time ran");
}

/// An animated toast leaves the footer when the window loses the focus.
#[gpui::test]
fn an_animated_toast_leaves_when_the_window_loses_the_focus(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    cx.update(|window, _| window.activate_window());
    select("Feedback", &window, cx);
    choose("start upload", &window, cx);
    settle(&window, cx);
    assert!(cx.debug_bounds("toast-animated").is_some());
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(toast(&window, cx).is_none(), "an animated toast leaves");
}

/// The toast key moves the focus to the toast's first action, and Enter
/// chooses it, calling the command back; an action's own shortcut runs it
/// while the toast shows.
#[gpui::test]
fn the_toast_key_reaches_its_actions_and_their_shortcuts_run_them(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    pointer_leaves(cx);
    select("Feedback", &window, cx);
    choose("start upload", &window, cx);
    choose("finish upload", &window, cx);

    cx.simulate_keystrokes(TOAST_KEY);
    settle(&window, cx);
    // Enter now chooses the toast's "Open", not the item's "Show HUD".
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Opened the upload".into())
    );
    assert!(!hidden(&window, cx), "the item's HUD did not run");

    choose("start upload", &window, cx);
    choose("finish upload", &window, cx);
    let uploaded = toast(&window, cx).expect("the upload's toast");
    // Retry's own shortcut.
    cx.simulate_keystrokes("ctrl-shift-r");
    done(&window, cx);
    let retried = toast(&window, cx).expect("the retried upload's toast");
    assert_ne!(retried.id, uploaded.id, "a new upload");
    assert_eq!(retried.toast.text(), "Uploaded: notes.txt");
}

/// "Show HUD" closes the window and shows the HUD in a window of its own
/// for 1.2 seconds; a failure's HUD stays 3 seconds.
#[gpui::test]
fn a_hud_shows_in_a_window_of_its_own_for_its_time(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    select("Feedback", &window, cx);
    choose("show hud", &window, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the launcher closed first");
    assert_eq!(hud(&window, cx).as_deref(), Some("Copied to Clipboard"));
    wait(Duration::from_millis(1100), cx);
    assert!(hud(&window, cx).is_some(), "still there before 1.2 seconds");
    wait(Duration::from_millis(200), cx);
    assert_eq!(hud(&window, cx), None, "gone after 1.2 seconds");

    window.update_in(cx, |window, w, cx| {
        window.request_hud(
            Hud {
                title: "Could not copy".into(),
                style: ToastStyle::Failure,
            },
            w,
            cx,
        )
    });
    cx.run_until_parked();
    wait(Duration::from_millis(2900), cx);
    assert!(hud(&window, cx).is_some(), "a failure stays longer");
    wait(Duration::from_millis(200), cx);
    assert_eq!(hud(&window, cx), None);
}

/// "Close" hides the window; summoned again, the launcher shows the
/// command's screen, as the Launcher setting's default restores it.
/// "Close to Root Search" returns to root search at once.
#[gpui::test]
fn close_hides_the_window_and_its_pop_decides_the_next_showing(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    select("Window", &window, cx);
    choose("close", &window, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));
    window.update_in(cx, |window, w, cx| {
        window.tray_selected(TrayAction::OpenPane, w, cx)
    });
    let view = settle(&window, cx);
    assert!(!hidden(&window, cx));
    assert_eq!(view.screen, Screen::Command, "the screen was restored");

    select("Window", &window, cx);
    choose("close to root", &window, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));
    assert!(matches!(done(&window, cx).screen, Screen::Root { .. }));
}
