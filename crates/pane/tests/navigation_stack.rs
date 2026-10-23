//! The navigation stack's window tests (#239): real key events drive the
//! navigation sample — the back key pops a designed view's stack, showing
//! the view below's last tree at once and then its re-render after the
//! pop event; Backspace pops too, but not on key repeat (the designed
//! screen shows no search field of its own yet, so Backspace is otherwise
//! unused on it — the List's search field, #240, owns the full
//! Backspace-on-empty behaviour); Shift+Esc drops the whole stack, as the
//! `window.pop-to-root` host function does.
//!
//! The Rust navigation sample (`guests/sample-nav`) is installed as a
//! package from `target/guests/packages`, its command declared
//! `"mode": "designed"`; the pane-core tests hold the tri-lingual parity.
//! The launcher's changes are wired to the window, as Pane's are, so the
//! pop event's re-render — which the launcher answers in the background —
//! reaches the window and is drawn.
//!
//! The sample's views have no search bar: each shows the top bar a pushed
//! view without one has (#482), the search bar's row with the back button
//! in the same place and the view's navigation title, whose click pops
//! one view as the back key does, the keyboard staying where the view
//! put it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, prelude::*, px, size};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Launcher, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/paint.rs"]
mod paint;
#[path = "support/settle.rs"]
mod settle;

use settle::{settle, until};

/// The launcher window with the Rust navigation sample installed, with the
/// launcher's background changes wired to it, and its command open.
fn open(cx: &mut TestAppContext) -> (TempDir, Entity<LauncherWindow>, &mut VisualTestContext) {
    let (data, window, cx) = installed(cx);
    open_sample(&window, cx);
    (data, window, cx)
}

/// Opens the navigation sample's command from root search.
fn open_sample(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_input("navigation sample");
    cx.simulate_keystrokes("enter");
    until(window, cx, |view| {
        matches!(view.screen, Screen::DesignedView(_))
    });
}

/// The launcher window with the Rust navigation sample installed, with the
/// launcher's background changes wired to it, at root search.
fn installed(cx: &mut TestAppContext) -> (TempDir, Entity<LauncherWindow>, &mut VisualTestContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(None, pane::settings::Overrides::parse(None, None), cx)
    });
    let runtime = Runtime::start().unwrap();
    let (changed, changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(Ok(runtime), Vec::new(), data.path().join("extensions"))
        .with_development(Arc::new(Toolchains::from_env(None)), changed);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-nav");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    let folder = data.path().join("sample-nav");
    copy_folder(&assembled, &folder);
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher_window = LauncherWindow::new(launcher, window, cx);
        launcher_window.follow_changes(changes, window, cx);
        launcher_window
    });
    (data, window, cx)
}

/// Copies what `from` holds into `to`, folders and all.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Waits until the designed view on screen shows `text`, which it does
/// once the guest's answer to the last event has arrived.
fn wait_for(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, text: &str) {
    let expected = text.to_owned();
    until(window, cx, move |view| match &view.screen {
        Screen::DesignedView(view) => {
            text_of(&view.tree.root).is_some_and(|found| found == expected)
        }
        _ => false,
    });
}

/// The first text of the tree `node` holds.
fn text_of(node: &pane_core::Node) -> Option<String> {
    use pane_core::{NodeKind, TextContent};
    let plain = |text: &pane_core::Text| match &text.content {
        TextContent::Plain(content) => Some(content.clone()),
        TextContent::Spans(spans) => (!spans.is_empty()).then(|| {
            spans
                .iter()
                .map(|span| span.text.clone())
                .collect::<String>()
        }),
    };
    match &node.kind {
        NodeKind::Text(text) => plain(text),
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(text_of)
            .or_else(|| node.children.iter().find_map(text_of)),
        _ => node.children.iter().find_map(text_of),
    }
}

/// How deep the designed view's navigation stack is.
fn depth(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> usize {
    cx.read_entity(window, |window, _| window.launcher().designed_stack_depth())
}

#[gpui::test]
fn the_back_key_pops_one_view_at_a_time_and_the_pop_event_re_renders(cx: &mut TestAppContext) {
    let (_data, window, cx) = open(cx);

    // The view opens with the keyboard on its first button: Enter presses
    // it, pushing the detail view above the rows.
    wait_for(&window, cx, "Nothing picked yet");
    assert_eq!(depth(&window, cx), 1);
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Detail: One");
    assert_eq!(depth(&window, cx), 2);

    // The back key pops it: the rows' last tree shows at once, and their
    // `onPop` answers the pop event that carried no result — the stack one
    // view deep again.
    cx.simulate_keystrokes("escape");
    wait_for(&window, cx, "Picked: nothing");
    assert_eq!(depth(&window, cx), 1);

    // Popping the last view leaves the command, as leaving its list does.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn backspace_pops_the_top_view_but_not_on_key_repeat(cx: &mut TestAppContext) {
    let (_data, window, cx) = open(cx);

    // Backspace pops the stack, as it pops a view whose search field is
    // empty.
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Detail: One");
    assert_eq!(depth(&window, cx), 2);
    cx.simulate_keystrokes("backspace");
    wait_for(&window, cx, "Picked: nothing");
    assert_eq!(depth(&window, cx), 1);

    // A held Backspace — the system's key repeat — pops nothing: only a
    // fresh press does.
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Detail: One");
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Deeper: One");
    assert_eq!(depth(&window, cx), 3);
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("backspace").unwrap(),
        is_held: true,
        prefer_character_input: false,
    });
    wait_for(&window, cx, "Deeper: One");
    assert_eq!(
        depth(&window, cx),
        3,
        "a repeat of the backspace key pops nothing"
    );
    cx.simulate_keystrokes("backspace");
    wait_for(&window, cx, "Detail: One");
    assert_eq!(depth(&window, cx), 2);
}

