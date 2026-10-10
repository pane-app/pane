//! #258's keys through the native window, on GPUI's test platform, with
//! real key events: Backspace with no modifiers, not an auto-repeat, goes
//! back one level — the Back action without clearing a search's text —
//! out of an empty command search and from a screen with no text field in
//! focus (a command's own list, a package preview, a details screen), but
//! never out of root search, a form or a field with text in it, and never
//! after a held Backspace empties a field. Alt+Up and Alt+Down move the
//! selection five rows at a time and clamp at the list's ends, in root
//! search, a command's list, a command's own search and the Actions
//! panel. Ctrl+Up and Ctrl+Down cross root search's result sections and
//! the Actions panel's groups, scrolling the section's label into view,
//! and in the vertical pinned layout the pinned rows count as the first
//! section while the horizontal strip is never entered. The Emacs and Vim
//! choices' pairs move the selection and the argument form's fields, with
//! Ctrl+K still opening the Actions panel beside the Vim pair. A rebound
//! Back-a-level or section jump takes effect, and Ctrl+Alt+Up/Down keep
//! moving the pins.

use std::path::{Path, PathBuf};

use futures::executor::block_on;
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{CommandRegistration, Launcher, LauncherView, Runtime, Screen, Status};

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/packages.rs"]
mod packages;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/setup.rs"]
mod setup;

#[path = "../../pane-core/tests/support/service.rs"]
mod service;

use a11y::focused_label;
use settle::{enter_flow, settle, settle_shown, until};
use setup::actions_shortcut;

/// The keystroke that moves the pinned home's focused pin one place
/// later on this system.
const MOVE_PIN_DOWN: &str = if cfg!(target_os = "macos") {
    "cmd-alt-down"
} else {
    "ctrl-alt-down"
};

/// The keystroke that moves it one place earlier.
const MOVE_PIN_UP: &str = if cfg!(target_os = "macos") {
    "cmd-alt-up"
} else {
    "ctrl-alt-up"
};

/// The Emacs choice's previous- and next-result keys, and its Left and
/// Right, on this system.
const EMACS_PREVIOUS: &str = if cfg!(target_os = "macos") {
    "ctrl-p"
} else {
    "alt-p"
};
const EMACS_NEXT: &str = if cfg!(target_os = "macos") {
    "ctrl-n"
} else {
    "alt-n"
};
const EMACS_LEFT: &str = if cfg!(target_os = "macos") {
    "ctrl-b"
} else {
    "alt-b"
};
const EMACS_RIGHT: &str = if cfg!(target_os = "macos") {
    "ctrl-f"
} else {
    "alt-f"
};

/// The Vim choice's previous- and next-result keys on this system: Ctrl+K
/// is still the Open actions binding on macOS, where the pair holds Ctrl.
const VIM_PREVIOUS: &str = if cfg!(target_os = "macos") {
    "ctrl-k"
} else {
    "alt-k"
};
const VIM_NEXT: &str = if cfg!(target_os = "macos") {
    "ctrl-j"
} else {
    "alt-j"
};

/// The calculator's assembled package, for its computed answers.
fn calculator() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/calculator")
}

/// The actions sample as a root command, titled "Actions sample": the
/// window's item_actions tests open it the same way, for a list whose
/// items carry several actions in sections.
fn actions_command() -> CommandRegistration {
    let component =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_actions.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    CommandRegistration {
        id: "actions-sample".into(),
        title: "Actions sample".into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
    }
}

/// A root command `id` titled `title`, whose component is the Rust
/// sample's: root search only lists it, and no key here runs it.
fn command(id: &str, title: &str) -> CommandRegistration {
    let component =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    CommandRegistration {
        id: id.into(),
        title: title.into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
    }
}

/// `count` root commands, "Command 01" and so on, never run.
fn numbered(count: usize) -> Vec<CommandRegistration> {
    (1..=count)
        .map(|index| {
            command(
                &format!("command-{index:02}"),
                &format!("Command {index:02}"),
            )
        })
        .collect()
}

/// Writes `record` as the host settings record of `data` and initializes
/// the settings from it, before any window is made.
fn settings_from(cx: &mut TestAppContext, data: &Path, record: &str) {
    std::fs::write(data.join("settings.json"), record).unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
}

/// Installs the assembled sample `name` into `launcher`, as a user does
/// through its preview.
fn install(launcher: &Launcher, name: &str) {
    let sources = tempfile::tempdir().unwrap();
    let folder = packages::assembled_package(name, &sources.path().join(name));
    block_on(launcher.install_package(&folder));
}

