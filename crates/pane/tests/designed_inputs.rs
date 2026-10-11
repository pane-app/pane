//! The keyed state of a designed view's window tests (#238): what the
//! window keeps by key survives the re-renders an extension's answers
//! draw — the caret, the input method's composition, the selection, the
//! scroll position, the focus and the hover — and the reorders that move
//! keyed nodes around. The fields are partially controlled: typing edits
//! at once (the sample echoes the value back, and nothing typed is
//! lost), and a value the extension sets replaces the text.
//!
//! The Rust sample's `components` gallery (`guests/sample-view`) holds
//! the fields: "Name" keyed `name` (asking for its value as the user
//! types), "Notes" keyed `notes`, a "Clear" button that sets both, and a
//! "Reorder" button that swaps the two fields. GPUI CE's test platform
//! cannot reach the window's input handler, so composition is driven on
//! the field's editing state, as the form's tests drive theirs.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Entity, EntityInputHandler as _, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/paint.rs"]
mod paint;

#[path = "support/settle.rs"]
mod settle;

use a11y::focused_label;
use paint::paints_fill_at;
use settle::until;

/// The dark theme's accent, which the "Surface" text's hover variant
/// fills with.
const DARK_ACCENT: u32 = 0xC9EE6AFF;

/// One test's window: the folders it installed from and the launcher
/// window, with the sample's gallery open.
struct Opened {
    _sources: TempDir,
    _data: TempDir,
    window: Entity<LauncherWindow>,
}

/// The launcher window in the dark theme, with the sample's components
/// gallery open.
fn open(cx: &mut TestAppContext) -> (Opened, &mut VisualTestContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some("dark"), None),
            cx,
        )
    });
    let launcher =
        Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-view");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    let folder = sources.path().join("sample-view");
    copy_folder(&assembled, &folder);
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("components sample");
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::DesignedView(_))
    });
    (
        Opened {
            _sources: sources,
            _data: data,
            window,
        },
        cx,
    )
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

/// The bounds of the element with debug selector `selector`.
fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
}

/// The value the field keyed `key` holds: its live text, what the user
/// typed.
fn field_text(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    key: &'static str,
) -> String {
    cx.read_entity(window, |window, cx| {
        window
            .designed_field_text(key, cx)
            .unwrap_or_else(|| panic!("the {key} field is open"))
    })
}

/// Focuses the field keyed `key`, as the keyboard would reach it.
fn focus_field(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, key: &'static str) {
    window.update_in(cx, |window, window_cx, cx| {
        window.focus_designed_field(key, window_cx, cx);
    });
}

/// The editing state of the field keyed `key`, which a platform input
/// method talks to.
fn field_of(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    key: &'static str,
) -> Entity<gpui_elements::editable_text::EditableTextState> {
    cx.read_entity(window, |window, _| window.designed_field(key))
        .unwrap_or_else(|| panic!("the {key} field is open"))
}

/// The marked (composing) range of the field keyed `key`, as a platform
/// input method sees it.
fn marked(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    key: &'static str,
) -> Option<std::ops::Range<usize>> {
    let field = field_of(window, cx, key);
    cx.update(|window, cx| field.update(cx, |field, cx| field.marked_text_range(window, cx)))
}

/// Waits until the gallery's echo names exactly `text`, so the answer to
/// the field's input events is on screen.
fn wait_for_echo(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, text: &'static str) {
    let drawn = format!("Echo: {text}");
    until(window, cx, |view| match &view.screen {
        Screen::DesignedView(view) => texts_of(&view.tree.root)
            .iter()
            .any(|held| held.as_str() == drawn.as_str()),
        _ => false,
    });
}

/// The text nodes' contents of `node`'s tree.
fn texts_of(node: &pane_core::Node) -> Vec<String> {
    let mut held = Vec::new();
    collect(node, &mut held);
    held
}

/// The text nodes' contents of `node`'s subtree, appended to `held`.
fn collect(node: &pane_core::Node, held: &mut Vec<String>) {
    if let pane_core::NodeKind::Text(text) = &node.kind {
        match &text.content {
            pane_core::TextContent::Plain(content) => held.push(content.clone()),
            pane_core::TextContent::Spans(spans) => held.push(
                spans
                    .iter()
                    .map(|span| span.text.clone())
                    .collect::<String>(),
            ),
        }
    }
    for child in &node.children {
        collect(child, held);
    }
}

