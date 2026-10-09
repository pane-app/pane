//! The selection and hover washes of ADR 0035 (#245) in the native
//! window: the selected row of every launcher list — root search's rows,
//! a command's list, the Actions panel's entries, the Pane menu's — is a
//! wash of the text colour at 10%, with no inset edge and no ring; the
//! selected computed answer card shows the same wash and drops its
//! accent ring. Where hovering does not move the selection (a command's
//! list, the footer-family buttons, root search's rows under an open
//! panel), an unselected surface under the pointer takes the fainter
//! wash of 5%, fading out over the hover fade's 70 ms once the pointer
//! leaves; where hovering does move it (root search's free rows, the
//! Actions panel's entries), only the selection wash shows. The
//! selection itself always changes at once, and under reduced motion the
//! hover wash leaves at once too.
//!
//! The removed edge and rings — like the focus rings that stay — are
//! shadows, which `painted_quads` does not see: what these tests pin is
//! the washes themselves, at their exact full-strength colour, the
//! frames the fade asks for and stops asking for, and the frames a
//! selection change never asks for.

use std::path::PathBuf;
use std::time::Duration;

use futures::executor::block_on;
use gpui::{Bounds, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, px};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/packages.rs"]
mod packages;
#[path = "support/paint.rs"]
mod paint;
#[path = "support/settle.rs"]
mod settle;
#[path = "support/wait.rs"]
mod wait;

use settle::{settle, until};
use wait::{frame, settle_frames};

/// The selection wash, the text colour at 10% (dark palette), in every
/// launcher list.
const SELECTION_WASH: u32 = 0xEDEDEF1A;
/// The hover wash, the text colour at 5% (dark palette), where hovering
/// does not move the selection.
const HOVER_WASH: u32 = 0xEDEDEF0D;
/// The selected row's wash before #245, which no surface paints now.
const OLD_SELECTED: u32 = 0xFFFFFF16;
/// The Actions panel's and the Pane menu's selected wash before #245.
const OLD_ACTIONS: u32 = 0xFFFFFF1C;
/// The computed answer card's fill, which the card keeps only while
/// unselected.
const CARD_FILL: u32 = 0xFFFFFF0F;
/// How long the hover wash's exit fades for, as the window tests' frames
/// count it.
const FADE: Duration = Duration::from_millis(70);
/// Where the fade has run to when these tests read it weaker than full.
const FADED_FOR: Duration = Duration::from_millis(30);

/// Open actions' default binding on this system (Ctrl+K deletes to the
/// end of the line in a macOS field).
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// A debug selector as a `&'static str`, as the window tests name one
/// built at run time.
fn selector(name: impl Into<String>) -> &'static str {
    name.into().leak()
}

/// One sample command registered, as the window tests open the launcher
/// (Pane registers no sample command itself, #162).
fn command(title: &str, guest: &str) -> CommandRegistration {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{guest}.wasm"));
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    CommandRegistration {
        id: guest.into(),
        title: title.into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
    }
}

/// The launcher window over `launcher`, as the window tests open it.
fn open_launcher(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &'_ mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// The launcher window over `commands` registered, settled on its first
/// drawn view.
fn open_with(
    cx: &mut TestAppContext,
    commands: Vec<CommandRegistration>,
) -> (Entity<LauncherWindow>, &'_ mut VisualTestContext) {
    open_launcher(cx, Launcher::new(Runtime::start(), commands))
}

/// A launcher over the three sample commands, as the window tests' three
/// rows: Alpha, Bravo, Charlie.
fn three_rows(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &'_ mut VisualTestContext) {
    let (window, cx) = open_with(
        cx,
        vec![
            command("Alpha", "sample_rust"),
            command("Bravo", "sample_js"),
            command("Charlie", "sample_ts"),
        ],
    );
    settle(&window, cx);
    (window, cx)
}

/// The launcher with the calculator package from `cargo xtask guests`
/// installed in `data`.
fn with_calculator(cx: &mut TestAppContext, data: &std::path::Path) -> Launcher {
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
    launcher
}

/// The bounds of the element with debug selector `name`.
fn bounds_of(cx: &mut VisualTestContext, name: &str) -> Bounds<Pixels> {
    cx.debug_bounds(selector(name))
        .unwrap_or_else(|| panic!("{name} is drawn"))
}

/// The bounds of `view`'s row at `index`, found by its title.
fn row_bounds(cx: &mut VisualTestContext, view: &LauncherView, index: usize) -> Bounds<Pixels> {
    let title = view.rows[index].title.clone();
    bounds_of(cx, &format!("row-{title}"))
}

/// The pointer's first event in the window, at `at`: it only records
/// where the pointer is.
fn arrive(cx: &mut VisualTestContext, at: gpui::Point<Pixels>) {
    cx.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
}

/// The pointer leaves the window, parking it where nothing is drawn.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
    cx.run_until_parked();
}