/// The window over `launcher`, with the keys bound as the binary does.
fn open_launcher(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx))
}

/// The window over `commands` root commands, on root search.
fn open_with(
    cx: &mut TestAppContext,
    commands: Vec<CommandRegistration>,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    open_launcher(cx, Launcher::new(Runtime::start(), commands))
}

/// The window over `commands` root commands and the sample `name`
/// installed as a package in `data`.
fn open_installed<'a>(
    cx: &'a mut TestAppContext,
    commands: Vec<CommandRegistration>,
    data: &Path,
    name: &str,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let launcher = Launcher::with_packages(Runtime::start(), commands, data.join("extensions"));
    install(&launcher, name);
    let (window, cx) = open_launcher(cx, launcher);
    settle(&window, cx);
    (window, cx)
}

/// The window over the sample `name`'s previewed package, in a data
/// folder of `data`'s.
fn open_preview<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    name: &str,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let sources = tempfile::tempdir().unwrap();
    let folder = packages::assembled_package(name, &sources.path().join(name));
    let launcher = Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"));
    let preview = folder.clone();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(move |window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.preview_package(&preview, window, cx);
        launcher
    });
    settle(&window, cx);
    (window, cx)
}

/// The window over the Rust sample as a root command, its list opened
/// with Enter: a command's own list, "Say hello" selected.
fn open_command_list(
    cx: &mut TestAppContext,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let (window, cx) = open_with(cx, vec![command("sample_rust", "Rust sample")]);
    cx.simulate_input("rust sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Rust sample")
    );
    (window, cx)
}

/// The window over the search sample installed and its command opened:
/// the command's own search field, empty, with focus.
fn open_command_search<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let (window, cx) = open_installed(cx, vec![], data, "sample-search");
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        }
    );
    assert_eq!(field_text(&window, cx), "");
    (window, cx)
}

/// The rows' titles, as owned strings, for comparing a whole list.
fn row_titles(view: &LauncherView) -> Vec<String> {
    view.rows.iter().map(|row| row.title.clone()).collect()
}

/// The titles of `view`'s rows.
fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// The title of `view`'s selected row.
fn selected_title(view: &LauncherView) -> &str {
    &view.rows[view.selected.expect("a row is selected")].title
}

/// The text in the window's query field.
fn field_text(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> String {
    let field = cx.read_entity(window, |window, _| window.query_field());
    cx.read_entity(&field, |field, _| field.as_str().to_owned())
}

/// Whether the query field has the focus.
fn query_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let input = cx.read_entity(window, |window, _| window.query_field());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Whether the launcher window is hidden, as the window drove it.
fn hidden(window: &Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Whether the Actions panel is open.
fn actions_open(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.actions_open())
}

/// The pins' titles, in order.
fn slot_titles(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read_entity(window, |window, _| window.launcher().quick_slots())
        .into_iter()
        .map(|slot| slot.title)
        .collect()
}

/// A repeat of a held `key`, as the system reports it — not a press.
fn press_held(cx: &mut VisualTestContext, key: &'static str) {
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse(key).unwrap(),
        is_held: true,
        prefer_character_input: false,
    });
}