#[gpui::test]
fn typing_edits_at_once_and_the_echo_never_fights_it(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    assert!(
        cx.debug_bounds("designed-input-Name").is_some(),
        "the gallery's fields are drawn"
    );

    // The field starts from the value its tree names.
    focus_field(&window, cx, "name");
    assert_eq!(field_text(&window, cx, "name"), "typed");

    // Typing edits at once: the text changes as the keys land, and the
    // value the view echoes back — its own, told through the input
    // events — never fights the typing.
    cx.simulate_input(" more");
    assert_eq!(
        field_text(&window, cx, "name"),
        "typed more",
        "typing edits at once"
    );
    wait_for_echo(&window, cx, "typed more");
    assert_eq!(
        field_text(&window, cx, "name"),
        "typed more",
        "the echo keeps what the user typed"
    );

    // Typing fast, while the echoes are still arriving: nothing typed is
    // lost — the input events are coalesced to the latest while one is in
    // flight, and the echo of a value the field itself reported is no
    // instruction.
    cx.simulate_input(" text");
    cx.simulate_input(" typed");
    cx.simulate_input(" fast");
    wait_for_echo(&window, cx, "typed more text typed fast");
    assert_eq!(
        field_text(&window, cx, "name"),
        "typed more text typed fast",
        "nothing typed fast was lost"
    );
}

#[gpui::test]
fn a_value_the_extension_sets_replaces_the_text(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    focus_field(&window, cx, "name");
    cx.simulate_input(" more");
    wait_for_echo(&window, cx, "typed more");

    // "Clear" sets the field's value: a value that differs from the
    // node's value in the extension's previous render replaces the text,
    // and its caret moves to the value's end.
    let clear = bounds(cx, "designed-button-Clear");
    cx.simulate_click(clear.center(), gpui::Modifiers::none());
    wait_for_echo(&window, cx, "");
    assert_eq!(
        field_text(&window, cx, "name"),
        "",
        "the extension's value replaced the text"
    );

    // Typing again works on the value the extension set.
    focus_field(&window, cx, "name");
    cx.simulate_input("set");
    wait_for_echo(&window, cx, "set");
    assert_eq!(field_text(&window, cx, "name"), "set");
}

#[gpui::test]
fn the_caret_and_selection_survive_re_renders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    focus_field(&window, cx, "name");
    cx.simulate_input(" world");
    wait_for_echo(&window, cx, "typed world");

    // The caret, moved into the text — not at its end.
    cx.simulate_keystrokes("left left left");
    assert_eq!(
        marked_caret(&window, cx, "name"),
        Some(8..8),
        "the caret moved into the text"
    );

    // A re-render the toggle's answer draws keeps it: the field's own
    // state, not a value the tree re-applies.
    let toggle = bounds(cx, "designed-toggle");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    until(&window, cx, |view| {
        !matches!(view.status, pane_core::Status::Running { .. })
    });
    assert_eq!(
        marked_caret(&window, cx, "name"),
        Some(8..8),
        "the caret survived the re-render"
    );

    // A selection survives a re-render too.
    focus_field(&window, cx, "name");
    cx.simulate_keystrokes("shift-right shift-right");
    assert_eq!(
        marked_caret(&window, cx, "name"),
        Some(8..10),
        "two characters selected"
    );
    let toggle = bounds(cx, "designed-toggle");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    until(&window, cx, |view| {
        !matches!(view.status, pane_core::Status::Running { .. })
    });
    assert_eq!(
        marked_caret(&window, cx, "name"),
        Some(8..10),
        "the selection survived the re-render"
    );
}

/// The caret or selection of the field keyed `key`: its selected range,
/// as a platform input method sees it (the test's texts are ASCII, so
/// the ranges are the byte ranges they read as).
fn marked_caret(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    key: &'static str,
) -> Option<std::ops::Range<usize>> {
    let field = field_of(window, cx, key);
    cx.update(|window, cx| {
        field.update(cx, |field, cx| {
            field
                .selected_text_range(true, window, cx)
                .map(|selection| selection.range)
        })
    })
}

#[gpui::test]
fn input_method_composition_survives_re_renders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    // A Clear makes the name field empty, so the composed text is the
    // whole of it.
    let clear = bounds(cx, "designed-button-Clear");
    cx.simulate_click(clear.center(), gpui::Modifiers::none());
    wait_for_echo(&window, cx, "");
    focus_field(&window, cx, "name");

    // What a platform input method does: mark composing text, then
    // replace it with the committed text. The field's own answer — the
    // input events' echo — redraws the tree, and the composition
    // survives it.
    let field = field_of(&window, cx, "name");
    cx.update(|window, cx| {
        field.update(cx, |field, cx| {
            field.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        })
    });
    assert_eq!(field_text(&window, cx, "name"), "にほ");
    assert_eq!(
        marked(&window, cx, "name"),
        Some(0..2),
        "the composition is marked"
    );
    wait_for_echo(&window, cx, "にほ");
    assert_eq!(
        marked(&window, cx, "name"),
        Some(0..2),
        "the composition survived the re-render"
    );

    // The committed text replaces it.
    cx.update(|window, cx| {
        field.update(cx, |field, cx| {
            field.replace_text_in_range(Some(0..2), "日本", window, cx);
        })
    });
    wait_for_echo(&window, cx, "日本");
    assert_eq!(field_text(&window, cx, "name"), "日本");
    assert_eq!(marked(&window, cx, "name"), None, "the composition ended");
}

