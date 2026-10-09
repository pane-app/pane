//! The launcher's chords fire only with the modifiers they declare (#251),
//! pressed as real keys through the window. An extra modifier never runs
//! the plainer chord: Ctrl+Shift+K opens no Actions panel, Shift+Enter
//! invokes no row and Ctrl+Alt+digit picks nothing — including in the
//! composed forms the platforms report, where the keystroke's key names
//! the character the layout composes in `key_char` (Shift types the
//! uppercase letter; Windows reports AltGr as Ctrl and Alt with the
//! composed character). An AltGr character reaches the focused field and
//! matches no chord. Ctrl+C (Command+C on macOS) copies a focused field's
//! selection and, with none, runs the selected row's copy action, while
//! Ctrl+Shift+C is unaffected. The numpad's digits pick as the digit
//! row's, and the number hints show only after Ctrl is held alone for
//! 400 ms — never on a chord — and hide on any other key, a key release,
//! a scroll, the window losing focus or being deactivated.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Entity, KeyDownEvent, Keystroke, Modifiers, TestAppContext, VisualTestContext, prelude::*,
};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/samples.rs"]
mod samples;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/wait.rs"]
mod wait;

use settle::{settle, settle_shown};

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The platform's copy chord: what a focused field copies its selection
/// with, and what the answer row's copy action is bound to.
const COPY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-c"
};

/// The field's select-all chord on this system.
const SELECT_ALL: &str = if cfg!(target_os = "macos") {
    "cmd-a"
} else {
    "ctrl-a"
};

/// Ctrl+Shift+C (Command+Shift+C on macOS): bound by nothing here.
const SHIFT_COPY: &str = if cfg!(target_os = "macos") {
    "cmd-shift-c"
} else {
    "ctrl-shift-c"
};

/// How long Ctrl must be held alone before the number hints show.
const HOLD: Duration = Duration::from_millis(400);

/// The launcher with the actions sample as its one command (as the item
/// actions tests register it).
fn opened_actions(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
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
    open_launcher(cx, Launcher::new(Runtime::start(), vec![command]))
}

/// The launcher with the three sample commands, at root search with a
/// blank query: three rows numbered 1 to 3, the field focused.
fn opened_samples(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let commands = samples::sample_commands();
    open_launcher(cx, Launcher::new(Runtime::start(), commands))
}

/// The launcher with the calculator package installed, as the window tests
/// install it, back at root search.
fn opened_calculator(
    cx: &mut TestAppContext,
    data: &std::path::Path,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator");
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"));
    // The install's guest check answers from the runtime thread.
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
    open_launcher(cx, launcher)
}

fn open_launcher(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
}

/// The actions sample's command opened from root search: the field types
/// its title, Enter opens it, "Alpha note" is selected and the status is
/// idle — the state the chord tests below start from.
fn opened_at_the_list(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let (window, cx) = opened_actions(cx);
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
    assert_eq!(selected_title(&view), "Alpha note");
    (window, cx)
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

fn actions_open(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.actions_open())
}

/// The text in the window's query field.
fn field_text(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// The text on the clipboard.
fn clipboard(cx: &mut VisualTestContext) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .expect("text on the clipboard")
}

/// Lets the window apply answers computed from the query until the rows
/// are `expected`.
fn wait_for_rows(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &[&str]) {
    settle::until(window, cx, |view| {
        view.rows
            .iter()
            .map(|row| row.title.as_str())
            .eq(expected.iter().copied())
    });
}

/// The chord `id` as the platforms report it when the layout composes a
/// character: the same modifiers and key, with `key_char` naming what the
/// press would type (Shift types the uppercase letter, Enter the newline,
/// AltGr the composed character Windows reports under Ctrl and Alt).
fn composed(id: &str, key_char: &str) -> Keystroke {
    let mut keystroke = Keystroke::parse(id).expect("the chord parses");
    keystroke.key_char = Some(key_char.to_owned());
    keystroke
}

/// Presses `keystroke` as a real key down; `character_input` says the
/// platform prefers its own character input for the press (Windows'
/// AltGr form).
fn press(cx: &mut VisualTestContext, keystroke: Keystroke, character_input: bool) {
    cx.simulate_event(KeyDownEvent {
        keystroke,
        is_held: false,
        prefer_character_input: character_input,
    });
}

/// Whether the number hints show `number`'s cap: the accessibility tree
/// holds an image node named "Ctrl+<number>" only while a row draws its
/// hint.
fn hint_shown(cx: &mut VisualTestContext, number: usize) -> bool {
    let tree: serde_json::Value = serde_json::from_str(&a11y::a11y(cx)).unwrap();
    let name = format!("Ctrl+{number}");
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .any(|node| node["aria"]["role"] == "Image" && node["aria"]["label"] == name)
}

/// Holds Ctrl alone until the hints have shown, delivering the frames
/// their slide asks for.
fn hold_ctrl_for_the_hints(cx: &mut VisualTestContext) {
    cx.simulate_modifiers_change(Modifiers::control());
    wait::frame(cx, HOLD);
    wait::settle_frames(cx);
}