/// Selects the row titled `title` with the arrow keys, as far down as the
/// list is long.
fn select(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, title: &str) {
    for _ in 0..40 {
        let view = settle(window, cx);
        if view
            .selected
            .is_some_and(|index| view.rows[index].title == title)
        {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    panic!("no row {title} is selected");
}

/// Whether the element `selector` names lies in the result list's view:
/// the rows' column, between the search header and the footer — the
/// region the virtual list draws its children into. A label scrolled
/// past is drawn above that column (or not at all, a virtual list laying
/// out only what is near the view), one scrolled into view intersects it
/// (#258).
fn in_view(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    let Some(label) = cx.debug_bounds(selector) else {
        return false;
    };
    let list = cx.debug_bounds("rows").expect("the result list");
    label.bottom() > list.top() && label.top() < list.bottom()
}

/// Backspace with no modifiers, on an empty command search, goes back one
/// level without clearing anything — and with text in the field, the
/// field's own Backspace deletes it, the command staying open.
#[gpui::test]
fn backspace_backs_out_of_an_empty_command_search_but_not_one_with_text(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_command_search(cx, data.path());

    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // Reopened, with text: the field keeps the key.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_input("ab");
    settle(&window, cx);
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch { query: "a".into() },
        "the field's Backspace deletes its text"
    );
    assert_eq!(field_text(&window, cx), "a");
}

/// Backspace backs out of a command's own list, a package preview and a
/// details screen — screens no text field has focus on — and a repeat of
/// a held key backs out of nothing.
#[gpui::test]
fn backspace_backs_out_of_a_command_list_a_preview_and_a_details_screen(cx: &mut TestAppContext) {
    // The command's own list: no text field in focus.
    let (window, cx) = open_command_list(cx);

    // A held key's repeats are not presses: nothing leaves the screen.
    press_held(cx, "backspace");
    press_held(cx, "backspace");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command);
    // The press is.
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // A package preview, over a window of its own.
    let data = tempfile::tempdir().unwrap();
    let (preview, preview_cx) = open_preview(cx, data.path(), "sample-arguments");
    let view = settle(&preview, preview_cx);
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    preview_cx.simulate_keystrokes("backspace");
    let view = settle(&preview, preview_cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // A details screen of the extension list, and the list itself.
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_installed(cx, vec![], data.path(), "sample-search");
    enter_flow(&window, cx);
    select(&window, cx, "Network use of Search sample");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::NetworkDetails { .. }),
        "{:?}",
        view.screen
    );
    // Back to the list the details screen came from, then out of it.
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Extensions { .. }),
        "{:?}",
        view.screen
    );
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

/// Backspace does nothing in root search — an empty query has nothing to
/// back out of, and it never hides the launcher — and never acts from a
/// form's or an argument field.
#[gpui::test]
fn backspace_does_nothing_in_root_search_and_never_from_a_form(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, numbered(3));
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: String::new()
        }
    );

    cx.simulate_keystrokes("backspace backspace");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(!hidden(&window, cx), "the launcher did not hide");

    // A query is the field's to delete.
    cx.simulate_input("command 01");
    settle(&window, cx);
    cx.simulate_keystrokes("backspace");
    assert_eq!(settle(&window, cx).query(), Some("command 0"));

    // A form's fields — an argument's among them — keep the key.
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_installed(cx, vec![], data.path(), "sample-arguments");
    cx.simulate_input("greet");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "the form stays open");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
}

/// A held Backspace that empties a field does not then back out of the
/// command: its repeats are not presses. A fresh press is.
#[gpui::test]
fn a_held_backspace_that_empties_a_field_does_not_back_out(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_command_search(cx, data.path());
    cx.simulate_input("ab");
    settle(&window, cx);

    // The press deletes, and the repeats empty the field.
    cx.simulate_keystrokes("backspace");
    press_held(cx, "backspace");
    press_held(cx, "backspace");
    press_held(cx, "backspace");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        },
        "the repeats backed out of nothing"
    );
    assert_eq!(field_text(&window, cx), "");

    // A fresh press backs out.
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

/// Alt+Down and Alt+Up move root search's selection five rows at a time,
/// stopping at the first and last rows.
#[gpui::test]
fn alt_arrows_move_root_searches_selection_five_rows(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, numbered(12));
    let view = settle(&window, cx);
    // The commands, then Pane's "Settings…" row, always last.
    assert_eq!(view.rows.len(), 13);
    assert_eq!(view.selected, Some(0));

    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(5));
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(10));
    // Past the end: the last row, and it stays there.
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(12));
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(12));

    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(7));
    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(2));
    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(0));
    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(0));
}

/// A command's own list answers the five-row moves the same way, and its
/// section jumps — the list draws no sections of its own, so all of it is
/// one — stop at its first and last rows.
#[gpui::test]
fn a_command_list_moves_five_rows_and_jumps_to_its_ends(cx: &mut TestAppContext) {
    let (window, cx) = open_command_list(cx);
    let view = settle(&window, cx);
    // The sample's eight items, its two platform-gated ones listed as
    // unavailable wherever they do not run, so the count is the same on
    // every system.
    assert_eq!(view.rows.len(), 8, "{:?}", titles(&view));
    assert_eq!(view.selected, Some(0));

    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(5));
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(7), "clamped at the last");
    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(2));

    // One section of its own: down goes to the last row, up from inside
    // it to its first, and no further.
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(settle(&window, cx).selected, Some(7));
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(settle(&window, cx).selected, Some(0));
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(settle(&window, cx).selected, Some(0));
}

