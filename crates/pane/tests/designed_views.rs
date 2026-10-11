//! The designed view's window tests: the tree the launcher holds is drawn
//! with GPUI's flex layout, its four components — column, row, text and
//! button — resolving their tokens onto the theme in light and dark, and
//! announcing their roles and names. Real key events press the sample's
//! buttons: Tab and Shift+Tab move through them, Enter and Space press
//! the focused one, and the window shows the tree the view answered with.
//! Escape stays with Pane, as it does for a custom view: it leaves the
//! screen.
//!
//! The Rust designed view sample (`guests/sample-view`) is installed as a
//! package from `target/guests/packages`, its command declared
//! `"mode": "designed"`; the pane-core tests hold the tri-lingual parity.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/paint.rs"]
mod paint;

#[path = "support/settle.rs"]
mod settle;

use a11y::accessibility;
use paint::{paints_background_at, paints_fill_at};
use settle::{settle, until};

/// The dark pill a button's default tone fills with (the theme's
/// `results.pill_fill`), and its light counterpart: what the tone token
/// resolves to in each appearance.
const DARK_PILL: u32 = 0xFFFFFF14;
const LIGHT_PILL: u32 = 0x00000014;
/// The danger tone of each appearance, whose destructive fill is 18% of.
const DARK_DANGER: u32 = 0xFF9A92FF;
const LIGHT_DANGER: u32 = 0xC9372FFF;

/// The alpha of the destructive tone's fill.
const DESTRUCTIVE_ALPHA: f32 = 0.18;

/// One test's window: the folders it installed from and the launcher
/// window, with the Rust designed view sample installed and its command
/// open.
struct Opened {
    _sources: TempDir,
    _data: TempDir,
    window: Entity<LauncherWindow>,
}

/// The launcher window in the `theme` (`light` or `dark`), with the Rust
/// designed view sample's command open.
fn open<'a>(cx: &'a mut TestAppContext, theme: &str) -> (Opened, &'a mut VisualTestContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some(theme), None),
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
    cx.simulate_input("designed view sample");
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

/// Whether the element with debug selector `selector` is drawn.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

/// The a11y tree's JSON, forced on so the tree is built regardless of
/// platform accessibility.
fn a11y(cx: &mut VisualTestContext) -> String {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    cx.update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree")
}

/// The a11y tree's nodes, as GPUI reports them to assistive technology.
fn accessible_nodes(json: &str) -> Vec<serde_json::Value> {
    let tree: serde_json::Value = serde_json::from_str(json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The a11y node with `role` and `label`, if the window reports one.
fn find<'a>(
    nodes: &'a [serde_json::Value],
    role: &str,
    label: &str,
) -> Option<&'a serde_json::Value> {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
}

/// Waits until the designed view's tree shows `text`, which it does once
/// the guest's answer to the last event has arrived.
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
    use pane_core::NodeKind;
    match &node.kind {
        NodeKind::Text(text) => match &text.content {
            pane_core::TextContent::Plain(content) => Some(content.clone()),
            pane_core::TextContent::Spans(spans) => {
                (!spans.is_empty()).then(|| spans.iter().map(|span| span.text.clone()).collect())
            }
        },
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(text_of)
            .or_else(|| node.children.iter().find_map(text_of)),
        _ => node.children.iter().find_map(text_of),
    }
}

/// The launcher's view, read behind the window.
fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    cx.read_entity(window, |window, _| window.launcher().view())
}

#[gpui::test]
fn the_components_resolve_their_tokens_to_the_theme_in_the_dark(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let _ = &opened;
    tokens_resolve_to_the_theme(cx, "dark", DARK_PILL, DARK_DANGER);
}

#[gpui::test]
fn the_components_resolve_their_tokens_to_the_theme_in_the_light(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "light");
    let _ = &opened;
    tokens_resolve_to_the_theme(cx, "light", LIGHT_PILL, LIGHT_DANGER);
}

