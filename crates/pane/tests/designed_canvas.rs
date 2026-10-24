//! The canvas's window tests (#242, the custom view's successor): a
//! canvas that fills the space the layout gives it is told its size —
//! named in the render context, and a change of it the resize event the
//! view asked for — and the input the designed fixture's canvas command
//! records, driven with real pointer and key events, arrives as the
//! events its tree named. The Rust, JavaScript and TypeScript samples'
//! colour picker, drawn as a canvas, is held by `window.rs`.

use std::fs;
use std::path::PathBuf;

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime, Screen};

#[path = "support/settle.rs"]
mod settle;

#[path = "support/a11y.rs"]
mod a11y;

use a11y::{a11y, focused_label};
use settle::{settle, until};

/// The launcher window with the designed fixture's package installed and
/// its canvas command (a canvas filling its space, drawing what it
/// receives) open.
fn open(cx: &mut TestAppContext) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    let data = tempfile::tempdir().unwrap();
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/designed");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    let source = data.path().join("designed");
    fs::create_dir_all(&source).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), source.join(entry.file_name())).unwrap();
    }
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    futures::executor::block_on(launcher.install_package(&source));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // The canvas command, not the counter the fixture's first command is:
    // root search finds it by its title.
    cx.simulate_input("canvas fixture");
    let view = settle(&window, cx);
    assert_eq!(
        view.rows.first().map(|row| row.title.as_str()),
        Some("Designed canvas fixture"),
        "{:?}",
        view.rows
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::DesignedView(_)),
        "{:?}",
        view.screen
    );
    // The data folder must outlive the window: the package loads from it.
    std::mem::forget(data);
    (window, cx)
}

/// The value of the canvas a designed view's screen shows, when it draws
/// one: the size the layout gave it, and what it has received.
fn canvas_value_of(screen: &Screen) -> Option<String> {
    let Screen::DesignedView(view) = screen else {
        return None;
    };
    fn value(node: &pane_core::Node) -> Option<String> {
        match &node.kind {
            pane_core::NodeKind::Canvas(canvas) => canvas.a11y.value.clone(),
            _ => node.children.iter().find_map(value),
        }
    }
    value(&view.tree.root)
}

/// Waits until the canvas's value is `expected`.
fn until_value(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, expected: &str) {
    let expected = expected.to_owned();
    until(window, cx, move |view| {
        canvas_value_of(&view.screen) == Some(expected.clone())
    });
}

/// Where the canvas's drawing area starts.
fn canvas_origin(cx: &mut VisualTestContext) -> gpui::Point<gpui::Pixels> {
    cx.debug_bounds("designed-canvas-fill")
        .expect("the canvas is drawn")
        .origin
}

/// The size the canvas's drawing area has.
fn canvas_size(cx: &mut VisualTestContext) -> (u32, u32) {
    let size = cx
        .debug_bounds("designed-canvas-fill")
        .expect("the canvas is drawn")
        .size;
    (size.width.as_f32() as u32, size.height.as_f32() as u32)
}

/// A canvas that fills its space is told its size, and a change of it is
/// the resize event: the value it draws moves with the window.
#[gpui::test]
fn a_filling_canvas_is_told_its_size_and_its_changes(cx: &mut TestAppContext) {
    let (window, cx) = open(cx);

    // The first drawing arrives with no size (nothing was laid out yet);
    // the first paint names it, and the view asked to be told of its
    // changes, so the value shows it at once.
    let (width, height) = canvas_size(cx);
    until_value(&window, cx, &format!("{width}x{height}"));

    // The window resizes: the canvas's area changes with it, the resize
    // event tells the view, and the value follows.
    cx.simulate_resize(gpui::size(px(700.), px(500.)));
    cx.run_until_parked();
    let (width, height) = canvas_size(cx);
    until_value(&window, cx, &format!("{width}x{height}"));
}

/// The input a canvas receives: the wheel, a double click, the secondary
/// button, and its own key events — each recorded by the fixture, so the
/// events Pane sent are what the value shows.
#[gpui::test]
fn the_canvas_receives_its_richer_input(cx: &mut TestAppContext) {
    let (window, cx) = open(cx);
    let origin = canvas_origin(cx);
    let at = |x: f32, y: f32| origin + gpui::point(px(x), px(y));

    // The pointer comes onto it first, as a user's does before the
    // wheel turns or a button presses there: its entering is an event of
    // its own, and the steps below are each their own events after it.
    cx.simulate_mouse_move(at(20., 10.), None, gpui::Modifiers::none());
    until_last(&window, cx, "pointer-enter");

    // The wheel over it, told where it turned.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at(20., 10.),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., -2.)),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    until_last(&window, cx, "wheel 20,10");

    // A double click: the second press, counted twice, is a press and
    // the double — then its release.
    cx.simulate_event(gpui::MouseDownEvent {
        position: at(20., 10.),
        modifiers: gpui::Modifiers::none(),
        button: gpui::MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: at(20., 10.),
        modifiers: gpui::Modifiers::none(),
        button: gpui::MouseButton::Left,
        click_count: 2,
    });
    until_last(
        &window,
        cx,
        "pointer-down 20,10; double-click 20,10; pointer-up 20,10",
    );

    // The secondary button.
    cx.simulate_mouse_down(
        at(30., 10.),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    until_last(&window, cx, "secondary 30,10");

    // The canvas takes the keyboard: its own key events, beyond the
    // arrows the semantic handlers take.
    cx.simulate_keystrokes("k");
    until_last(&window, cx, "key k");
}

/// Runs the window until the events the canvas's value names end with
/// `last`: the fixture keeps the latest few, so a step's own events are
/// read as the newest of them.
fn until_last(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, last: &str) {
    let last = last.to_owned();
    until(window, cx, move |view| {
        canvas_value_of(&view.screen)
            .and_then(|value| value.split_once("; ").map(|(_, events)| events.to_owned()))
            .is_some_and(|events| events.ends_with(&last))
    });
}

/// The canvas's accessibility: one node, with the role its tree named
/// (from the widened set), its label and its value — the drawing adds no
/// nodes of its own — and the keyboard, as the focused node.
#[gpui::test]
fn the_canvas_is_one_node_with_a_role_a_label_and_a_value(cx: &mut TestAppContext) {
    let (window, cx) = open(cx);

    // The canvas has the keyboard, and assistive technology reads it as
    // the slider its tree named, by its label.
    assert_eq!(focused_label(cx).as_deref(), Some("Filler"));
    let nodes: serde_json::Value = serde_json::from_str(&a11y(cx)).unwrap();
    let roles: Vec<String> = nodes["nodes"]
        .as_object()
        .map(|nodes| {
            nodes
                .values()
                .map(|node| node["aria"]["role"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        roles.iter().filter(|role| *role == "Slider").count(),
        1,
        "one canvas node: {roles:?}"
    );
    // The canvas, the tree's column, the menu button, the window's scroll
    // container, the status line, the announcer and the window, as the
    // window's own count of a designed view reads them (#121), and the
    // top bar's back button and heading (#482): the drawing adds no
    // nodes of its own beside its one canvas node.
    let mut sorted = roles.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "Button",
            "Button",
            "GenericContainer",
            "Group",
            "Heading",
            "Label",
            "Slider",
            "Status",
            "Window"
        ],
        "{roles:?}"
    );
    let value: Option<String> =
        cx.update(|_, cx| canvas_value_of(&window.read(cx).launcher().view().screen));
    assert!(value.is_some(), "the canvas names its value");
}