/// A command's own search answers the five-row moves over the service it
/// searches: six results for a one-letter query, the fifth row below the
/// first and the last row the clamp.
#[gpui::test]
fn a_command_search_moves_five_rows(cx: &mut TestAppContext) {
    let service = service::Service::start();
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = open_installed(cx, vec![], data.path(), "sample-search");

    // Point the command's search at this test's service.
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("down enter");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    cx.simulate_input(&service.url());
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    cx.simulate_input("a");
    let view = settle(&window, cx);
    assert_eq!(titles(&view).len(), 6, "{:?}", titles(&view));
    assert_eq!(view.selected, Some(0));
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(5));
    cx.simulate_keystrokes("alt-down");
    assert_eq!(settle(&window, cx).selected, Some(5), "clamped at the last");
    cx.simulate_keystrokes("alt-up");
    assert_eq!(settle(&window, cx).selected, Some(0));
}

/// Ctrl+Down and Ctrl+Up cross root search's result sections: from the
/// computed answer's "Calculator" to the matches' "Results", the first
/// row of the next section and of the previous one, the last row when
/// there is no next section, and the section's label scrolling into view
/// over the row the jump lands on.
#[gpui::test]
fn ctrl_arrows_cross_root_searches_sections_scrolling_the_label_into_view(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let commands: Vec<_> = (1..=20)
        .map(|index| command(&format!("command-{index:02}"), &format!("6*7 {index:02}")))
        .collect();
    let launcher =
        Launcher::with_packages(Runtime::start(), commands, data.path().join("extensions"));
    block_on(launcher.install_package(&calculator()));
    let (window, cx) = open_launcher(cx, launcher);

    // The answer above twenty matches: "Calculator" and "Results".
    cx.simulate_input("6*7");
    let expected: Vec<String> = std::iter::once("42".to_owned())
        .chain((1..=20).map(|index| format!("6*7 {index:02}")))
        .collect();
    until(&window, cx, |view| row_titles(view) == expected);
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    assert!(in_view(cx, "section-Calculator"));
    assert!(in_view(cx, "section-Results"));

    // The first row of the next section: the first match.
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(selected_title(&settle(&window, cx)), "6*7 01");

    // Ten rows down, the "Results" label is out of view; the jump back
    // lands on the section's first row with its label scrolled into view.
    cx.simulate_keystrokes("alt-down alt-down");
    assert_eq!(settle(&window, cx).selected, Some(10));
    assert!(!in_view(cx, "section-Results"), "the label is out of view");
    cx.simulate_keystrokes("ctrl-up");
    let view = settle(&window, cx);
    assert_eq!(selected_title(&view), "6*7 01");
    assert!(
        in_view(cx, "section-Results"),
        "the label scrolled into view"
    );

    // From the section's first row, up: the "Calculator" answer, and
    // nothing above it to reach.
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(selected_title(&settle(&window, cx)), "42");
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(selected_title(&settle(&window, cx)), "42");

    // No next section past the last: the last row, and it stays there.
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(selected_title(&settle(&window, cx)), "6*7 01");
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(selected_title(&settle(&window, cx)), "6*7 20");
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(selected_title(&settle(&window, cx)), "6*7 20");
}

/// In the vertical pinned layout the pinned rows count as the first
/// section: Ctrl+Up from the rows reaches the first pin, and Ctrl+Down
/// from the pins comes back to the rows' first row, the focus following.
#[gpui::test]
fn the_vertical_pinned_rows_count_as_the_first_section(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    settings_from(
        cx,
        data.path(),
        r#"{ "version": 1, "pinnedLayout": "vertical" }"#,
    );
    let (window, cx) = pinned_home(cx, data.path(), &["sample_ts", "sample_rust"]);
    assert!(cx.debug_bounds("slot-1").is_some());
    assert_eq!(settle(&window, cx).selected, Some(0));

    // From the rows' first row, up: the first pin.
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));

    // From the pins, down: the rows' first row, focus back in the field.
    cx.simulate_keystrokes("ctrl-down");
    assert!(query_has_focus(&window, cx));
    assert_eq!(selected_title(&settle(&window, cx)), "Alpha");

    // The first pin is the pins' own first row: up stays on it.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
    // From the second pin, up: the first.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 2: Alpha"));
    cx.simulate_keystrokes("ctrl-up");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
}