/// An extra modifier never runs the plainer chord: Ctrl+Shift+K opens no
/// Actions panel, Shift+Enter invokes no row and Ctrl+Alt+digit picks
/// nothing, in the plain grammatical forms and in the composed forms the
/// platforms report; the chords without the extra modifier are what run.
#[gpui::test]
fn an_extra_modifier_never_runs_a_plainer_chord(cx: &mut TestAppContext) {
    let (window, cx) = opened_at_the_list(cx);

    // Ctrl+Shift+K is not Ctrl+K: the panel stays closed, nothing runs.
    for keystroke in [
        Keystroke::parse("ctrl-shift-k").unwrap(),
        composed("ctrl-shift-k", "K"),
    ] {
        press(cx, keystroke, false);
        let view = settle(&window, cx);
        assert!(!actions_open(&window, cx), "the panel stays closed");
        assert_eq!(view.status, Status::Idle, "nothing ran");
        assert_eq!(selected_title(&view), "Alpha note");
    }

    // Shift+Enter is not Enter: the row is not invoked, in the plain form
    // and as the platforms report it (Enter's key_char is the newline).
    for keystroke in [
        Keystroke::parse("shift-enter").unwrap(),
        composed("shift-enter", "\n"),
    ] {
        press(cx, keystroke, false);
        cx.simulate_keystrokes("shift-enter");
        let view = settle(&window, cx);
        assert_eq!(view.status, Status::Idle, "nothing ran");
        assert_eq!(selected_title(&view), "Alpha note");
        assert_eq!(view.screen, Screen::Command, "no row was opened");
    }

    // Ctrl+Alt+digit is not Ctrl+digit: nothing is picked, in the plain
    // form and as Windows reports AltGr (the composed character in
    // key_char).
    for keystroke in [
        Keystroke::parse("ctrl-alt-1").unwrap(),
        composed("ctrl-alt-2", "@"),
    ] {
        press(cx, keystroke, false);
        cx.simulate_keystrokes("ctrl-alt-1");
        let view = settle(&window, cx);
        assert_eq!(view.status, Status::Idle, "nothing was picked");
        assert_eq!(selected_title(&view), "Alpha note", "the selection stays");
        assert_eq!(view.screen, Screen::Command, "no row was opened");
    }

    // The chords as they declare themselves are what run: Ctrl+K opens
    // the panel, and Ctrl+1 picks the first row.
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(actions_open(&window, cx), "the plainer chord works");
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_keystrokes("ctrl-1");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Open: Alpha note".into()),
        "Ctrl+1 picks the first row"
    );
}

/// A character the layout composes with AltGr — reported on Windows as
/// Ctrl and Alt, with the character in `key_char` — is typed into the
/// focused field and never matches a Ctrl+Alt chord: no numbered row is
/// picked. The raw event Windows delivers dispatches nothing (the
/// platform types the character itself, after the key), and the
/// keystroke's own character input reaches the field.
#[gpui::test]
fn an_altgr_character_reaches_the_field_and_matches_no_chord(cx: &mut TestAppContext) {
    let (window, cx) = opened_samples(cx);

    // The key Windows reports for AltGr and the digit: Ctrl, Alt, the
    // digit's key and the composed character, preferring character input.
    // The field takes text input, so the keymap stands aside for it, and
    // no chord runs.
    press(cx, composed("ctrl-alt-2", "@"), true);
    let view = settle(&window, cx);
    assert_eq!(field_text(&window, cx), "", "the key typed nothing");
    assert_eq!(view.screen, Screen::Root { query: "".into() });
    assert!(
        cx.debug_bounds("pin-hint").is_some(),
        "the blank query still shows the home"
    );

    // The character the press composes reaches the field: the query takes
    // it, and the chord Ctrl+Alt+2 picked nothing (Ctrl+2 would have
    // opened the second row's command).
    cx.simulate_keystrokes("ctrl-alt-2->@");
    settle(&window, cx);
    assert_eq!(field_text(&window, cx), "@", "the character was typed");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "@".into()
        },
        "no row was picked"
    );
    assert!(
        cx.debug_bounds("pin-hint").is_none(),
        "the query holds the character"
    );
}