/// The gallery's scroll region's position, kept by its key.
fn scrolled(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    cx.read_entity(window, |window, _| window.designed_scroll("gallery"))
        .expect("the gallery's scroll is keyed")
}

#[gpui::test]
fn the_scroll_position_survives_re_renders_and_reorders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    // The gallery's scroll region is keyed: its position is kept by its
    // key, so the re-renders the extension's answers draw never jump it.
    assert_eq!(
        scrolled(&window, cx).y,
        px(0.),
        "the gallery starts at its top"
    );

    // The wheel scrolls it.
    let heading = bounds(cx, "designed-text-The UI component set");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: heading.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(300.))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let at = scrolled(&window, cx);
    assert!(at.y > px(0.), "the gallery scrolled: {at:?}");

    // A re-render keeps the position: "Reorder" redraws the tree with
    // the fields swapped.
    let reorder = bounds(cx, "designed-button-Reorder");
    cx.simulate_click(reorder.center(), gpui::Modifiers::none());
    until(&window, cx, |view| {
        !matches!(view.status, pane_core::Status::Running { .. })
    });
    let after = scrolled(&window, cx);
    assert_eq!(after, at, "the scroll position survived the re-render");
}

#[gpui::test]
fn the_focus_survives_re_renders_and_reorders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    focus_field(&window, cx, "name");
    assert_eq!(focused_label(cx).as_deref(), Some("Name"));

    // A re-render keeps the focus: the field's own answer (its input
    // events' echo) redraws the tree with the keyboard where it was.
    cx.simulate_input(" more");
    wait_for_echo(&window, cx, "typed more");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Name"),
        "the focus survived the re-render"
    );

    // A reorder moves the keyed fields around, from the keyboard: Tab
    // reaches the "Reorder" button, Enter swaps the fields, and the
    // button keeps the keyboard through the re-render that moved them
    // (its keyed handle, still drawn, still focused).
    cx.simulate_keystrokes("tab tab tab");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Reorder"),
        "Tab reached the Reorder button"
    );
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        !matches!(view.status, pane_core::Status::Running { .. })
    });
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Reorder"),
        "the focused button survived the re-render"
    );

    // The fields swapped, their state with them: Shift+Tab back to the
    // name field, now second, holds everything typed in it.
    cx.simulate_keystrokes("shift-tab shift-tab");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Name"),
        "the name field is where the tree put it"
    );
    cx.simulate_input("!");
    wait_for_echo(&window, cx, "typed more!");
    assert_eq!(
        field_text(&window, cx, "name"),
        "typed more!",
        "the field kept its state across the reorder"
    );
}

#[gpui::test]
fn the_hover_survives_re_renders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;
    // The pointer over a node with a hover variant: the variant applies
    // with no call into the extension, and a re-render — the field's own
    // answer, which never moves the pointer — keeps it applied.
    let surface = bounds(cx, "designed-text-Surface");
    let over = surface.center();
    cx.simulate_mouse_move(over, None::<gpui::MouseButton>, gpui::Modifiers::none());
    let hovered = bounds(cx, "designed-text-Surface");
    assert!(
        paints_fill_at(cx, hovered, DARK_ACCENT),
        "the hover variant fills with the accent"
    );

    // A re-render keeps the hover: typing in the field answers with a
    // tree, and the variant still applies.
    focus_field(&window, cx, "name");
    cx.simulate_input(" more");
    wait_for_echo(&window, cx, "typed more");
    let hovered = bounds(cx, "designed-text-Surface");
    assert!(
        paints_fill_at(cx, hovered, DARK_ACCENT),
        "the hover survived the re-render"
    );
}

#[gpui::test]
fn tab_order_follows_the_tree_across_reorders(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let window = opened.window;

    // Tab walks the focusable controls in tree order: from the name
    // field to the notes area, the fields' row below it.
    focus_field(&window, cx, "name");
    cx.simulate_keystrokes("tab");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Notes"),
        "Tab moves from the name field to the notes area"
    );

    // A reorder swaps the fields in the tree; the order follows it: from
    // the name field, Tab now reaches the buttons' row.
    let reorder = bounds(cx, "designed-button-Reorder");
    cx.simulate_click(reorder.center(), gpui::Modifiers::none());
    until(&window, cx, |view| {
        !matches!(view.status, pane_core::Status::Running { .. })
    });
    focus_field(&window, cx, "name");
    cx.simulate_keystrokes("tab");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Clear"),
        "the tab order follows the tree across the reorder"
    );
}
