//! What a command does after it acts (#141) in the launcher's window, with
//! real key events and the Rust actions sample: the toast in the footer
//! where the status line was, its time (3 seconds for a success, paused
//! while the pointer is over it; an animated one stays until it is
//! updated), the toast key that reaches its actions and its actions' own
//! shortcuts, the HUD's window — its time (1.2 seconds, 3 for a failure,
//! then a fade out over about a second; a pending one stays until the
//! launcher is active again), its shape and placement (#250: content-sized
//! at most 500 wide, 46 logical pixels tall — 56 with a message — centred
//! with its bottom edge 150 above the display's bottom), what it draws
//! (icon, title, message) and what it announces through its live region —
//! and `close` hiding the window. The core's rules (the other languages,
//! the hidden- and compact-window rule, stale handles, subtitles) are
//! `pane-core`'s `feedback.rs`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::tray::TrayAction;
use pane_core::{
    CommandRegistration, Hud, Icon, IconSource, Launcher, LauncherView, Runtime, Screen,
    ShownToast, Status, ToastStyle,
};

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/wait.rs"]
mod wait;

use settle::{settle, settle_bare, settle_shown};
use wait::frame;

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

/// Shows `hud` as the launcher's window seam would, and answers a context
/// over the HUD's own window, as the launcher's window's context answers
/// over it.
fn hud_shown(
    hud: pane_core::Hud,
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> VisualTestContext {
    window.update_in(cx, |window, w, cx| window.request_hud(hud, w, cx));
    cx.run_until_parked();
    let handle = cx
        .read_entity(window, |window, _| window.hud_window())
        .expect("the HUD's window");
    VisualTestContext::from_window(handle, &cx.cx)
}

/// The HUD window's pill quad, its popover fill, and the alpha it is drawn
/// with: the fade out shows in it.
fn pill_alpha(hud: &mut VisualTestContext) -> f32 {
    let mut fills: Vec<(f32, f32)> = hud
        .update(|window, _| window.painted_quads())
        .into_iter()
        .filter_map(|quad| {
            let fill = quad.background.as_solid()?;
            Some((
                quad.bounds.size.width.0 * quad.bounds.size.height.0,
                fill.alpha,
            ))
        })
        .collect();
    fills.sort_by(|a, b| a.0.total_cmp(&b.0));
    fills.pop().expect("the HUD's pill").1
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
/// for 1.2 seconds, then fades out over about a second; a failure's HUD
/// stays 3 seconds and fades the same way.
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
    assert!(
        hud(&window, cx).is_some(),
        "its 1.2 seconds up, it fades out instead of closing at once"
    );
    wait(Duration::from_millis(1100), cx);
    assert_eq!(hud(&window, cx), None, "gone once its fade has run");

    hud_shown(Hud::new(ToastStyle::Failure, "Could not copy"), &window, cx);
    wait(Duration::from_millis(2900), cx);
    assert!(hud(&window, cx).is_some(), "a failure stays longer");
    wait(Duration::from_millis(200), cx);
    assert!(hud(&window, cx).is_some(), "a failure's fade has begun too");
    wait(Duration::from_millis(1100), cx);
    assert_eq!(hud(&window, cx), None);
}

/// The HUD is placed on the display the launcher showed on, centred
/// horizontally, its bottom edge 150 logical pixels above that display's
/// bottom, 46 logical pixels tall — 56 with a message — and content-sized,
/// at most 500 wide (#250).
#[gpui::test]
fn a_hud_is_placed_centred_150_pixels_above_the_bottom_of_its_display(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    let display = cx.update(|_, cx| cx.displays().remove(0)).bounds();

    let mut hud = hud_shown(
        Hud::new(ToastStyle::Success, "Copied to Clipboard"),
        &window,
        cx,
    );
    let bounds = hud.update(|window, _| window.bounds());
    assert_eq!(bounds.size.height, px(46.), "one line");
    assert_eq!(
        bounds.bottom(),
        display.origin.y + display.size.height - px(150.),
        "the bottom edge 150 above the display's"
    );
    assert_eq!(
        bounds.origin.x + bounds.size.width / 2.,
        display.origin.x + display.size.width / 2.,
        "centred on the display"
    );
    assert!(
        bounds.size.width >= px(160.) && bounds.size.width <= px(500.),
        "content-sized, within its bounds: {:?}",
        bounds.size
    );

    // A message makes the second line: taller, the same placement.
    let explained = Hud {
        message: Some("the clipboard was full".into()),
        ..Hud::new(ToastStyle::Success, "Copied to Clipboard")
    };
    let mut hud = hud_shown(explained, &window, cx);
    let bounds = hud.update(|window, _| window.bounds());
    assert_eq!(bounds.size.height, px(56.), "two lines");
    assert_eq!(
        bounds.bottom(),
        display.origin.y + display.size.height - px(150.),
        "the bottom edge 150 above the display's"
    );
    assert_eq!(
        bounds.origin.x + bounds.size.width / 2.,
        display.origin.x + display.size.width / 2.,
        "centred on the display"
    );

    // The widest it ever is: a longer title is truncated into 500.
    let mut hud = hud_shown(Hud::new(ToastStyle::Success, "x".repeat(500)), &window, cx);
    assert_eq!(
        hud.update(|window, _| window.bounds()).size.width,
        px(500.),
        "the widest"
    );
}

/// Under reduced motion the HUD closes at its time with no fade: gone
/// just past its 1.2 seconds, where full motion is still fading.
#[gpui::test]
fn reduced_motion_closes_a_hud_at_its_time_without_a_fade(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    hud_shown(Hud::new(ToastStyle::Success, "Copied"), &window, cx);
    wait(Duration::from_millis(1100), cx);
    assert!(hud(&window, cx).is_some(), "still there before 1.2 seconds");
    wait(Duration::from_millis(200), cx);
    assert_eq!(hud(&window, cx), None, "closed at its time, not faded");
}

/// One HUD at a time: a newer one replaces the HUD still shown, closing
/// its window, and the replaced one's time, once past, closes nothing.
#[gpui::test]
fn a_newer_hud_replaces_one_still_shown(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    hud_shown(Hud::new(ToastStyle::Success, "Copied"), &window, cx);
    let first = cx
        .read_entity(&window, |window, _| window.hud_window())
        .expect("the first HUD's window");
    hud_shown(Hud::new(ToastStyle::Failure, "Could not copy"), &window, cx);
    assert_eq!(hud(&window, cx).as_deref(), Some("Could not copy"));
    let second = cx
        .read_entity(&window, |window, _| window.hud_window())
        .expect("the second HUD's window");
    assert_ne!(first, second, "the newer HUD's own window");
    assert!(
        first.update(cx, |_, _, _| ()).is_err(),
        "the replaced HUD's window closed"
    );
    // The first's 1.2 seconds, once past, closed nothing: the failure
    // keeps its own time.
    wait(Duration::from_millis(1300), cx);
    assert!(hud(&window, cx).is_some(), "the failure's time still runs");
}

/// A pending HUD (work in progress) has no time of its own: it stays
/// until it is updated — which replaces it as any newer HUD does — or
/// until the launcher is active again.
#[gpui::test]
fn a_pending_hud_stays_until_it_is_updated_or_the_launcher_is_active_again(
    cx: &mut TestAppContext,
) {
    let (window, cx) = opened(cx);
    hud_shown(Hud::new(ToastStyle::Animated, "Uploading…"), &window, cx);
    wait(Duration::from_secs(10), cx);
    assert_eq!(
        hud(&window, cx).as_deref(),
        Some("Uploading…"),
        "a pending HUD stays"
    );

    // An update replaces it, and the update's own time runs.
    hud_shown(Hud::new(ToastStyle::Success, "Uploaded"), &window, cx);
    assert_eq!(hud(&window, cx).as_deref(), Some("Uploaded"));
    wait(Duration::from_millis(1300), cx);
    assert!(hud(&window, cx).is_some(), "the update's fade has begun");
    wait(Duration::from_millis(1100), cx);
    assert_eq!(hud(&window, cx), None);

    // Another pending one ends when the launcher comes forward again.
    hud_shown(Hud::new(ToastStyle::Animated, "Uploading…"), &window, cx);
    assert!(hud(&window, cx).is_some(), "pending again");
    window.update_in(cx, |window, w, cx| {
        window.tray_selected(TrayAction::OpenPane, w, cx)
    });
    cx.run_until_parked();
    assert_eq!(hud(&window, cx), None, "the launcher active again ends it");
    assert!(!hidden(&window, cx));
}

/// The HUD never takes the focus: shown while the launcher has it, the
/// launcher keeps it, and the HUD's window is never the active one.
#[gpui::test]
fn a_hud_never_takes_the_focus(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    // Only an active window can be seen to keep the focus: the test
    // platform opens none active.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let launcher = cx.window_handle();
    assert_eq!(cx.update(|_, cx| cx.active_window()), Some(launcher));
    let mut hud = hud_shown(Hud::new(ToastStyle::Success, "Copied"), &window, cx);
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        Some(launcher),
        "the launcher keeps the focus"
    );
    assert!(
        !hud.update(|window, _| window.is_window_active()),
        "the HUD's window is never active"
    );
}