/// The horizontal strip is not entered: from the rows' first row, Ctrl+Up
/// leaves the selection where it is, and the focus in the search field.
#[gpui::test]
fn the_horizontal_strip_is_not_entered(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_home(cx, data.path(), &["sample_ts", "sample_rust"]);
    assert!(cx.debug_bounds("slot-1").is_some());
    assert_eq!(settle(&window, cx).selected, Some(0));
    assert!(query_has_focus(&window, cx));

    cx.simulate_keystrokes("ctrl-up");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "the selection stays");
    assert!(query_has_focus(&window, cx), "the focus stays in the field");
    // Down from the strip still leaves it for the first row.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
    cx.simulate_keystrokes("ctrl-down");
    assert!(query_has_focus(&window, cx));
    assert_eq!(selected_title(&settle(&window, cx)), "Alpha");
}

/// The Actions panel answers Alt+Up and Alt+Down over its entries, and
/// Ctrl+Up and Ctrl+Down across its groups: the first entry of the next
/// group, its own first entry first on the way up, the last entry when no
/// group follows, and the first of all. Enter runs what the panel has
/// selected, which names the entry each jump lands on.
#[gpui::test]
fn the_actions_panel_jumps_entries_and_groups(cx: &mut TestAppContext) {
    let (window, cx) = open_with(cx, vec![actions_command()]);
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);

    // "Alpha note" has nine actions: an untitled run, then "Edit",
    // "Share" and "Danger" sections.
    let runs = panel_runs;

    // Five entries at a time, clamped at the last.
    assert_eq!(
        runs("alt-down", &window, cx),
        Status::Result("Copy Link: Alpha note".into())
    );
    assert_eq!(
        runs("alt-down alt-down", &window, cx),
        Status::Result("Delete: Alpha note".into())
    );
    assert_eq!(
        runs("alt-up", &window, cx),
        Status::Result("Duplicate: Alpha note".into())
    );

    // The groups: the "Edit", "Share" and "Danger" sections' first
    // entries, the last entry past the last group, and the first of all.
    assert_eq!(
        runs("ctrl-down", &window, cx),
        Status::Result("Rename: Alpha note".into())
    );
    assert_eq!(
        runs("ctrl-down ctrl-down", &window, cx),
        Status::Result("Copy Link: Alpha note".into())
    );
    assert_eq!(
        runs("ctrl-down ctrl-down ctrl-down", &window, cx),
        Status::Result("Delete: Alpha note".into())
    );
    assert_eq!(
        runs("ctrl-down ctrl-down ctrl-down ctrl-down", &window, cx),
        Status::Result("Delete: Alpha note".into())
    );
    assert_eq!(
        runs("ctrl-up", &window, cx),
        Status::Result("Open: Alpha note".into())
    );
    // From inside "Share" — past its first entry — up lands on its own
    // first entry first, before the group above it.
    assert_eq!(
        runs("alt-down down ctrl-up", &window, cx),
        Status::Result("Copy Link: Alpha note".into())
    );
}

/// Opens the Actions panel over the selected item, presses `keys`, and
/// answers the outcome of the entry the panel then has selected, which
/// Enter runs — the toast names the entry each jump lands on.
fn panel_runs(keys: &str, window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Status {
    cx.simulate_keystrokes(actions_shortcut());
    settle(window, cx);
    assert!(actions_open(window, cx));
    cx.simulate_keystrokes(keys);
    settle(window, cx);
    cx.simulate_keystrokes("enter");
    settle_shown(window, cx)
}

/// The Emacs choice's pair moves the selection — root search's — and its
/// Left and Right move between the argument form's fields.
#[gpui::test]
fn the_emacs_pair_moves_the_selection_and_the_argument_fields(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    settings_from(
        cx,
        data.path(),
        r#"{ "version": 1, "navigationBindings": "emacs" }"#,
    );
    let (window, cx) = open_installed(cx, vec![], data.path(), "sample-arguments");

    cx.simulate_keystrokes(EMACS_NEXT);
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes(EMACS_PREVIOUS);
    assert_eq!(settle(&window, cx).selected, Some(0));

    // The argument form: Left and Right between its fields.
    cx.simulate_input("greet");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
    cx.simulate_keystrokes(EMACS_RIGHT);
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    // The tone field: its chosen segment is what assistive technology
    // reads of it, the radio group's active descendant.
    cx.simulate_keystrokes(EMACS_RIGHT);
    assert_eq!(focused_label(cx).as_deref(), Some("Warm"));
    cx.simulate_keystrokes(EMACS_LEFT);
    assert_eq!(focused_label(cx).as_deref(), Some("Secret"));
    cx.simulate_keystrokes(EMACS_LEFT);
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));
}