#[gpui::test]
fn shift_escape_drops_the_whole_stack(cx: &mut TestAppContext) {
    let (_data, window, cx) = open(cx);

    // Two views pushed, and the deeper view's own "Pop to root" drops the
    // whole stack through the window host function — Shift+Esc does the
    // same.
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Detail: One");
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Deeper: One");
    assert_eq!(depth(&window, cx), 3);
    // Return to root is Command+Esc on macOS.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-escape"
    } else {
        "shift-escape"
    });
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(
        cx.read_entity(&window, |window, _| window.designed_button_count()),
        0,
        "the designed view's controls are gone"
    );
}

/// How long the pointer rests before a tooltip shows, with room to spare.
const TOOLTIP_DELAY: Duration = Duration::from_millis(700);

/// Whether root search's query field has keyboard focus.
fn query_has_focus(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> bool {
    use gpui::Focusable;
    let input = cx.read_entity(window, |window, _| window.query_field());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Whether the top bar names its view `title`.
fn titled(cx: &mut VisualTestContext, title: &str) -> bool {
    a11y::headings(cx).iter().any(|heading| heading == title)
}

#[gpui::test]
fn a_view_with_no_search_bar_has_a_top_bar_with_a_back_button(cx: &mut TestAppContext) {
    let (_data, window, cx) = installed(cx);
    settle(&window, cx);

    // Root search: the magnifier at the search bar's left, the results
    // under its row, and no top bar.
    let magnifier = cx
        .debug_bounds("search-magnifier")
        .expect("root search shows the magnifier");
    let results = cx.debug_bounds("rows").expect("root search's results");
    assert!(cx.debug_bounds("top-bar").is_none());

    // The view has no search bar: its top bar is the search bar's row,
    // the back button exactly where the magnifier was and the view
    // starting where the results did, so nothing jumps. It names the view
    // by its navigation title, and the keyboard is on the view's first
    // button, where the view put it.
    open_sample(&window, cx);
    wait_for(&window, cx, "Nothing picked yet");
    let bar = cx.debug_bounds("top-bar").expect("the view's top bar");
    let back = cx
        .debug_bounds("search-back")
        .expect("the top bar's back button");
    assert!(cx.debug_bounds("search-magnifier").is_none());
    assert_eq!(back, magnifier, "the button takes the magnifier's place");
    assert_eq!(
        bar.bottom(),
        results.top(),
        "the bar is the search bar's row"
    );
    let view = cx.debug_bounds("designed-view").expect("the view's tree");
    assert_eq!(view.top(), results.top(), "the view starts below the bar");
    assert!(titled(cx, "Navigation sample"));
    assert_eq!(a11y::focused_label(cx).as_deref(), Some("One"));

    // The search bar's back button: the split views' fill at rest, the
    // footer's open wash under the pointer, and the tooltip naming the
    // back key. Pointing at it takes no focus.
    let square = gpui::Bounds::new(back.origin + point(px(6.), px(6.)), size(px(32.), px(32.)));
    let washed = |cx: &mut VisualTestContext, dark: u32, light: u32| {
        paint::paints_fill_at(cx, square, dark) || paint::paints_fill_at(cx, square, light)
    };
    assert!(washed(cx, 0xFFFFFF0F, 0x0000000F), "the resting fill");
    cx.simulate_mouse_move(back.center(), None, Modifiers::none());
    cx.executor().advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
    assert!(washed(cx, 0xFFFFFF1A, 0x0000001A), "the hover wash");
    assert!(
        cx.debug_bounds("tooltip-Back (Escape)").is_some(),
        "the tooltip names the back key"
    );
    assert_eq!(a11y::focused_label(cx).as_deref(), Some("One"));

    // A pushed view's bar is the same, named by its own title, the
    // keyboard on its own first button.
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Detail: One");
    assert_eq!(depth(&window, cx), 2);
    assert_eq!(cx.debug_bounds("search-back"), Some(back));
    assert!(titled(cx, "One"));
    assert_eq!(a11y::focused_label(cx).as_deref(), Some("Deeper"));

    // A click pops one view, as the back key does: the view below shows,
    // its keyboard on its first button again.
    cx.simulate_click(back.center(), Modifiers::none());
    wait_for(&window, cx, "Picked: nothing");
    assert_eq!(depth(&window, cx), 1);
    assert!(titled(cx, "Navigation sample"));
    assert_eq!(a11y::focused_label(cx).as_deref(), Some("One"));

    // The last view's click leaves the command for root search, whose
    // field takes the keyboard and whose magnifier is back.
    cx.simulate_click(back.center(), Modifiers::none());
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::Root { .. })
    });
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(query_has_focus(&window, cx), "the field has the keyboard");
    assert!(cx.debug_bounds("top-bar").is_none());
    assert_eq!(cx.debug_bounds("search-magnifier"), Some(magnifier));
}