/// The HUD's text is announced through a live region of its window — the
/// title, with the message after it, as both the region's name and its
/// value, as the launcher's announcer says the footer's toasts (#132) —
/// so a screen reader hears it while the launcher is elsewhere.
#[gpui::test]
fn a_hud_announces_its_title_and_message_through_a_live_region(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    let explained = Hud {
        message: Some("3 files".into()),
        ..Hud::new(ToastStyle::Success, "Copied")
    };
    let mut hud = hud_shown(explained, &window, cx);
    let (name, value) = a11y::announcer_of(&a11y::a11y(&mut hud));
    assert_eq!(name, "Copied: 3 files");
    assert_eq!(value, "Copied: 3 files");
}

/// The HUD fades out over about a second: its pill is drawn at full
/// strength until its time is up, then fainter as the frames it asks for
/// pass, until the window closes at the fade's end.
#[gpui::test]
fn a_hud_fades_out_over_about_a_second(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    let mut shown = hud_shown(Hud::new(ToastStyle::Success, "Copied"), &window, cx);
    // A still HUD asks for no frame, and is drawn at full strength.
    assert_eq!(frame(&mut shown, Duration::ZERO), 0);
    assert_eq!(pill_alpha(&mut shown), 1.);
    // Its 1.2 seconds up, the fade begins and asks for frames.
    wait(Duration::from_millis(1300), cx);
    assert!(hud(&window, cx).is_some(), "its fade has begun");
    assert!(
        frame(&mut shown, Duration::ZERO) >= 1,
        "the fade asks for frames"
    );
    // Half a second into it, the pill is about half as strong.
    frame(&mut shown, Duration::from_millis(500));
    let half = pill_alpha(&mut shown);
    assert!(
        (0.35..0.65).contains(&half),
        "half a second in, about half as strong: {half}"
    );
    // Past the fade's span the window closes.
    frame(&mut shown, Duration::from_millis(600));
    assert_eq!(hud(&window, cx), None, "gone once the fade has run");
}

/// The HUD draws an optional icon, a one-line title and an optional
/// one-line message: the icon before the title on its line, the message
/// under it as the second line.
#[gpui::test]
fn a_hud_draws_its_icon_its_title_and_its_message(cx: &mut TestAppContext) {
    let (window, cx) = opened(cx);
    let explained = Hud {
        message: Some("3 files".into()),
        icon: Some(Icon::new(IconSource::Builtin {
            name: "clipboard".into(),
            filled: false,
        })),
        ..Hud::new(ToastStyle::Success, "Copied")
    };
    let mut hud = hud_shown(explained, &window, cx);
    let dot = hud.debug_bounds("toast-success").expect("the style's dot");
    let icon = hud.debug_bounds("icon-hud").expect("the icon");
    let title = hud.debug_bounds("hud-title").expect("the title");
    let message = hud.debug_bounds("hud-message").expect("the message");
    assert!(dot.left() < icon.left(), "the dot, then the icon");
    assert!(icon.right() <= title.left(), "the icon before the title");
    assert!(
        message.top() >= title.bottom(),
        "the message is the second line, under the title"
    );
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