/// The four components' tokens, resolved onto the theme of `theme`: the
/// column lays its children out, the row beside each other, the default
/// button fills with the theme's pill and the destructive one with its
/// danger tone, which the appearance decides (white 8% in the dark, black
/// 8% in the light).
fn tokens_resolve_to_the_theme(cx: &mut VisualTestContext, theme: &str, pill: u32, danger: u32) {
    // The column: the text above the row of buttons; the row: the buttons
    // beside each other.
    let text = bounds(cx, "designed-text-Count: 0");
    let up = bounds(cx, "designed-button-Increment");
    let down = bounds(cx, "designed-button-Decrement");
    let reset = bounds(cx, "designed-button-Reset");
    assert!(text.origin.y < up.origin.y, "{theme}: the text is above");
    assert!(
        up.origin.y == down.origin.y && down.origin.y == reset.origin.y,
        "{theme}: the buttons sit on one line"
    );
    assert!(
        up.origin.x < down.origin.x && down.origin.x < reset.origin.x,
        "{theme}: the buttons sit beside each other"
    );

    // The default tone: the theme's pill fill.
    assert!(
        paints_fill_at(cx, up, pill),
        "{theme}: the default button fills with the pill"
    );
    // The destructive tone: the theme's danger colour, at the fill an
    // icon's danger disc uses.
    let mut destructive = gpui::rgb_to_hsla(gpui::rgba(danger));
    destructive.alpha *= DESTRUCTIVE_ALPHA;
    assert!(
        paints_background_at(cx, reset, gpui::solid_background(destructive)),
        "{theme}: the destructive button fills with the danger tone"
    );
}

#[gpui::test]
fn the_components_announce_their_roles_and_names(cx: &mut TestAppContext) {
    let (_opened, cx) = open(cx, "dark");
    let json = a11y(cx);
    let nodes = accessible_nodes(&json);

    // The text: a label read by its content; the buttons: buttons read by
    // their labels; the containers: groups, which the tree names when it
    // gives a name (the sample's does not).
    assert!(
        find(&nodes, "Label", "Count: 0").is_some(),
        "the text is a label named by its content: {json}"
    );
    for label in ["Increment", "Decrement", "Reset"] {
        assert!(
            find(&nodes, "Button", label).is_some(),
            "the {label} button is a button named by its label: {json}"
        );
    }
    assert!(
        nodes.iter().any(|node| node["role"] == "Group"),
        "the containers are groups: {json}"
    );
}

#[gpui::test]
fn real_key_events_press_the_buttons_and_the_window_shows_the_new_tree(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let window = opened.window;

    // The view opens with the keyboard on its first button; Enter presses
    // it, and the window shows the tree the view answered with.
    assert!(
        drawn(cx, "designed-text-Count: 0"),
        "the first tree is drawn"
    );
    let (focused, json) = accessibility(cx);
    let buttons = cx.read_entity(&window, |window, _| window.designed_button_count());
    assert!(
        focused.as_deref() == Some("Increment"),
        "the first button has the keyboard: {focused:?}, {buttons} buttons held, in {json}"
    );
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Count: 1");
    assert!(drawn(cx, "designed-text-Count: 1"));

    // The focus stayed on the same button (its place in the tree kept it),
    // so Enter presses it again — and Space does too.
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Count: 2");
    cx.simulate_keystrokes("space");
    wait_for(&window, cx, "Count: 3");

    // Tab moves to the next button; Enter presses that one.
    cx.simulate_keystrokes("tab");
    cx.simulate_keystrokes("enter");
    wait_for(&window, cx, "Count: 2");

    // Escape stays with Pane: it leaves the command, the view closing
    // with it, back at root search.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert!(!drawn(cx, "designed-text-Count: 2"));
}

#[gpui::test]
fn a_click_presses_the_button_under_the_pointer(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let window = opened.window;
    let reset = bounds(cx, "designed-button-Reset");
    cx.simulate_click(reset.center(), gpui::Modifiers::none());
    wait_for(&window, cx, "Count: 0");
    // A second press keeps the count at its floor.
    cx.simulate_keystrokes("enter enter");
    wait_for(&window, cx, "Count: 0");
    let view = view(&window, cx);
    assert_eq!(view.status, pane_core::Status::Idle);
}