/// The Vim choice's pair moves the selection, and Ctrl+K still opens the
/// Actions panel beside it — with the panel's own list answering the pair
/// as it answers Up and Down.
#[gpui::test]
fn the_vim_pair_moves_the_selection_and_ctrl_k_opens_the_panel(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    settings_from(
        cx,
        data.path(),
        r#"{ "version": 1, "navigationBindings": "vim" }"#,
    );
    let (window, cx) = open_with(cx, vec![actions_command()]);
    cx.simulate_input("actions sample");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    assert_eq!(settle(&window, cx).selected, Some(0));

    cx.simulate_keystrokes(VIM_NEXT);
    assert_eq!(settle(&window, cx).selected, Some(1));
    cx.simulate_keystrokes(VIM_PREVIOUS);
    assert_eq!(settle(&window, cx).selected, Some(0));

    // Ctrl+K opens the panel, and the pair moves the panel's own
    // selection: Enter runs "Copy", the second entry.
    cx.simulate_keystrokes(actions_shortcut());
    settle(&window, cx);
    assert!(actions_open(&window, cx));
    cx.simulate_keystrokes(VIM_NEXT);
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Copy: Alpha note".into())
    );
}

/// A rebound Back-a-level and section jump take effect — through the host
/// settings' record, as the Keyboard page writes it — and the keys they
/// superseded stop acting.
#[gpui::test]
fn a_rebound_back_a_level_and_section_jump_take_effect(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    settings_from(
        cx,
        data.path(),
        r#"{ "version": 1, "keyboard": { "back-a-level": "f9", "next-section": "f6" } }"#,
    );
    let (window, cx) = open_installed(cx, numbered(12), data.path(), "sample-search");

    // The command's own search, empty: F9 backs out of it, and the
    // Backspace the record moved it off no longer does.
    cx.simulate_input("package search");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    cx.simulate_keystrokes("backspace");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::CommandSearch {
            query: String::new()
        },
        "the Backspace no longer backs out"
    );
    cx.simulate_keystrokes("f9");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // Root search's one section: F6 jumps to the last row, Ctrl+Down —
    // the key it superseded — no longer jumps at all.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    cx.simulate_keystrokes("ctrl-down");
    assert_eq!(
        settle(&window, cx).selected,
        Some(0),
        "the old key does nothing"
    );
    cx.simulate_keystrokes("f6");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(view.rows.len() - 1), "the last row");
}

/// Ctrl+Alt+Up and Ctrl+Alt+Down keep moving the focused pin, beside the
/// launcher's new Alt and Ctrl arrow keys: neither of those moves a pin.
#[gpui::test]
fn ctrl_alt_arrows_keep_moving_the_pins(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = pinned_home(cx, data.path(), &["sample_ts", "sample_rust"]);
    assert_eq!(slot_titles(&window, cx), ["Charlie", "Alpha"]);

    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
    cx.simulate_keystrokes(MOVE_PIN_DOWN);
    settle(&window, cx);
    assert_eq!(slot_titles(&window, cx), ["Alpha", "Charlie"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 2: Charlie"));

    // Alt+Down moves the selection, never the pin: the focus stays on
    // the slot, and the pins keep their places.
    cx.simulate_keystrokes("alt-down");
    settle(&window, cx);
    assert_eq!(slot_titles(&window, cx), ["Alpha", "Charlie"]);
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Pinned 2: Charlie"),
        "the focus stays on the slot"
    );

    // The move key still moves, focus following.
    cx.simulate_keystrokes(MOVE_PIN_UP);
    settle(&window, cx);
    assert_eq!(slot_titles(&window, cx), ["Charlie", "Alpha"]);
    assert_eq!(focused_label(cx).as_deref(), Some("Pinned 1: Charlie"));
}

/// The window over root commands — Alpha, Bravo and Charlie — whose
/// quick-slots record first pins `pins` (command ids, in order).
fn pinned_home<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    pins: &[&str],
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let pins: Vec<serde_json::Value> = pins
        .iter()
        .map(|id| serde_json::json!({ "command": id }))
        .collect();
    let record = serde_json::json!({ "version": 2, "pins": pins });
    std::fs::write(data.join("quick-slots.json"), record.to_string()).unwrap();
    let launcher = Launcher::new(
        Runtime::start(),
        vec![
            command("sample_rust", "Alpha"),
            command("sample_js", "Bravo"),
            command("sample_ts", "Charlie"),
        ],
    )
    .with_quick_slots(data);
    let (window, cx) = open_launcher(cx, launcher);
    settle(&window, cx);
    (window, cx)
}