/// Whether the window painted any fill at exactly `bounds`: the full
/// hover wash under the pointer, or the fading one behind it (whose
/// exact strength the fade's curve decides, so no hex matches it).
fn paints_any_fill_at(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) -> bool {
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let near = |scaled: gpui::ScaledPixels, logical: gpui::Pixels| {
            (scaled.0 / scale - f32::from(logical)).abs() <= 0.5
        };
        window.painted_quads().iter().any(|quad| {
            near(quad.bounds.origin.x, bounds.origin.x)
                && near(quad.bounds.origin.y, bounds.origin.y)
                && near(quad.bounds.size.width, bounds.size.width)
                && near(quad.bounds.size.height, bounds.size.height)
        })
    })
}

/// The hover wash at `bounds` fades out over its span once the pointer
/// has left: the window asks for frames while the fade runs, the wash is
/// still drawn while it does but weaker than its full strength, and once
/// the span has passed it is gone and the window is idle again.
fn fades_out(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) {
    assert!(
        frame(cx, FADED_FOR) >= 1,
        "the fade's exit asked for a frame while it ran"
    );
    assert!(
        paints_any_fill_at(cx, bounds),
        "the wash is still drawn while it fades"
    );
    assert!(
        !paint::paints_fill_at(cx, bounds, HOVER_WASH),
        "weaker than the wash under the pointer"
    );
    assert!(
        frame(cx, FADE - FADED_FOR + Duration::from_millis(1)) >= 1,
        "the fade's last frame"
    );
    assert!(
        !paints_any_fill_at(cx, bounds),
        "the wash is gone once the span has passed"
    );
    assert_eq!(settle_frames(cx), 0, "the window is idle again");
}

/// The selected row in root search: the text colour's wash at 10%, not
/// the wash it was before (#245) and not the hover wash; and a key moves
/// the selection at once, asking for no frame — the selection never
/// fades.
#[gpui::test]
fn the_selected_root_row_is_the_text_colours_wash_and_a_key_moves_it_at_once(
    cx: &mut TestAppContext,
) {
    let (window, cx) = three_rows(cx);
    settle_frames(cx);

    let view = settle(&window, cx);
    let first = row_bounds(cx, &view, 0);
    assert!(
        paint::paints_fill_at(cx, first, SELECTION_WASH),
        "the selected row's wash"
    );
    assert!(
        !paint::paints_fill_at(cx, first, OLD_SELECTED),
        "not the wash it was before"
    );
    assert!(
        !paint::paints_fill_at(cx, first, HOVER_WASH),
        "not the hover wash"
    );

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    let view = settle(&window, cx);
    let second = row_bounds(cx, &view, 1);
    assert!(
        paint::paints_fill_at(cx, second, SELECTION_WASH),
        "the wash moved with the selection"
    );
    assert!(
        !paint::paints_fill_at(cx, first, SELECTION_WASH),
        "and left the first row"
    );
    assert_eq!(
        settle_frames(cx),
        0,
        "the selection's move asked for no frame"
    );
}