/// Ctrl+C (Command+C on macOS) with a selection in the focused query
/// field copies the selected text — the field's own copy wins, even
/// though the selected row's action is bound to the same chord — and
/// with no selection the chord reaches the row and runs its copy action.
/// Ctrl+Shift+C is unaffected: neither the field nor the row binds it.
#[gpui::test]
fn ctrl_c_copies_a_selection_and_without_one_runs_the_rows_copy_action(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = opened_calculator(cx, data.path());

    cx.simulate_input("6*7");
    wait_for_rows(&window, cx, &["42"]);
    assert!(
        cx.debug_bounds("row-42").is_some(),
        "the answer is the selected row"
    );

    // A selection in the field: the field's copy wins, and the answer's
    // action on the same chord does not run.
    cx.simulate_keystrokes(SELECT_ALL);
    cx.simulate_keystrokes(COPY);
    assert_eq!(clipboard(cx), "6*7", "the field's selection was copied");
    let view = settle(&window, cx);
    assert_eq!(view.status, Status::Idle, "the row's action did not run");

    // Nothing selected: the chord goes on to the row and runs its action.
    cx.simulate_keystrokes("right");
    cx.simulate_keystrokes(COPY);
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Copied 42 to the clipboard".into()),
        "the row's copy action ran"
    );
    assert_eq!(clipboard(cx), "42");

    // Ctrl+Shift+C is unaffected: nothing binds it, so nothing runs.
    cx.simulate_keystrokes(SHIFT_COPY);
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Copied 42 to the clipboard".into()),
        "nothing further ran"
    );
    assert_eq!(clipboard(cx), "42");
}

/// The numpad's digits act as the digit row's for Ctrl+digit chords:
/// every platform reports a numpad digit as the digit's own key (Windows
/// maps the key's character, Linux drops the keypad's "kp_" prefix,
/// macOS reads the key's unmodified characters), so the chord the window
/// reads — Ctrl and the digit — is one key press for both rows.
#[gpui::test]
fn the_numpads_digits_pick_as_the_digit_rows_do(cx: &mut TestAppContext) {
    let (window, cx) = opened_samples(cx);

    // Ctrl+2, the numpad's 2 as the digit row's, picks the second row.
    cx.simulate_keystrokes("ctrl-2");
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "JavaScript sample"),
        "the second row was picked"
    );
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // Ctrl+0 names nothing on either row: nothing is picked.
    cx.simulate_keystrokes("ctrl-0");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Root { query: "".into() });
}

/// The number hints show after Ctrl is held alone for 400 ms — not
/// before — and hide when Ctrl is released.
#[gpui::test]
fn the_number_hints_show_after_ctrl_is_held_alone_for_400ms(cx: &mut TestAppContext) {
    let (_window, cx) = opened_samples(cx);

    cx.simulate_modifiers_change(Modifiers::control());
    wait::frame(cx, HOLD - Duration::from_millis(1));
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "the hold is not yet long enough");

    wait::frame(cx, Duration::from_millis(1));
    wait::settle_frames(cx);
    assert!(hint_shown(cx, 1), "the hints show after the hold");
    assert!(hint_shown(cx, 2), "the rows below show theirs");

    cx.simulate_modifiers_change(Modifiers::none());
    wait::frame(cx, Duration::from_millis(0));
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "releasing Ctrl hides them");
}

/// A chord never shows the number hints: any other modifier ends the
/// hold, and so does a key pressed while Ctrl is held, before the hints
/// show.
#[gpui::test]
fn a_chord_never_shows_the_number_hints(cx: &mut TestAppContext) {
    let (_window, cx) = opened_samples(cx);

    // Another modifier while Ctrl is held: not Ctrl alone.
    cx.simulate_modifiers_change(Modifiers::control());
    cx.simulate_modifiers_change(Modifiers::control() | Modifiers::shift());
    wait::frame(cx, HOLD);
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "a chord's modifiers show no hints");

    // A key pressed while Ctrl is held: a chord, not a look at the
    // numbers, and the hold shows nothing.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.simulate_modifiers_change(Modifiers::control());
    cx.simulate_keystrokes("ctrl-j");
    wait::frame(cx, HOLD);
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "a chord key press cancels the hold");
}

/// The number hints hide when the user does anything else: any other key
/// pressed, a scroll, or the window losing focus.
#[gpui::test]
fn a_key_a_scroll_or_losing_focus_hides_the_number_hints(cx: &mut TestAppContext) {
    let (_window, cx) = opened_samples(cx);

    // Another key pressed while Ctrl is still held: the moment of
    // choosing a number is over.
    hold_ctrl_for_the_hints(cx);
    assert!(hint_shown(cx, 1));
    cx.simulate_keystrokes("ctrl-j");
    wait::frame(cx, Duration::from_millis(0));
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "a key press hides them");

    // A scroll through the list: the user is moving it, not choosing a
    // number.
    hold_ctrl_for_the_hints(cx);
    assert!(hint_shown(cx, 1));
    let row = cx
        .debug_bounds("row-Rust sample")
        .expect("the row is drawn")
        .center();
    cx.simulate_scroll(row, gpui::point(gpui::px(0.), gpui::px(-40.)));
    wait::frame(cx, Duration::from_millis(0));
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "a scroll hides them");

    // The window losing focus: the Ctrl release would go to another
    // window.
    hold_ctrl_for_the_hints(cx);
    assert!(hint_shown(cx, 1));
    cx.deactivate_window();
    wait::frame(cx, Duration::from_millis(0));
    wait::settle_frames(cx);
    assert!(!hint_shown(cx, 1), "deactivating the window hides them");
}