/// Hovering a root search row moves the selection to it (the reference's
/// root follows the pointer), so the row under the pointer shows the
/// selection wash alone — never the fainter hover wash.
#[gpui::test]
fn hovering_a_root_row_selects_it_so_only_the_selection_wash_shows(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    settle_frames(cx);

    // Two moves: the first only records the pointer's position, as a
    // resting pointer never undoes the keys' selection.
    let bravo = bounds_of(cx, "row-Bravo").center();
    arrive(cx, bravo - gpui::point(px(1.), px(0.)));
    cx.simulate_mouse_move(bravo, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    let view = settle(&window, cx);
    assert_eq!(
        view.selected,
        Some(1),
        "the pointer selected where it moved"
    );
    assert!(
        paint::paints_fill_at(cx, row_bounds(cx, &view, 1), SELECTION_WASH),
        "the row it selected shows the selection wash"
    );
    assert!(
        !paint::paints_fill_at(cx, row_bounds(cx, &view, 1), HOVER_WASH),
        "and no hover wash: hovering here is selecting"
    );
}

/// A command's list: hovering moves no selection there (a click runs a
/// row), so an unselected row under the pointer takes the fainter hover
/// wash — the selected row keeps its own — and once the pointer leaves,
/// the wash fades out over its 70 ms while the window asks for frames.
#[gpui::test]
fn a_commands_list_hovers_with_the_fainter_wash_which_fades_out_when_the_pointer_leaves(
    cx: &mut TestAppContext,
) {
    let (window, cx) = open_with(cx, vec![command("Rust sample", "sample_rust")]);
    settle(&window, cx);
    settle_frames(cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    settle_frames(cx);

    let say = bounds_of(cx, "row-Say hello");
    let wait = bounds_of(cx, "row-Wait briefly");
    cx.simulate_mouse_move(wait.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert!(
        paint::paints_fill_at(cx, wait, HOVER_WASH),
        "the row under the pointer takes the hover wash"
    );
    assert!(
        !paint::paints_fill_at(cx, wait, SELECTION_WASH),
        "hovering selected nothing"
    );
    assert!(
        paint::paints_fill_at(cx, say, SELECTION_WASH),
        "the selected row keeps its wash"
    );
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "the selection is untouched");

    pointer_leaves(cx);
    fades_out(cx, wait);
}

/// Under reduced motion the hover wash leaves at once: the pointer's
/// departure asks for no frame and draws no fading wash.
#[gpui::test]
fn under_reduced_motion_the_hover_wash_leaves_at_once(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, vec![command("Rust sample", "sample_rust")]);
    settle(&window, cx);
    settle_frames(cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    settle_frames(cx);

    let wait = bounds_of(cx, "row-Wait briefly");
    cx.simulate_mouse_move(wait.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert!(
        paint::paints_fill_at(cx, wait, HOVER_WASH),
        "the wash arrives at once under the pointer"
    );

    cx.update(|_, cx| cx.set_reduce_motion(true));
    pointer_leaves(cx);
    assert_eq!(
        frame(cx, Duration::ZERO),
        0,
        "the departure asked for no frame"
    );
    assert!(
        !paints_any_fill_at(cx, wait),
        "the wash left at once, not fading"
    );
    assert_eq!(settle_frames(cx), 0, "the window is idle");
}

/// The Actions panel's entries: hovering an entry selects it (the panel
/// follows the pointer over its list), so the entry under the pointer
/// shows the selection wash alone and no entry ever shows the hover wash.
#[gpui::test]
fn the_actions_panel_selects_under_the_pointer_and_draws_no_hover_wash(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    settle_frames(cx);
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);

    let label = cx.read_entity(&window, |window, _| {
        window.launcher().selected_action().label.to_string()
    });
    let primary = bounds_of(cx, &format!("action-{label}"));
    assert!(
        paint::paints_fill_at(cx, primary, SELECTION_WASH),
        "the selected entry's wash"
    );
    assert!(
        !paint::paints_fill_at(cx, primary, OLD_ACTIONS),
        "not the wash it was before"
    );

    let pin = bounds_of(cx, "action-Pin");
    cx.simulate_mouse_move(pin.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    settle(&window, cx);
    assert!(
        paint::paints_fill_at(cx, pin, SELECTION_WASH),
        "the entry the pointer moved over is the selected one now"
    );
    assert!(
        !paint::paints_fill_at(cx, pin, HOVER_WASH),
        "no hover wash: hovering here is selecting"
    );
    assert!(
        !paints_any_fill_at(cx, primary),
        "the entry the selection left paints nothing"
    );
}

/// Root search's rows under the open Actions panel: the panel holds its
/// target, so the pointer moves no selection there — the row under it
/// takes the hover wash while the selected row keeps its own.
#[gpui::test]
fn root_rows_under_an_open_actions_panel_take_the_hover_wash(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    settle_frames(cx);

    // Open the panel with the pointer, so the pointer stays in mouse
    // modality: a keyboard open suppresses hover until the pointer moves
    // again.
    let button = bounds_of(cx, "actions-button").center();
    cx.simulate_click(button, Modifiers::none());
    settle(&window, cx);
    settle_frames(cx);

    let view = settle(&window, cx);
    let bravo = row_bounds(cx, &view, 1);
    cx.simulate_mouse_move(bravo.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert!(
        paint::paints_fill_at(cx, bravo, HOVER_WASH),
        "the row under the panel takes the hover wash"
    );
    assert!(
        paint::paints_fill_at(cx, row_bounds(cx, &view, 0), SELECTION_WASH),
        "the panel's target keeps the selection wash"
    );
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "the panel holds its target");
}

/// The selected computed answer card shows the selection wash — the
/// accent ring it was drawn with before #245 is gone with the row's
/// inset edge, both shadows the paint cannot see.
#[gpui::test]
fn the_selected_answer_card_shows_the_selection_wash_without_its_ring(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let launcher = with_calculator(cx, data.path());
    let (window, cx) = open_launcher(cx, launcher);
    cx.simulate_input("6*7");
    until(&window, cx, |view| {
        view.rows.iter().map(|row| row.title.as_str()).eq(["42"])
    });
    let card = bounds_of(cx, "row-42");
    assert!(
        cx.debug_bounds("answer-value").is_some(),
        "the answer is drawn as the card"
    );
    assert!(
        paint::paints_fill_at(cx, card, SELECTION_WASH),
        "the selected card's wash"
    );
    assert!(
        !paint::paints_fill_at(cx, card, CARD_FILL),
        "not the card's unselected fill"
    );
    assert!(
        !paint::paints_fill_at(cx, card, OLD_SELECTED),
        "not the row wash it was before"
    );
}

/// The Pane menu's selected entry shows the selection wash (the wash its
/// family was drawn with before #245), and the menu's own button — the
/// footer's mark — takes the hover wash, which fades out once the
/// pointer leaves it.
#[gpui::test]
fn the_pane_menu_washes_its_entry_and_its_mark_fades_out(cx: &mut TestAppContext) {
    let (window, cx) = three_rows(cx);
    settle_frames(cx);

    cx.simulate_click(bounds_of(cx, "footer-menu").center(), Modifiers::none());
    settle(&window, cx);
    settle_frames(cx);
    let item = bounds_of(cx, "menu-item-Settings");
    assert!(
        paint::paints_fill_at(cx, item, SELECTION_WASH),
        "the selected entry's wash"
    );
    assert!(
        !paint::paints_fill_at(cx, item, OLD_ACTIONS),
        "not the wash it was before"
    );

    // Close the menu and let its popup's exit finish, so the frames below
    // can only be the mark's fade.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    settle_frames(cx);

    let mark = bounds_of(cx, "footer-menu");
    cx.simulate_mouse_move(mark.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert!(
        paint::paints_fill_at(cx, mark, HOVER_WASH),
        "the mark's hover wash"
    );
    pointer_leaves(cx);
    fades_out(cx, mark);
}

/// A confirmation's buttons are the footer's family: the one under the
/// pointer takes the hover wash, which fades out once the pointer leaves.
#[gpui::test]
fn a_confirmations_buttons_hover_wash_fades_out_when_the_pointer_leaves(cx: &mut TestAppContext) {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let folder =
        packages::assembled_package("sample-actions", &sources.path().join("sample-actions"));
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    cx.executor().allow_parking();
    block_on(launcher.install_package(&folder));
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));

    cx.simulate_input("Actions");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    // The "Confirm" item, then its action through the Actions panel.
    let mut confirm = false;
    for _ in 0..8 {
        if selected_title(&settle(&window, cx)) == "Confirm" {
            confirm = true;
            break;
        }
        cx.simulate_keystrokes("down");
    }
    assert!(confirm, "the Confirm item is listed");
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    cx.simulate_input("ask");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    wait::until(cx, |cx| {
        cx.debug_bounds("confirmation").is_some().then_some(())
    });

    let primary = bounds_of(cx, "confirmation-primary");
    cx.simulate_mouse_move(primary.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    assert!(
        paint::paints_fill_at(cx, primary, HOVER_WASH),
        "the button under the pointer takes the hover wash"
    );
    pointer_leaves(cx);
    fades_out(cx, primary);
}

fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}
